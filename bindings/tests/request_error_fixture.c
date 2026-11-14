/* Offline native fixture: distinguish API-local errors from persistent Request results. */
#include "pmac.h"
#include <string.h>
static int error_case;
PMAC_EXPORT void test_set_error_case(int value) { error_case = value; }
static int32_t copy_error(int local, pmac_error_info* info, char* out, size_t capacity, size_t* required) {
    const char* message;
    if (error_case == 2) return PMAC_INVALID_HANDLE;
    memset(info, 0, sizeof(*info));
    info->struct_size = sizeof(*info); info->abi_version = PMAC_ABI_VERSION;
    info->request_id = local ? 0 : 7; info->certainty = local ? 0 : 2;
    if (local) {
        info->status = error_case == 0 ? PMAC_PARSE_ERROR : PMAC_INVALID_ARGUMENT;
        message = error_case == 0 ? "regex parse error: unclosed character class" : "unrelated last error";
    } else {
        info->status = error_case == 0 ? PMAC_OK : PMAC_REMOTE_ERROR;
        message = error_case == 0 ? "" : "stdin:1:1: error #31: Buffer closed due to another thread:";
    }
    *required = strlen(message);
    if (capacity < *required) return PMAC_BUFFER_TOO_SMALL;
    if (*required) memcpy(out, message, *required);
    return PMAC_OK;
}
PMAC_EXPORT int32_t pmac_copy_last_error(pmac_error_info* i, char* o, size_t c, size_t* r) { return copy_error(1, i, o, c, r); }
PMAC_EXPORT int32_t pmac_request_copy_error(uint64_t id, pmac_error_info* i, char* o, size_t c, size_t* r) { (void)id; return copy_error(0, i, o, c, r); }
PMAC_EXPORT int32_t pmac_request_copy_match(uint64_t id, const uint8_t* p, size_t n, uint32_t g, uint8_t* o, size_t c, size_t* r) {
    (void)id; (void)p; (void)n; (void)g; (void)o; (void)c; (void)r;
    return error_case == 0 ? PMAC_PARSE_ERROR : PMAC_REMOTE_ERROR;
}
PMAC_EXPORT int32_t pmac_request_release(uint64_t id) { (void)id; return PMAC_OK; }
PMAC_EXPORT int32_t pmac_request_cancel(uint64_t id) { (void)id; return PMAC_OK; }