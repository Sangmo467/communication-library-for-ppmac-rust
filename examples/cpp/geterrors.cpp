/* C++17 geterrors：设置周期与各类状态掩码，只在状态变化时接收输出。回调不是 UI 线程；交给其他线程时必须复制文本并使用有界队列。主线程按 Enter 后 stop 等待远端退出和回调屏障；monitor 必须先于 Client/Runtime 销毁。本例需真实设备，不由离线 CTest 执行。 */
#include "pmac.hpp"
#include <cstdlib>
#include <iostream>
#include <stdexcept>

static std::string requiredEnv(const char* name) {
    const char* value = std::getenv(name);
    if (!value || !*value) throw std::runtime_error(std::string("Set ") + name);
    return value;
}
int main() {
    try {
        pmac::Runtime runtime;
        auto client = runtime.createClient();
        pmac::ConnectOptions connection;
        connection.host = requiredEnv("PMAC_HOST");
        connection.username = "root";
        connection.knownHosts = requiredEnv("PMAC_KNOWN_HOSTS");
        if (const char* password = std::getenv("PMAC_PASSWORD")) connection.password = password;
        client.connectSync(connection);
        pmac::GetErrorsConfig config;
        config.periodSeconds = 0.01;
        config.date = true;
        config.globalMask = UINT64_C(0x7ff);
        config.coordMask = UINT64_C(0x180000000807ffc);
        config.motorMask = UINT64_C(0x8c007ffc);
        config.ringMask = UINT64_C(0x803e);
        auto monitor = client.startGetErrors(config,
            [](uint32_t kind, const std::string& text) {
                // SETTINGS: startup echo; ERROR: device status transitions;
                // STDERR: remote program diagnostics. This callback owns no UI thread.
                std::cout << '[' << kind << "] " << text << '\n';
                // Copy text into an owned queue entry before another thread uses it.
            },
            [](const pmac::Error& error) {
                std::cerr << "Monitor terminated: " << error.what() << '\n';
            });
        std::cout << "Monitoring. Press Enter to stop.\n";
        std::cin.get();
        monitor.stop(); // waits for the process cleanup and callback lifetime barrier
        // monitor destructor releases its ID before client/runtime destructors run.
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
