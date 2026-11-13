use pmac_core::{Error, Operation};
use pmac_protocol::{DartCommand, DartTransfer, DART_MAX_DATA_BYTES};

use crate::helpers::{bytes, submit_request};
use crate::types::PmacRequestOptions;
use crate::{ffi_status, write_id};

#[no_mangle]
pub extern "C" fn pmac_dart_get_async(
    id: u64,
    command: u32,
    address: u32,
    data_len: usize,
    options: *const PmacRequestOptions,
    out_id: *mut u64,
) -> i32 {
    ffi_status(|| {
        unsafe { write_id(out_id, 0)? };
        let command = DartCommand::try_from(command).map_err(Error::from)?;
        let transfer = DartTransfer::get(command, address, data_len).map_err(Error::from)?;
        let request = submit_request(id, Operation::Dart(transfer), options)?;
        unsafe { write_id(out_id, request) }
    })
}

#[no_mangle]
pub extern "C" fn pmac_dart_set_async(
    id: u64,
    command: u32,
    address: u32,
    data: *const u8,
    data_len: usize,
    options: *const PmacRequestOptions,
    out_id: *mut u64,
) -> i32 {
    ffi_status(|| {
        unsafe { write_id(out_id, 0)? };
        let command = DartCommand::try_from(command).map_err(Error::from)?;
        if data_len == 0 || data_len > DART_MAX_DATA_BYTES {
            return Err(Error::invalid(format!(
                "data length must be 1..={DART_MAX_DATA_BYTES} bytes"
            )));
        }
        let data = unsafe { bytes(data, data_len)? }.to_vec();
        let transfer = DartTransfer::set(command, address, data).map_err(Error::from)?;
        let request = submit_request(id, Operation::Dart(transfer), options)?;
        unsafe { write_id(out_id, request) }
    })
}

#[no_mangle]
pub extern "C" fn pmac_dart_connect_async(
    id: u64,
    options: *const PmacRequestOptions,
    out_id: *mut u64,
) -> i32 {
    ffi_status(|| {
        unsafe { write_id(out_id, 0)? };
        let request = submit_request(id, Operation::DartConnect, options)?;
        unsafe { write_id(out_id, request) }
    })
}

#[no_mangle]
pub extern "C" fn pmac_dart_disconnect_async(
    id: u64,
    options: *const PmacRequestOptions,
    out_id: *mut u64,
) -> i32 {
    ffi_status(|| {
        unsafe { write_id(out_id, 0)? };
        let request = submit_request(id, Operation::DartDisconnect, options)?;
        unsafe { write_id(out_id, request) }
    })
}
