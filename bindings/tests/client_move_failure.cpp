#define PMAC_STATIC
#include "pmac.hpp"
#include <cassert>
#include <vector>
#include <type_traits>
static uint64_t next_client = 100;
static bool fail_close;
static std::vector<uint64_t> closed;
extern "C" {
int32_t pmac_runtime_create(const pmac_runtime_config*, uint64_t* id) { *id=1; return PMAC_OK; }
int32_t pmac_runtime_cleanup(uint64_t) { return PMAC_OK; }
int32_t pmac_client_create(uint64_t,const pmac_client_config*,uint64_t* id) { *id=next_client++; return PMAC_OK; }
int32_t pmac_client_close(uint64_t id,uint32_t,uint32_t) { closed.push_back(id); return fail_close ? PMAC_SHUTDOWN_PENDING : PMAC_OK; }
int32_t pmac_client_release(uint64_t) {return PMAC_OK;}
}
int main() {
 static_assert(std::is_nothrow_move_constructible<pmac::Client>::value, "move construction stays noexcept");
 pmac::Runtime runtime;
 auto a=runtime.createClient(); auto b=runtime.createClient();
 fail_close=true; bool threw=false;
 try { a=std::move(b); } catch(const pmac::Error& e) {threw=true; assert(e.status()==PMAC_SHUTDOWN_PENDING);}
 assert(threw);
 fail_close=false; closed.clear();
 assert(a.reset()==PMAC_OK); assert(b.reset()==PMAC_OK);
 assert((closed==std::vector<uint64_t>{100,101}));
 auto c=runtime.createClient(); auto d=runtime.createClient();
 closed.clear(); c=std::move(d);
 assert((closed==std::vector<uint64_t>{102}));
 assert(c.reset()==PMAC_OK); assert(d.reset()==PMAC_OK);
 assert((closed==std::vector<uint64_t>{102,103}));
}