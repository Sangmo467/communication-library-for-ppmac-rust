use pmac_core::{Error, Operation};
use pmac_protocol::{parse_bool, parse_f64, parse_u32, parse_u64, Command, CommandEffect};

use crate::helpers::{execute, finish_request};
use crate::types::PmacRequestOptions;

pub(crate) fn run(
    client: u64,
    command: Command,
    options: *const PmacRequestOptions,
) -> Result<Vec<u8>, Error> {
    let request = execute(client, Operation::Command(command), options)?;
    finish_request(request)
}

pub(crate) fn u64_query(
    client: u64,
    command: Command,
    options: *const PmacRequestOptions,
    out: *mut u64,
) -> i32 {
    crate::ffi_status(|| {
        if out.is_null() {
            return Err(Error::invalid("output pointer is null"));
        }
        let value = parse_u64(&run(client, command, options)?).map_err(Error::from)?;
        unsafe { out.write(value) };
        Ok(())
    })
}

pub(crate) fn f64_query(
    client: u64,
    command: Command,
    options: *const PmacRequestOptions,
    out: *mut f64,
) -> i32 {
    crate::ffi_status(|| {
        if out.is_null() {
            return Err(Error::invalid("output pointer is null"));
        }
        let value = parse_f64(&run(client, command, options)?).map_err(Error::from)?;
        unsafe { out.write(value) };
        Ok(())
    })
}

pub(crate) fn u32_query(
    client: u64,
    command: Command,
    options: *const PmacRequestOptions,
    out: *mut u32,
) -> i32 {
    crate::ffi_status(|| {
        if out.is_null() {
            return Err(Error::invalid("output pointer is null"));
        }
        let value = parse_u32(&run(client, command, options)?).map_err(Error::from)?;
        unsafe { out.write(value) };
        Ok(())
    })
}

pub(crate) fn bool_query(
    client: u64,
    command: Command,
    options: *const PmacRequestOptions,
    out: *mut u32,
) -> i32 {
    crate::ffi_status(|| {
        if out.is_null() {
            return Err(Error::invalid("output pointer is null"));
        }
        let value = parse_bool(&run(client, command, options)?).map_err(Error::from)?;
        unsafe { out.write(u32::from(value)) };
        Ok(())
    })
}

pub(crate) fn motor_closed_loop(
    client: u64,
    motor: i32,
    options: *const PmacRequestOptions,
    out: *mut u32,
) -> i32 {
    crate::ffi_status(|| {
        if out.is_null() {
            return Err(Error::invalid("output pointer is null"));
        }
        let first = Command::raw(format!("Motor[{motor}].ClosedLoop"))?;
        if !parse_bool(&run(client, first, options)?).map_err(Error::from)? {
            unsafe { out.write(0) };
            return Ok(());
        }
        let second = Command::raw(format!("Motor[{motor}].AmpEna"))?;
        let enabled = parse_bool(&run(client, second, options)?).map_err(Error::from)?;
        unsafe { out.write(u32::from(enabled)) };
        Ok(())
    })
}

pub(crate) fn raw_system(text: &str) -> Result<Command, Error> {
    Command::raw(text).map_err(Error::from)
}
pub(crate) fn motor(motor: i32, operation: &str) -> Result<Command, Error> {
    Command::motor(
        motor,
        operation,
        if operation == "?"
            || operation == "p"
            || operation == "d"
            || operation == "v"
            || operation == "f"
        {
            CommandEffect::ReadOnly
        } else {
            CommandEffect::Control
        },
    )
    .map_err(Error::from)
}
pub(crate) fn coord(coord: i32, operation: &str) -> Result<Command, Error> {
    Command::coord(
        coord,
        operation,
        if operation == "?" {
            CommandEffect::ReadOnly
        } else {
            CommandEffect::Control
        },
    )
    .map_err(Error::from)
}

macro_rules! unit_export {
    ($name:ident, $command:expr) => {
        #[no_mangle]
        pub extern "C" fn $name(client: u64, options: *const PmacRequestOptions) -> i32 {
            crate::ffi_status(|| run(client, $command?, options).map(|_| ()))
        }
    };
}

unit_export!(pmac_abort, Ok::<Command, Error>(Command::abort()));

#[no_mangle]
pub extern "C" fn pmac_plc_enable(
    client: u64,
    plc: i32,
    options: *const PmacRequestOptions,
) -> i32 {
    crate::ffi_status(|| run(client, Command::plc_enable(plc, true)?, options).map(|_| ()))
}

#[no_mangle]
pub extern "C" fn pmac_plc_disable(
    client: u64,
    plc: i32,
    options: *const PmacRequestOptions,
) -> i32 {
    crate::ffi_status(|| run(client, Command::plc_enable(plc, false)?, options).map(|_| ()))
}

macro_rules! coord_unit_export {
    ($name:ident, $operation:literal) => {
        #[no_mangle]
        pub extern "C" fn $name(
            client: u64,
            coord: i32,
            options: *const PmacRequestOptions,
        ) -> i32 {
            crate::ffi_status(|| run(client, self::coord(coord, $operation)?, options).map(|_| ()))
        }
    };
}
coord_unit_export!(pmac_coord_enable, "enable");
coord_unit_export!(pmac_coord_hold, "hold");
coord_unit_export!(pmac_coord_run, "run");

#[no_mangle]
pub extern "C" fn pmac_coord_exec_prog(
    client: u64,
    coord: i32,
    program: i32,
    options: *const PmacRequestOptions,
) -> i32 {
    crate::ffi_status(|| {
        run(client, Command::coord_exec_prog(coord, program)?, options).map(|_| ())
    })
}

macro_rules! motor_unit_export {
    ($name:ident, $operation:literal) => {
        #[no_mangle]
        pub extern "C" fn $name(
            client: u64,
            motor_number: i32,
            options: *const PmacRequestOptions,
        ) -> i32 {
            crate::ffi_status(|| {
                run(client, self::motor(motor_number, $operation)?, options).map(|_| ())
            })
        }
    };
}
motor_unit_export!(pmac_motor_kill, "kill");
motor_unit_export!(pmac_motor_enable, "j/");
motor_unit_export!(pmac_motor_jog_stop, "jog/");

#[no_mangle]
pub extern "C" fn pmac_motor_abs_jog(
    client: u64,
    motor_number: i32,
    position: f64,
    options: *const PmacRequestOptions,
) -> i32 {
    crate::ffi_status(|| {
        run(
            client,
            Command::motor_abs_jog(motor_number, position)?,
            options,
        )
        .map(|_| ())
    })
}

#[no_mangle]
pub extern "C" fn pmac_motor_inc_jog(
    client: u64,
    motor_number: i32,
    distance: f64,
    options: *const PmacRequestOptions,
) -> i32 {
    crate::ffi_status(|| {
        run(
            client,
            Command::motor_inc_jog(motor_number, distance)?,
            options,
        )
        .map(|_| ())
    })
}

#[no_mangle]
pub extern "C" fn pmac_coord_status(
    client: u64,
    coord_number: i32,
    options: *const PmacRequestOptions,
    out: *mut u64,
) -> i32 {
    match self::coord(coord_number, "?") {
        Ok(command) => u64_query(client, command, options, out),
        Err(error) => error.code as i32,
    }
}

#[no_mangle]
pub extern "C" fn pmac_motor_status(
    client: u64,
    motor_number: i32,
    options: *const PmacRequestOptions,
    out: *mut u64,
) -> i32 {
    match self::motor(motor_number, "?") {
        Ok(command) => u64_query(client, command, options, out),
        Err(error) => error.code as i32,
    }
}

#[no_mangle]
pub extern "C" fn pmac_motor_position(
    client: u64,
    motor_number: i32,
    options: *const PmacRequestOptions,
    out: *mut f64,
) -> i32 {
    match self::motor(motor_number, "p") {
        Ok(command) => f64_query(client, command, options, out),
        Err(error) => error.code as i32,
    }
}

#[no_mangle]
pub extern "C" fn pmac_motor_desired_position(
    client: u64,
    motor_number: i32,
    options: *const PmacRequestOptions,
    out: *mut f64,
) -> i32 {
    match self::motor(motor_number, "d") {
        Ok(command) => f64_query(client, command, options, out),
        Err(error) => error.code as i32,
    }
}

#[no_mangle]
pub extern "C" fn pmac_motor_velocity(
    client: u64,
    motor_number: i32,
    options: *const PmacRequestOptions,
    out: *mut f64,
) -> i32 {
    match self::motor(motor_number, "v") {
        Ok(command) => f64_query(client, command, options, out),
        Err(error) => error.code as i32,
    }
}

#[no_mangle]
pub extern "C" fn pmac_motor_position_error(
    client: u64,
    motor_number: i32,
    options: *const PmacRequestOptions,
    out: *mut f64,
) -> i32 {
    match self::motor(motor_number, "f") {
        Ok(command) => f64_query(client, command, options, out),
        Err(error) => error.code as i32,
    }
}

#[no_mangle]
pub extern "C" fn pmac_servo_error_count(
    client: u64,
    options: *const PmacRequestOptions,
    out: *mut u32,
) -> i32 {
    match raw_system("Sys.ServoErrorCtr") {
        Ok(command) => u32_query(client, command, options, out),
        Err(error) => error.code as i32,
    }
}

#[no_mangle]
pub extern "C" fn pmac_phase_error_count(
    client: u64,
    options: *const PmacRequestOptions,
    out: *mut u32,
) -> i32 {
    match raw_system("Sys.PhaseErrorCtr") {
        Ok(command) => u32_query(client, command, options, out),
        Err(error) => error.code as i32,
    }
}

#[no_mangle]
pub extern "C" fn pmac_servo_count(
    client: u64,
    options: *const PmacRequestOptions,
    out: *mut u32,
) -> i32 {
    match raw_system("Sys.ServoCount") {
        Ok(command) => u32_query(client, command, options, out),
        Err(error) => error.code as i32,
    }
}

#[no_mangle]
pub extern "C" fn pmac_phase_time(
    client: u64,
    options: *const PmacRequestOptions,
    out: *mut f64,
) -> i32 {
    match raw_system("Sys.PhaseTime") {
        Ok(command) => f64_query(client, command, options, out),
        Err(error) => error.code as i32,
    }
}

#[no_mangle]
pub extern "C" fn pmac_servo_time(
    client: u64,
    options: *const PmacRequestOptions,
    out: *mut f64,
) -> i32 {
    match raw_system("Sys.ServoTime") {
        Ok(command) => f64_query(client, command, options, out),
        Err(error) => error.code as i32,
    }
}

#[no_mangle]
pub extern "C" fn pmac_abort_all_flag(
    client: u64,
    options: *const PmacRequestOptions,
    out: *mut u32,
) -> i32 {
    match raw_system("Sys.AbortAll") {
        Ok(command) => bool_query(client, command, options, out),
        Err(error) => error.code as i32,
    }
}

#[no_mangle]
pub extern "C" fn pmac_plc_running(
    client: u64,
    plc: i32,
    options: *const PmacRequestOptions,
    out: *mut u32,
) -> i32 {
    match raw_system(&format!("Plc[{plc}].Active")) {
        Ok(command) => bool_query(client, command, options, out),
        Err(error) => error.code as i32,
    }
}

#[no_mangle]
pub extern "C" fn pmac_coord_prog_running(
    client: u64,
    coord_number: i32,
    options: *const PmacRequestOptions,
    out: *mut u32,
) -> i32 {
    match raw_system(&format!("Coord[{coord_number}].ProgActive")) {
        Ok(command) => bool_query(client, command, options, out),
        Err(error) => error.code as i32,
    }
}

#[no_mangle]
pub extern "C" fn pmac_motor_closed_loop(
    client: u64,
    motor_number: i32,
    options: *const PmacRequestOptions,
    out: *mut u32,
) -> i32 {
    motor_closed_loop(client, motor_number, options, out)
}

#[no_mangle]
pub extern "C" fn pmac_coord_motor_enabled(
    client: u64,
    coord_number: i32,
    options: *const PmacRequestOptions,
    out: *mut u32,
) -> i32 {
    match raw_system(&format!("Coord[{coord_number}].AmpEna")) {
        Ok(command) => bool_query(client, command, options, out),
        Err(error) => error.code as i32,
    }
}
