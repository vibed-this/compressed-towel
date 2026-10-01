# -*- coding: utf-8 -*-
"""依赖安装示例。

输出 **必须 MUST**是 UTF-8（本文件即 UTF-8 编码，且 launcher.toml 里
[env] 保持 PYTHONUTF8=1 / PYTHONIOENCODING=utf-8；启动器不做 GBK 猜测）。
子进程 stdin 为 null，只做展示，不可交互（不要调用 input()）。
"""

import subprocess
import sys


def main() -> int:
    # **推荐 RECOMMENDED**用 "{sys.executable} -m pip" 口吻安装依赖，
    # 保证用的是 hooks 经 ${RUNTIME} 传入的运行时，而不是 PATH 里的 python。
    print("[install] installing requirements...")
    subprocess.check_call(
        [sys.executable, "-m", "pip", "install", "-r", "app/requirements.txt"]
    )
    print("[install] OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
