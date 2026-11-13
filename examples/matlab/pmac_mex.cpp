// Synchronous MATLAB gateway. All mx/mex calls run on MATLAB's calling thread.
// Build with build_pmac_mex; see example_getresponse.m for a complete session.
#include "mex.h"
#include "pmac.hpp"
#include <array>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <exception>
#include <limits>
#include <memory>
#include <string>
#include <thread>
#include <vector>

namespace {
class GatewayError : public std::runtime_error {
public:
    GatewayError(const char* id, const std::string& text) : std::runtime_error(text), id(id) {}
    const char* id;
};
[[noreturn]] void argument(const std::string& text) {
    throw GatewayError("pmac:Argument", text);
}
struct MxDestroy { void operator()(mxArray* value) const { if (value) mxDestroyArray(value); } };
using MxOwner = std::unique_ptr<mxArray, MxDestroy>;
struct MxFree { void operator()(char* value) const { mxFree(value); } };

std::string text(const mxArray* value, const char* name) {
    if (!value) argument(std::string("Missing ") + name);
    MxOwner converted;
    if (mxIsClass(value, "string")) {
        if (mxGetNumberOfElements(value) != 1) argument(std::string(name) + " must be scalar text");
        mxArray* output = nullptr;
        mxArray* input = const_cast<mxArray*>(value);
        MxOwner error(mexCallMATLABWithTrap(1, &output, 1, &input, "char"));
        converted.reset(output);
        if (error || !output) argument(std::string(name) + " cannot be converted to char");
        value = output;
    }
    if (!mxIsChar(value) || mxGetNumberOfDimensions(value) > 2 || mxGetM(value) > 1)
        argument(std::string(name) + " must be a character row vector or string scalar");
    const mxChar* chars = mxGetChars(value);
    for (mwSize i = 0; i < mxGetNumberOfElements(value); ++i)
        if (chars[i] == 0) argument(std::string(name) + " contains NUL");
    std::unique_ptr<char, MxFree> utf8(mxArrayToUTF8String(value));
    if (!utf8) throw std::bad_alloc();
    std::string result(utf8.get());
    if (result.size() > 65536) argument(std::string(name) + " exceeds 64 KiB");
    return result;
}
void scalarStruct(const mxArray* value) {
    if (!mxIsStruct(value) || mxGetNumberOfElements(value) != 1)
        argument("connect options must be a scalar struct");
}
const mxArray* field(const mxArray* options, const char* name) {
    return mxGetField(options, 0, name);
}
std::string stringField(const mxArray* options, const char* name, const char* fallback = "") {
    const auto* value = field(options, name);
    return value ? text(value, name) : std::string(fallback);
}
uint32_t integerField(const mxArray* options, const char* name, uint32_t fallback, uint32_t maximum) {
    const auto* value = field(options, name);
    if (!value) return fallback;
    if (!mxIsNumeric(value) || mxIsComplex(value) || mxIsSparse(value) || mxGetNumberOfElements(value) != 1)
        argument(std::string(name) + " must be an integer scalar");
    const double number = mxGetScalar(value);
    if (!std::isfinite(number) || number < 0 || number > maximum || std::floor(number) != number)
        argument(std::string(name) + " is outside its integer range");
    return static_cast<uint32_t>(number);
}
bool boolField(const mxArray* options, const char* name, bool fallback) {
    const auto* value = field(options, name);
    if (!value) return fallback;
    if ((!mxIsLogical(value) && !mxIsNumeric(value)) || mxIsComplex(value) || mxIsSparse(value) || mxGetNumberOfElements(value) != 1)
        argument(std::string(name) + " must be logical scalar or numeric 0/1");
    const double number = mxGetScalar(value);
    if (number != 0 && number != 1) argument(std::string(name) + " must be 0 or 1");
    return number != 0;
}
void check(int32_t status, const char* operation) {
    if (status == PMAC_OK) return;
    pmac_error_info info{};
    size_t required = 0;
    const auto first = pmac_copy_last_error(&info, nullptr, 0, &required);
    std::string detail;
    if ((first == PMAC_OK || first == PMAC_BUFFER_TOO_SMALL) && info.status == status) {
        std::vector<char> bytes(required);
        if (pmac_copy_last_error(&info, bytes.data(), bytes.size(), &required) == PMAC_OK)
            detail.assign(bytes.begin(), bytes.end());
    }
    throw pmac::Error(status, info.request_id, info.certainty,
        std::string(operation) + ": " + pmac::statusName(status) + (detail.empty() ? "" : ": " + detail));
}
struct Session {
    pmac_runtime_id runtime = 0;
    pmac_client_id client = 0;
    bool closing = false;
};
std::unique_ptr<Session> current;
bool locked = false;
bool exitRegistered = false;
// Survives MATLAB's nonlocal error exit without truncating controller diagnostics.
std::string pendingError;

// IDs remain in current if a step fails, allowing pmac_mex('close') to retry.
void closeSession() {
    if (current) {
        current->closing = true;
        if (current->client) {
            check(pmac_client_close(current->client, 0, 10000), "close client");
            check(pmac_client_release(current->client), "release client");
            current->client = 0;
        }
        if (current->runtime) {
            check(pmac_runtime_shutdown(current->runtime, 10000), "shutdown runtime");
            check(pmac_runtime_release(current->runtime), "release runtime");
            current->runtime = 0;
        }
    }
    check(pmac_runtime_cleanup_wait(), "join runtime cleanup workers");
    current.reset();
    if (locked) { mexUnlock(); locked = false; }
}
void atExit() noexcept {
    // MATLAB may be exiting. Complete native cleanup before this binary can unload.
    // A cleanup failure deliberately retains the lock and retries with the same IDs.
    for (;;) {
        try { closeSession(); return; }
        catch (...) { std::this_thread::sleep_for(std::chrono::milliseconds(50)); }
    }
}
void connect(const mxArray* options) {
    scalarStruct(options);
    if (current) argument("A session already exists; call pmac_mex('close') first");
    const char* allowed[] = {"host", "username", "password", "privateKey", "publicKey", "passphrase",
        "knownHosts", "pinnedSha256", "acceptAnyHostKey", "port", "tcpProbe", "tcpProbeTimeoutMs",
        "keepaliveIntervalMs", "keepaliveMax", "workers"};
    for (int i = 0; i < mxGetNumberOfFields(options); ++i) {
        const std::string name(mxGetFieldNameByNumber(options, i));
        bool found = false;
        for (const auto* candidate : allowed) if (name == candidate) found = true;
        if (!found) argument("Unknown connect field: " + name);
    }
    const auto host = stringField(options, "host");
    const auto username = stringField(options, "username", "root");
    const auto password = stringField(options, "password");
    const auto privateKey = stringField(options, "privateKey");
    const auto publicKey = stringField(options, "publicKey");
    const auto passphrase = stringField(options, "passphrase");
    const auto knownHosts = stringField(options, "knownHosts");
    if (host.empty() || username.empty()) argument("host and username must be nonempty");
    pmac_connect_options c{};
    c.struct_size = sizeof(c); c.abi_version = PMAC_ABI_VERSION;
    auto bytes = [](const std::string& s) { return pmac_bytes{reinterpret_cast<const uint8_t*>(s.data()), s.size()}; };
    c.host = bytes(host); c.username = bytes(username); c.password = bytes(password);
    c.private_key = bytes(privateKey); c.public_key = bytes(publicKey);
    c.passphrase = bytes(passphrase); c.known_hosts = bytes(knownHosts);
    c.port = static_cast<uint16_t>(integerField(options, "port", 22, 65535));
    if (!c.port) argument("port must be between 1 and 65535");
    c.control_pool_size = 1;
    c.flags = (boolField(options, "tcpProbe", true) ? PMAC_CONNECT_TCP_PROBE : 0u)
        | (boolField(options, "acceptAnyHostKey", false) ? PMAC_CONNECT_ACCEPT_ANY_HOST_KEY : 0u);
    c.tcp_probe_timeout_ms = integerField(options, "tcpProbeTimeoutMs", 600, UINT32_MAX);
    c.keepalive_interval_ms = integerField(options, "keepaliveIntervalMs", 1000, UINT32_MAX);
    c.keepalive_max = integerField(options, "keepaliveMax", 3, UINT32_MAX);
    if (const auto* pin = field(options, "pinnedSha256")) {
        if (!mxIsUint8(pin) || mxIsComplex(pin) || mxGetNumberOfElements(pin) != 32)
            argument("pinnedSha256 must contain exactly 32 uint8 bytes");
        c.pinned_sha256 = {static_cast<const uint8_t*>(mxGetData(pin)), 32};
    }
    pmac_runtime_config runtime{};
    runtime.struct_size = sizeof(runtime); runtime.abi_version = PMAC_ABI_VERSION;
    runtime.worker_threads = integerField(options, "workers", 2, 256);
    if (!runtime.worker_threads) argument("workers must be between 1 and 256");
    current = std::make_unique<Session>();
    mexLock(); locked = true;
    try {
        check(pmac_runtime_create(&runtime, &current->runtime), "create runtime");
        check(pmac_client_create(current->runtime, nullptr, &current->client), "create client");
        pmac_request_id id = 0;
        check(pmac_connect_submit(current->client, &c, &id), "connect");
        pmac::Request request(id);
        request.wait();
    } catch (...) {
        const auto original = std::current_exception();
        try { closeSession(); }
        catch (const std::exception& cleanup) {
            try { std::rethrow_exception(original); }
            catch (const std::exception& primary) {
                throw GatewayError("pmac:CleanupPending", std::string(primary.what()) + "; cleanup also failed: "
                    + cleanup.what() + ". Session remains locked; retry pmac_mex('close').");
            }
        }
        std::rethrow_exception(original);
    }
}
std::vector<uint8_t> getresponse(const std::string& command, const std::string& pattern) {
    if (!current || !current->client || current->closing)
        argument("No ready session; call pmac_mex('connect', options) first");
    pmac::RequestOptions options;
    const auto native = options.native();
    pmac_request_id id = 0;
    check(pmac_get_response_async(current->client, reinterpret_cast<const uint8_t*>(command.data()),
        command.size(), &native, &id), "GetResponse");
    pmac::Request request(id);
    request.wait();
    return pattern.empty() ? request.response() : request.match(pattern);
}
void dispatch(int nlhs, mxArray* plhs[], int nrhs, const mxArray* prhs[]) {
    if (nrhs < 1 || nlhs > 1) argument("Use zero or one output and a command string as the first input");
    const auto operation = text(prhs[0], "operation");
    if (operation == "version") {
        if (nrhs != 1) argument("Usage: version = pmac_mex('version')");
        if (nlhs) plhs[0] = mxCreateString(pmac_version_string());
        else mexPrintf("PMAC %s (ABI %u)\n", pmac_version_string(), pmac_abi_version());
    } else if (operation == "connect") {
        if (nrhs != 2) argument("Usage: pmac_mex('connect', options)");
        connect(prhs[1]);
        if (nlhs) plhs[0] = mxCreateLogicalScalar(true);
    } else if (operation == "getresponse") {
        if (nrhs < 2 || nrhs > 3) argument("Usage: bytes = pmac_mex('getresponse', command, optionalPattern)");
        const auto response = getresponse(text(prhs[1], "command"), nrhs == 3 ? text(prhs[2], "pattern") : "");
        if (nlhs) {
            plhs[0] = mxCreateNumericMatrix(1, static_cast<mwSize>(response.size()), mxUINT8_CLASS, mxREAL);
            if (!response.empty()) std::memcpy(mxGetData(plhs[0]), response.data(), response.size());
        }
    } else if (operation == "close") {
        if (nrhs != 1) argument("Usage: pmac_mex('close')");
        closeSession();
        if (nlhs) plhs[0] = mxCreateLogicalScalar(true);
    } else argument("Unknown operation; supported: connect, getresponse, close, version");
}
} // namespace

void mexFunction(int nlhs, mxArray* plhs[], int nrhs, const mxArray* prhs[]) {
    const char* identifier = nullptr;
    const char* emergency = nullptr;
    pendingError.clear();
    try {
        try {
            if (!exitRegistered) { mexAtExit(atExit); exitRegistered = true; }
            dispatch(nlhs, plhs, nrhs, prhs);
        } catch (const GatewayError& e) {
            identifier = e.id;
            pendingError = e.what();
        } catch (const pmac::Error& e) {
            identifier = "pmac:Native";
            pendingError = std::string(e.what()) + " [status=" + std::to_string(e.status())
                + ", certainty=" + std::to_string(e.certainty())
                + ", request=" + std::to_string(e.requestId()) + "]";
        } catch (const std::exception& e) {
            identifier = "pmac:Internal";
            pendingError = e.what();
        } catch (...) {
            identifier = "pmac:Internal";
            pendingError = "Unknown native exception";
        }
    } catch (...) {
        // Allocating the complete diagnostic can itself fail under memory pressure.
        identifier = "pmac:ResourceLimit";
        emergency = "Insufficient memory to retain the native diagnostic";
    }
    // All C++ exception objects have unwound. The complete message remains in
    // static storage across MATLAB's nonlocal error exit.
    if (identifier) mexErrMsgIdAndTxt(identifier, "%s", emergency ? emergency : pendingError.c_str());
}
