/* 同一源文件分别使用 C++17 和 C++98 编译，验证两个头文件适配器。
 * Runtime 必须先创建、最后销毁；Client 只在其 Runtime 存活期间使用。
 * 本程序只创建本地资源，不读取设备状态，也不会发送控制命令。 */
#ifdef PMAC_EXAMPLE_CPP98
#include "pmac_cpp98.hpp"
#else
#include "pmac.hpp"
#endif
#include <iostream>
int main() {
    try {
        if (pmac_abi_version() / 1000 != PMAC_ABI_MAJOR) return 1;
#ifdef PMAC_EXAMPLE_CPP98
        pmac98::Options options;
        if (options.native().flags != 0u) return 3;
        options.passthrough = false;
        if (options.native().flags != PMAC_REQUEST_CLASSIFY) return 3;
        options.read_pool = true;
        if (options.native().flags != PMAC_REQUEST_READ_POOL) return 3;
        pmac98::Runtime runtime;
        pmac98::Client client(runtime);
#else
        // 默认透传；显式关闭后启用分类；只读池总是分类，不携带透传标志。
        pmac::RequestOptions options;
        if (options.native().flags != 0u) return 3;
        options.passthrough = false;
        if (options.native().flags != PMAC_REQUEST_CLASSIFY) return 3;
        options.readPool = true;
        if (options.native().flags != PMAC_REQUEST_READ_POOL) return 3;
        pmac::Runtime runtime;
        pmac::Client client = runtime.createClient();
#endif
        std::cout << "PMAC " << pmac_version_string() << ", " << sizeof(void*) * 8 << "-bit\n";
        // RAII 按与创建相反的顺序销毁对象；不要在析构之前卸载 DLL/so。
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 2;
    }
}
