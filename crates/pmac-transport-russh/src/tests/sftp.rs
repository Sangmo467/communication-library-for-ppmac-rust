use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use russh_sftp::protocol::{Attrs, Data, FileAttributes, Handle, OpenFlags, Status, StatusCode};

#[derive(Clone)]
struct StoredFile {
    data: Vec<u8>,
    attrs: FileAttributes,
}

#[derive(Default)]
pub(super) struct Memory {
    files: Mutex<HashMap<String, StoredFile>>,
    fail_writes: AtomicBool,
    fail_publish: AtomicBool,
    corrupt_reads: AtomicBool,
    write_gate: super::AsyncGate,
    read_gate: super::AsyncGate,
}

impl Memory {
    fn put(&self, path: &str, data: &[u8]) {
        self.files.lock().unwrap().insert(
            path.into(),
            StoredFile {
                data: data.to_vec(),
                attrs: FileAttributes {
                    size: Some(data.len() as u64),
                    permissions: Some(0o100640),
                    uid: Some(1000),
                    gid: Some(1000),
                    mtime: Some(1),
                    atime: Some(1),
                    ..FileAttributes::default()
                },
            },
        );
    }
    fn get(&self, path: &str) -> Vec<u8> {
        self.files.lock().unwrap().get(path).unwrap().data.clone()
    }
    pub(super) fn publish(&self, staging: &str, destination: &str) -> bool {
        if self.fail_publish.load(Ordering::SeqCst) {
            return false;
        }
        let mut files = self.files.lock().unwrap();
        let Some(file) = files.remove(staging) else {
            return false;
        };
        files.insert(destination.into(), file);
        true
    }
}

pub(super) struct MemorySession {
    memory: Arc<Memory>,
    handles: HashMap<String, String>,
    next_handle: u64,
}
impl MemorySession {
    pub(super) fn new(memory: Arc<Memory>) -> Self {
        Self {
            memory,
            handles: HashMap::new(),
            next_handle: 0,
        }
    }
    fn path(&self, handle: &str) -> Result<&str, StatusCode> {
        self.handles
            .get(handle)
            .map(String::as_str)
            .ok_or(StatusCode::Failure)
    }
    fn attrs(&self, id: u32, path: &str) -> Result<Attrs, StatusCode> {
        if path == "/denied" {
            return Err(StatusCode::PermissionDenied);
        }
        let files = self.memory.files.lock().unwrap();
        let file = files.get(path).ok_or(StatusCode::NoSuchFile)?;
        let mut attrs = file.attrs.clone();
        attrs.size = Some(file.data.len() as u64);
        Ok(Attrs { id, attrs })
    }
}
fn ok(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: String::new(),
        language_tag: String::new(),
    }
}

impl russh_sftp::server::Handler for MemorySession {
    type Error = StatusCode;
    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        flags: OpenFlags,
        attrs: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        let mut files = self.memory.files.lock().unwrap();
        if flags.contains(OpenFlags::CREATE) {
            if flags.contains(OpenFlags::EXCLUDE) && files.contains_key(&filename) {
                return Err(StatusCode::Failure);
            }
            files.entry(filename.clone()).or_insert(StoredFile {
                data: Vec::new(),
                attrs,
            });
        } else if !files.contains_key(&filename) {
            return Err(StatusCode::NoSuchFile);
        }
        self.next_handle += 1;
        let handle = self.next_handle.to_string();
        self.handles.insert(handle.clone(), filename);
        Ok(Handle { id, handle })
    }
    async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
        self.handles.remove(&handle).ok_or(StatusCode::Failure)?;
        Ok(ok(id))
    }
    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        length: u32,
    ) -> Result<Data, Self::Error> {
        let path = self.path(&handle)?;
        let mut data = {
            let files = self.memory.files.lock().unwrap();
            let file = files.get(path).ok_or(StatusCode::NoSuchFile)?;
            let start = usize::try_from(offset).map_err(|_| StatusCode::Failure)?;
            if start >= file.data.len() {
                return Err(StatusCode::Eof);
            }
            let end = start.saturating_add(length as usize).min(file.data.len());
            file.data[start..end].to_vec()
        };
        self.memory.read_gate.enter().await;
        if self.memory.corrupt_reads.load(Ordering::SeqCst) && !data.is_empty() {
            data[0] ^= 0xff;
        }
        Ok(Data { id, data })
    }
    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, Self::Error> {
        if self.memory.fail_writes.load(Ordering::SeqCst) {
            return Err(StatusCode::Failure);
        }
        let path = self.path(&handle)?;
        {
            let mut files = self.memory.files.lock().unwrap();
            let file = files.get_mut(path).ok_or(StatusCode::NoSuchFile)?;
            let start = usize::try_from(offset).map_err(|_| StatusCode::Failure)?;
            let end = start.checked_add(data.len()).ok_or(StatusCode::Failure)?;
            if file.data.len() < end {
                file.data.resize(end, 0);
            }
            file.data[start..end].copy_from_slice(&data);
        }
        self.memory.write_gate.enter().await;
        Ok(ok(id))
    }
    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        self.attrs(id, &path)
    }
    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        self.attrs(id, &path)
    }
    async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, Self::Error> {
        self.attrs(id, self.path(&handle)?)
    }
    async fn setstat(
        &mut self,
        id: u32,
        path: String,
        attrs: FileAttributes,
    ) -> Result<Status, Self::Error> {
        let mut files = self.memory.files.lock().unwrap();
        let file = files.get_mut(&path).ok_or(StatusCode::NoSuchFile)?;
        file.attrs.uid = attrs.uid;
        file.attrs.gid = attrs.gid;
        file.attrs.permissions = attrs.permissions;
        Ok(ok(id))
    }
    async fn remove(&mut self, id: u32, path: String) -> Result<Status, Self::Error> {
        self.memory
            .files
            .lock()
            .unwrap()
            .remove(&path)
            .ok_or(StatusCode::NoSuchFile)?;
        Ok(ok(id))
    }
}

pub(super) fn quoted_args(command: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut chars = command.trim().chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\'' => quoted = !quoted,
            '\\' if !quoted => {
                if let Some(ch) = chars.next() {
                    current.push(ch);
                }
            }
            ch if ch.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            ch => current.push(ch),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

#[test]
fn upload_and_download_stream_verified_data_without_fsync_extension() {
    let fixture = super::Fixture::new(None);
    fixture.connect();
    let payload = (0..200_000)
        .map(|index| (index % 251) as u8)
        .collect::<Vec<_>>();
    let source = super::TestFile::new(&payload);
    let destination = super::TestFile::new(b"old local target");
    let remote = "/path with 'quotes'";
    fixture
        .observed
        .filesystem
        .put(remote, b"old remote target");
    let result = fixture
        .run(pmac_core::Operation::Upload {
            local: source.0.to_string_lossy().into(),
            remote: remote.into(),
        })
        .unwrap();
    assert_eq!(
        result.response,
        format!("{:x}", md5::compute(&payload)).as_bytes()
    );
    assert_eq!(fixture.observed.filesystem.get(remote), payload);
    assert_eq!(
        fixture
            .observed
            .filesystem
            .files
            .lock()
            .unwrap()
            .get(remote)
            .unwrap()
            .attrs
            .permissions,
        Some(0o100640)
    );
    fixture
        .run(pmac_core::Operation::Download {
            remote: remote.into(),
            local: destination.0.to_string_lossy().into(),
        })
        .unwrap();
    assert_eq!(std::fs::read(&destination.0).unwrap(), payload);
    assert_eq!(fixture.observed.filesystem.files.lock().unwrap().len(), 1);
}

#[test]
fn failed_remote_write_preserves_target_and_removes_owned_staging() {
    let fixture = super::Fixture::new(None);
    fixture.connect();
    let source = super::TestFile::new(b"new content");
    fixture.observed.filesystem.put("/target", b"original");
    fixture
        .observed
        .filesystem
        .fail_writes
        .store(true, Ordering::SeqCst);
    assert!(fixture
        .run(pmac_core::Operation::Upload {
            local: source.0.to_string_lossy().into(),
            remote: "/target".into()
        })
        .is_err());
    assert_eq!(fixture.observed.filesystem.get("/target"), b"original");
    assert_eq!(fixture.observed.filesystem.files.lock().unwrap().len(), 1);
}

#[test]
fn failed_checksum_preserves_target_and_removes_owned_staging() {
    let fixture = super::Fixture::new(None);
    fixture.connect();
    let source = super::TestFile::new(b"new content");
    fixture.observed.filesystem.put("/target", b"original");
    fixture
        .observed
        .filesystem
        .corrupt_reads
        .store(true, Ordering::SeqCst);
    let error = fixture
        .run(pmac_core::Operation::Upload {
            local: source.0.to_string_lossy().into(),
            remote: "/target".into(),
        })
        .unwrap_err();
    assert_eq!(error.code, pmac_core::ErrorCode::ChecksumMismatch);
    assert_eq!(fixture.observed.filesystem.get("/target"), b"original");
    assert_eq!(fixture.observed.filesystem.files.lock().unwrap().len(), 1);
}

#[test]
fn confirmed_publish_failure_preserves_target_and_removes_owned_staging() {
    let fixture = super::Fixture::new(None);
    fixture.connect();
    let source = super::TestFile::new(b"new content");
    fixture.observed.filesystem.put("/target", b"original");
    fixture
        .observed
        .filesystem
        .fail_publish
        .store(true, Ordering::SeqCst);
    let error = fixture
        .run(pmac_core::Operation::Upload {
            local: source.0.to_string_lossy().into(),
            remote: "/target".into(),
        })
        .unwrap_err();
    assert_eq!(error.code, pmac_core::ErrorCode::FileError);
    assert_eq!(fixture.observed.filesystem.get("/target"), b"original");
    assert_eq!(fixture.observed.filesystem.files.lock().unwrap().len(), 1);
}

#[test]
fn file_exists_distinguishes_missing_from_denied_and_empty_transfers_work() {
    let fixture = super::Fixture::new(None);
    fixture.connect();
    assert_eq!(
        fixture
            .run(pmac_core::Operation::FileExists {
                remote: "/missing".into()
            })
            .unwrap()
            .response,
        b"0"
    );
    assert_eq!(
        fixture
            .run(pmac_core::Operation::FileExists {
                remote: "/denied".into()
            })
            .unwrap_err()
            .code,
        pmac_core::ErrorCode::FileError
    );
    let source = super::TestFile::new(b"");
    fixture
        .run(pmac_core::Operation::Upload {
            local: source.0.to_string_lossy().into(),
            remote: "/empty".into(),
        })
        .unwrap();
    assert!(fixture.observed.filesystem.get("/empty").is_empty());
    fixture
        .run(pmac_core::Operation::FileRemove {
            remote: "/empty".into(),
        })
        .unwrap();
    assert_eq!(
        fixture
            .run(pmac_core::Operation::FileExists {
                remote: "/empty".into()
            })
            .unwrap()
            .response,
        b"0"
    );
}

#[test]
fn cancelling_during_publish_reports_publish_unknown_and_preserves_target() {
    let fixture = super::Fixture::new(None);
    fixture.connect();
    let source = super::TestFile::new(b"new content");
    fixture.observed.filesystem.put("/target", b"original");
    fixture.observed.publish_gate.block();
    let request = fixture
        .client
        .submit(
            fixture.runtime.next_id(),
            pmac_core::Operation::Upload {
                local: source.0.to_string_lossy().into(),
                remote: "/target".into(),
            },
            pmac_core::RequestOptions {
                timeout_ms: 10_000,
                ..pmac_core::RequestOptions::default()
            },
        )
        .unwrap();
    fixture.observed.publish_gate.wait(1);
    request.cancel();
    let error = fixture.run_result(request).unwrap_err();
    fixture.observed.publish_gate.release();
    assert_eq!(error.code, pmac_core::ErrorCode::PublishUnknown);
    assert_eq!(
        error.certainty,
        pmac_core::ExecutionCertainty::MayHaveExecuted
    );
    assert_eq!(fixture.observed.filesystem.get("/target"), b"original");
    assert_eq!(fixture.observed.filesystem.files.lock().unwrap().len(), 1);
    assert!(fixture.client.pool_snapshot().unknown_write);
    fixture
        .run(pmac_core::Operation::Command(
            pmac_protocol::Command::raw("#1p").unwrap(),
        ))
        .unwrap();
}

#[test]
fn deadline_during_publish_reports_publish_unknown_without_replaying() {
    let fixture = super::Fixture::new(None);
    fixture.connect();
    let source = super::TestFile::new(b"new content");
    fixture.observed.filesystem.put("/target", b"original");
    fixture.observed.publish_gate.block();
    let request = fixture
        .client
        .submit(
            fixture.runtime.next_id(),
            pmac_core::Operation::Upload {
                local: source.0.to_string_lossy().into(),
                remote: "/target".into(),
            },
            pmac_core::RequestOptions {
                // ARMv7 的 QEMU 执行需先完成 SSH/SFTP 握手，再在发布门控处等待超时。
                // 保持门控阻塞及全部结果断言，只给模拟器更充足的前置阶段时间。
                timeout_ms: if cfg!(target_arch = "arm") { 3_000 } else { 250 },
                ..pmac_core::RequestOptions::default()
            },
        )
        .unwrap();
    fixture.observed.publish_gate.wait(1);
    let error = fixture.run_result(request).unwrap_err();
    fixture.observed.publish_gate.release();
    assert_eq!(error.code, pmac_core::ErrorCode::PublishUnknown);
    assert!(error.message.contains("deadline"));
    assert_eq!(fixture.observed.filesystem.get("/target"), b"original");
    assert_eq!(fixture.observed.filesystem.files.lock().unwrap().len(), 1);
    assert_eq!(
        fixture.observed.password_attempts.load(Ordering::Acquire),
        2,
        "no automatic reconnect or replay"
    );
}

#[test]
fn cancelling_partially_written_upload_cleans_owned_staging_only() {
    let fixture = super::Fixture::new(None);
    fixture.connect();
    let source = super::TestFile::new(&vec![42; 200_000]);
    fixture.observed.filesystem.put("/target", b"original");
    fixture.observed.filesystem.write_gate.block();
    let request = fixture
        .client
        .submit(
            fixture.runtime.next_id(),
            pmac_core::Operation::Upload {
                local: source.0.to_string_lossy().into(),
                remote: "/target".into(),
            },
            pmac_core::RequestOptions {
                timeout_ms: 10_000,
                ..pmac_core::RequestOptions::default()
            },
        )
        .unwrap();
    fixture.observed.filesystem.write_gate.wait(1);
    request.cancel();
    fixture.observed.filesystem.write_gate.release();
    let error = fixture.run_result(request).unwrap_err();
    assert_eq!(error.code, pmac_core::ErrorCode::Cancelled);
    assert_eq!(fixture.observed.filesystem.get("/target"), b"original");
    assert_eq!(fixture.observed.filesystem.files.lock().unwrap().len(), 1);
}

#[test]
fn cancelling_download_preserves_local_target_and_removes_local_staging() {
    let fixture = super::Fixture::new(None);
    fixture.connect();
    fixture
        .observed
        .filesystem
        .put("/source", &vec![42; 200_000]);
    let target = super::TestFile::new(b"original local target");
    fixture.observed.filesystem.read_gate.block();
    let request = fixture
        .client
        .submit(
            fixture.runtime.next_id(),
            pmac_core::Operation::Download {
                remote: "/source".into(),
                local: target.0.to_string_lossy().into(),
            },
            pmac_core::RequestOptions {
                timeout_ms: 10_000,
                ..pmac_core::RequestOptions::default()
            },
        )
        .unwrap();
    fixture.observed.filesystem.read_gate.wait(1);
    request.cancel();
    fixture.observed.filesystem.read_gate.release();
    let error = fixture.run_result(request).unwrap_err();
    assert_eq!(error.code, pmac_core::ErrorCode::Cancelled);
    assert_eq!(std::fs::read(&target.0).unwrap(), b"original local target");
    let prefix = format!("{}.pmac-", target.0.file_name().unwrap().to_string_lossy());
    assert!(!std::fs::read_dir(target.0.parent().unwrap())
        .unwrap()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().starts_with(&prefix)));
}
