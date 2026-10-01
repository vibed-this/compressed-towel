# AGENTS.md — MyDistLauncher 项目规范

本项目栈为 Rust + Slint（桌面空窗口启动器）。

规范来源：`/Users/zxk/Repos/agents-dot-md`（`0f5a923` 起的复制件，存于 `docs/specs/`，非引用；漂移时以用户提示词为准）。

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT", "RECOMMENDED", "MAY", and "OPTIONAL" in this document are to be interpreted as described in RFC 2119.
行文格式为“中文情态词 + 英文关键词”整体加粗（如 `**必须 MUST**`），**严禁 MUST NOT**只加粗英文关键词。

## 覆盖优先级

处理任何任务时，**必须 MUST**按以下顺序覆盖：

用户提示词要求 > 本文件要求 > 该语言官方默认规范（如 rustfmt / Clippy 默认）要求

## 规范缺失声明

该规范库 `languages/` 下无 Rust 规范：Rust 相关只遵循用户提示词 + Rust 官方默认（rustfmt / Clippy），
**严禁 MUST NOT**把该库 Python / Go / Java 等语言规范套用到 Rust 代码。

## 必读规范（`docs/specs/` 原样复制件）

- Git 操作（提交信息、提交粒度、操作边界、忽略与安全）：涉及提交、分支或任何 git 写操作前，**必须 MUST**先完整读取 `docs/specs/basis/Git.md` 并遵守；无 git 操作的纯代码任务**不建议 SHOULD NOT**预读它。
- 通用基础（注释、重构、流水线）：写代码任务开始前，**必须 MUST**先完整读取 `docs/specs/basis/Basis.md` 并遵守。

## 本项目约定

- 检查以官方工具为准：`cargo fmt --check`、**推荐 RECOMMENDED** `cargo clippy -- -D warnings`。
- Slint 取舍（见 `Cargo.toml`）：`backend-winit` + 纯 `renderer-software`（最小体积，无 OpenGL 依赖），
  `release` 用 `opt-level = "z" + lto + strip + panic = "abort"`。如需 GPU 特效 / `drop-shadow` /
  旋转缩放，改回 `renderer-femtovg` 需经用户确认（体积会明显回升）。
- 构建：`cargo build --release`；产物 `target/release/my_dist_launcher` **禁止 MUST NOT**入库（`target/` 已在 `.gitignore` 声明）。
