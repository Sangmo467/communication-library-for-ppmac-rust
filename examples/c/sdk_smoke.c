/* 离线最小例程：验证预编译库 ABI、创建及释放资源，不连接控制器。
 * 编译入口为 examples/CMakeLists.txt；Windows 必须使用与 DLL 相同位数的编译器。
 * 将这些调用复制到应用时，保留每一步返回码检查和逆序释放。 */
#include "pmac.h"
#include <stdio.h>
int main(void) {
    pmac_runtime_info info = {0};
    pmac_runtime_id runtime = 0;
    pmac_client_id client = 0;
    pmac_runtime_config rc = {0};
    pmac_client_config cc = {0};
    int32_t status = pmac_get_runtime_info(&info);
    if (status != PMAC_OK || info.abi_major != PMAC_ABI_MAJOR ||
        info.pointer_width != (uint32_t)(sizeof(void*) * 8u)) return 1;
    printf("PMAC %s, ABI %u.%u, %u-bit\n", pmac_version_string(), info.abi_major, info.abi_minor, info.pointer_width);
    /* 结构头说明布局版本；所有保留字段必须保持零。 */
    rc.struct_size = sizeof(rc); rc.abi_version = PMAC_ABI_VERSION; rc.worker_threads = 2;
    if (pmac_runtime_create(&rc, &runtime) != PMAC_OK) return 2;
    cc.struct_size = sizeof(cc); cc.abi_version = PMAC_ABI_VERSION;
    cc.max_requests = 64; cc.max_file_tasks = 1; cc.max_retained_bytes = 80u * 1024u * 1024u;
    status = pmac_client_create(runtime, &cc, &client);
    /* 即使创建 Client 失败，也必须关闭已经创建的 Runtime。 */
    if (status == PMAC_OK) {
        status = pmac_client_close(client, 0, 5000);
        if (status == PMAC_OK) status = pmac_client_release(client);
    }
    /* 关闭 Pending 时应保留 ID 并再次等待；嵌入式应用不可直接卸载动态库。 */
    if (pmac_runtime_shutdown(runtime, 5000) != PMAC_OK) return 3;
    if (pmac_runtime_release(runtime) != PMAC_OK) return 4;
    return status == PMAC_OK ? 0 : 5;
}
