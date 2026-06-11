# 开发与构建

本文档说明 Velotype 的日常开发、构建、测试与打包流程。项目根目录下的 `scripts/` 提供了一组封装好的 shell 脚本，避免每次手动输入较长的 Cargo 命令。

[English](development.md) | [中文](development.zh-CN.md)

## 前置需求

- Git
- 支持 Rust 2024 edition 的 Rust toolchain
- Cargo
- GPUI 与系统工具链所需的平台原生构建依赖

## 运行模式概览

Velotype 是 GPUI 桌面应用，**没有内置 UI 热重载**。开发时有三种常用方式：

| 模式 | 脚本 | 说明 |
| --- | --- | --- |
| 开发运行 | `./scripts/dev.sh` | 等价于 `cargo run`，debug 构建、增量编译快；`Cargo.toml` 已对 gpui 等依赖做了 dev 优化 |
| 监听重启 | `./scripts/watch.sh` | 监听源码变更，自动重新编译并重启进程（需安装 `cargo-watch`） |
| 发布运行 | `./scripts/run.sh` | 运行 release 二进制；若产物不存在会先执行构建 |

> **注意：** `watch.sh` 的实现是「改代码 → 重新编译 → 重启进程」，不是 Web 前端那种无感热更新。每次重启会丢失当前未保存的编辑状态，开发时请注意保存。

## 脚本一览

```
scripts/
├── common.sh                  # 公共变量与工具函数（供其他脚本引用）
├── dev.sh                     # 开发模式运行
├── watch.sh                   # 文件变更监听 + 自动重启
├── build.sh                   # release 构建
├── run.sh                     # 运行 release 二进制
├── test.sh                    # 运行测试
├── check.sh                   # 快速编译检查
├── bench.sh                   # 运行 Criterion 基准测试
├── clean.sh                   # 清理 target/ 与 dist/
├── package.sh                 # 按平台打包
├── create_macos_app_dist.sh   # 创建 macOS .app 应用包
└── create_macos_pkg_dist.sh   # 创建 macOS PKG 安装包
```

首次使用前，请确保脚本具有可执行权限：

```bash
chmod +x scripts/*.sh
```

## 日常开发

### 启动开发版

```bash
./scripts/dev.sh
```

打开指定 Markdown 文件：

```bash
./scripts/dev.sh test.md
```

查看命令行帮助：

```bash
./scripts/dev.sh -- --help
```

### 监听源码变更（自动重启）

先安装 `cargo-watch`：

```bash
cargo install cargo-watch
```

然后运行：

```bash
./scripts/watch.sh
./scripts/watch.sh test.md
```

监听范围包括 `src/`、`assets/`、`resources/`、`build.rs` 和 `Cargo.toml`。

### 快速编译检查

不生成可执行文件，仅验证能否通过编译：

```bash
./scripts/check.sh
```

## 构建与运行

### Release 构建

```bash
./scripts/build.sh
```

等价于 `cargo build --release`。构建产物位于 `target/release/velotype`（Windows 为 `velotype.exe`）。

如需锁定依赖版本（与 CI 一致）：

```bash
./scripts/build.sh --locked
```

### 运行 Release 版本

```bash
./scripts/run.sh
./scripts/run.sh test.md
./scripts/run.sh --detach    # macOS：后台启动，不占用终端
```

若 release 二进制尚不存在，`run.sh` 会自动先执行构建。

也可以直接使用 Cargo：

```bash
cargo build --release
./target/release/velotype
```

## 测试与基准

### 运行测试

```bash
./scripts/test.sh
./scripts/test.sh editor::tests
```

### 运行基准测试

```bash
./scripts/bench.sh
./scripts/bench.sh render_loop
```

## 清理

删除 Cargo 构建产物和本地 `dist/` 目录：

```bash
./scripts/clean.sh
```

## 打包分发

`package.sh` 会根据当前操作系统自动选择打包方式，也可显式指定目标：

```bash
./scripts/package.sh                  # 自动检测平台
./scripts/package.sh macos-app      # macOS：创建 Velotype.app
./scripts/package.sh macos-pkg 0.5.7  # macOS：创建 PKG 安装包
./scripts/package.sh linux          # Linux：tar.gz 压缩包
./scripts/package.sh windows        # Windows：zip 压缩包
```

### macOS 分步打包

也可以分步使用原有脚本：

```bash
# 1. 构建 release 并创建 .app
./scripts/create_macos_app_dist.sh

# 2. 基于 .app 创建 PKG 安装包
./scripts/create_macos_pkg_dist.sh 0.5.7
```

产物输出在 `dist/` 目录下。

## 与 CI 的对应关系

GitHub Actions 工作流（`.github/workflows/build-release.yml`）在各平台执行 `cargo build --release --locked`，并按平台打包为 zip、tar.gz、.app 或 .pkg。本地发布前可用 `./scripts/build.sh --locked` 与 `./scripts/package.sh` 复现相近流程。

## 常见问题

**Q: 为什么 dev 模式也比纯 debug 流畅？**

`Cargo.toml` 的 `[profile.dev.package]` 将 gpui、文本渲染等重依赖在 dev 构建时也设为 `opt-level = 3`，在保持自身代码可调试的同时，减少框架层的性能开销。

**Q: 可以直接用 Cargo 命令吗？**

可以。脚本只是对常用 Cargo 命令的便捷封装，例如：

```bash
cargo run                          # 同 ./scripts/dev.sh
cargo build --release              # 同 ./scripts/build.sh
cargo test                         # 同 ./scripts/test.sh
cargo bench                        # 同 ./scripts/bench.sh
```

## 行内代码执行

渲染模式下，光标位于 `` `code` `` 行内代码 span 内时可执行 shell 命令：

| 操作 | 说明 |
| --- | --- |
| **Cmd+Enter**（macOS）/ **Ctrl+Enter**（其他） | 执行当前行内代码 |
| **运行按钮** | 光标或 hover 在行内代码上时，span 右侧显示播放图标 |

**默认行为：** 通过子进程在文档目录（或当前工作目录）执行命令，结果以 Popover 浮层展示 stdout/stderr、退出码与耗时。输出过长时自动截断。

**偏好设置（文件 → 偏好）：**

- **允许运行代码** — 关闭后执行前弹出禁用提示
- **行内代码在系统终端中执行** — 开启后在 macOS 用 Terminal.app 打开（`cd` 到文档目录后执行），浮层仅提示「已在系统终端中打开」；非 macOS 暂不支持

与围栏代码块执行的区别：行内代码使用紧凑 Popover，不在段落下方插入大块输出面板；两种执行互斥，启动其一会先停止另一个。

## 相关文档

- [行内代码执行 — AI 实施提示词](inline-code-run-implementation.zh-CN.md)：分步提示词，供 AI Agent 按序实现行内代码执行功能
