# Wiki 链接 `[[路径/文件名]]` — 方案设计

本文档描述 Velotype / Markman 中 **Obsidian 风格 Wiki 链接** `[[路径/文件名]]` 的功能设计、与现有架构的映射，以及分阶段实施计划。可直接复制各 Phase 内容交给 AI Agent 执行。

[开发与构建](development.zh-CN.md) | [行内代码执行提示词示例](inline-code-run-implementation.zh-CN.md)

---

## 需求概述

用户在 Markdown 正文中输入 `[[路径/文件名]]` 语法时：

| 场景 | 期望行为 |
| --- | --- |
| **预览（渲染模式）** | 显示为可点击的超链接样式；双击或激活后打开本地文件 |
| **编辑（光标在标签内）** | 自动弹出文件选择菜单，列出当前工作区下的文件，按目录层级展示 |
| **输入过滤** | 支持在菜单中输入关键字，对路径进行**模糊匹配**（与 Cmd+P 快速打开一致） |

---

## 语法定义

### MVP（Phase 1）

最简形态，路径即显示文本：

```markdown
[[docs/README.zh-CN.md]]
[[src/components/block/runtime/mod.rs]]
```

- **源码存储**：`[[相对路径]]`
- **预览显示**：路径字符串（或文件名，见待决事项）
- **打开目标**：解析后的本地文件绝对路径

### 可选扩展（Phase 3 及以后）

| 语法 | 含义 |
| --- | --- |
| `[[path\|显示名]]` | 自定义链接文字 |
| `[[path#heading]]` | 跳转到目标文档内的标题锚点 |
| `[[path#heading\|显示名]]` | 组合形式 |

Phase 1 不实现扩展语法，但数据模型与解析器应预留 `|` 分隔符的扩展空间。

---

## 与现有架构的映射

项目 inline 链路已较完整，Wiki 链接主要是新增一种 `InlineLink` 变体，而非另起一套系统。

| 能力 | 现有模块 | Wiki 链接用法 |
| --- | --- | --- |
| 解析 | `src/components/markdown/inline/normalize.rs` | 在 `parse_inline_link` **之前**识别 `[[...]]` |
| 数据模型 | `src/components/markdown/inline/fragment.rs` — `InlineLink` | 新增 `WikiLink { path: String }` |
| 序列化 | `src/components/markdown/inline/mod.rs` — `serialize_markdown` | 复用现有 link run 序列化，补 variant 的 marker 方法 |
| 预览渲染 | `InlineRenderCache` + `InlineSpan.link` | 自动带链接样式，无需改渲染管线 |
| 聚焦编辑 / 投影 | `src/components/block/runtime/projection.rs` | 展开为 `[[` + 路径文本 + `]]` |
| 模糊搜索 | `src/editor/quick_file_open.rs` — `fuzzy_match_score` | 直接复用 |
| 目录树 | `src/editor/workspace.rs` — `WorkspaceTreeNode`、`scan_workspace_dir` | 复用扫描逻辑 |
| 打开文件 | `src/editor/workspace.rs` — `open_workspace_file` | 替代 `cx.open_url` |
| 链接点击 | `src/components/block/interactions.rs` → `BlockEvent::RequestOpenLink` | 区分本地路径与 URL |
| HTML 导出 | `src/export/html.rs` | 预处理或自定义事件输出 `<a href="...">` |

### Wiki 链接 vs 普通 inline 链接

普通链接 `[label](target)` 在投影编辑时展开为：

```text
[  +  label  +  ](  +  target  +  )
 ↑              ↑              ↑
 open_marker    标签文本        middle + LinkTargetText + close
```

Wiki 链接 `[[path]]` 的路径**就是** fragment 的 `text`，投影时只需展开分隔符：

```text
[[  +  path  +  ]]
 ↑           ↑
 open_marker  close_marker（无 middle、无 LinkTargetText）
```

对应 `InlineLink` 方法：

| 方法 | Wiki 链接返回值 |
| --- | --- |
| `open_marker()` | `"[["` |
| `close_marker()` | `"]]"` |
| `middle_marker()` | `None` |
| `editable_text()` | `None`（路径在 label fragment 中） |
| `is_source_preserving()` | `true` |
| `open_target()` | 解析后的绝对/相对路径 |
| `raw_target()` | 源码中的路径字符串 |

---

## 关键代码入口

### Link projection（已有，Wiki 链接可直接复用）

`src/components/block/runtime/projection.rs` 中，当 `expand_link == true` 时：

1. 插入 `OpeningDelimiter`（`link.open_marker()`）
2. 将 link run 的 fragment 文本作为 `StyledText`（可编辑的路径）
3. 若 `middle_marker()` 为 `Some` 则插入 `MiddleDelimiter`（Wiki 链接跳过）
4. 若 `editable_text()` 为 `Some` 则插入 `LinkTargetText`（Wiki 链接跳过）
5. 插入 `ClosingDelimiter`（`link.close_marker()`）

### 模糊匹配（已有，可直接复用）

`src/editor/quick_file_open.rs` 中的 `fuzzy_match_score`：

- 子序列匹配
- 连续匹配加分
- 分隔符边界（`/`、`_`、`-`、`.`）加分

### 工作区文件树（已有，可直接复用）

`src/editor/workspace.rs`：

- `scan_workspace_dir` — 递归扫描目录
- `WorkspaceTreeNode` — 树形节点
- `render_workspace_nodes` — 带缩进的层级渲染（侧栏文件树）

### 打开链接（需扩展）

当前流程：`BlockEvent::RequestOpenLink` → `request_open_link_prompt` → 用户确认 → `cx.open_url`。

Wiki 链接需增加分支：若 `open_target` 为工作区内的相对路径且文件存在 → 调用 `open_workspace_file`，跳过 URL 确认框（或仅对域外路径保留确认框）。

---

## 实施阶段

### Phase 1：解析 + 预览 + 打开（核心） ✅ 已实现

**目标**：`[[path]]` 可解析、预览态可点击、双击打开本地文件。

#### 1.1 扩展 `InlineLink`

文件：`src/components/markdown/inline/fragment.rs`、`link_image.rs`

```rust
/// Obsidian-style wiki link: `[[relative/path.md]]`
WikiLink { path: String },
```

实现上述 marker / hit / `is_source_preserving` 方法。

#### 1.2 解析器

新建 `src/components/markdown/inline/wiki_link.rs`（或并入 `link_image.rs`）：

- 在 `normalize.rs` 的 token 循环中，于 `parse_inline_link` **之前**调用 `parse_wiki_link`
- 检测 `[[`，读取至 `]]` 为止
- 路径内允许 `/`、`.`、`-`、`_`、空格（trim 后）等常见文件名字符
- 空路径 `[[ ]]` 不解析，保留原文
- 未闭合 `[[` 在 Phase 1 可保留为普通文本，Phase 2 再处理实时补全

#### 1.3 序列化

`src/components/markdown/inline/mod.rs` 的 `serialize_markdown` 已按 link run 处理 marker，补全 `WikiLink` 的 marker 方法即可自动生成 `[[path]]`。

#### 1.4 打开行为

- 在 `src/editor/events.rs` 或 `src/editor/render.rs` 中，解析 `open_target`：
  - 若为 `http://` / `https://` / `mailto:` 等 → 走现有 URL 流程
  - 若为相对路径 → 基于**工作区根目录** resolve（`effective_workspace_root().join(path)`）
  - 若文件存在 → `open_workspace_file(path)`
  - 若不存在 → 可选提示「文件不存在」

#### 1.5 HTML 导出

`src/export/html.rs`：在 pulldown-cmark 管线之前或之后，将 `[[...]]` 转为标准 Markdown 链接或自定义 HTML `<a>`。

#### 1.6 测试

- `src/components/markdown/inline/mod.rs` 或独立测试：解析、序列化 round-trip
- `src/components/block/runtime/tests.rs`：投影展开 `[[path]]`
- link hit-test：`open_target` / `prompt_target` 正确

**验收**：`cargo test` 全绿；手动在渲染模式下双击 `[[docs/README.zh-CN.md]]` 可打开文件。

**建议提交**：`feat(inline): 解析 [[wiki]] 链接并在预览中可点击打开`

---

### Phase 2：编辑时文件选择器（核心交互） ✅ 已实现

**目标**：光标在 Wiki 链接内时，弹出带层级展示与模糊匹配的文件选择 overlay。

#### 2.1 触发条件

```rust
// 伪逻辑
block.projection.as_ref()
    .and_then(|p| p.link_run_fully_covering_range(&block.selected_range))
    .filter(|run| matches!(run.link, InlineLink::WikiLink { .. }))
```

附加触发（可选）：

- 用户输入 `[[` 后自动弹出（未完成闭合时）
- 光标在 `LinkTargetText` 等价区域（Wiki 链接为路径 `StyledText` 段）

#### 2.2 Overlay UI

参考：

- `src/editor/code_language_menu.rs` — 锚定到 block/caret 附近的绝对定位 overlay
- `src/editor/quick_file_open.rs` — 搜索输入 + 结果列表 + 键盘导航

布局示意：

```text
┌─────────────────────────────┐
│ 🔍 docs/refactor...         │  ← 单行输入，实时模糊过滤
├─────────────────────────────┤
│ ▼ docs/                     │
│   ├ README.zh-CN.md         │
│   ├ development.zh-CN.md    │
│ ▼ src/components/           │
│   └ block/runtime/mod.rs    │  ← 按目录层级缩进
└─────────────────────────────┘
```

行为：

| 状态 | 展示 |
| --- | --- |
| 搜索框为空 | 完整目录树（目录可折叠，复用 `WorkspaceTreeNode`） |
| 有输入 | 扁平化匹配列表，每项带 `detail` 面包屑（与 Cmd+P 一致） |
| 选择文件 | 写入 link fragment 的 `text`，触发 Markdown 重序列化 |
| 键盘 | ↑↓ 移动高亮、Enter 确认、Esc 关闭 |

#### 2.3 状态管理

在 `src/editor/mod.rs` 新增状态（与 `quick_file_open` 并列），例如：

```rust
pub(super) wiki_link_picker: WikiLinkPickerState {
    open: bool,
    block_entity_id: Option<EntityId>,
    link_clean_range: Option<Range<usize>>,
    input: SingleLineFieldState,
    tree: Option<WorkspaceTreeNode>,
    results: Vec<QuickFileOpenResult>, // 或专用类型
    selection: usize,
    focus_handle: FocusHandle,
}
```

Block 侧通过 `BlockEvent::RequestWikiLinkPicker { ... }` 通知 Editor 打开/更新/关闭。

#### 2.4 共享逻辑提取（可选）

将 `fuzzy_match_score`、`collect_all_files`、`file_display_label` 从 `quick_file_open.rs` 提取到 `src/editor/file_search.rs`（或 `src/util/`），供 Cmd+P 与 Wiki 选择器共用。

#### 2.5 渲染

新建 `src/editor/wiki_link_picker.rs`，在 `src/editor/overlays.rs` 中作为 `EditorOverlayKind::WikiLinkPicker` 挂载。

共享逻辑位于 `src/editor/file_search.rs`（递归文件收集、目录树构建、模糊匹配）。

**验收**：

- 光标移入 `[[...]]` 内自动弹出选择器
- 输入关键字后列表实时过滤
- 选择文件后路径更新且序列化为 `[[新路径]]`
- Esc / 点击外部关闭，不影响其他 overlay

**建议提交**：`feat(editor): Wiki 链接编辑时弹出项目文件选择器`

---

### Phase 3：体验增强

| 项 | 说明 |
| --- | --- |
| 输入 `[[` 自动弹出 | 未完成闭合时即显示选择器 |
| 未闭合 `[[` 实时补全 | 输入过程中动态更新候选 |
| 目标不存在警告 | 预览态虚线 underline 或 muted 色 |
| `[[path\|label]]` | 自定义显示名 |
| `[[path#heading]]` | 跳转到目标文档内标题（复用 outline 逻辑） |
| 工具栏按钮 | 插入 Wiki 链接模板 |
| HTML/PDF 导出完善 | 相对路径解析与 `base_dir` 一致 |

---

## 建议 PR 拆分

1. `feat(inline): 解析 [[wiki]] 链接并在预览中可点击打开` — Phase 1
2. `feat(editor): Wiki 链接编辑时弹出项目文件选择器（层级 + 模糊匹配）` — Phase 2
3. `feat(export): HTML 导出支持 Wiki 链接` — Phase 1.5 或 Phase 3
4. `feat(inline): 支持 [[path\|label]] 与锚点语法` — Phase 3

---

## 已确认决策（Phase 1）

| 问题 | 决策 |
| --- | --- |
| **匹配文件范围** | 工作区内**所有文件**（不限于 `.md`） |
| **路径基准** | **工作区根目录**（与 Cmd+P 快速打开一致） |
| **显示文本** | 完整相对路径（Phase 3 再支持 `\|显示名`） |
| **打开不存在文件** | 回退到现有「打开链接？」确认框（Phase 3 可改为专用提示） |

---

## 待决事项（Phase 2 及以后）

| 问题 | 选项 | 建议 |
| --- | --- | --- |
| **选择器文件范围** | 与 Phase 1 一致：工作区全部文件 | 已确认 |
| **打开不存在文件** | 静默失败 / 提示 / 提供创建 | Phase 3 可提供「创建并打开」 |

---

## 约束

- **不新增外部依赖**（除非 Phase 3 有明确需求）
- **匹配现有 GPUI overlay 模式**（`occlude`、点击外部 dismiss、`dismiss_contextual_overlays`）
- **中英文 i18n**：新增 UI 字符串补 `src/i18n/mod.rs` 与 locale JSON
- **测试**：`cargo test` 全绿；解析/序列化/投影须有单元测试
- **提交消息**：简体中文，格式见 `.cursorrules`

---

## 总览块（Agent 执行 Phase 时附上）

```markdown
# Velotype 功能：Wiki 链接 [[path]]

## 项目背景
Velotype / Markman 是 Rust + GPUI 的块级 Markdown 编辑器。inline 文本采用 fragment + 属性模型，Markdown 标记在 I/O 边界解析/序列化。

核心模块：
- `src/components/markdown/inline/` — 解析、序列化、InlineLink
- `src/components/block/runtime/projection.rs` — 聚焦时展开 Markdown 分隔符供编辑
- `src/editor/quick_file_open.rs` — Cmd+P 模糊文件搜索
- `src/editor/workspace.rs` — 工作区文件树、open_workspace_file

## 目标
实现 Obsidian 风格 `[[路径/文件名]]`：
- 预览：超链接样式，双击打开本地文件
- 编辑：光标在链接内弹出文件选择器（目录层级 + 模糊匹配）

## 设计文档
详见 `docs/wiki-link-implementation.zh-CN.md`

## 约束
- 复用 InlineLink / projection / fuzzy_match_score / WorkspaceTreeNode
- 最小 diff，匹配现有 overlay 与事件模式
- `cargo test` 全绿
- 提交消息简体中文
```

---

## 与重构计划的关系

本功能为**新增特性**，不依赖 [refactor-execution-plan.zh-CN.md](refactor-execution-plan.zh-CN.md) 的 22 步重构。若 Phase 2 实施时 Editor overlay 已按步骤 8–15 拆分，Wiki 选择器应挂载到新的 overlay 模块而非继续膨胀 `Editor` 上帝对象。
