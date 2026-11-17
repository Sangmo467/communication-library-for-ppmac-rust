#define PMAC_STATIC
#include "pmac_cpp98.hpp"
#if __cplusplus >= 201703L
#include "pmac.hpp"
#endif
#include <cassert>
#include <cstring>
static uint32_t observed_timeout, observed_flags;
extern "C" {
int32_t pmac_runtime_create(const pmac_runtime_config*, uint64_t* id) { *id=1; return PMAC_OK; }
int32_t pmac_runtime_cleanup(uint64_t) { return PMAC_OK; }
int32_t pmac_client_create(uint64_t,const pmac_client_config*,uint64_t* id) { *id=2; return PMAC_OK; }
int32_t pmac_client_close(uint64_t,uint32_t,uint32_t) { return PMAC_OK; }
int32_t pmac_client_release(uint64_t) {return PMAC_OK;}
int32_t pmac_command_submit(uint64_t,const uint8_t*,size_t,const pmac_request_options* o,uint64_t* id) { observed_timeout=o->timeout_ms; observed_flags=o->flags; *id=3; return PMAC_OK; }
int32_t pmac_get_response_async(uint64_t c,const uint8_t* p,size_t n,const pmac_request_options* o,uint64_t* id) { return pmac_command_submit(c,p,n,o,id); }
int32_t pmac_request_wait(uint64_t id,uint32_t,pmac_request_info* i) { std::memset(i,0,sizeof(*i)); i->request_id=id; return PMAC_OK; }
int32_t pmac_request_release(uint64_t) { return PMAC_OK; }
int32_t pmac_request_copy_response(uint64_t,uint8_t*,size_t,size_t* required) { *required=0; return PMAC_OK; }
int32_t pmac_request_copy_stderr(uint64_t,uint8_t*,size_t,size_t* required) { *required=0; return PMAC_OK; }
int32_t pmac_request_copy_match(uint64_t,const uint8_t*,size_t,uint32_t,uint8_t*,size_t,size_t* required) { *required=0; return PMAC_OK; }
int32_t pmac_copy_last_error(pmac_error_info*,char*,size_t,size_t*) { return PMAC_INVALID_HANDLE; }
int32_t pmac_request_copy_error(uint64_t,pmac_error_info*,char*,size_t,size_t*) { return PMAC_INVALID_HANDLE; }
}
static void expect(uint32_t timeout) { assert(observed_timeout==timeout); }
int main() {
 pmac98::Runtime oldRuntime; pmac98::Client old(oldRuntime);
 old.command("$$$"); expect(30000);
 old.command(" \t\r\n$$$***\v\f"); expect(30000);
 old.getResponse("$$$***"); expect(30000);
 old.getResponse("$$$", "(.*)"); expect(30000);
 old.command("Sys.ServoCount"); expect(5000);
 old.command("$$$ Sys.ServoCount"); expect(5000);
 pmac98::Options legacy; old.command("$$$",legacy); expect(5000);
 legacy.timeout_ms=1234; old.getResponse("$$$","",legacy); expect(1234);
 legacy.timeout_ms=0; legacy.infinite=true; old.command("$$$***",legacy); expect(0); assert(observed_flags&PMAC_REQUEST_INFINITE);
#if __cplusplus >= 201703L
 pmac::Runtime runtime; pmac::Client c=runtime.createClient();
 c.command("$$$"); expect(30000);
 c.commandText("$$$***"); expect(30000);
 c.commandAsync("$$$"); expect(30000);
 c.getResponseAsync("$$$***"); expect(30000);
 c.getResponse(" \t\r\n$$$***\v\f"); expect(30000);
 c.getResponse("$$$", "(.*)"); expect(30000);
 c.commandText("Sys.ServoCount"); expect(5000);
 c.command("$$$\nSys.ServoCount"); expect(5000);
 pmac::RequestOptions options; c.command("$$$",options); expect(5000);
 c.getResponseAsync("$$$***",options); expect(5000);
 options.timeoutMs=1234; c.getResponse("$$$","",options); expect(1234);
 options.timeoutMs=0; options.infinite=true; c.commandText("$$$***",options); expect(0); assert(observed_flags&PMAC_REQUEST_INFINITE);
#endif
}