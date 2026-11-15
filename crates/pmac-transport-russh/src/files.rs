use pmac_core::{Error, ErrorCode, ExecutionContext, OperationResult, Session};
use russh_sftp::protocol::{FileAttributes, OpenFlags};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::session::{sftp_error, shell_quote, validate_path, RusshSession};

const CHUNK_SIZE: usize = 64 * 1024;

impl RusshSession {
    pub(crate) async fn upload_file(
        &self,
        local: &str,
        remote: &str,
        context: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        validate_path(local)?;
        validate_path(remote)?;
        self.check_staging_capacity()?;
        let sftp = self.sftp(context).await?;
        let staging = format!("{remote}.pmac-{}-{}.tmp", self.nonce(), self.sequence()?);
        let publish = format!(
            "/bin/mv -T -- {} {}",
            shell_quote(&staging)?,
            shell_quote(remote)?
        );
        // Validate the generated command before uploading or marking publication.
        // Quoting can expand even a short path beyond the Shell API limit.
        if publish.len() > 1023 {
            return Err(Error::invalid(
                "upload publish command exceeds 1023 bytes after path quoting",
            ));
        }
        let previous = match sftp.symlink_metadata(remote).await {
            Ok(metadata) if metadata.file_type().is_symlink() || metadata.file_type().is_dir() => {
                return Err(Error::new(
                    ErrorCode::FileError,
                    "upload target must not be a symlink or directory",
                ));
            }
            Ok(metadata) => Some(metadata),
            Err(error) if crate::session::is_not_found(&error) => None,
            Err(error) => return Err(sftp_error(error)),
        };
        let mut source = tokio::fs::File::open(local).await.map_err(file_error)?;
        let source_before = source.metadata().await.map_err(file_error)?;
        let mut target = match sftp
            .open_with_flags(
                &staging,
                OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE,
            )
            .await
        {
            Ok(file) => file,
            Err(error) => return Err(sftp_error(error)),
        };
        self.track_staging(&staging, true);
        let mut digest = md5::Context::new();
        let mut buffer = vec![0_u8; CHUNK_SIZE];
        let transfer = async {
            loop {
                let count = source.read(&mut buffer).await.map_err(file_error)?;
                if count == 0 {
                    break;
                }
                digest.consume(&buffer[..count]);
                target
                    .write_all(&buffer[..count])
                    .await
                    .map_err(file_error)?;
            }
            target.sync_all().await.map_err(sftp_error)?;
            target.close().await.map_err(file_error)
        };
        if let Err(error) = transfer.await {
            self.remove_owned_remote_staging(&sftp, &staging).await;
            return Err(error);
        }
        let source_after = source.metadata().await.map_err(file_error)?;
        if !same_local_snapshot(&source_before, &source_after) {
            self.remove_owned_remote_staging(&sftp, &staging).await;
            return Err(Error::new(
                ErrorCode::FileError,
                "local upload source changed during transfer",
            )
            .sent());
        }
        if let Some(metadata) = previous {
            let preserved = FileAttributes {
                uid: metadata.uid,
                gid: metadata.gid,
                permissions: metadata.permissions,
                ..FileAttributes::default()
            };
            if let Err(error) = sftp.set_metadata(&staging, preserved).await {
                self.remove_owned_remote_staging(&sftp, &staging).await;
                return Err(sftp_error(error));
            }
        }
        let local_digest = digest.compute();
        let remote_digest = match remote_md5(&sftp, &staging).await {
            Ok(digest) => digest,
            Err(error) => {
                self.remove_owned_remote_staging(&sftp, &staging).await;
                return Err(error);
            }
        };
        if local_digest != remote_digest {
            self.remove_owned_remote_staging(&sftp, &staging).await;
            return Err(Error::new(
                ErrorCode::ChecksumMismatch,
                "uploaded MD5 does not match local source",
            )
            .sent());
        }
        context.mark_publish_started();
        match self.shell(&publish, false, context).await {
            Ok(result) if result.exit_code == Some(0) => {
                context.mark_publish_confirmed();
                self.untrack_staging(&staging, true);
                Ok(OperationResult::response(
                    format!("{local_digest:x}").into_bytes(),
                ))
            }
            Ok(_) => {
                context.mark_publish_confirmed();
                self.remove_owned_remote_staging(&sftp, &staging).await;
                let mut error =
                    Error::new(ErrorCode::FileError, "remote staging file publish failed");
                error.certainty = pmac_core::ExecutionCertainty::Confirmed;
                Err(error)
            }
            Err(error) => Err(Error::new(
                ErrorCode::PublishUnknown,
                format!("publish result is unknown: {}", error.message),
            )
            .sent()
            .fatal()),
        }
    }
    pub(crate) async fn download_file(
        &self,
        remote: &str,
        local: &str,
        context: &ExecutionContext,
    ) -> Result<OperationResult, Error> {
        validate_path(remote)?;
        self.check_staging_capacity()?;
        validate_path(local)?;
        let sftp = self.sftp(context).await?;
        let remote_before = sftp.metadata(remote).await.map_err(sftp_error)?;
        let permissions = match tokio::fs::symlink_metadata(local).await {
            Ok(metadata) if metadata.file_type().is_symlink() || metadata.is_dir() => {
                return Err(Error::new(
                    ErrorCode::FileError,
                    "download target must not be a symlink or directory",
                ));
            }
            Ok(metadata) => Some(metadata.permissions()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(file_error(error)),
        };
        let staging = format!("{local}.pmac-{}-{}.tmp", self.nonce(), self.sequence()?);
        let source = sftp
            .open_with_flags(remote, OpenFlags::READ)
            .await
            .map_err(sftp_error)?;
        let target = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)
            .await
            .map_err(file_error)?;
        self.track_staging(&staging, false);
        let transfer = async {
            let mut source = source;
            let mut target = target;
            let mut digest = md5::Context::new();
            let mut buffer = vec![0_u8; CHUNK_SIZE];
            let mut downloaded_bytes = 0_u64;
            loop {
                let count = source.read(&mut buffer).await.map_err(file_error)?;
                if count == 0 {
                    break;
                }
                digest.consume(&buffer[..count]);
                downloaded_bytes = downloaded_bytes
                    .checked_add(count as u64)
                    .ok_or_else(|| file_error("download size overflow"))?;
                target
                    .write_all(&buffer[..count])
                    .await
                    .map_err(file_error)?;
            }
            source.close().await.map_err(file_error)?;
            target.sync_all().await.map_err(file_error)?;
            // Release the file handle before replacement on Windows.
            drop(target);
            let local_digest = digest.compute();
            let verified_local = local_md5(&staging).await?;
            let verified_remote = remote_md5(&sftp, remote).await?;
            let remote_after = sftp.metadata(remote).await.map_err(sftp_error)?;
            if remote_before.size != remote_after.size
                || remote_before.mtime != remote_after.mtime
                || remote_before
                    .size
                    .is_some_and(|size| size != downloaded_bytes)
            {
                return Err(file_error("remote source changed during download"));
            }
            if local_digest != verified_local || local_digest != verified_remote {
                return Err(Error::new(
                    ErrorCode::ChecksumMismatch,
                    "download checksum does not match remote source or local staging",
                )
                .sent());
            }
            if let Some(permissions) = permissions {
                tokio::fs::set_permissions(&staging, permissions)
                    .await
                    .map_err(file_error)?;
            }
            context.mark_publish_started();
            let published = replace_local(&staging, local).await;
            context.mark_publish_confirmed();
            published?;
            self.untrack_staging(&staging, false);
            Ok(OperationResult::response(
                format!("{local_digest:x}").into_bytes(),
            ))
        }
        .await;
        if transfer.is_err() {
            match tokio::fs::remove_file(&staging).await {
                Ok(()) => self.untrack_staging(&staging, false),
                Err(error) => {
                    tracing::warn!(path = staging, %error, "owned local staging could not be removed")
                }
            }
        }
        transfer
    }
    pub(crate) fn nonce(&self) -> &str {
        &self.nonce
    }
    pub(crate) fn sequence(&self) -> Result<u64, Error> {
        self.next_sequence()
    }
}

async fn local_md5(path: &str) -> Result<md5::Digest, Error> {
    let mut file = tokio::fs::File::open(path).await.map_err(file_error)?;
    let mut digest = md5::Context::new();
    let mut buffer = vec![0_u8; CHUNK_SIZE];
    loop {
        let count = file.read(&mut buffer).await.map_err(file_error)?;
        if count == 0 {
            break;
        }
        digest.consume(&buffer[..count]);
    }
    Ok(digest.compute())
}
async fn remote_md5(
    sftp: &russh_sftp::client::SftpSession,
    path: &str,
) -> Result<md5::Digest, Error> {
    let mut file = sftp
        .open_with_flags(path, OpenFlags::READ)
        .await
        .map_err(sftp_error)?;
    let mut digest = md5::Context::new();
    let mut buffer = vec![0_u8; CHUNK_SIZE];
    loop {
        let count = file.read(&mut buffer).await.map_err(file_error)?;
        if count == 0 {
            break;
        }
        digest.consume(&buffer[..count]);
    }
    file.close().await.map_err(file_error)?;
    Ok(digest.compute())
}

fn same_local_snapshot(before: &std::fs::Metadata, after: &std::fs::Metadata) -> bool {
    before.len() == after.len() && before.modified().ok() == after.modified().ok()
}

fn file_error(error: impl std::fmt::Display) -> Error {
    Error::new(ErrorCode::FileError, format!("file I/O failed: {error}")).sent()
}

#[cfg(not(windows))]
async fn replace_local(staging: &str, destination: &str) -> Result<(), Error> {
    tokio::fs::rename(staging, destination)
        .await
        .map_err(file_error)
}

#[cfg(windows)]
async fn replace_local(staging: &str, destination: &str) -> Result<(), Error> {
    let staging = staging.to_owned();
    let destination = destination.to_owned();
    tokio::task::spawn_blocking(move || {
        let mut path = tempfile::TempPath::try_from_path(staging).map_err(file_error)?;
        // The session retains cleanup ownership on failure or cancellation.
        path.disable_cleanup(true);
        path.persist(destination)
            .map_err(|error| file_error(error.error))
    })
    .await
    .map_err(|error| Error::internal(format!("local publication worker failed: {error}")))?
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[tokio::test]
    async fn atomic_publication_replaces_an_existing_target() {
        let directory = tempfile::tempdir().unwrap();
        let staging = directory.path().join("staging.tmp");
        let target = directory.path().join("target.bin");
        tokio::fs::write(&staging, b"new verified bytes")
            .await
            .unwrap();
        tokio::fs::write(&target, b"original bytes").await.unwrap();
        replace_local(staging.to_str().unwrap(), target.to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(
            tokio::fs::read(&target).await.unwrap(),
            b"new verified bytes"
        );
        assert!(!staging.exists());
    }

    #[tokio::test]
    async fn denied_atomic_publication_preserves_existing_target_and_staging() {
        use std::os::windows::fs::OpenOptionsExt;
        let directory = tempfile::tempdir().unwrap();
        let staging = directory.path().join("staging.tmp");
        let target = directory.path().join("target.bin");
        tokio::fs::write(&staging, b"new verified bytes")
            .await
            .unwrap();
        tokio::fs::write(&target, b"original bytes").await.unwrap();
        // Permit read sharing only; deny rename/delete while this handle is open.
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&target)
            .unwrap();
        let error = replace_local(staging.to_str().unwrap(), target.to_str().unwrap())
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::FileError);
        assert_eq!(tokio::fs::read(&target).await.unwrap(), b"original bytes");
        assert_eq!(
            tokio::fs::read(&staging).await.unwrap(),
            b"new verified bytes"
        );
        drop(held);
    }
}
