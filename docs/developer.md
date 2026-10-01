# MyDistLauncher 开发者文档

面向分发应用的开发者：说明分发目录怎么摆、`launcher.toml` 怎么写、hooks 脚本怎么写。
最终用户只经 UI 改白名单项，不直接读本文。

本文的情态词遵循项目行文格式：**必须 MUST**、**禁止 MUST NOT**、
**推荐 RECOMMENDED**、**可以 MAY** 整体加粗。

## 1. 目录结构

分发目录（开发者随应用分发）：

```text
dist/
  launcher.exe              # 启动器本体
  launcher.toml             # 开发者编写（从 launcher.template.toml 复制）
  python/                   # 运行时目录（示例为 portable Python），开发者预置
    python.exe
    Scripts/
  app/                      # 用户代码，start 默认跑 app/main.py
    main.py
    requirements.txt
  assets/
    logo.png                # splash 展示的 logo
  scripts/                  # hooks 脚本；缺失且与内建同名时用 exe 内建版本（见第 8 节）
```

最终用户机器上的数据目录（启动器自动创建）：

```text
%APPDATA%/<AppName>/        # <AppName> 取自 [app].name
  user.toml                 # 最终用户经 UI 修改的白名单项
  logs/                     # 运行日志
```

说明：

- `launcher.toml` 由开发者编写并随应用分发，最终用户不直接编辑它。
- `user.toml` 只包含白名单叶子 key，由 UI 写入，结构与优先级见第 7 节。
- `templates/` 下的脚本是内建进 exe 的模板来源；分发目录里的 `scripts/`
  与模板同名即视为"外部定制"，优先使用（见第 8 节）。实际 hooks 路径
  以 `launcher.toml` 为准，**可以 MAY**放在分发目录内任意位置，
  但**禁止 MUST NOT**超出分发目录。

## 2. launcher.toml 全字段参考

完整可运行模板见仓库根目录 `launcher.template.toml`，复制为分发目录下的
`launcher.toml` 后修改。下面按节说明每个字段。

### 2.1 [app]

| 字段    | 类型   | 默认值 | 说明                                            |
| ------- | ------ | ------ | ----------------------------------------------- |
| name    | string | 无     | 应用名：显示在 splash 上，同时决定 `%APPDATA%` 子目录名 |
| version | string | 无     | 应用版本：`check` 展示用，不参与启动逻辑        |

`[app].name` **必须 MUST**配置，启动器用它确定用户数据目录。

### 2.2 [splash]

| 字段   | 类型   | 默认值 | 说明                                              |
| ------ | ------ | ------ | ------------------------------------------------- |
| logo   | string | 无     | logo 路径（相对 exe 目录）；缺配或文件缺失时只显示文字 |

splash 为固定尺寸小窗口（约 480×360），全程语义：第 6 节。

### 2.3 [env]

`[env]` 为注入全部 hooks 子进程的环境变量表，键值均为字符串：

```toml
[env]
RUNTIME = "python/python.exe"
PYTHONUTF8 = "1"
PYTHONIOENCODING = "utf-8"
MYAPP_MODE = "prod"

[env.PATH]
prepend = ["python/Scripts", "app/bin"]
```

- `RUNTIME` 为运行时路径示例，可换任意运行时，不限 python；hooks 里以 `${RUNTIME}` 引用。
- `${VAR}` 只支持大括号形式，分两阶段单遍展开：阶段一解 `[env]` 自身的值，`${VAR}` 只认父进程环境变量，兄弟项不可见，不支持互引；阶段二解 hook argv（含 `argv[0]`）、`PATH.prepend`、`logo`，`${VAR}` 先查阶段一结果（配置优先），未命中回落父进程环境，再未命中展空。
- `[env]` 展开后的全表注入全部 hooks 子进程的环境。
- `PYTHONUTF8=1` **推荐 RECOMMENDED**保留，使 Python 子进程默认使用 UTF-8（见第 5 节）。
- `[env.PATH].prepend` 插到 `PATH` 最前面；数组元素**可以 MAY**是相对 exe 目录或绝对路径。
- 自定义变量（如 `MYAPP_MODE`）**可以 MAY**在 hooks 的 argv 里以 `${VAR}` 引用（见第 3 节）。

### 2.4 [hooks]

四段全部为命令数组（argv 形式），示例：

```toml
[hooks]
check_update = ["scripts/check_update.bat"]
install = ["${RUNTIME}", "scripts/install.py"]
start = ["${RUNTIME}", "app/main.py"]
end = ["scripts/cleanup.bat"]
```

- 命令只允许数组写法，**禁止 MUST NOT**写成字符串。
- `.bat` 由启动器自动经 `cmd /C` 运行，`.ps1` 经 `powershell` 运行，
  配置里直接写脚本路径即可，不要自己再包一层 `cmd` / `powershell`。
- 子进程 `stdin` 为 null：只展示输出，不交互；脚本里**禁止 MUST NOT**等待用户输入。
- `start` 缺配报错；其余三段缺配直接跳过。失败语义见第 6 节。

### 2.5 [launcher]

| 字段          | 类型         | 默认值 | 说明                                            |
| ------------- | ------------ | ------ | ----------------------------------------------- |
| log_level     | string       | `info` | `trace \| debug \| info \| warn \| error`       |
| user_editable | [string]     | `[]`   | 白名单：仅叶子 key 的点分路径，仅对 `user.toml` 生效 |

示例：

```toml
[launcher]
log_level = "info"
user_editable = ["launcher.log_level"]
```

`user_editable` 只接受叶子 key（如 `launcher.log_level`），**禁止 MUST NOT**写成
中间表（如 `app`）。不在白名单的 key 即使出现在 `user.toml` 里也会被拒绝启动。

## 3. 命令写法与 ${RUNTIME} / ${VAR} 规则

1. 命令**必须 MUST**是 TOML 数组，第一项为可执行体，其余为参数，例如
   `["${RUNTIME}", "app/main.py", "--serve"]`。
2. `${VAR}` 只支持大括号形式，不支持 `$VAR` 等其他形式。展开分两阶段单遍完成：
   阶段一解 `[env]` 自身的值，`${VAR}` 只认父进程环境变量，兄弟项不可见，
   `[env]` 条目之间不支持互引；阶段二解 hook argv（含 `argv[0]`）、`[env.PATH].prepend`、
   `logo`，`${VAR}` 先查阶段一结果（配置优先），未命中回落父进程环境，再未命中展空。
   `[env]` 展开后的全表注入全部 hooks 子进程的环境。
3. `argv[0]` 解析顺序：
   - 先做阶段二 `${VAR}` 展开；
   - 相对路径：相对 exe 所在目录解析（见第 4 节），且解析结果**必须 MUST**落在分发目录内；
   - 绝对路径：直接使用；
   - 其他（不含路径分隔符）：按 `PATH` 查找（含 `[env.PATH]` 生效后的 `PATH`）。
4. 参数按字面传递给子进程；hooks 子进程的工作目录固定为 exe 所在目录，
   因此相对路径参数（如 `app/main.py`）相对 exe 目录生效。

## 4. confinement 说明

- 所有相对路径（hooks 的 `argv[0]` 与参数、`logo`、
  `[env.PATH]` 条目）都相对 exe 所在目录（分发目录）解析。
  经 `${RUNTIME}` 等引用展开后落到 argv 里的路径同样受本节约束。
- 解析结果**必须 MUST**落在分发目录内；含 `..` 越界、绝对路径指向目录外、
  符号链接解析后落到目录外的，一律拒绝执行并报错。
- `check` 子命令同样执行该检查，提前暴露越界配置，而不是等到双击时才失败。
- 如有把日志、缓存写到目录外的需求，不要写相对路径，应使用绝对路径
  或 `%APPDATA%` 下的用户数据目录。

## 5. UTF-8 约定

脚本输出**必须 MUST**是 UTF-8，启动器不做 GBK 猜测、不做转码。
`[env]` 中的 `PYTHONUTF8=1` 只管 Python 进程；bat / ps1 **必须 MUST**各自落实。

### 5.1 bat：首行固定

```bat
chcp 65001 >nul
@echo off
REM 输出 **必须 MUST**是 UTF-8（首行 chcp 65001 不可删除）。
echo [check_update] OK
exit /b 0
```

- 第一行**必须 MUST**是 `chcp 65001 >nul`，前面不加注释与空行。
- 文件本身保存为 UTF-8（带不带 BOM 均可）；`echo` 的中文按 UTF-8 解码展示。

### 5.2 ps1：显式设置编码

```powershell
# 输出 **必须 MUST**是 UTF-8（启动器不做 GBK 猜测）。
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()
Write-Output "[check_update] OK"
exit 0
```

- 文件保存为 UTF-8；避免使用 `Write-Host` 输出非 ASCII 内容。
- 启动器经 `powershell` 运行 `.ps1`，不要在命令里再包一层。

### 5.3 py：声明加环境变量

```python
# -*- coding: utf-8 -*-
"""输出 **必须 MUST**是 UTF-8；配合 [env] PYTHONUTF8=1 使用。"""
import subprocess
import sys

subprocess.check_call([sys.executable, "-m", "pip", "install", "-r", "app/requirements.txt"])
```

- 文件保存为 UTF-8；**推荐 RECOMMENDED**使用 `"{sys.executable} -m pip"` 口吻，
  即 `[sys.executable, "-m", "pip", ...]`，保证命中 `RUNTIME` 指向的运行时。
- 不要调用 `input()`；不要依赖控制台代码页；不要输出 GBK 字节。

## 6. 启动流程、失败与退出码语义

正常流程：

```text
双击 launcher.exe
  → splash 显示（应用名 + logo）
  → 顺序运行 [hooks] check_update → install
  → start：splash 关闭，启动器 wait 常驻等待
  → 应用退出后重现 splash，运行 end
  → 退出，退出码透传 start 的退出码
```

规则：

1. `check_update`、`install`、`end` 缺配跳过；`start` 缺配直接报错退出。
2. `check_update` / `install` 任一段非零退出即中止：`start` 不再运行，
   `end` 也不再运行；启动器退出码为该段的退出码。
3. `start` 退出后（无论退出码是否为零）都会跑 `end`；`end` 成功时启动器
   退出码透传 `start` 的退出码，`end` 自身非零则退出码为 `end` 的码
  （`start` 的码保留在日志里）。
4. 所有 hooks 输出只展示不交互（`stdin=null`）；需要用户确认的流程
   **禁止 MUST NOT**放在 hooks 里阻塞等待。

## 7. 配置优先级、--config 与 check

优先级（从低到高）：

```text
内置默认 < launcher.toml < user.toml（仅白名单叶子 key）< MDL_* 环境变量
```

- `user.toml` 只接受 `[launcher].user_editable` 列出的叶子 key，
  非白名单 key 会导致启动报错。
- `MDL_*` 环境变量优先级最高，仅支持以下四个：
  `MDL_APP_NAME`、`MDL_SPLASH_LOGO`、`MDL_PYTHON_PATH`、`MDL_LAUNCHER_LOG_LEVEL`，
  例如 `MDL_LAUNCHER_LOG_LEVEL=debug` 覆盖 `launcher.log_level`。
  其中 `MDL_PYTHON_PATH` 已失效：它改写合并表中的 `python.path` 叶子，
  而表解析已删除 `[python]` 节，效果是被静默忽略；名字保留占位以不断旧脚本，
  不要再使用。另三个（`MDL_APP_NAME`、`MDL_SPLASH_LOGO`、`MDL_LAUNCHER_LOG_LEVEL`）有效。
- `--config <path>` **可以 MAY**指定 `launcher.toml` 的位置（默认取 exe 同目录），
  主要用于开发期调试；confinement 仍相对 exe 目录判定，不随 `--config` 改变。
- 缺省回退：默认位置的 `launcher.toml` 缺失时，用 exe 内建的模板快照代替
  （内容即 `launcher.template.toml`，见第 8 节），仍需按真实分发修改后才能跑通；
  `--config` 显式指定的文件缺失则依然报错（显式路径拼错应 loud 失败）。
- `check` 子命令校验并预演解析，不运行任何 hooks：

```text
launcher.exe check                         # 校验默认 launcher.toml
launcher.exe --config D:\app\launcher.toml check
```

`check` 覆盖：TOML 解析、字段类型、`start` 是否缺配、路径 confinement、
hooks 数组形状、`RUNTIME` 显示（取自合并后 `[env]`，无则显示未配置），以及
`logo` / 各 hook `argv[0]` 展开后程序文件是否存在（不存在即报错，运行时指错在此暴露）。

## 8. 内建资源与外部覆盖

构建期 `build.rs` 扫描 `templates/` 生成清单，`src/embedded.rs` 用 `include_str!`
编入 exe（零新依赖）。含两部分：`templates/` 下全部脚本（键为相对 `templates/`
的正斜杠路径，如 `scripts/check_update.bat`）与默认配置（`launcher.template.toml`
快照，见 `embedded::DEFAULT_LAUNCHER_TOML`）。

规则：

1. 外部优先：外部同名文件存在即用外部，内建只做缺省回退。
2. `logo` 与 `[env.PATH].prepend` 不参与回退；logo 缺失只显示文字，不报错。
3. 解压目录在系统临时目录下（`mdl-embedded-<应用>-<pid>`），只解压本轮实际
   引用的脚本；`end` 跑完后删除，`check` 退出前同样清理；crash 残留只清
   mtime 超过一天的，并发实例不受影响。

## 9. macOS .app 打包

`sh tools/macos/pack-app.sh`（可配 `APP_NAME` / `BUNDLE_ID`，版本取 `Cargo.toml`）
产出 `target/release/<AppName>.app`：二进制进 `Contents/MacOS/`，
`launcher.toml` 与 `scripts/` 进 `Contents/Resources/`，
`Info.plist` 由 `tools/macos/Info.plist.template` 渲染。

- 运行在 `.app` 内（`Name.app/Contents/MacOS/<bin>`）时资源根目录自动指向
  同包的 `Contents/Resources`；裸二进制行为不变。`--config` 仍可指向包外配置覆盖。
- 图标由下游提供：把 `.icns` 丢进 `Contents/Resources`，取消 `Info.plist` 里
  图标段的注释并改成实际文件名；无图标时不声明，双击照常可用。
- 只有本地 ad-hoc 签名（`codesign -s -`）；分发到别的机器各自右键打开过
  Gatekeeper。正式 Developer ID 签名与公证、`dmg` 均为后续事项。

## 10. pack（后续计划，未实现）

`pack` 用于把分发目录打成单文件安装包或压缩包，当前版本**尚未实现**，
**禁止 MUST NOT**在脚本或文档中假设它存在。

- 占位用法（未实现，不要使用）：`launcher.exe pack --out app-setup.exe`。
- 在 `pack` 落地前，分发方式为直接复制分发目录（含 `python/`）。
- 本节在 `pack` 实现后更新，在此之前以本段为准。

## 11. FAQ

### 路径缺失：启动直接报错说 start 缺配？

`[hooks].start` 缺配是致命错误。检查三处：`launcher.toml` 是否在 exe 同目录
（开发期是否漏了 `--config` 指向别处）、`start` 是否写成了字符串而不是数组、
`app/main.py` 相对 exe 目录是否存在。先跑 `launcher.exe check` 看校验输出。

### 路径缺失：${RUNTIME} 找不到运行时？

`${RUNTIME}` 取自合并后 `[env]` 的 `RUNTIME`。检查该值文件是否存在、
是否为相对 exe 目录的有效路径；用了裸命令名时检查 `[env.PATH].prepend`
是否包含其目录。`check` 会逐项报告解析结果。

### 乱码：中文输出变成问号或方块？

输出**必须 MUST**是 UTF-8，启动器不转码。按第 5 节逐项核对：
bat 首行是否为 `chcp 65001 >nul`、ps1 是否设置了 `OutputEncoding`、
py 是否配合 `PYTHONUTF8=1`、文件本身是否存成了 UTF-8（常见坑是 GBK 存盘）。

### 退出码：应用报错退出但 launcher 退出码是 0？

启动器退出码透传 `start` 的退出码。如果经过了多层包装（bat 再调 py），
确认每一层都透传了退出码：bat 用 `exit /b %ERRORLEVEL%` 结尾，
不要以裸 `exit /b 0` 吞掉错误；ps1 用 `exit $LASTEXITCODE`。
`end` 的退出码不覆盖透传值，问题一定出在 `start` 链路内部。
