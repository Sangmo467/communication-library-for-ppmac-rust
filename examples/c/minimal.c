#include "pmac.h"
#include <stdio.h>
#include <stddef.h>

_Static_assert(offsetof(pmac_connect_options, host) == 2u * sizeof(uint32_t), "host offset changed");
_Static_assert(offsetof(pmac_connect_options, port) == 2u * sizeof(uint32_t) + 8u * sizeof(pmac_bytes), "port offset changed");
_Static_assert(offsetof(pmac_request_info, request_id) == 2u * sizeof(uint32_t), "request ID offset changed");

int main(void) {
    pmac_runtime_info info;
    if (pmac_get_runtime_info(&info) != PMAC_OK) return 1;
    if (info.abi_major != PMAC_ABI_MAJOR || info.pointer_width != (uint32_t)(sizeof(void*) * 8u)) return 2;
    printf("pmac %s, ABI %u.%u, %u-bit\n", pmac_version_string(), info.abi_major, info.abi_minor, info.pointer_width);
    return 0;
}
