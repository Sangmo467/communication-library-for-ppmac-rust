#ifndef PMAC_HPP
#define PMAC_HPP

#include "pmac.h"
#include "pmac_status.hpp"
#include "pmac_error.hpp"

#include <cstring>
#include <functional>
#include <exception>
#include <memory>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace pmac {

class Error : public std::runtime_error {
public:
    Error(int32_t status, uint64_t request, uint32_t certainty, const std::string& message)
        : std::runtime_error(message), status_(status), request_(request), certainty_(certainty) {}
    int32_t status() const noexcept { return status_; }
    uint64_t requestId() const noexcept { return request_; }
    uint32_t certainty() const noexcept { return certainty_; }
    const char* statusName() const noexcept { return pmac::statusName(status_); }
    ErrorCategory category() const noexcept { return pmac::errorCategory(status_); }
    bool isTransportError() const noexcept { return pmac::isTransportError(status_); }
private:
    int32_t status_;
    uint64_t request_;
    uint32_t certainty_;
};

struct RequestOptions {
    uint32_t timeoutMs = 5000;
    bool infinite = false;
    bool readPool = false;
    size_t maxResponseBytes = 1024 * 1024;
    pmac_request_options native() const {
        pmac_request_options value{};
        value.struct_size = sizeof(value);
        value.abi_version = PMAC_ABI_VERSION;
        value.timeout_ms = timeoutMs;
        value.flags = (infinite ? PMAC_REQUEST_INFINITE : 0u) | (readPool ? PMAC_REQUEST_READ_POOL : 0u);
        value.max_response_bytes = maxResponseBytes;
        return value;
    }
};

struct ConnectOptions {
    std::string host;
    uint16_t port = 22;
    std::string username;
    std::string password;
    std::string privateKey;
    std::string publicKey;
    std::string passphrase;
    std::string knownHosts;
    std::vector<uint8_t> pinnedSha256;
    bool acceptAnyHostKey = false;
    bool tcpProbe = true;
    uint32_t tcpProbeTimeoutMs = 600;
    uint32_t keepaliveIntervalMs = 1000;
    uint32_t keepaliveMax = 3;
    uint8_t controlPoolSize = 1;
};

struct PoolProfile {
    uint32_t targetSize = 1, maxMembers = 4, minReady = 1, connectTimeoutMs = 60000;
    pmac_pool_profile native() const {
        return {sizeof(pmac_pool_profile), PMAC_ABI_VERSION, targetSize, maxMembers, minReady, connectTimeoutMs};
    }
};
struct RuntimeOwner {
        explicit RuntimeOwner(uint32_t workers) {
            pmac_runtime_config config{};
            config.struct_size = sizeof(config); config.abi_version = PMAC_ABI_VERSION; config.worker_threads = workers;
            check(pmac_runtime_create(&config, &id));
        }
        ~RuntimeOwner() { detail::runtimeCleanup(id, detail::nativeRuntimeApi()); }
        uint64_t id = 0;
        static void check(int32_t status) { if (status != PMAC_OK) throw Error(status, 0, 0, "PMAC runtime error"); }
    };

class Request {
public:
    Request() = default;
    explicit Request(uint64_t id, std::shared_ptr<RuntimeOwner> owner = {}) : id_(id), owner_(std::move(owner)) {}
    ~Request() { release(); }
    Request(const Request&) = delete;
    Request& operator=(const Request&) = delete;
    Request(Request&& other) noexcept : id_(other.id_), owner_(std::move(other.owner_)) { other.id_ = 0; }
    Request& operator=(Request&& other) noexcept {
        if (this != &other) { release(); id_ = other.id_; owner_ = std::move(other.owner_); other.id_ = 0; }
        return *this;
    }

    pmac_request_info waitNoThrow(uint32_t waitMs = 0xffffffffu) {
        pmac_request_info info{};
        const int32_t status = pmac_request_wait(id_, waitMs, &info);
        info.status = status;
        return info;
    }

    void wait() {
        for (;;) {
            auto info = waitNoThrow();
            if (info.status == PMAC_WAIT_PENDING) continue;
            check(info.status); return;
        }
    }
    std::pair<uint64_t, uint64_t> route() const {
        uint64_t member = 0, epoch = 0; check(pmac_request_get_route(id_, &member, &epoch)); return {member, epoch};
    }

    void cancel() { check(pmac_request_cancel(id_)); }

    std::vector<uint8_t> response() const { return copy(false); }
    std::vector<uint8_t> stderrBytes() const { return copy(true); }

    std::vector<uint8_t> match(const std::string& pattern, uint32_t group = 0) const {
        size_t required = 0;
        int32_t status = pmac_request_copy_match(id_, bytes(pattern), pattern.size(), group, nullptr, 0, &required);
        if (status != PMAC_BUFFER_TOO_SMALL && status != PMAC_OK) { throwStatus(status); }
        std::vector<uint8_t> output(required);
        status = pmac_request_copy_match(id_, bytes(pattern), pattern.size(), group, output.data(), output.size(), &required);
        check(status);
        return output;
    }

    uint64_t id() const noexcept { return id_; }
    void release() noexcept { if (id_) { (void)pmac_request_release(id_); id_ = 0; } owner_.reset(); }

private:
    std::vector<uint8_t> copy(bool stderrOutput) const {
        size_t required = 0;
        int32_t status = stderrOutput ? pmac_request_copy_stderr(id_, nullptr, 0, &required)
                                      : pmac_request_copy_response(id_, nullptr, 0, &required);
        if (status != PMAC_BUFFER_TOO_SMALL && status != PMAC_OK) { throwStatus(status); }
        std::vector<uint8_t> output(required);
        status = stderrOutput ? pmac_request_copy_stderr(id_, output.data(), output.size(), &required)
                              : pmac_request_copy_response(id_, output.data(), output.size(), &required);
        check(status);
        return output;
    }

    static const uint8_t* bytes(const std::string& value) { return reinterpret_cast<const uint8_t*>(value.data()); }
    void check(int32_t status) const { if (status != PMAC_OK) throwStatus(status); }
    [[noreturn]] void throwStatus(int32_t status) const {
        pmac_error_info info{}; size_t required = 0;
        (void)pmac_request_copy_error(id_, &info, nullptr, 0, &required);
        std::vector<char> message(required);
        const int32_t copied = pmac_request_copy_error(id_, &info, message.data(), message.size(), &required);
        throw Error(status, id_, info.certainty, copied == PMAC_OK ? std::string(message.begin(), message.end()) : "PMAC request failed with status " + std::to_string(status));
    }
    uint64_t id_ = 0;
    std::shared_ptr<RuntimeOwner> owner_;
};

struct GatherConfig {
    uint32_t period = 1;
    std::vector<std::string> addresses;
    std::string localIp;
    uint16_t port = 0;
    uint32_t maxSamples = 2048, batchRows = 128, queueBatches = 32, timeoutMs = 5000;
};
// Borrowed view valid only for the duration of the callback; values are row-major.
struct GatherBatch {
    const double* data;
    size_t rows, items;
    uint64_t firstSample;
    double at(size_t row, size_t item) const {
        if (row >= rows || item >= items) throw std::out_of_range("Gather sample index");
        return data[row * items + item];
    }
    std::vector<double> copy() const { return {data, data + rows * items}; }
};
class Gather {
public:
    using OnData = std::function<void(const GatherBatch&)>;
    using OnError = std::function<void(const Error&)>;
    Gather() = default;
    ~Gather() { release(); }
    Gather(const Gather&) = delete;
    Gather& operator=(const Gather&) = delete;
    Gather(Gather&& other) noexcept
        : id_(std::exchange(other.id_, 0)), context_(std::move(other.context_)), owner_(std::move(other.owner_)) {}
    Gather& operator=(Gather&& other) noexcept {
        if (this != &other) { release(); id_ = std::exchange(other.id_, 0); context_ = std::move(other.context_); owner_ = std::move(other.owner_); }
        return *this;
    }
    // Use requestStop inside callbacks. stop is a barrier and must run outside callbacks.
    void requestStop() { check(pmac_gather_request_stop(id_)); }
    void stop() {
        const auto status = pmac_gather_stop(id_);
        if (status == PMAC_INVALID_ARGUMENT) { check(status); }
        if (context_ && context_->exception) std::rethrow_exception(context_->exception);
        check(status);
    }
private:
    friend class Client;
    struct Context { OnData data; OnError error; std::exception_ptr exception; };
    static int32_t dispatch(void* user, int32_t status, const double* data, size_t rows,
                            size_t items, uint64_t first, const uint8_t* message, size_t length) noexcept {
        auto& c = *static_cast<Context*>(user);
        try {
            if (status == PMAC_OK) c.data({data, rows, items, first});
            else if (c.error) c.error(Error(status, 0, 0, std::string(reinterpret_cast<const char*>(message), length)));
            return 0;
        } catch (...) { c.exception = std::current_exception(); return 1; }
    }
    Gather(uint64_t client, std::shared_ptr<RuntimeOwner> owner, const GatherConfig& config, OnData onData, OnError onError)
        : context_(new Context{std::move(onData), std::move(onError), {}}), owner_(std::move(owner)) {
        if (!context_->data) throw std::invalid_argument("Gather data callback is required");
        if (config.addresses.empty() || config.addresses.size() > 128) throw std::invalid_argument("Gather needs 1..=128 addresses");
        std::vector<pmac_bytes> addresses;
        for (const auto& a : config.addresses) addresses.push_back({reinterpret_cast<const uint8_t*>(a.data()), a.size()});
        pmac_gather_config c{};
        c.struct_size = sizeof(c); c.abi_version = PMAC_ABI_VERSION;
        c.period = config.period; c.items = static_cast<uint32_t>(addresses.size()); c.addresses = addresses.data();
        c.local_ip = {reinterpret_cast<const uint8_t*>(config.localIp.data()), config.localIp.size()};
        c.port = config.port; c.max_samples = config.maxSamples; c.batch_rows = config.batchRows;
        c.queue_batches = config.queueBatches; c.timeout_ms = config.timeoutMs;
        check(pmac_gather_start(client, &c, dispatch, context_.get(), &id_));
    }
    static void check(int32_t status) {
        if (status == PMAC_OK) return;
        pmac_error_info info{}; size_t required = 0;
        (void)pmac_copy_last_error(&info, nullptr, 0, &required);
        std::vector<char> message(required);
        const auto copied = pmac_copy_last_error(&info, message.data(), message.size(), &required);
        throw Error(status, 0, info.certainty, copied == PMAC_OK ? std::string(message.begin(), message.end()) : "Gather operation failed");
    }
    void release() noexcept {
        if (id_) { (void)pmac_gather_release(id_); id_ = 0; }
    }
    uint64_t id_ = 0;
    std::unique_ptr<Context> context_;
    std::shared_ptr<RuntimeOwner> owner_;
};
class Runtime;

class Client {
public:
    Client() = default;
    ~Client() { reset(); }
    Client(const Client&) = delete;
    Client& operator=(const Client&) = delete;
    Client(Client&& other) noexcept { *this = std::move(other); }
    Client& operator=(Client&& other) noexcept {
        if (this != &other) { reset(); id_ = other.id_; runtime_ = std::move(other.runtime_); other.id_ = 0; }
        return *this;
    }

    Gather startGather(const GatherConfig& config, Gather::OnData onData, Gather::OnError onError = {}) {
        return Gather(id_, runtime_, config, std::move(onData), std::move(onError));
    }
    Request connect(const ConnectOptions& options) {
        NativeConnect native(options);
        return createRequest(pmac_connect_submit(id_, &native.value, &requestSlot()));
    }

    Request preparePoolAsync(const ConnectOptions& options, const PoolProfile& profile = {}) {
        NativeConnect native(options); auto p = profile.native();
        return createRequest(pmac_pool_prepare_async(id_, &native.value, &p, &requestSlot()));
    }
    Request resizePoolAsync(const PoolProfile& profile) {
        auto p = profile.native(); return createRequest(pmac_pool_resize_async(id_, &p, &requestSlot()));
    }
    Request repairPoolAsync(uint64_t member = 0, const RequestOptions& options = {}) {
        auto o = options.native(); return createRequest(pmac_pool_repair_async(id_, member, &o, &requestSlot()));
    }
    Request drainPoolAsync(uint64_t member = 0, const RequestOptions& options = {}) {
        auto o = options.native(); return createRequest(pmac_pool_drain_async(id_, member, &o, &requestSlot()));
    }
    Request repairMaintenanceAsync(const RequestOptions& options = {}) {
        auto o = options.native(); return createRequest(pmac_pool_repair_maintenance_async(id_, &o, &requestSlot()));
    }
    pmac_pool_state poolState() const { pmac_pool_state s{}; check(pmac_pool_get_state(id_, &s)); return s; }
    std::vector<pmac_pool_member> poolMembers() const {
        for (;;) {
            size_t required = 0; int status = pmac_pool_get_members(id_, nullptr, 0, &required);
            if (status != PMAC_OK && status != PMAC_BUFFER_TOO_SMALL) check(status);
            std::vector<pmac_pool_member> result(required);
            status = pmac_pool_get_members(id_, result.data(), result.size(), &required);
            if (status == PMAC_BUFFER_TOO_SMALL) continue;
            check(status); result.resize(required); return result;
        }
    }
    void confirmRecovery(uint64_t revision, uint32_t flags) { check(pmac_pool_confirm_recovery(id_, revision, flags)); }
    void connectSync(const ConnectOptions& options) { Request request = connect(options); request.wait(); }
    Request commandAsync(const std::string& command, const RequestOptions& options = {}) {
        auto native = options.native();
        return createRequest(pmac_command_submit(id_, byteData(command), command.size(), &native, &requestSlot()));
    }
    std::vector<uint8_t> command(const std::string& command, const RequestOptions& options = {}) {
        Request request = commandAsync(command, options); request.wait(); return request.response();
    }
    std::string commandText(const std::string& command, const RequestOptions& options = {}) {
        auto bytes = this->command(command, options); return std::string(bytes.begin(), bytes.end());
    }
    Request getResponseAsync(const std::string& command, const RequestOptions& options = {}) {
        auto native = options.native();
        return createRequest(pmac_get_response_async(id_, byteData(command), command.size(), &native, &requestSlot()));
    }
    std::vector<uint8_t> getResponse(const std::string& command, const std::string& pattern = "", const RequestOptions& options = {}) {
        Request request = getResponseAsync(command, options);
        request.wait();
        return pattern.empty() ? request.response() : request.match(pattern);
    }
    Request dartConnectAsync(const RequestOptions& options = {}) {
        auto native = options.native();
        return createRequest(pmac_dart_connect_async(id_, &native, &requestSlot()));
    }
    void dartConnect(const RequestOptions& options = {}) {
        Request request = dartConnectAsync(options); request.wait();
    }
    Request dartDisconnectAsync(const RequestOptions& options = {}) {
        auto native = options.native();
        return createRequest(pmac_dart_disconnect_async(id_, &native, &requestSlot()));
    }
    void dartDisconnect(const RequestOptions& options = {}) {
        Request request = dartDisconnectAsync(options); request.wait();
    }
    Request dartGetAsync(uint32_t command, uint32_t address, size_t length, const RequestOptions& options = {}) {
        auto native = options.native();
        return createRequest(pmac_dart_get_async(id_, command, address, length, &native, &requestSlot()));
    }
    std::vector<uint8_t> dartGet(uint32_t command, uint32_t address, size_t length, const RequestOptions& options = {}) {
        Request request = dartGetAsync(command, address, length, options);
        request.wait(); return request.response();
    }
    Request dartSetAsync(uint32_t command, uint32_t address, const std::vector<uint8_t>& data, const RequestOptions& options = {}) {
        auto native = options.native();
        return createRequest(pmac_dart_set_async(id_, command, address, data.data(), data.size(), &native, &requestSlot()));
    }
    void dartSet(uint32_t command, uint32_t address, const std::vector<uint8_t>& data, const RequestOptions& options = {}) {
        Request request = dartSetAsync(command, address, data, options); request.wait();
    }
    Request shellAsync(const std::string& command, bool persistent = true, const RequestOptions& options = {}) {
        auto native = options.native();
        return createRequest(pmac_shell_submit(id_, byteData(command), command.size(), persistent ? 1u : 0u, &native, &requestSlot()));
    }
    std::vector<uint8_t> shell(const std::string& command, bool persistent = true, const RequestOptions& options = {}) {
        Request request = shellAsync(command, persistent, options); request.wait(); return request.response();
    }

    void upload(const std::string& local, const std::string& remote, const RequestOptions& options = {}) {
        auto native = options.native();
        Request request = createRequest(pmac_upload_submit(id_, byteData(local), local.size(), byteData(remote), remote.size(), &native, &requestSlot()));
        request.wait();
    }
    void download(const std::string& remote, const std::string& local, const RequestOptions& options = {}) {
        auto native = options.native();
        Request request = createRequest(pmac_download_submit(id_, byteData(remote), remote.size(), byteData(local), local.size(), &native, &requestSlot()));
        request.wait();
    }
    void uploadBuffer(const std::string& local, const std::string& remote, bool cleanup = false, const RequestOptions& options = {}) {
        auto native = options.native();
        Request request = createRequest(pmac_buffer_upload_submit(id_, byteData(local), local.size(), byteData(remote), remote.size(), cleanup ? 1u : 0u, &native, &requestSlot()));
        request.wait();
    }
    bool fileExists(const std::string& remote, const RequestOptions& options = {}) {
        auto native = options.native();
        Request request = createRequest(pmac_file_exists_submit(id_, byteData(remote), remote.size(), &native, &requestSlot()));
        request.wait(); auto response = request.response();
        if (response.size() != 1 || (response[0] != '0' && response[0] != '1'))
            throw Error(PMAC_PARSE_ERROR, request.id(), 2u, "file existence response must be exactly 0 or 1");
        return response[0] == '1';
    }
    // PMAC_OK distinguishes a confirmed missing file (exists=false) from failure.
    // On failure exists is unchanged; diagnostics can be classified by statusName/category.
    int32_t fileExistsNoThrow(const std::string& remote, bool& exists, const RequestOptions& options = {}) noexcept {
        const pmac_request_options native = options.native();
        const detail::FileExistsApi api = detail::nativeFileExistsApi();
        return detail::fileExistsNoThrowCore(id_, byteData(remote), remote.size(), &native, exists, api);
    }
    void removeFile(const std::string& remote, const RequestOptions& options = {}) {
        auto native = options.native();
        Request request = createRequest(pmac_file_remove_submit(id_, byteData(remote), remote.size(), &native, &requestSlot()));
        request.wait();
    }

    pmac_state_snapshot state() const {
        pmac_state_snapshot value{};
        check(pmac_client_get_state(id_, &value));
        return value;
    }
    bool isConnected() const { return state().state == 4u; }
    void close(uint32_t waitMs = 1000) { if (id_) check(pmac_client_close(id_, 0, waitMs)); }
    int32_t reset() noexcept {
        if (!id_) { runtime_.reset(); return PMAC_OK; }
        int32_t status = pmac_client_close(id_, 0, 1000);
        if (status != PMAC_OK) return status;
        status = pmac_client_release(id_);
        if (status == PMAC_OK) { id_ = 0; runtime_.reset(); }
        return status;
    }
    static std::vector<uint64_t> pendingRecovery() {
        for (;;) {
            size_t required = 0;
            int32_t status = pmac_client_recovery_ids(nullptr, 0, &required);
            if (status != PMAC_OK && status != PMAC_BUFFER_TOO_SMALL) return {};
            std::vector<uint64_t> ids(required);
            status = pmac_client_recovery_ids(ids.empty() ? nullptr : ids.data(), ids.size(), &required);
            if (status == PMAC_BUFFER_TOO_SMALL) continue;
            if (status != PMAC_OK) return {};
            ids.resize(required);
            return ids;
        }
    }

    void abort(const RequestOptions& o = {}) { check(pmac_abort(id_, native(o))); }
    void coordEnable(int32_t n, const RequestOptions& o = {}) { check(pmac_coord_enable(id_, n, native(o))); }
    void coordExecProg(int32_t n, int32_t p, const RequestOptions& o = {}) { check(pmac_coord_exec_prog(id_, n, p, native(o))); }
    void coordHold(int32_t n, const RequestOptions& o = {}) { check(pmac_coord_hold(id_, n, native(o))); }
    void coordRun(int32_t n, const RequestOptions& o = {}) { check(pmac_coord_run(id_, n, native(o))); }
    void plcEnable(int32_t n, const RequestOptions& o = {}) { check(pmac_plc_enable(id_, n, native(o))); }
    void plcDisable(int32_t n, const RequestOptions& o = {}) { check(pmac_plc_disable(id_, n, native(o))); }
    void motorKill(int32_t n, const RequestOptions& o = {}) { check(pmac_motor_kill(id_, n, native(o))); }
    void motorEnable(int32_t n, const RequestOptions& o = {}) { check(pmac_motor_enable(id_, n, native(o))); }
    void motorJogStop(int32_t n, const RequestOptions& o = {}) { check(pmac_motor_jog_stop(id_, n, native(o))); }
    void motorAbsJog(int32_t n, double v, const RequestOptions& o = {}) { check(pmac_motor_abs_jog(id_, n, v, native(o))); }
    void motorIncJog(int32_t n, double v, const RequestOptions& o = {}) { check(pmac_motor_inc_jog(id_, n, v, native(o))); }
    uint64_t coordStatus(int32_t n, const RequestOptions& o = {}) { uint64_t v = 0; check(pmac_coord_status(id_, n, native(o), &v)); return v; }
    uint64_t motorStatus(int32_t n, const RequestOptions& o = {}) { uint64_t v = 0; check(pmac_motor_status(id_, n, native(o), &v)); return v; }
    double motorPosition(int32_t n, const RequestOptions& o = {}) { double v = 0; check(pmac_motor_position(id_, n, native(o), &v)); return v; }
    double motorDesiredPosition(int32_t n, const RequestOptions& o = {}) { double v = 0; check(pmac_motor_desired_position(id_, n, native(o), &v)); return v; }
    double motorVelocity(int32_t n, const RequestOptions& o = {}) { double v = 0; check(pmac_motor_velocity(id_, n, native(o), &v)); return v; }
    double motorPositionError(int32_t n, const RequestOptions& o = {}) { double v = 0; check(pmac_motor_position_error(id_, n, native(o), &v)); return v; }
    uint32_t servoErrorCount(const RequestOptions& o = {}) { uint32_t v = 0; check(pmac_servo_error_count(id_, native(o), &v)); return v; }
    uint32_t phaseErrorCount(const RequestOptions& o = {}) { uint32_t v = 0; check(pmac_phase_error_count(id_, native(o), &v)); return v; }
    uint32_t servoCount(const RequestOptions& o = {}) { uint32_t v = 0; check(pmac_servo_count(id_, native(o), &v)); return v; }
    double phaseTime(const RequestOptions& o = {}) { double v = 0; check(pmac_phase_time(id_, native(o), &v)); return v; }
    double servoTime(const RequestOptions& o = {}) { double v = 0; check(pmac_servo_time(id_, native(o), &v)); return v; }
    bool abortAll(const RequestOptions& o = {}) { return booleanQuery(pmac_abort_all_flag, o); }
    bool plcRunning(int32_t n, const RequestOptions& o = {}) { uint32_t v = 0; check(pmac_plc_running(id_, n, native(o), &v)); return v != 0; }
    bool coordProgRunning(int32_t n, const RequestOptions& o = {}) { uint32_t v = 0; check(pmac_coord_prog_running(id_, n, native(o), &v)); return v != 0; }
    bool motorClosedLoop(int32_t n, const RequestOptions& o = {}) { uint32_t v = 0; check(pmac_motor_closed_loop(id_, n, native(o), &v)); return v != 0; }
    bool coordMotorEnabled(int32_t n, const RequestOptions& o = {}) { uint32_t v = 0; check(pmac_coord_motor_enabled(id_, n, native(o), &v)); return v != 0; }

private:
    using Core = RuntimeOwner;

    struct NativeConnect {
        explicit NativeConnect(const ConnectOptions& source) : source(source) {
            value.struct_size = sizeof(value); value.abi_version = PMAC_ABI_VERSION;
            value.port = source.port; value.control_pool_size = source.controlPoolSize;
            value.flags = (source.tcpProbe ? PMAC_CONNECT_TCP_PROBE : 0u) | (source.acceptAnyHostKey ? PMAC_CONNECT_ACCEPT_ANY_HOST_KEY : 0u);
            value.tcp_probe_timeout_ms = source.tcpProbeTimeoutMs; value.keepalive_interval_ms = source.keepaliveIntervalMs; value.keepalive_max = source.keepaliveMax;
            set(value.host, source.host); set(value.username, source.username); set(value.password, source.password);
            set(value.private_key, source.privateKey); set(value.public_key, source.publicKey); set(value.passphrase, source.passphrase); set(value.known_hosts, source.knownHosts);
            if (!source.pinnedSha256.empty()) { value.pinned_sha256 = { source.pinnedSha256.data(), source.pinnedSha256.size() }; }
        }
        static void set(pmac_bytes& target, const std::string& text) { target = { byteData(text), text.size() }; }
        const ConnectOptions& source;
        pmac_connect_options value{};
    };

    explicit Client(uint64_t id, std::shared_ptr<Core> runtime) : id_(id), runtime_(std::move(runtime)) {}
    Request createRequest(int32_t status) {
        if (status != PMAC_OK) { requestSlot() = 0; check(status); }
        return Request(std::exchange(requestSlot(), 0), runtime_);
    }
    bool booleanQuery(int32_t (*function)(uint64_t, const pmac_request_options*, uint32_t*), const RequestOptions& o) {
        uint32_t v = 0; check(function(id_, native(o), &v)); return v != 0;
    }
    static const uint8_t* byteData(const std::string& value) { return reinterpret_cast<const uint8_t*>(value.data()); }
    static const pmac_request_options* native(const RequestOptions& value) {
        thread_local pmac_request_options result{};
        result = value.native();
        return &result;
    }
    static uint64_t& requestSlot() { thread_local uint64_t id = 0; return id; }
    static void check(int32_t status) {
        if (status == PMAC_OK) return;
#ifdef PMAC_ENABLE_LAST_ERROR_DETAILS
        pmac_error_info info{}; size_t required = 0;
        int copied = pmac_copy_last_error(&info, nullptr, 0, &required);
        if ((copied == PMAC_OK || copied == PMAC_BUFFER_TOO_SMALL) && info.status == status) {
            std::vector<char> message(required);
            if (required == 0 || pmac_copy_last_error(&info, message.data(), message.size(), &required) == PMAC_OK)
                throw Error(status, info.request_id, info.certainty, std::string(message.begin(), message.end()));
        }
#endif
        throw Error(status, 0, 1u, "PMAC operation failed with status " + std::to_string(status));
    }

    uint64_t id_ = 0;
    std::shared_ptr<Core> runtime_;
    friend class Runtime;
};

class Runtime {
public:
    explicit Runtime(uint32_t workers = 2) : core_(std::make_shared<Client::Core>(workers)) {}
    Client createClient(uint32_t maxRequests = 64, size_t maxRetainedBytes = 80 * 1024 * 1024, uint32_t maxFileTasks = 1) {
        pmac_client_config config{};
        config.struct_size = sizeof(config); config.abi_version = PMAC_ABI_VERSION;
        config.max_requests = maxRequests; config.max_retained_bytes = maxRetainedBytes; config.max_file_tasks = maxFileTasks;
        uint64_t id = 0; Client::Core::check(pmac_client_create(core_->id, &config, &id)); return Client(id, core_);
    }
    static const char* version() { return pmac_version_string(); }
    static uint32_t versionNumber() { return pmac_version_number(); }
    static uint32_t abiVersion() { return pmac_abi_version(); }
    static pmac_runtime_info runtimeInfo() { pmac_runtime_info value{}; Client::Core::check(pmac_get_runtime_info(&value)); return value; }
    int32_t shutdownNoThrow(uint32_t waitMs = 1000) noexcept { return pmac_runtime_shutdown(core_->id, waitMs); }
private:
    std::shared_ptr<Client::Core> core_;
};

} // namespace pmac
#endif
