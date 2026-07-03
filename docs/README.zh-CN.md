# Markasdfdsafman

<div align="center">

![Markman](../assets/icon/markman-banner.png)

**以关联思考，以心流写作 —— 基于 Rust 与 GPUI 的新一代原生 Markdown 知识工作区。**

![Markman 应用截图](../assets/screenshots/markman.png)

[编辑器展示](../assets/showcase/showcase.md)

[English](../README.md) | [中文](README.zh-CN.md)

[![Rust](https://img.shields.io/badge/Rust-2024-f74c00?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![GPUI](https://img.shields.io/badge/GUI-GPUI%200.2-4b7bec)](https://gpui.rs/)
[![Platforms](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-2ea44f)](#快速开始)
[![Portable](https://img.shields.io/badge/app-portable%20single%20binary-8b5cf6)](#更多能力)
[![Export](https://img.shields.io/badge/export-HTML%20%7C%20PDF-0ea5e9)](#更多能力)
[![Release](https://img.shields.io/badge/releases-GitHub-181717?logo=github)](https://github.com/meetrize/markman/releases)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](../LICENSE)

</div>

Markman 是一款块级原生 Markdown 编辑器与个人知识工作区。没有 Electron，没有 WebView，也没有预览窗同步循环 —— 只有一个高性能编辑表面，让结构、渲染与编辑始终同频。

> **说明：** 应用显示名称为 **Markman**。可执行文件与 CLI 命令为 `markman`。旧版 release 可能仍为 `velotype`。用户配置保存在 Markman 配置目录（macOS 上例如 `~/Library/Application Support/Markman`），首次启动时会自动从旧版 Velotype 目录迁移。

## 核心亮点

### 知识图谱 —— 看见笔记之间的连接

将整个 Markdown 工作区变成一张活的关系网。Markman 扫描全部 `#标签` 与 `[[Wiki 链接]]`，以力导向布局渲染交互式图谱 —— 纯 GPUI 原生绘制，无需内嵌浏览器。拖拽节点、平移缩放画布、点击跳转笔记或按标签过滤；需要纵览全局时，还可将图谱弹出为独立窗口。

### AI 知识库 —— 让 AI 真正读懂你的工作区

你的笔记就是知识源。Markman 将工作区内容注入 AI 对话 —— 跨文件总结主题、对比笔记、通过标签与 Wiki 链接发现关联，让回答始终锚定在你写下的原文之上。上下文模式支持从选区、全文到整个知识库灵活切换。

### 侧栏 AI 对话 —— 持续、流式的智能协作

工作区左侧 **AI** 面板与文件、大纲、标签、图谱并列。支持多轮对话与流式回复，随时切换上下文（选区 / 全文 / 工作区 / 代码块），可将选区固定到对话线程，一键开启新对话。选区工具栏与弹出式 AI 对话框则保留快捷就地编辑能力。

### 预览即编辑 —— 真正的所见即所得

多数编辑器让你在「源码」与「只读预览」之间二选一。Markman 将二者合一：在渲染模式下直接编辑标题、列表、表格、代码、公式等，块树模型让解析与显示天然对齐；需要时一键切换到带行号的 Markdown 源码。同一模型、两种视图、零同步延迟。

### 多栏分栏布局 —— 并排书写，突破单列

用原生分栏块打破单列排版限制。在 Markdown 内并排组织内容 —— 对比说明、看板式任务板、摘要与详情并列 —— 分栏在编辑器内实时渲染，导出 HTML 与 PDF 时同样忠实还原。

## 更多能力

### 编辑与导航

- **Block 模型** — Markdown 表达为可编辑块树，结构与渲染天然统一。
- **格式工具栏** — 标题、加粗、斜体、列表、待办、引用、链接、图片、表格等一键设置。
- **丰富导航** — 按词/按块移动、跨块选择、双击选词，以及可配置的快捷键。
- **文档工具** — 文内搜索、工作区全局搜索、快速打开、自动保存、上下文菜单。

### 工作区

- **文件夹工作区** — 打开目录，侧栏浏览文件，瞬间切换笔记。
- **大纲面板** — 按标题与块结构跳转文档。
- **标签面板** — 按 `#标签` 浏览与过滤笔记，附带出现位置预览。

### Markdown 与内容

- **完整语法支持** — 标题、列表、任务列表、引用、callout、表格、脚注、reference 链接、图片、注释块等。
- **代码块** — Tree-sitter 语法高亮、行号、折叠、语言选择、终端运行与输出面板。
- **Mermaid** — 编辑器内渲染图表，支持从内置模板插入。
- **数学公式** — 上标/下标行内编辑；启用时使用 RaTeX 渲染。
- **安全 HTML** — 在支持范围内受控处理原生 HTML。

### 导出与自定义

- **HTML 与 PDF 导出** — HTML 映射当前主题 CSS；PDF 通过本地 Chromium 复用同一套主题化管线。
- **主题** — 导入 JSONC 主题包，覆盖颜色、字体、间距、菜单、代码高亮与布局 token。
- **语言包** — 局部 JSONC 语言文件，缺失文案回退英文。
- **全局热键** — 在支持的平台上随时切换应用可见性。

### 平台

- **原生 GPUI 界面** — 纯 Rust 桌面渲染，不依赖 Electron、Tauri 或 WebView。
- **便携单文件** — 编译后为单个可执行文件，支持 Windows、Linux、macOS。
- **macOS 打包** — 提供 `.app` 或 PKG 安装包，可选配置 CLI 符号链接。

## 快速开始

### 1. 下载 release

从 [GitHub Releases](https://github.com/meetrize/markman/releases) 下载对应平台构建。

#### Windows 与 Linux

1. 下载对应平台的 `.zip` 或 `.tar.gz`。
2. 解压得到 `markman` 可执行文件。
3. 直接运行。

#### macOS

**方式 1：`.app` 应用包**

1. 下载 `markman-*.zip`。
2. 解压得到 `Markman.app`。
3. 拖到 `/Applications` 或直接运行。

**方式 2：PKG 安装包（推荐）**

1. 下载 `markman-*.pkg`。
2. 运行安装程序，应用安装到 `/Applications`。
3. 自动配置 `markman` CLI 命令。

> **CLI 说明：** PKG 安装通过 `postinstall` / `preuninstall` 脚本管理 `/usr/local/bin/markman` 符号链接。仅使用 `.app` 时，可在应用内 **帮助 → 安装 CLI 命令** 手动配置。移动或删除应用包会导致符号链接失效。

### 2. 从源码构建

前置需求：

- Git
- 支持 **Rust 2024 edition** 的工具链
- Cargo
- GPUI 所需的平台原生构建依赖

```bash
git clone https://github.com/meetrize/markman.git
cd markman
cargo build --release
```

构建产物位于 `target/release/markman`。

日常开发、测试与打包说明见 [开发与构建指南](development.zh-CN.md)。

## Roadmap

Markman 已覆盖大多数日常 Markdown 写作与知识工作需求。仍在计划中的能力包括：

- [x] 超大文档的性能优化
- [x] 工作区模式、大纲导航与知识图谱
- [x] 侧栏 AI 对话与工作区上下文
- [ ] 内置图床
- [ ] 更完善的 IME 行为

## 自定义主题与语言

视觉主题与界面语言分开管理。主题文件可覆盖全局颜色、字体、尺寸、菜单、弹窗、表格控件、图片占位、代码高亮与布局 token。缺失字段会继承基准主题（通过 `base_theme_id` 指定 `markman` 或 `markman-light`；旧版 `velotype` id 仍兼容）。

语言包同样采用局部配置策略，缺失文案回退英文。

示例文件：

- [自定义主题 JSONC](../assets/custom-theme.example.jsonc)
- [自定义语言 JSONC](../assets/custom-language.example.jsonc)

在应用内通过 **主题 → 添加主题配置** 或 **语言 → 添加语言配置** 导入。导入时接受 JSONC 注释；保存后会规范化为严格 JSON。

## 架构

| 模块 | 职责 |
| --- | --- |
| `editor` | 窗口级状态：视图模式、保存/关闭、撤销、选择、source mapping、树变更、导出、工作区、AI、知识图谱、文件拖拽。 |
| `components::block` | 可编辑块运行时、GPUI 输入、块渲染、块事件、图片/表格/代码块运行时状态。 |
| `components::markdown` | Markdown 数据模型，以及 inline、link、image、footnote、table、HTML、code highlight 的解析与序列化。 |
| `config` | 应用行为与主题配置。 |
| `export` | HTML 与 PDF 导出管线。 |
| `theme` | 视觉 token、内置默认值、自定义主题导入、全局主题管理。 |
| `i18n` | 内置 UI 文案、语言包、locale 匹配、运行时语言切换。 |
| `net` | AI 流式请求与远程图片加载所需的 HTTP client。 |

编辑器以原生 block tree 为运行时模型。导入时将稳定支持的 Markdown 转为结构化块；保存时再序列化为规范化 Markdown。对当前运行时不稳定支持的语法，会保留原始源码并保持可见、可编辑。

## 贡献

仓库仍在快速迭代。报告解析或渲染问题时，请使用 [issue 模板](https://github.com/meetrize/markman/issues/new/choose) 以便复现。

提交代码时建议在 `dev` 分支以小补丁形式扩展现有 parser/runtime 模型，而非整体替换。

## 许可证

Markman 使用 [Apache License 2.0](../LICENSE)。

## Star History

![Star History Chart](https://api.star-history.com/chart?repos=meetrize/markman&type=date&legend=top-left)