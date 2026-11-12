#ifndef PMAC_CPP98_HPP
#define PMAC_CPP98_HPP

#include "pmac.h"
#include "pmac_status.hpp"
#include "pmac_error.hpp"
#include <stdexcept>
#include <string>
#include <vector>

namespace pmac98 {

class Error : public std::runtime_error {
public:
    explicit Error(int status, const std::string& operation) : std::runtime_error(operation + " failed"), status_(status) {}
    int status() const { return status_; }
    const char* statusName() const { return pmac::statusName(status_); }
    pmac::ErrorCategory category() const { return pmac::errorCategory(status_); }
    bool isTransportError() const { return pmac::isTransportError(status_); }
private:
    int status_;
};

inline void check(int status, const char* operation) { if (status != PMAC_OK) throw Error(status, operation); }

struct Options {
    uint32_t timeout_ms;
    bool infinite;
    bool read_pool;
    size_t max_response_bytes;
    Options() : timeout_ms(5000), infinite(false), read_pool(false), max_response_bytes(1024 * 1024) {}
    pmac_request_options native() const {
        pmac_request_options value = {};
        value.struct_size = sizeof(value); value.abi_version = PMAC_ABI_VERSION;
        value.timeout_ms = timeout_ms; value.flags = (infinite ? PMAC_REQUEST_INFINITE : 0u) | (read_pool ? PMAC_REQUEST_READ_POOL : 0u); value.max_response_bytes = max_response_bytes;
        return value;
    }
};

struct ConnectOptions {
    std::string host, username, password, private_key, public_key, passphrase, known_hosts;
    uint16_t port;
    bool accept_any_host_key;
    uint8_t control_pool_size;
    std::vector<uint8_t> pinned_sha256;
    ConnectOptions(const std::string& host, const std::string& username, const std::string& password = "")
        : host(host), username(username), password(password), port(22), accept_any_host_key(false), control_pool_size(1) {}
};

struct PoolProfile {
    uint32_t target_size, max_members, min_ready, connect_timeout_ms;
    PoolProfile() : target_size(1), max_members(4), min_ready(1), connect_timeout_ms(60000) {}
    pmac_pool_profile native() const {
        pmac_pool_profile result = {sizeof(pmac_pool_profile), PMAC_ABI_VERSION, target_size, max_members, min_ready, connect_timeout_ms};
        return result;
    }
};
class Runtime {
public:
    explicit Runtime(uint32_t workers = 2) : id_(0) {
        pmac_runtime_config config = {};
        config.struct_size = sizeof(config); config.abi_version = PMAC_ABI_VERSION; config.worker_threads = workers;
        check(pmac_runtime_create(&config, &id_), "runtime_create");
    }
    ~Runtime() { pmac::detail::runtimeCleanup(id_, pmac::detail::nativeRuntimeApi()); }
    uint64_t id() const { return id_; }
    static const char* version() { return pmac_version_string(); }
    static uint32_t versionNumber() { return pmac_version_number(); }
    static uint32_t abiVersion() { return pmac_abi_version(); }
    static pmac_runtime_info runtimeInfo() {
        pmac_runtime_info info = {};
        check(pmac_get_runtime_info(&info), "get_runtime_info");
        return info;
    }
private:
    Runtime(const Runtime&); Runtime& operator=(const Runtime&);
    uint64_t id_;
};

class Client {
public:
    explicit Client(Runtime& runtime) : id_(0) {
        pmac_client_config config = {};
        config.struct_size = sizeof(config); config.abi_version = PMAC_ABI_VERSION;
        config.max_requests = 64; config.max_file_tasks = 1; config.max_retained_bytes = 80 * 1024 * 1024;
        check(pmac_client_create(runtime.id(), &config, &id_), "client_create");
    }
    ~Client() { (void)reset(); }
    int32_t reset() throw() {
        if (!id_) return PMAC_OK;
        int32_t status = pmac_client_close(id_, 0, 1000);
        if (status != PMAC_OK) return status;
        status = pmac_client_release(id_);
        if (status == PMAC_OK) id_ = 0;
        return status;
    }
    static std::vector<uint64_t> pendingRecovery() {
        for (;;) {
            size_t required = 0;
            int32_t status = pmac_client_recovery_ids(0, 0, &required);
            if (status != PMAC_OK && status != PMAC_BUFFER_TOO_SMALL) return std::vector<uint64_t>();
            std::vector<uint64_t> ids(required);
            status = pmac_client_recovery_ids(ids.empty() ? 0 : &ids[0], ids.size(), &required);
            if (status == PMAC_BUFFER_TOO_SMALL) continue;
            if (status != PMAC_OK) return std::vector<uint64_t>();
            ids.resize(required); return ids;
        }
    }

    void connect(const ConnectOptions& source) {
        pmac_connect_options options = {};
        options.struct_size = sizeof(options); options.abi_version = PMAC_ABI_VERSION;
        options.host = bytes(source.host); options.username = bytes(source.username); options.password = bytes(source.password);
        options.private_key = bytes(source.private_key); options.public_key = bytes(source.public_key); options.passphrase = bytes(source.passphrase); options.known_hosts = bytes(source.known_hosts);
        options.port = source.port; options.control_pool_size = source.control_pool_size;
        if (!source.pinned_sha256.empty()) { options.pinned_sha256.data = &source.pinned_sha256[0]; options.pinned_sha256.len = source.pinned_sha256.size(); } options.flags = PMAC_CONNECT_TCP_PROBE | (source.accept_any_host_key ? PMAC_CONNECT_ACCEPT_ANY_HOST_KEY : 0);
        options.tcp_probe_timeout_ms = 600; options.keepalive_interval_ms = 1000; options.keepalive_max = 3;
        pmac_request_id request = 0;
        check(pmac_connect_submit(id_, &options, &request), "connect_submit");
        pmac_request_info info = {};
        const int status = pmac_request_wait(request, 0xffffffff, &info);
        if (status != PMAC_OK) { (void)pmac_request_release(request); throw Error(status, "connect"); }
        check(pmac_request_release(request), "request_release");
    }

    std::vector<uint8_t> command(const std::string& command, const Options& options = Options()) {
        pmac_request_options native = options.native(); pmac_request_id request = 0;
        check(pmac_command_submit(id_, data(command), command.size(), &native, &request), "command_submit");
        pmac_request_info info = {};
        const int status = pmac_request_wait(request, 0xffffffff, &info);
        if (status != PMAC_OK) { (void)pmac_request_release(request); throw Error(status, "command"); }
        size_t required = 0;
        int copy_status = pmac_request_copy_response(request, 0, 0, &required);
        if (copy_status != PMAC_BUFFER_TOO_SMALL && copy_status != PMAC_OK) { (void)pmac_request_release(request); throw Error(copy_status, "copy_response"); }
        std::vector<uint8_t> result(required);
        copy_status = pmac_request_copy_response(request, result.empty() ? 0 : &result[0], result.size(), &required);
        (void)pmac_request_release(request);
        check(copy_status, "copy_response");
        return result;
    }

    void resizePool(const PoolProfile& profile) {
        pmac_pool_profile p = profile.native(); uint64_t request = 0;
        check(pmac_pool_resize_async(id_, &p, &request), "pool_resize_submit"); wait(request, "pool_resize");
    }
    void repairPool(uint64_t member = 0, const Options& options = Options()) {
        pmac_request_options o = options.native(); uint64_t request = 0;
        check(pmac_pool_repair_async(id_, member, &o, &request), "pool_repair_submit"); wait(request, "pool_repair");
    }
    void drainPool(uint64_t member = 0, const Options& options = Options()) {
        pmac_request_options o = options.native(); uint64_t request = 0;
        check(pmac_pool_drain_async(id_, member, &o, &request), "pool_drain_submit"); wait(request, "pool_drain");
    }
    void repairMaintenance(const Options& options = Options()) {
        pmac_request_options o = options.native(); uint64_t request = 0;
        check(pmac_pool_repair_maintenance_async(id_, &o, &request), "maintenance_repair_submit"); wait(request, "maintenance_repair");
    }
    pmac_pool_state poolState() const { pmac_pool_state state = {}; check(pmac_pool_get_state(id_, &state), "pool_state"); return state; }
    std::vector<pmac_pool_member> poolMembers() const {
        for (;;) {
            size_t required = 0; int status = pmac_pool_get_members(id_, 0, 0, &required);
            if (status != PMAC_OK && status != PMAC_BUFFER_TOO_SMALL) check(status, "pool_members");
            std::vector<pmac_pool_member> result(required);
            status = pmac_pool_get_members(id_, result.empty() ? 0 : &result[0], result.size(), &required);
            if (status == PMAC_BUFFER_TOO_SMALL) continue;
            check(status, "pool_members"); result.resize(required); return result;
        }
    }
    void confirmRecovery(uint64_t revision, uint32_t flags) { check(pmac_pool_confirm_recovery(id_, revision, flags), "confirm_recovery"); }
    std::vector<uint8_t> getResponse(const std::string& text, const std::string& pattern = "", const Options& options = Options()) {
        pmac_request_options native = options.native(); uint64_t request = 0;
        check(pmac_get_response_async(id_, data(text), text.size(), &native, &request), "get_response_submit");
        ScopedRequest owner(request); pmac_request_info info = {};
        check(pmac_request_wait(request, 0xffffffffu, &info), "get_response_wait");
        size_t required = 0;
        int status = pattern.empty() ? pmac_request_copy_response(request, 0, 0, &required) : pmac_request_copy_match(request, data(pattern), pattern.size(), 0, 0, 0, &required);
        if (status != PMAC_BUFFER_TOO_SMALL && status != PMAC_OK) check(status, "get_response_copy");
        std::vector<uint8_t> result(required);
        status = pattern.empty() ? pmac_request_copy_response(request, result.empty()?0:&result[0], result.size(), &required) : pmac_request_copy_match(request, data(pattern), pattern.size(), 0, result.empty()?0:&result[0], result.size(), &required);
        check(status, "get_response_copy"); return result;
    }
    void upload(const std::string& local, const std::string& remote, const Options& options = Options()) { pmac_request_options native = options.native(); pmac_request_id request = 0; check(pmac_upload_submit(id_, data(local), local.size(), data(remote), remote.size(), &native, &request), "upload_submit"); wait(request, "upload"); }
    void download(const std::string& remote, const std::string& local, const Options& options = Options()) { pmac_request_options native = options.native(); pmac_request_id request = 0; check(pmac_download_submit(id_, data(remote), remote.size(), data(local), local.size(), &native, &request), "download_submit"); wait(request, "download"); }
    void uploadBuffer(const std::string& local, const std::string& remote, bool cleanup = false, const Options& options = Options()) { pmac_request_options native = options.native(); pmac_request_id request = 0; check(pmac_buffer_upload_submit(id_, data(local), local.size(), data(remote), remote.size(), cleanup ? 1 : 0, &native, &request), "buffer_upload_submit"); wait(request, "buffer_upload"); }
    bool fileExists(const std::string& remote, const Options& options = Options()) {
        bool exists = false;
        check(fileExistsNoThrow(remote, exists, options), "file_exists");
        return exists;
    }
    int32_t fileExistsNoThrow(const std::string& remote, bool& exists, const Options& options = Options()) throw() {
        const pmac_request_options native = options.native();
        const pmac::detail::FileExistsApi api = pmac::detail::nativeFileExistsApi();
        return pmac::detail::fileExistsNoThrowCore(id_, data(remote), remote.size(), &native, exists, api);
    }
    void removeFile(const std::string& remote, const Options& options = Options()) { pmac_request_options native = options.native(); pmac_request_id request = 0; check(pmac_file_remove_submit(id_, data(remote), remote.size(), &native, &request), "file_remove_submit"); wait(request, "file_remove"); }

    void abort(const Options& o = Options()) { pmac_request_options native_options = o.native(); check(pmac_abort(id_, &native_options), "abort"); }
    void coordEnable(int n, const Options& o = Options()) { pmac_request_options native_options = o.native(); check(pmac_coord_enable(id_, n, &native_options), "coord_enable"); }
    void coordExecProg(int n, int p, const Options& o = Options()) { pmac_request_options native_options = o.native(); check(pmac_coord_exec_prog(id_, n, p, &native_options), "coord_exec_prog"); }
    void coordHold(int n, const Options& o = Options()) { pmac_request_options native_options = o.native(); check(pmac_coord_hold(id_, n, &native_options), "coord_hold"); }
    void coordRun(int n, const Options& o = Options()) { pmac_request_options native_options = o.native(); check(pmac_coord_run(id_, n, &native_options), "coord_run"); }
    void plcEnable(int n, const Options& o = Options()) { pmac_request_options native_options = o.native(); check(pmac_plc_enable(id_, n, &native_options), "plc_enable"); }
    void plcDisable(int n, const Options& o = Options()) { pmac_request_options native_options = o.native(); check(pmac_plc_disable(id_, n, &native_options), "plc_disable"); }
    void motorKill(int n, const Options& o = Options()) { pmac_request_options native_options = o.native(); check(pmac_motor_kill(id_, n, &native_options), "motor_kill"); }
    void motorEnable(int n, const Options& o = Options()) { pmac_request_options native_options = o.native(); check(pmac_motor_enable(id_, n, &native_options), "motor_enable"); }
    void motorJogStop(int n, const Options& o = Options()) { pmac_request_options native_options = o.native(); check(pmac_motor_jog_stop(id_, n, &native_options), "motor_jog_stop"); }
    void motorAbsJog(int n, double v, const Options& o = Options()) { pmac_request_options native_options = o.native(); check(pmac_motor_abs_jog(id_, n, v, &native_options), "motor_abs_jog"); }
    void motorIncJog(int n, double v, const Options& o = Options()) { pmac_request_options native_options = o.native(); check(pmac_motor_inc_jog(id_, n, v, &native_options), "motor_inc_jog"); }
    pmac::CoordStatus coordStatus(int n, const Options& o = Options()) { pmac_request_options native_options = o.native(); uint64_t v = 0; check(pmac_coord_status(id_, n, &native_options, &v), "coord_status"); return pmac::CoordStatus(v); }
    pmac::MotorStatus motorStatus(int n, const Options& o = Options()) { pmac_request_options native_options = o.native(); uint64_t v = 0; check(pmac_motor_status(id_, n, &native_options, &v), "motor_status"); return pmac::MotorStatus(v); }
    double motorPosition(int n, const Options& o = Options()) { pmac_request_options native_options = o.native(); double v = 0; check(pmac_motor_position(id_, n, &native_options, &v), "motor_position"); return v; }
    double motorDesiredPosition(int n, const Options& o = Options()) { pmac_request_options native_options = o.native(); double v = 0; check(pmac_motor_desired_position(id_, n, &native_options, &v), "motor_desired_position"); return v; }
    double motorVelocity(int n, const Options& o = Options()) { pmac_request_options native_options = o.native(); double v = 0; check(pmac_motor_velocity(id_, n, &native_options, &v), "motor_velocity"); return v; }
    double motorPositionError(int n, const Options& o = Options()) { pmac_request_options native_options = o.native(); double v = 0; check(pmac_motor_position_error(id_, n, &native_options, &v), "motor_position_error"); return v; }
    uint32_t servoErrorCount(const Options& o = Options()) { pmac_request_options native_options = o.native(); uint32_t v = 0; check(pmac_servo_error_count(id_, &native_options, &v), "servo_error_count"); return v; }
    uint32_t phaseErrorCount(const Options& o = Options()) { pmac_request_options native_options = o.native(); uint32_t v = 0; check(pmac_phase_error_count(id_, &native_options, &v), "phase_error_count"); return v; }
    uint32_t servoCount(const Options& o = Options()) { pmac_request_options native_options = o.native(); uint32_t v = 0; check(pmac_servo_count(id_, &native_options, &v), "servo_count"); return v; }
    double phaseTime(const Options& o = Options()) { pmac_request_options native_options = o.native(); double v = 0; check(pmac_phase_time(id_, &native_options, &v), "phase_time"); return v; }
    double servoTime(const Options& o = Options()) { pmac_request_options native_options = o.native(); double v = 0; check(pmac_servo_time(id_, &native_options, &v), "servo_time"); return v; }
    bool abortAll(const Options& o = Options()) { pmac_request_options native_options = o.native(); uint32_t v = 0; check(pmac_abort_all_flag(id_, &native_options, &v), "abort_all"); return v != 0; }
    bool plcRunning(int n, const Options& o = Options()) { pmac_request_options native_options = o.native(); uint32_t v = 0; check(pmac_plc_running(id_, n, &native_options, &v), "plc_running"); return v != 0; }
    bool coordProgRunning(int n, const Options& o = Options()) { pmac_request_options native_options = o.native(); uint32_t v = 0; check(pmac_coord_prog_running(id_, n, &native_options, &v), "coord_prog_running"); return v != 0; }
    bool motorClosedLoop(int n, const Options& o = Options()) { pmac_request_options native_options = o.native(); uint32_t v = 0; check(pmac_motor_closed_loop(id_, n, &native_options, &v), "motor_closed_loop"); return v != 0; }
    bool coordMotorEnabled(int n, const Options& o = Options()) { pmac_request_options native_options = o.native(); uint32_t v = 0; check(pmac_coord_motor_enabled(id_, n, &native_options, &v), "coord_motor_enabled"); return v != 0; }

private:
    class ScopedRequest {
    public:
        explicit ScopedRequest(uint64_t id) : id_(id) {}
        ~ScopedRequest() { (void)pmac_request_release(id_); }
    private:
        ScopedRequest(const ScopedRequest&); ScopedRequest& operator=(const ScopedRequest&);
        uint64_t id_;
    };    Client(const Client&); Client& operator=(const Client&);
    static const uint8_t* data(const std::string& value) { return reinterpret_cast<const uint8_t*>(value.data()); }
    static pmac_bytes bytes(const std::string& value) { pmac_bytes result = { data(value), value.size() }; return result; }
    static void wait(pmac_request_id request, const char* operation) { pmac_request_info info = {}; const int status = pmac_request_wait(request, 0xffffffff, &info); (void)pmac_request_release(request); check(status, operation); }
    uint64_t id_;
};
}
#endif
