use std::sync::Arc;

use pmac_core::{ClientConfig, Runtime};
use pmac_testkit::MockTransport;

use super::*;
use crate::business::{pmac_motor_abs_jog, pmac_motor_position};
use crate::registry::Entry;

fn empty() -> PmacBytes {
    PmacBytes {
        data: std::ptr::null(),
        len: 0,
    }
}

fn text_value(value: &'static str) -> PmacBytes {
    PmacBytes {
        data: value.as_ptr(),
        len: value.len(),
    }
}

fn connect_options() -> PmacConnectOptions {
    PmacConnectOptions {
        struct_size: size_of::<PmacConnectOptions>() as u32,
        abi_version: ABI_VERSION,
        host: text_value("mock"),
        username: text_value("tester"),
        password: empty(),
        private_key: empty(),
        public_key: empty(),
        passphrase: empty(),
        known_hosts: empty(),
        pinned_sha256: empty(),
        port: 22,
        control_pool_size: 1,
        reserved0: 0,
        flags: CONNECT_ACCEPT_ANY_HOST_KEY,
        tcp_probe_timeout_ms: 100,
        keepalive_interval_ms: 1000,
        keepalive_max: 3,
        reserved: [0; 4],
    }
}

fn mock_client(runtime: &Arc<Runtime>, transport: Arc<MockTransport>) -> u64 {
    let id = registry().reserve(client_kind()).unwrap();
    let client = runtime
        .create_client_with_id(id, transport, ClientConfig::default())
        .unwrap();
    registry()
        .put(
            id,
            Entry::Client {
                client,
                runtime: runtime.clone(),
            },
        )
        .unwrap();
    id
}

fn runtime() -> (u64, Arc<Runtime>) {
    let runtime = Runtime::new(2).unwrap();
    let id = registry()
        .insert(runtime_kind(), Entry::Runtime(runtime.clone()))
        .unwrap();
    (id, runtime)
}

#[test]
fn exported_request_flow_reuses_completed_result() {
    let (runtime_id, runtime) = runtime();
    let mock = Arc::new(MockTransport::default());
    let client = mock_client(&runtime, mock.clone());
    let options = connect_options();
    let mut connect = 0;
    assert_eq!(pmac_connect_submit(client, &options, &mut connect), 0);
    let mut info = std::mem::MaybeUninit::<PmacRequestInfo>::uninit();
    assert_eq!(pmac_request_wait(connect, 1000, info.as_mut_ptr()), 0);
    assert_eq!(pmac_request_release(connect), 0);

    let text = b"#1p";
    assert_eq!(
        pmac_command_submit(
            client,
            text.as_ptr(),
            1,
            std::ptr::null(),
            std::ptr::null_mut()
        ),
        ErrorCode::InvalidArgument as i32
    );
    assert!(mock.session.commands().is_empty());

    let mut command = 0;
    assert_eq!(
        pmac_command_submit(
            client,
            text.as_ptr(),
            text.len(),
            std::ptr::null(),
            &mut command
        ),
        0
    );
    assert_eq!(pmac_request_wait(command, 1000, info.as_mut_ptr()), 0);
    let mut required = 0;
    assert_eq!(
        pmac_request_copy_response(command, std::ptr::null_mut(), 0, &mut required),
        ErrorCode::BufferTooSmall as i32
    );
    assert_eq!(required, 1);
    let mut response = [0_u8; 4];
    assert_eq!(
        pmac_request_copy_response(
            command,
            response.as_mut_ptr(),
            response.len(),
            &mut required
        ),
        0
    );
    assert_eq!(response[0], b'1');
    assert_eq!(pmac_request_release(command), 0);
    assert_eq!(
        pmac_request_release(command),
        ErrorCode::InvalidHandle as i32
    );

    let mut get_response = 0;
    let query = b"#2p";
    assert_eq!(
        pmac_get_response_async(
            client,
            query.as_ptr(),
            query.len(),
            std::ptr::null(),
            &mut get_response
        ),
        0
    );
    assert_eq!(pmac_request_wait(get_response, 1000, info.as_mut_ptr()), 0);
    let pattern = b"(\\d)";
    assert_eq!(
        pmac_request_copy_match(
            get_response,
            pattern.as_ptr(),
            pattern.len(),
            1,
            response.as_mut_ptr(),
            response.len(),
            &mut required
        ),
        0
    );
    assert_eq!(response[0], b'2');
    assert_eq!(pmac_request_release(get_response), 0);

    let mut position = 0.0;
    assert_eq!(
        pmac_motor_position(client, 1, std::ptr::null(), &mut position),
        0
    );
    assert_eq!(position, 1.0);
    assert_eq!(
        pmac_motor_abs_jog(client, 1, f64::NAN, std::ptr::null()),
        ErrorCode::InvalidArgument as i32
    );
    assert_eq!(
        pmac_client_get_state(runtime_id, std::ptr::null_mut()),
        ErrorCode::InvalidArgument as i32
    );

    assert_eq!(pmac_client_close(client, 0, 1000), 0);
    assert_eq!(pmac_client_release(client), 0);
    assert_eq!(pmac_runtime_shutdown(runtime_id, 1000), 0);
    assert_eq!(pmac_runtime_release(runtime_id), 0);
}

#[test]
fn runtime_rejects_invalid_configuration_without_leaking_handle() {
    let config = PmacRuntimeConfig {
        struct_size: 1,
        abi_version: ABI_VERSION,
        worker_threads: 2,
        reserved: 0,
    };
    assert_eq!(
        pmac_runtime_create(&config, std::ptr::null_mut()),
        ErrorCode::InvalidArgument as i32
    );
    assert_eq!(
        pmac_request_wait(0xdead, 0, std::ptr::null_mut()),
        ErrorCode::InvalidArgument as i32
    );
}

#[test]
fn password_and_private_key_authentication_options_are_preserved() {
    let mut password_only = connect_options();
    password_only.password = text_value("test-password");
    let parsed = unsafe { parse_connect_options(&password_only) }.unwrap();
    assert_eq!(
        parsed.password.as_ref().map(|value| value.as_str()),
        Some("test-password")
    );
    assert!(parsed.private_key.is_none());

    let mut key_only = connect_options();
    key_only.private_key = text_value("C:/test/id_ed25519");
    key_only.public_key = text_value("C:/test/id_ed25519.pub");
    key_only.passphrase = text_value("test-passphrase");
    let parsed = unsafe { parse_connect_options(&key_only) }.unwrap();
    assert!(parsed.password.is_none());
    assert_eq!(
        parsed.private_key.as_ref().and_then(|path| path.to_str()),
        Some("C:/test/id_ed25519")
    );
    assert_eq!(
        parsed.public_key_path().and_then(|path| path.to_str()),
        Some("C:/test/id_ed25519.pub")
    );
    assert_eq!(
        parsed.passphrase.as_ref().map(|value| value.as_str()),
        Some("test-passphrase")
    );
}
