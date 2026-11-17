"""离线验证 Python 绑定和预编译库，直接运行此文件，无需 pip 安装或控制器。"""
import os
from pathlib import Path
import sys

# SDK 结构保持固定；把本例复制到应用后也可改为 pip install ./bindings/python。
root = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(root / "bindings" / "python"))
from pmac import Runtime

# 显式使用绝对路径，避免 PATH 中旧版本 DLL/so 被意外加载。
library = root / "native" / ("pmac.dll" if os.name == "nt" else ("libpmac.dylib" if sys.platform == "darwin" else "libpmac.so"))
with Runtime(library_path=str(library)) as runtime:
    print(f"PMAC {runtime.native_version}, ABI={runtime.native_abi_version}")
    # Client 先释放，Runtime 后释放；离线例程仅检查本地资源生命周期。
    with runtime.create_client() as client:
        print(f"initial state={client.state().state}")
