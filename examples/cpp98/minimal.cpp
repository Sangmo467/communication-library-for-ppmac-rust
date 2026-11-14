#include "pmac_cpp98.hpp"

int main() {
    try {
        pmac98::Runtime runtime;
        pmac98::Client client(runtime);
        pmac::CoordStatus status(uint64_t(3) << 28);
        return status.feedHold() == 3 ? 0 : 2;
    } catch (const pmac98::Error&) {
        return 1;
    }
}
