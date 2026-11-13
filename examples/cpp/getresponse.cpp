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
int main(int argc, char** argv) {
    if (argc > 3 || (argc > 1 && std::string(argv[1]) == "--help")) {
        std::cout << "Usage: getresponse_cpp [command [pattern]]\nDefault command: Sys.ServoTime\n"
                     "Set PMAC_HOST, PMAC_KNOWN_HOSTS and authentication environment variables.\n";
        return argc > 3 ? 2 : 0;
    }
    try {
        pmac::Runtime runtime;
        auto client = runtime.createClient();
        pmac::ConnectOptions connection;
        connection.host = requiredEnv("PMAC_HOST");
        connection.knownHosts = requiredEnv("PMAC_KNOWN_HOSTS");
        connection.username = std::getenv("PMAC_USER") ? std::getenv("PMAC_USER") : "root";
        if (const char* password = std::getenv("PMAC_PASSWORD")) connection.password = password;
        if (const char* key = std::getenv("PMAC_PRIVATE_KEY")) connection.privateKey = key;
        if (const char* passphrase = std::getenv("PMAC_PASSPHRASE")) connection.passphrase = passphrase;
        client.connectSync(connection);
        const std::string command = argc > 1 ? argv[1] : "Sys.ServoTime";
        const std::string pattern = argc > 2 ? argv[2] : "";
        pmac::RequestOptions options;
        options.timeoutMs = 5000;
        const auto response = client.getResponse(command, pattern, options);
        if (!response.empty()) std::cout.write(reinterpret_cast<const char*>(response.data()), response.size());
        std::cout << '\n';
        // client and runtime are destroyed after the completed request's result is copied.
        return 0;
    } catch (const pmac::Error& error) {
        std::cerr << "PMAC status=" << error.status() << " certainty=" << error.certainty()
                  << " request=" << error.requestId() << ": " << error.what() << '\n';
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
    }
    return 1;
}
