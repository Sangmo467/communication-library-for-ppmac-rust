#define PMAC_STATIC
#include "pmac_cpp98.hpp"
#if __cplusplus >= 201703L
#include "pmac.hpp"
#endif
#include <cassert>
#include <string>
extern "C" void test_set_error_case(int);
int main() {
    const char* expected[] = {"unclosed character class", "error #31", "failed"};
    for (int mode = 0; mode < 3; ++mode) {
        test_set_error_case(mode);
        const int status = mode == 0 ? PMAC_PARSE_ERROR : PMAC_REMOTE_ERROR;
        const pmac98::Error legacy(status, "match", 7, 2);
        assert(std::string(legacy.what()).find(expected[mode]) != std::string::npos);
#if __cplusplus >= 201703L
        pmac::Request request(7);
        bool failed = false;
        try { request.match("["); }
        catch (const pmac::Error& error) {
            failed = true;
            assert(error.status() == status);
            if (mode != 2) assert(error.certainty() == 2);
            assert(std::string(error.what()).find(expected[mode]) != std::string::npos);
        }
        assert(failed);
#endif
    }
}