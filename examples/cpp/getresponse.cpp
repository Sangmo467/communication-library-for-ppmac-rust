#include "pmac.hpp"
#include <cstdlib>
#include <iostream>
#include <stdexcept>
#include <string>

static std::string requiredEnv(const char* name) {
    const char* value = std::getenv(name);
    if (!value || !*value) throw std::runtime_error(std::string("Set ") + name);
    return value;
}
// C++17 完整示例：连接和命令保持在同一调用线程，以保留 GPASCII 通道亲和。
// --check 不访问设备；可用于确认 DLL/so 的架构、ABI 和依赖均可加载。
int main(int argc, char** argv) {
    if (argc == 2 && std::string(argv[1]) == "--check") {
        pmac_runtime_info info = {};
        if (pmac_get_runtime_info(&info) != PMAC_OK || info.abi_major != PMAC_ABI_MAJOR ||
            info.pointer_width != sizeof(void*) * 8u) return 1;
        std::cout << "PMAC " << pmac_version_string() << ", " << info.pointer_width << "-bit\n";
        return 0;
    }
    if (argc > 3 || (argc > 1 && std::string(argv[1]) == "--help")) {
        std::cout << "Usage: getresponse_cpp [command [pattern]]\nDefault command: Sys.ServoTime\n"
                     "Set PMAC_HOST, PMAC_KNOWN_HOSTS and authentication environment variables.\n";
        return argc > 3 ? 2 : 0;
    }
    try {
        // Runtime 拥有后台工作线程；声明在 Client 之前，保证其最后析构。
        pmac::Runtime runtime;
        auto client = runtime.createClient();
        pmac::ConnectOptions connection;
        connection.host = requiredEnv("PMAC_HOST");
        connection.knownHosts = requiredEnv("PMAC_KNOWN_HOSTS");
        connection.username = std::getenv("PMAC_USER") ? std::getenv("PMAC_USER") : "root";
        if (const char* password = std::getenv("PMAC_PASSWORD")) connection.password = password;
        if (const char* key = std::getenv("PMAC_PRIVATE_KEY")) connection.privateKey = key;
        if (const char* passphrase = std::getenv("PMAC_PASSPHRASE")) connection.passphrase = passphrase;
        // known_hosts 校验不可省略；密码/私钥只从环境变量读取。
        client.connectSync(connection);
        const std::string command = argc > 1 ? argv[1] : "Sys.ServoTime";
        const std::string pattern = argc > 2 ? argv[2] : "";
        pmac::RequestOptions options;
        options.timeoutMs = 5000;
        // 仅执行一次命令并复制结果；不要因通信超时自动重放写操作。
        const auto response = client.getResponse(command, pattern, options);
        if (!response.empty()) std::cout.write(reinterpret_cast<const char*>(response.data()), response.size());
        std::cout << '\n';
        // 响应已复制到 vector，随后按 Client、Runtime 的顺序释放。
        return 0;
    } catch (const pmac::Error& error) {
        std::cerr << "PMAC status=" << error.status() << " certainty=" << error.certainty()
                  << " request=" << error.requestId() << ": " << error.what() << '\n';
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
    }
    return 1;
}