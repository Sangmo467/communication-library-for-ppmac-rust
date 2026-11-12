#ifndef PMAC_ERROR_HPP
#define PMAC_ERROR_HPP

#include "pmac.h"
#include <new>

namespace pmac {

inline const char* statusName(int32_t status) {
    switch (status) {
    case PMAC_OK: return "PMAC_OK";
    case PMAC_INVALID_ARGUMENT: return "PMAC_INVALID_ARGUMENT";
    case PMAC_INVALID_HANDLE: return "PMAC_INVALID_HANDLE";
    case PMAC_QUEUE_FULL: return "PMAC_QUEUE_FULL";
    case PMAC_QUEUE_TIMEOUT: return "PMAC_QUEUE_TIMEOUT";
    case PMAC_EXEC_TIMEOUT: return "PMAC_EXEC_TIMEOUT";
    case PMAC_WAIT_PENDING: return "PMAC_WAIT_PENDING";
    case PMAC_CANCELLED: return "PMAC_CANCELLED";
    case PMAC_DISCONNECTED: return "PMAC_DISCONNECTED";
    case PMAC_PROTOCOL_ERROR: return "PMAC_PROTOCOL_ERROR";
    case PMAC_REMOTE_ERROR: return "PMAC_REMOTE_ERROR";
    case PMAC_BUFFER_TOO_SMALL: return "PMAC_BUFFER_TOO_SMALL";
    case PMAC_RESOURCE_LIMIT: return "PMAC_RESOURCE_LIMIT";
    case PMAC_CLOSING: return "PMAC_CLOSING";
    case PMAC_SHUTDOWN_PENDING: return "PMAC_SHUTDOWN_PENDING";
    case PMAC_CONNECT_ERROR: return "PMAC_CONNECT_ERROR";
    case PMAC_AUTHENTICATION: return "PMAC_AUTHENTICATION";
    case PMAC_HOST_KEY: return "PMAC_HOST_KEY";
    case PMAC_FILE_NOT_FOUND: return "PMAC_FILE_NOT_FOUND";
    case PMAC_FILE_ERROR: return "PMAC_FILE_ERROR";
    case PMAC_CHECKSUM_MISMATCH: return "PMAC_CHECKSUM_MISMATCH";
    case PMAC_PUBLISH_UNKNOWN: return "PMAC_PUBLISH_UNKNOWN";
    case PMAC_PARSE_ERROR: return "PMAC_PARSE_ERROR";
    case PMAC_NO_READY_LANE: return "PMAC_NO_READY_LANE";
    case PMAC_POOL_WAIT_TIMEOUT: return "PMAC_POOL_WAIT_TIMEOUT";
    case PMAC_UNSUPPORTED: return "PMAC_UNSUPPORTED";
    case PMAC_INTERNAL: return "PMAC_INTERNAL";
    case PMAC_RECOVERY_REQUIRED: return "PMAC_RECOVERY_REQUIRED";
    case PMAC_MAINTENANCE_ACTIVE: return "PMAC_MAINTENANCE_ACTIVE";
    case PMAC_SUPERSEDED: return "PMAC_SUPERSEDED";
    default: return "PMAC_UNKNOWN_STATUS";
    }
}

enum ErrorCategory {
    NoError, ArgumentError, LifecycleError, ResourceError, QueueError,
    TransportError, AuthenticationError, IdentityError, RemoteError,
    FileError, ParseError, CancellationError, PoolError, RecoveryError,
    UnsupportedError, InternalError, UnknownError
};

inline ErrorCategory errorCategory(int32_t status) {
    switch (status) {
    case PMAC_OK: return NoError;
    case PMAC_INVALID_ARGUMENT: case PMAC_BUFFER_TOO_SMALL: return ArgumentError;
    case PMAC_INVALID_HANDLE: case PMAC_CLOSING: case PMAC_SHUTDOWN_PENDING: return LifecycleError;
    case PMAC_RESOURCE_LIMIT: return ResourceError;
    case PMAC_QUEUE_FULL: case PMAC_QUEUE_TIMEOUT: case PMAC_WAIT_PENDING: return QueueError;
    case PMAC_CONNECT_ERROR: case PMAC_DISCONNECTED: case PMAC_EXEC_TIMEOUT: case PMAC_PROTOCOL_ERROR: return TransportError;
    case PMAC_AUTHENTICATION: return AuthenticationError;
    case PMAC_HOST_KEY: return IdentityError;
    case PMAC_REMOTE_ERROR: return RemoteError;
    case PMAC_FILE_NOT_FOUND: case PMAC_FILE_ERROR: case PMAC_CHECKSUM_MISMATCH: case PMAC_PUBLISH_UNKNOWN: return FileError;
    case PMAC_PARSE_ERROR: return ParseError;
    case PMAC_CANCELLED: return CancellationError;
    case PMAC_NO_READY_LANE: case PMAC_POOL_WAIT_TIMEOUT: case PMAC_MAINTENANCE_ACTIVE: case PMAC_SUPERSEDED: return PoolError;
    case PMAC_RECOVERY_REQUIRED: return RecoveryError;
    case PMAC_UNSUPPORTED: return UnsupportedError;
    case PMAC_INTERNAL: return InternalError;
    default: return UnknownError;
    }
}

// Classification is diagnostic. Retry decisions also require request certainty.
inline bool isTransportError(int32_t status) { return errorCategory(status) == TransportError; }

namespace detail {
struct RuntimeApi {
    int32_t (*shutdown)(pmac_runtime_id, uint32_t);
    int32_t (*release)(pmac_runtime_id);
};

inline void runtimeCleanup(pmac_runtime_id id, const RuntimeApi& api) throw() {
    if (!id) return;
    for (;;) {
        int32_t status = api.shutdown(id, 1000);
        if (status == PMAC_OK) {
            status = api.release(id);
            if (status != PMAC_SHUTDOWN_PENDING) return;
        } else if (status != PMAC_SHUTDOWN_PENDING) {
            return;
        }
    }
}

inline RuntimeApi nativeRuntimeApi() {
    const RuntimeApi api = {pmac_runtime_shutdown, pmac_runtime_release};
    return api;
}

struct FileExistsApi {
    int32_t (*submit)(pmac_client_id, const uint8_t*, size_t, const pmac_request_options*, pmac_request_id*);
    int32_t (*wait)(pmac_request_id, uint32_t, pmac_request_info*);
    int32_t (*copy)(pmac_request_id, uint8_t*, size_t, size_t*);
    int32_t (*release)(pmac_request_id);
};

class FileQueryLease {
public:
    explicit FileQueryLease(int32_t (*release)(pmac_request_id)) : id(0), release_(release) {}
    ~FileQueryLease() throw() { try { if (id) (void)release_(id); } catch (...) { } }
    pmac_request_id id;
private:
    FileQueryLease(const FileQueryLease&);
    FileQueryLease& operator=(const FileQueryLease&);
    int32_t (*release_)(pmac_request_id);
};

inline int32_t fileExistsNoThrowCore(pmac_client_id client, const uint8_t* path, size_t length,
                                    const pmac_request_options* options, bool& exists,
                                    const FileExistsApi& api) throw() {
    try {
        FileQueryLease request(api.release);
        int32_t status = api.submit(client, path, length, options, &request.id);
        if (status != PMAC_OK) return status;
        pmac_request_info info = {};
        do { status = api.wait(request.id, 0xffffffffu, &info); } while (status == PMAC_WAIT_PENDING);
        if (status != PMAC_OK) return status;
        size_t required = 0;
        uint8_t value = 0;
        status = api.copy(request.id, &value, 1, &required);
        if (status == PMAC_BUFFER_TOO_SMALL && required != 1) return PMAC_PARSE_ERROR;
        if (status != PMAC_OK) return status;
        if (required != 1 || (value != '0' && value != '1')) return PMAC_PARSE_ERROR;
        exists = value == '1';
        return PMAC_OK;
    } catch (const std::bad_alloc&) { return PMAC_RESOURCE_LIMIT; }
      catch (...) { return PMAC_INTERNAL; }
}

inline FileExistsApi nativeFileExistsApi() {
    const FileExistsApi api = {pmac_file_exists_submit, pmac_request_wait,
                              pmac_request_copy_response, pmac_request_release};
    return api;
}
} // namespace detail
} // namespace pmac
#endif
