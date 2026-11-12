use std::sync::Arc;

use pmac_core::{Client, Error, ErrorCode, Operation, Request, RequestOptions, Runtime};
use pmac_transport_russh::RusshTransport;

use crate::registry::{registry, Entry};
use crate::types::{PmacClientConfig, PmacRequestOptions, ABI_VERSION, REQUEST_INFINITE};

pub(crate) fn invalid_handle() -> Error {
    Error::new(
        ErrorCode::InvalidHandle,
        "invalid, released, or wrong-type handle",
    )
}

pub(crate) fn client(id: u64) -> Result<(Arc<Client>, Arc<Runtime>), Error> {
    registry().client(id).ok_or_else(invalid_handle)
}
pub(crate) fn request(id: u64) -> Result<(Arc<Request>, Arc<Runtime>), Error> {
    registry().request(id).ok_or_else(invalid_handle)
}

pub(crate) fn submit_request(
    client_id: u64,
    operation: Operation,
    options: *const PmacRequestOptions,
) -> Result<u64, Error> {
    let options = unsafe { request_options(options)? };
    submit_owned(client_id, operation, options)
}

pub(crate) fn submit_owned(
    client_id: u64,
    operation: Operation,
    options: RequestOptions,
) -> Result<u64, Error> {
    let (client, runtime) = client(client_id)?;
    let request_id = registry().reserve(crate::registry::request_kind())?;
    let request = match client.submit(request_id, operation, options) {
        Ok(request) => request,
        Err(error) => {
            registry().remove(request_id, crate::registry::request_kind());
            return Err(error);
        }
    };
    registry().put(request_id, Entry::Request { request, runtime })?;
    Ok(request_id)
}

pub(crate) fn execute(
    client_id: u64,
    operation: Operation,
    options: *const PmacRequestOptions,
) -> Result<Arc<Request>, Error> {
    let id = submit_request(client_id, operation, options)?;
    let (request, _) = request(id)?;
    loop {
        if request.wait(1000).completed {
            break;
        }
    }
    Ok(request)
}

pub(crate) fn finish_request(request: Arc<Request>) -> Result<Vec<u8>, Error> {
    let id = request.id();
    let result = request
        .result()
        .ok_or_else(|| Error::internal("completed request has no result"))?;
    let copied = match result.as_ref() {
        Ok(result) => Ok(result.response.clone()),
        Err(error) => Err(error.clone()),
    };
    registry().remove(id, crate::registry::request_kind());
    copied
}

pub(crate) unsafe fn request_options(
    options: *const PmacRequestOptions,
) -> Result<RequestOptions, Error> {
    if options.is_null() {
        return Ok(RequestOptions::default());
    }
    let options = unsafe { read_input(options)? };
    validate_header(
        options.struct_size,
        options.abi_version,
        size_of::<PmacRequestOptions>(),
    )?;
    if options.flags & !3 != 0 {
        return Err(Error::invalid("unknown request flags"));
    }
    Ok(RequestOptions {
        timeout_ms: options.timeout_ms,
        infinite: options.flags & REQUEST_INFINITE != 0,
        read_pool: options.flags & 2 != 0,
        max_response_bytes: if options.max_response_bytes == 0 {
            RequestOptions::default().max_response_bytes
        } else {
            options.max_response_bytes
        },
    })
}

pub(crate) unsafe fn client_config(
    options: *const PmacClientConfig,
) -> Result<pmac_core::ClientConfig, Error> {
    if options.is_null() {
        return Ok(pmac_core::ClientConfig::default());
    }
    let options = unsafe { read_input(options)? };
    validate_header(
        options.struct_size,
        options.abi_version,
        size_of::<PmacClientConfig>(),
    )?;
    Ok(pmac_core::ClientConfig {
        max_requests: options.max_requests as usize,
        max_file_tasks: options.max_file_tasks as usize,
        max_retained_bytes: options.max_retained_bytes,
    })
}

pub(crate) fn validate_header(
    struct_size: u32,
    abi_version: u32,
    expected: usize,
) -> Result<(), Error> {
    if (struct_size as usize) < expected {
        return Err(Error::invalid(
            "structure size is smaller than the linked ABI definition",
        ));
    }
    if abi_version / 1000 != ABI_VERSION / 1000 {
        return Err(Error::invalid("ABI major version mismatch"));
    }
    Ok(())
}

pub(crate) unsafe fn bytes<'a>(data: *const u8, len: usize) -> Result<&'a [u8], Error> {
    if len == 0 {
        return Ok(&[]);
    }
    if data.is_null() {
        return Err(Error::invalid("null data with non-zero length"));
    }
    if len > isize::MAX as usize {
        return Err(Error::invalid("buffer length exceeds isize::MAX"));
    }
    Ok(unsafe { std::slice::from_raw_parts(data, len) })
}

pub(crate) unsafe fn text(data: *const u8, len: usize) -> Result<String, Error> {
    if len > 64 * 1024 {
        return Err(Error::invalid(
            "UTF-8 input exceeds the bounded 64 KiB text limit",
        ));
    }
    let bytes = unsafe { bytes(data, len)? };
    let text =
        std::str::from_utf8(bytes).map_err(|_| Error::invalid("text must be valid UTF-8"))?;
    if text.as_bytes().contains(&0) {
        return Err(Error::invalid("text contains NUL"));
    }
    Ok(text.to_owned())
}

pub(crate) unsafe fn write_id(out: *mut u64, id: u64) -> Result<(), Error> {
    if out.is_null() {
        return Err(Error::invalid("output ID pointer is null"));
    }
    unsafe { out.write(id) };
    Ok(())
}

pub(crate) fn copy_bytes(
    source: &[u8],
    out: *mut u8,
    capacity: usize,
    required: *mut usize,
) -> Result<(), Error> {
    if required.is_null() {
        return Err(Error::invalid("required pointer is null"));
    }
    unsafe { required.write(source.len()) };
    if capacity < source.len() || (!source.is_empty() && out.is_null()) {
        return Err(Error::new(
            ErrorCode::BufferTooSmall,
            "output buffer is too small; retry with the same request ID",
        ));
    }
    if !source.is_empty() {
        if capacity > isize::MAX as usize {
            return Err(Error::invalid("buffer capacity exceeds isize::MAX"));
        }
        unsafe { std::ptr::copy_nonoverlapping(source.as_ptr(), out, source.len()) };
    }
    Ok(())
}

pub(crate) struct ClientReservation(u64);

impl ClientReservation {
    fn new() -> Result<Self, Error> {
        Ok(Self(registry().reserve(crate::registry::client_kind())?))
    }

    fn publish(mut self, client: Arc<Client>, runtime: Arc<Runtime>) -> Result<u64, Error> {
        registry().put(
            self.0,
            Entry::Client {
                client: client.clone(),
                runtime,
            },
        )?;
        let id = self.0;
        self.0 = 0;
        Ok(id)
    }
}

impl Drop for ClientReservation {
    fn drop(&mut self) {
        if self.0 != 0 {
            registry().remove(self.0, crate::registry::client_kind());
        }
    }
}

pub(crate) fn create_client(
    runtime: Arc<Runtime>,
    config: pmac_core::ClientConfig,
) -> Result<(u64, Arc<Client>), Error> {
    let reservation = ClientReservation::new()?;
    let client = runtime.create_client_with_id(reservation.0, Arc::new(RusshTransport), config)?;
    let id = reservation.publish(client.clone(), runtime)?;
    Ok((id, client))
}

/// Read only the common header until the caller-provided size has been validated.
pub(crate) unsafe fn read_input<T: Copy>(input: *const T) -> Result<T, Error> {
    if input.is_null() {
        return Err(Error::invalid("input structure is null"));
    }
    let header = input.cast::<u32>();
    let struct_size = unsafe { header.read_unaligned() };
    if (struct_size as usize) < size_of::<T>() {
        return Err(Error::invalid(
            "input structure is shorter than the required ABI layout",
        ));
    }
    let abi_version = unsafe { header.add(1).read_unaligned() };
    validate_header(struct_size, abi_version, size_of::<T>())?;
    Ok(unsafe { input.read_unaligned() })
}
