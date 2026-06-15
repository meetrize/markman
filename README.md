# Markman

<div align="center">

![Markman](./assets/icon/markman-banner.png)

**Think in connections. Write in flow. A next-generation native Markdown workspace — built with Rust and GPUI.**

![Markman app screenshot](./assets/screenshots/markman.png)

[Editor Showcase](./assets/showcase/showcase.md)

[English](README.md) | [中文](docs/README.zh-CN.md)

[![Rust](https://img.shields.io/badge/Rust-2024-f74c00?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![GPUI](https://img.shields.io/badge/GUI-GPUI%200.2-4b7bec)](https://gpui.rs/)
[![Platforms](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-2ea44f)](#quick-start)
[![Portable](https://img.shields.io/badge/app-portable%20single%20binary-8b5cf6)](#more-capabilities)
[![Export](https://img.shields.io/badge/export-HTML%20%7C%20PDF-0ea5e9)](#more-capabilities)
[![Release](https://img.shields.io/badge/releases-GitHub-181717?logo=github)](https://github.com/meetrize/markman/releases)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

</div>

Markman is a block-native Markdown editor and personal knowledge workspace. No Electron. No WebView. No preview-pane sync loop — just a single, high-performance surface where structure, rendering, and editing stay in lockstep.

> **Note:** The display name is **Markman**. The executable and CLI command are `markman`. Older releases used `velotype`. User settings live under the Markman config directory (for example `~/Library/Application Support/Markman` on macOS) and migrate automatically from the legacy Velotype location on first launch.

## Highlights

### Knowledge graph — see how your notes connect

Turn a folder of Markdown into a living network. Markman scans `#tags` and `[[wiki links]]` across your workspace and renders an interactive force-directed graph — native GPUI, no embedded browser. Drag nodes, pan and zoom the canvas, click to jump to a note or filter by tag. Pop the graph into its own window when you need a bird's-eye view of your thinking.

### AI knowledge base — your workspace, understood

Your notes are the knowledge source. Markman feeds your entire workspace into AI conversations — summarize themes across files, compare notes, surface related content through tags and wiki links, and keep answers grounded in what you actually wrote. Context modes let you scope from a selection to the full document to the whole library.

### Sidebar AI chat — sustained, streaming dialogue

A dedicated **AI** tab sits alongside Files, Outline, Tags, and Graph. Hold multi-turn conversations with streaming responses, switch context on the fly (selection, full document, workspace, code block), pin selections into the thread, and start fresh whenever you need a clean slate. Selection toolbar actions and the pop-up AI dialog remain for quick in-place edits.

### True preview editing — WYSIWYG without the split brain

Most editors force a choice: raw source *or* a read-only preview. Markman merges them. Edit directly in rendered mode — headings, lists, tables, code, math, and more — while a block tree keeps parsing and display perfectly aligned. Toggle to Markdown source with line numbers whenever you want the raw text. One model, two views, zero sync lag.

### Multi-column layout — write side by side

Break out of single-column flow with native column blocks. Arrange content in parallel columns — comparisons, kanban-style boards, summary + detail panels — right inside your Markdown. Columns render inline in the editor and export faithfully to HTML and PDF.

## More capabilities

### Editing & navigation

- **Block model** — Markdown as an editable block tree; structure and rendering stay unified.
- **Format toolbar** — Headings, bold, italic, lists, tasks, quotes, links, images, tables, and more.
- **Rich navigation** — Word/block movement, cross-block selection, double-click word select, configurable shortcuts.
- **Document tools** — In-document search, workspace-wide search, quick-open, auto-save, context menus.

### Workspace

- **Folder workspace** — Open a directory, browse files in the side drawer, switch notes instantly.
- **Outline panel** — Jump through headings and block structure.
- **Tags panel** — Browse and filter notes by `#tag` with occurrence previews.

### Markdown & content

- **Full syntax support** — Headings, lists, task lists, quotes, callouts, tables, footnotes, reference links, images, comment blocks.
- **Code blocks** — Tree-sitter highlighting, line numbers, folding, language picker, run-in-terminal with output panel.
- **Mermaid** — Render diagrams in-editor; insert from built-in templates.
- **Math** — Superscript/subscript editing; RaTeX-based formula rendering.
- **Safe HTML** — Controlled native HTML where supported.

### Export & customization

- **HTML & PDF export** — Theme-aware CSS for HTML; PDF via local Chromium with the same pipeline.
- **Themes** — Import JSONC theme packs for colors, typography, spacing, menus, code highlighting, and layout.
- **Language packs** — Partial JSONC locale files with English fallback.
- **Global hotkey** — Toggle app visibility from anywhere on supported platforms.

### Platform

- **Native GPUI UI** — Pure Rust desktop rendering; no Electron, Tauri, or WebView shell.
- **Portable binary** — Single executable after build; Windows, Linux, and macOS.
- **macOS packaging** — `.app` bundle or PKG installer with optional CLI symlink.

## Quick Start

### 1. Download a release

Download the build for your platform from [GitHub Releases](https://github.com/meetrize/markman/releases).

#### Windows and Linux

1. Download the `.zip` or `.tar.gz` archive for your platform.
2. Extract the `markman` executable.
3. Run it directly.

#### macOS

**Option 1: `.app` bundle**

1. Download `markman-*.zip`.
2. Unzip to get `Markman.app`.
3. Drag to `/Applications` or run in place.

**Option 2: PKG installer (recommended)**

1. Download `markman-*.pkg`.
2. Run the installer — the app is placed in `/Applications`.
3. The `markman` CLI command is configured automatically.

> **CLI note:** PKG installs manage the `/usr/local/bin/markman` symlink via `postinstall` / `preuninstall` scripts. For `.app`-only installs, use **Help → Install CLI Command** inside the app. Moving or deleting the app bundle breaks the symlink.

### 2. Build from source

Prerequisites:

- Git
- Rust toolchain with **2024 edition** support
- Cargo
- Platform build dependencies required by GPUI

```bash
git clone https://github.com/meetrize/markman.git
cd markman
cargo build --release
```

The binary is at `target/release/markman`.

For development, testing, and packaging, see the [Development & Build guide](docs/development.md).

## Roadmap

Markman already covers most day-to-day Markdown authoring and knowledge-work needs. Planned work includes:

- [x] Performance for very large documents
- [x] Workspace mode, outline navigation, and knowledge graph
- [x] Sidebar AI chat with workspace context
- [ ] Built-in image hosting
- [ ] More complete IME behavior

## Theme & language customization

Theme and UI language are managed separately. Theme files can override global colors, fonts, sizes, menus, dialogs, table controls, image placeholders, code highlighting, and layout tokens. Missing fields inherit the base theme (`markman` or `markman-light` via `base_theme_id`; legacy `velotype` ids still work).

Language packs use the same partial-config approach — missing strings fall back to English.

Example files:

- [Custom theme JSONC](assets/custom-theme.example.jsonc)
- [Custom language JSONC](assets/custom-language.example.jsonc)

Import via **Theme → Add Theme Config** or **Language → Add Language Config** in the app. JSONC comments are accepted on import; saved configs are normalized to strict JSON.

## Architecture

| Layer | Responsibility |
| --- | --- |
| `editor` | Window state: view mode, save/close, undo, selection, source mapping, tree mutation, export, workspace, AI, knowledge graph, and file drop. |
| `components::block` | Editable block runtime, GPUI input, rendering, block events, image/table/code runtime state. |
| `components::markdown` | Markdown models and parse/serialize helpers for inline text, links, images, footnotes, tables, HTML, and code highlighting. |
| `config` | App behavior and theme configuration. |
| `export` | HTML and PDF export pipelines. |
| `theme` | Visual tokens, built-in defaults, custom theme import, global theme manager. |
| `i18n` | Built-in UI strings, language packs, locale matching, runtime language selection. |
| `net` | HTTP client for AI streaming and remote image loading. |

The editor uses a native block tree as its runtime model. Supported Markdown is converted to structured blocks on import and serialized back to canonical Markdown on save. Unstable syntax is preserved as raw source and remains visible and editable.

## Contributing

This repository moves quickly. When reporting parsing or rendering issues, please use the [issue template](https://github.com/meetrize/markman/issues/new/choose) so problems can be reproduced.

For code changes, prefer small patches on the `dev` branch and extend the existing parser/runtime model rather than replacing it wholesale.

## License

Markman is licensed under the [Apache License 2.0](LICENSE).

## Star History

[![Star History Chart](https://api.star-history.com/chart?repos=meetrize/markman&type=date&legend=top-left)](https://api.star-history.com/chart?repos=meetrize/markman&type=date&legend=top-left)
