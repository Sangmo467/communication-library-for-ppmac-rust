#include "pmac.hpp"
#include <iostream>

int main() {
    try {
        pmac::Runtime runtime;
        pmac::Client client = runtime.createClient();
        std::cout << pmac::Runtime::version() << '\n';
        pmac::MotorStatus status(uint64_t(1) << 63);
        return status.triggerMove() ? 0 : 2;
    } catch (const pmac::Error& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
