/* C++17 连续 Gather：配置采样周期、变量地址和反向 TCP 端口。控制器需可访问本机端口；配置会修改设备全局 Gather 状态。batch 数据仅在回调期间有效，跨线程使用必须 batch.copy()。回调内请求停止用 requestStop，阻塞 stop 由主线程执行。本例需真实设备，不由离线 CTest 执行。 */
#include "pmac.hpp"
#include <atomic>
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

        pmac::GatherConfig config;
        config.period = 1;
        config.addresses = {"Motor[1].ActPos.a", "Motor[2].ActPos.a"};
        config.port = 18999; // Change this port, or use 0 for an available port.
        std::atomic<uint64_t> samples{0};
        auto gather = client.startGather(config,
            [&](const pmac::GatherBatch& batch) {
                // batch.at(row, 0/1) contains Motor[1/2].ActPos.a.
                // Copy with batch.copy() before handing values to another thread.
                samples.fetch_add(batch.rows, std::memory_order_relaxed);
            },
            [](const pmac::Error& error) {
                std::cerr << "Gather stopped: " << error.what() << '\n';
            });
        std::cout << "Gather running. Press Enter to stop.\n";
        std::cin.get();
        gather.stop();
        std::cout << "Received " << samples.load() << " sample rows.\n";
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
