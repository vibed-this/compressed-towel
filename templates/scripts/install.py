# -*- coding: utf-8 -*-
"""依赖安装示例（Python 模式）。

输出 **必须 MUST**是 UTF-8（本文件即 UTF-8 编码，且 launcher.toml 里
[env] 保持 PYTHONUTF8=1 / PYTHONIOENCODING=utf-8；启动器不做 GBK 猜测）。
子进程 stdin 为 null，只做展示，不可交互（不要调用 input()）。

Python 模式下业务单包已由启动器在供给阶段经 uv 装好
（`uv pip install --only-binary :all:`，只装远端 wheel），
本脚本只做业务后处理（如数据目录初始化、导入自检）。
需要额外依赖时，把它们声明进业务 wheel 的 Requires-Dist，
不要在这里再调 pip（uv 是唯一的包安装器，且无 pip 回退）。
"""


def main() -> int:
    print("[install] package already installed by launcher, verifying...")
    try:
        import myapp  # 按真实包名改
    except ImportError as exc:
        print(f"[install] FAILED: {exc}")
        return 1
    print("[install] OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
