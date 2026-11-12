#ifdef NDEBUG
#undef NDEBUG
#endif
#if defined(PMAC_TEST_CPP17)
#include "pmac.hpp"
#else
#include "pmac_cpp98.hpp"
#endif
#include <cassert>
#include <cstring>
#include <new>
#include <string>

namespace {
int32_t admission_status = PMAC_OK;
int32_t completion_status = PMAC_OK;
std::string response;
unsigned submits = 0, releases = 0, waits = 0, pending_waits = 0;
bool allocation_failure = false;

int32_t submit(pmac_client_id, const uint8_t*, size_t, const pmac_request_options*, pmac_request_id* request) {
    ++submits;
    *request = admission_status == PMAC_OK ? 17 : 0;
    return admission_status;
}
int32_t wait(pmac_request_id request, uint32_t, pmac_request_info* info) {
    assert(request == 17);
    ++waits;
    if (pending_waits) { --pending_waits; return PMAC_WAIT_PENDING; }
    info->request_id = request;
    info->status = completion_status;
    return completion_status;
}
int32_t copy(pmac_request_id request, uint8_t* output, size_t capacity, size_t* required) {
    assert(request == 17);
    if (allocation_failure) throw std::bad_alloc();
    *required = response.size();
    if (capacity < response.size()) return PMAC_BUFFER_TOO_SMALL;
    if (!response.empty()) std::memcpy(output, response.data(), response.size());
    return PMAC_OK;
}
int32_t release(pmac_request_id request) {
    assert(request == 17);
    ++releases;
    return PMAC_OK;
}
void reset(const char* value, int32_t status) {
    response = value;
    admission_status = PMAC_OK;
    completion_status = status;
    submits = releases = waits = pending_waits = 0;
    allocation_failure = false;
}
int32_t query(bool& exists) {
    const pmac::detail::FileExistsApi api = {submit, wait, copy, release};
    return pmac::detail::fileExistsNoThrowCore(1, 0, 0, 0, exists, api);
}
void run_decoder_regressions() {
    bool exists = false;
    reset("1", PMAC_OK);
    assert(query(exists) == PMAC_OK && exists);
    assert(submits == 1 && releases == 1);

    reset("0", PMAC_OK);
    assert(query(exists) == PMAC_OK && !exists);
    assert(submits == 1 && releases == 1);

    exists = true;
    reset("", PMAC_FILE_ERROR);
    assert(query(exists) == PMAC_FILE_ERROR && exists);
    assert(submits == 1 && releases == 1);

    reset("", PMAC_OK);
    assert(query(exists) == PMAC_PARSE_ERROR && exists);
    reset("false", PMAC_OK);
    assert(query(exists) == PMAC_PARSE_ERROR && exists);
    reset("2", PMAC_OK);
    assert(query(exists) == PMAC_PARSE_ERROR && exists);

    reset("", PMAC_OK);
    admission_status = PMAC_QUEUE_FULL;
    assert(query(exists) == PMAC_QUEUE_FULL && exists);
    assert(submits == 1 && releases == 0 && waits == 0);

    reset("1", PMAC_OK);
    allocation_failure = true;
    assert(query(exists) == PMAC_RESOURCE_LIMIT && exists);
    assert(releases == 1);

    reset("0", PMAC_OK);
    pending_waits = 1;
    assert(query(exists) == PMAC_OK && !exists);
    assert(submits == 1 && waits == 2 && releases == 1);
}
} // namespace

int main() {
    run_decoder_regressions();
    for (int status = 0; status >= PMAC_SUPERSEDED; --status)
        assert(std::strcmp(pmac::statusName(status), "PMAC_UNKNOWN_STATUS") != 0);
    assert(std::strcmp(pmac::statusName(99), "PMAC_UNKNOWN_STATUS") == 0);
    assert(pmac::isTransportError(PMAC_DISCONNECTED));
    assert(!pmac::isTransportError(PMAC_FILE_ERROR));
    assert(pmac::errorCategory(PMAC_HOST_KEY) == pmac::IdentityError);

#if defined(PMAC_TEST_CPP17)
    pmac::Runtime runtime;
    pmac::Client client = runtime.createClient();
    const pmac_runtime_info info = pmac::Runtime::runtimeInfo();
    assert(pmac::Runtime::versionNumber() == info.version_number);
    assert(pmac::Runtime::abiVersion() == PMAC_ABI_VERSION);
    pmac::Error error(PMAC_FILE_ERROR, 0, 0, "denied");
#else
    pmac98::Runtime runtime;
    pmac98::Client client(runtime);
    const pmac_runtime_info info = pmac98::Runtime::runtimeInfo();
    assert(pmac98::Runtime::versionNumber() == info.version_number);
    assert(pmac98::Runtime::abiVersion() == PMAC_ABI_VERSION);
    pmac98::Error error(PMAC_FILE_ERROR, "denied");
#endif
    assert(info.pointer_width == sizeof(void*) * 8);
    assert(error.category() == pmac::FileError && !error.isTransportError());
    assert(std::strcmp(error.statusName(), "PMAC_FILE_ERROR") == 0);
    bool exists = true;
    assert(client.fileExistsNoThrow("/not-contacted", exists) == PMAC_NO_READY_LANE);
    assert(exists); // Failure never claims that the remote file is missing.
    return 0;
}
