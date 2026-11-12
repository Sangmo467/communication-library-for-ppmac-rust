use crate::{ffi_status, helpers::*, parse_connect_options, types::*};
use pmac_core::{Error, ErrorCode, Operation, PoolProfile, RequestOptions};

unsafe fn profile(input: *const PmacPoolProfile) -> Result<PoolProfile, Error> {
    if input.is_null() {
        return Err(Error::invalid("pool profile is null"));
    }
    let p = unsafe { read_input(input)? };
    validate_header(p.struct_size, p.abi_version, size_of::<PmacPoolProfile>())?;
    let profile = PoolProfile {
        target_size: p.target_size,
        max_members: p.max_members,
        min_ready: p.min_ready,
        connect_timeout_ms: p.connect_timeout_ms,
    };
    profile.validate()?;
    Ok(profile)
}
fn options_for(profile: PoolProfile) -> RequestOptions {
    RequestOptions {
        timeout_ms: profile.connect_timeout_ms,
        ..Default::default()
    }
}
#[no_mangle]
pub extern "C" fn pmac_pool_prepare_async(
    id: u64,
    connect: *const PmacConnectOptions,
    input: *const PmacPoolProfile,
    out: *mut u64,
) -> i32 {
    ffi_status(|| {
        unsafe {
            write_id(out, 0)?;
        }
        let profile = unsafe { profile(input)? };
        let options = unsafe { parse_connect_options(connect)? };
        let request = submit_owned(
            id,
            Operation::PoolPrepare { options, profile },
            options_for(profile),
        )?;
        unsafe { write_id(out, request) }
    })
}
#[no_mangle]
pub extern "C" fn pmac_pool_resize_async(
    id: u64,
    input: *const PmacPoolProfile,
    out: *mut u64,
) -> i32 {
    ffi_status(|| {
        unsafe {
            write_id(out, 0)?;
        }
        let p = unsafe { profile(input)? };
        let request = submit_owned(id, Operation::PoolResize(p), options_for(p))?;
        unsafe { write_id(out, request) }
    })
}
#[no_mangle]
pub extern "C" fn pmac_pool_repair_async(
    id: u64,
    member_id: u64,
    options: *const PmacRequestOptions,
    out: *mut u64,
) -> i32 {
    ffi_status(|| {
        unsafe {
            write_id(out, 0)?;
        }
        let request = submit_request(id, Operation::PoolRepair { member_id }, options)?;
        unsafe { write_id(out, request) }
    })
}
#[no_mangle]
pub extern "C" fn pmac_pool_drain_async(
    id: u64,
    member_id: u64,
    options: *const PmacRequestOptions,
    out: *mut u64,
) -> i32 {
    ffi_status(|| {
        unsafe {
            write_id(out, 0)?;
        }
        let request = submit_request(id, Operation::PoolDrain { member_id }, options)?;
        unsafe { write_id(out, request) }
    })
}
#[no_mangle]
pub extern "C" fn pmac_pool_repair_maintenance_async(
    id: u64,
    options: *const PmacRequestOptions,
    out: *mut u64,
) -> i32 {
    ffi_status(|| {
        unsafe {
            write_id(out, 0)?;
        }
        let request = submit_request(id, Operation::RepairMaintenance, options)?;
        unsafe { write_id(out, request) }
    })
}
#[no_mangle]
pub extern "C" fn pmac_pool_get_state(id: u64, out: *mut PmacPoolState) -> i32 {
    ffi_status(|| {
        if out.is_null() {
            return Err(Error::invalid("pool state output is null"));
        }
        let (client, _) = client(id)?;
        let s = client.pool_snapshot();
        unsafe {
            out.write(PmacPoolState {
                struct_size: size_of::<PmacPoolState>() as u32,
                abi_version: ABI_VERSION,
                revision: s.revision,
                desired: s.desired,
                ready: s.ready,
                busy: s.busy,
                creating: s.creating,
                failed: s.failed,
                retiring: s.retiring,
                release_uncertain: s.release_uncertain,
                max_members: s.max_members,
                unknown_write: u32::from(s.unknown_write),
                maintenance_active: u32::from(s.maintenance_active),
                maintenance_failed: u32::from(s.maintenance_failed),
                last_create_error: s.last_create_error as i32,
            });
        }
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn pmac_pool_get_members(
    id: u64,
    out: *mut PmacPoolMember,
    capacity: usize,
    required: *mut usize,
) -> i32 {
    ffi_status(|| {
        if required.is_null() {
            return Err(Error::invalid("member count output is null"));
        }
        let (client, _) = client(id)?;
        let members = client.pool_members();
        unsafe {
            required.write(members.len());
        }
        if capacity < members.len() || out.is_null() && !members.is_empty() {
            return Err(Error::new(
                ErrorCode::BufferTooSmall,
                "pool member output buffer is too small",
            ));
        }
        if capacity > isize::MAX as usize / size_of::<PmacPoolMember>() {
            return Err(Error::invalid("member capacity overflow"));
        }
        for (index, m) in members.into_iter().enumerate() {
            unsafe {
                out.add(index).write(PmacPoolMember {
                    struct_size: size_of::<PmacPoolMember>() as u32,
                    abi_version: ABI_VERSION,
                    member_id: m.member_id,
                    epoch: m.epoch,
                    state: m.state as u32,
                    control: u32::from(m.control),
                    last_valid_response_unix_ms: m.last_response_unix_ms,
                    last_error: m.last_error as i32,
                    reserved: 0,
                });
            }
        }
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn pmac_pool_confirm_recovery(id: u64, expected_revision: u64, flags: u32) -> i32 {
    ffi_status(|| client(id)?.0.confirm_recovery(expected_revision, flags))
}
#[no_mangle]
pub extern "C" fn pmac_request_get_route(id: u64, member_id: *mut u64, epoch: *mut u64) -> i32 {
    ffi_status(|| {
        if member_id.is_null() || epoch.is_null() {
            return Err(Error::invalid("route output is null"));
        }
        let (request, _) = request(id)?;
        let info = request.info();
        unsafe {
            member_id.write(info.member_id);
            epoch.write(info.connection_epoch);
        }
        Ok(())
    })
}
