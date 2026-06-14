# 行内标签 `#tag` — Hook 自动开发步骤

本文档为 Markman 添加 **Obsidian 风格行内标签 `#tag` 与标签云索引** 的分步实施指南，配合 `.cursor/hashtag-queue/` 与 stop hook **自动链式执行**。也可手动复制各步骤内容交给 Agent。

[方案设计](hashtag-tag-implementation.zh-CN.md) | [Hook 使用说明](#hook-自动链式执行) | [开发与构建](development.zh-CN.md)

---

## 方案概述

| 层次 | 内容 |
| --- | --- |
| **Phase 1** | inline 解析 `#tag`、渲染 pill 样式、Markdown round-trip |
| **Phase 2** | 扫描工作区 `.md` 构建标签索引 |
| **Phase 3** | 侧栏 Tags Tab、引用列表、点击跳转 |
| **Phase 4** | 输入补全、front matter 等（**本队列不自动执行**，见文末可选步骤） |

语法要点：`#tag` 紧跟标签体；支持 `#project/alpha`；排除 `#fff` / `#1a2b3c` hex；代码 span 与 `\#tag` 为字面量。

## 现有代码参考

| 模块 | 路径 | 职责 |
| --- | --- | --- |
| inline fragment | `src/components/markdown/inline/fragment.rs` | `InlineFragment`、`InlineLink`、`InlineEmoji` |
| 解析入口 | `src/components/markdown/inline/normalize.rs` | token 循环，wiki / emoji 等 |
| Wiki 链接参考 | `src/components/markdown/inline/wiki_link.rs` | source-preserving inline 扩展范例 |
| emoji 参考 | `src/components/markdown/inline/emoji.rs` | 独立 metadata 字段范例 |
| 序列化 | `src/components/markdown/inline/mod.rs` | `InlineTextTree::serialize_markdown` |
| 渲染 | `src/components/block/element.rs` | inline span 样式 |
| 主题 | `src/theme/theme.rs` | `text_link` 等色值 |
| 工作区扫描 | `src/editor/workspace.rs` | `collect_markdown_files`、`search_markdown_files` |
| 搜索跳转 | `src/editor/workspace.rs` | `PendingWorkspaceSearchJump` |
| HTML 导出 | `src/export/html.rs` | wiki link 预处理参考 |
| 重构队列参考 | `.cursor/hooks/refactor-queue-next.sh` | stop hook 链式投递模式 |

---

## Hook 自动链式执行

### 机制

| 文件 | 作用 |
| --- | --- |
| `.cursor/hashtag-queue/steps.json` | 5 步标题与状态（`pending` / `in_progress` / `done`） |
| `.cursor/hashtag-queue/state.json` | 队列开关 `enabled`、下一待执行步 `nextStep` |
| `.cursor/hooks/hashtag-queue-next.sh` | Agent **stop** 时投递下一步提示词 |
| `.cursor/hooks.json` | 注册 stop hook（与 refactor 队列并列，**同时只启用一个**） |
| 本文档 | 每步完整任务说明与验收标准 |

### 启用

```json
// .cursor/hashtag-queue/state.json
{
  "enabled": true,
  "nextStep": 1,
  "totalSteps": 5,
  "planDoc": "docs/hashtag-tag-hook-steps.zh-CN.md"
}
```

**注意：** 启用 hashtag 队列前，请将 `.cursor/refactor-queue/state.json` 的 `enabled` 设为 `false`，避免两个 stop hook 同时投递任务。

### 暂停 / 从某步继续

- `enabled: false` — 暂停自动链式执行
- `nextStep: N` — 从第 N 步开始（例如步骤 1 已手动完成时设为 `2`）

### 手动入队

将下方模板中的 `{N}`、`{标题}` 替换后，在 Agent 输入框按 **Enter**（非 Cmd+Enter）加入队列：

```markdown
请阅读 `docs/hashtag-tag-hook-steps.zh-CN.md` 中的 **步骤 {N}/5：{标题}** 及「总览块」。

要求：
1. 只做该步骤范围内的改动
2. 解析与索引规则保持单一来源（`hashtag.rs`）
3. 完成后运行 `cargo test`（必要时 `./scripts/check.sh`）
4. 用中文总结：改了哪些文件、如何验证、建议的 commit message

开始执行。
```

---

## 总览（每步开头附上）

```markdown
# Markman 功能：行内标签 #tag 与标签云索引

## 项目背景
Markman 是 Rust + GPUI 的块级 Markdown 编辑器。inline 采用 fragment + 属性模型；工作区侧栏已有文件、大纲、跨文件搜索。Wiki 链接与 emoji 是类似的 inline 扩展参考。

核心模块：
- `src/components/markdown/inline/` — 解析、序列化、fragment 元数据
- `src/components/block/element.rs` — inline 渲染
- `src/theme/theme.rs` — 主题色
- `src/editor/workspace.rs` — 侧栏、Markdown 扫描、搜索跳转
- `docs/hashtag-tag-implementation.zh-CN.md` — 完整方案设计

## 目标
1. 段落内 `#tag` 解析、渲染 pill 样式、Markdown round-trip
2. 扫描工作区 Markdown 构建标签索引（canonical 小写）
3. 侧栏 Tags Tab 展示标签列表，点击跳转到引用位置

## 约束
- 新增 `InlineTag` 独立字段，不占用 `InlineLink`
- 不新增外部依赖
- hex 色值 `#rgb` / `#rrggbb` 不解析为标签
- 中英文 i18n 都要补（Phase 3 起）
- 新增 SVG 须按 `.cursor/rules/icon-assets.mdc` 在 `src/main.rs` 注册
- 提交消息简体中文（`.cursorrules`）
- 每步结束：`cargo test` 通过

## 设计文档
详见 `docs/hashtag-tag-implementation.zh-CN.md`
```

---

## 步骤 1/5：inline 数据模型与 hashtag 解析器

```markdown
# 步骤 1/5：inline 数据模型与 hashtag 解析器

实现 Phase 1 的解析层。本步**不做**渲染样式、索引、侧栏 UI。

## 任务

### 1.1 扩展 fragment 模型

文件：`src/components/markdown/inline/fragment.rs`

新增：

```rust
pub struct InlineTag {
    pub name: String,   // 不含 `#`，如 project/alpha
    pub source: String, // 含 `#`，如 #project/alpha
}

pub struct InlineTagHit {
    pub name: String,
    pub source: String,
}
```

在 `InlineFragment`、`InlineSpan`、`InlineInsertionAttributes` 增加 `tag: Option<InlineTag>` / `Option<InlineTagHit>`。
全局补全 `tag: None`（编译器会指引遗漏处，含 `projection.rs` 等）。

### 1.2 新建解析器

文件：`src/components/markdown/inline/hashtag.rs`，在 `inline/mod.rs` 注册。

实现并导出（供后续索引复用）：

- `is_valid_tag_char(ch: char) -> bool` — 字母、数字、`_`、`-`、`/`、Unicode 字母
- `is_hex_color_tag(body: &str) -> bool` — 恰好 3 或 6 位 hex
- `normalize_tag_name(name: &str) -> String` — 索引用小写 canonical
- `locate_hashtag_in_str(line: &str, start: usize) -> Option<(usize, usize, InlineTag)>` — 行内扫描（索引用）
- `locate_hashtag(tokens, index) -> Option<(body_start, end_index)>`
- `parse_hashtag(...)` — 与 wiki_link / emoji 相同的 NormalizeBuilder 映射模式

解析规则：

| 规则 | 行为 |
| --- | --- |
| `#` 后有空格 | 不解析 |
| `#fff` / `#1a2b3c` | 不解析（hex） |
| `#123`（非 3/6 hex） | 解析为标签 |
| 标签体不能以 `/` 开头或结尾 | 不解析 |
| 最小长度 | 标签体 ≥ 1 字符 |

### 1.3 接入 normalize

文件：`src/components/markdown/inline/normalize.rs`

在 `parse_emoji_shortcode` **之后**、普通 emit **之前**：

```rust
if tokens[index].ch == '#'
    && let Some(next_index) = parse_hashtag(...)
{
    index = next_index;
    continue;
}
```

### 1.4 render cache 字段贯通（仅数据结构）

在 `InlineTextTree` / render cache 构建处，将 `fragment.tag` 映射到 `InlineSpan.tag`（本步可不实现 pill 样式，但字段须贯通以便编译）。

## 测试

在 `hashtag.rs` 或 `inline/mod.rs` 添加：

- `See #rust here` → 可见文本含 `#rust`，fragment 带 `InlineTag`
- `#fff`、 `#1a2b3c` 不解析
- `` `#tag` `` 代码 span 内为字面量
- `\#tag` 为字面量
- `#project/alpha` 解析正确
- `#/bad`、 `#bad/` 不解析
- round-trip：`serialize_markdown` 输出与输入一致（本步可先写基础 round-trip 测试）

运行：`cargo test`

## 验收

- `cargo test` 全绿
- 不引入渲染 pill、索引、侧栏改动
- 列出新增 public / pub(crate) API

## 建议 commit

`feat(inline): 添加 InlineTag 模型与 #tag 解析器`
```

---

## 步骤 2/5：序列化、主题色与渲染 pill 样式

```markdown
# 步骤 2/5：序列化、主题色与渲染 pill 样式

依赖步骤 1 的 `InlineTag` 与解析器。完成 Phase 1 的用户可见渲染。

## 任务

### 2.1 序列化

文件：`src/components/markdown/inline/mod.rs`

- fragment 带 `tag` 时输出 `tag.source`（已含 `#`）
- 与 bold/code/link 组合时顺序正确
- 补全 round-trip 测试

### 2.2 主题色

文件：`src/theme/theme.rs`

新增（light / dark 各一组，与 `text_link` 区分，偏 tag/chip 风格）：

- `text_tag: Hsla`
- `tag_background: Hsla`

更新 theme JSON 解析默认值与现有 theme 测试。

### 2.3 渲染 pill 样式

文件：`src/components/block/element.rs`（或 inline render cache 消费处）

当 `span.tag.is_some()`：

- 前景 `theme.colors.text_tag`
- 背景 `theme.colors.tag_background`
- 圆角 pill：`rounded_sm` + 水平 padding（与链接 underline 区分）
- 可见文本仍为 `#tag`（含 `#`）

### 2.4 投影编辑

文件：`src/components/block/runtime/projection.rs`

Phase 1 **无需**展开分隔符；确认聚焦编辑时光标可正常进入/删除 `#tag` 文本。
若有 fragment 合并逻辑，确保 `tag` 字段参与相等性判断。

## 测试

- 渲染 cache：`spans()` 中带 tag 的 span 范围正确
- 主题：light/dark 均含 `text_tag`、`tag_background`
- round-trip：`#rust` 与 `**#bold-tag**` 等组合
- GPUI 测试（可选）：段落含 `#rust` 时 span 可 hit-test（为 Phase 3 预留）

运行：`cargo test`

## 验收

- `./scripts/dev.sh` 手动验证：段落输入 `See #rust here`，渲染模式显示 pill 样式
- 序列化后 Markdown 不变
- `cargo test` 全绿

## 建议 commit

`feat(inline): 渲染 #tag 标签 pill 样式并补主题色`
```

---

## 步骤 3/5：HTML 导出与 Phase 1 收尾测试

```markdown
# 步骤 3/5：HTML 导出与 Phase 1 收尾

依赖步骤 1–2。补齐 HTML 导出与 Phase 1 边界测试，**不开始**工作区索引。

## 任务

### 3.1 HTML 导出

文件：`src/export/html.rs`

参考 `rewrite_wiki_links` 模式，对行内 `#tag` 做最小处理：

- 保留可见 `#tag` 文本
- 可选：包一层 `<span class="mm-tag">#name</span>`（CSS 可后续补）

确保不破坏已有 wiki link / emoji 预处理顺序。

### 3.2 边界测试补齐

必须覆盖：

| 场景 | 期望 |
| --- | --- |
| 块级标题 `# Title` | inline 层不解析为 tag（块级测试或文档级测试） |
| 多标签同行 | 各自独立 fragment |
| 中文标签 `#工作` | 正确解析 |
| 大小写 `#Rust` | 渲染保留原样；`normalize_tag_name` 为小写 |

### 3.3 代码整理

- 删除调试输出
- 确认 `hashtag.rs` 的合法性函数可被外部 crate 模块 `pub(crate)` 引用

## 测试

```bash
cargo test
# 可选
cargo test export::html
```

## 验收

- Phase 1 完整可用：解析、渲染、序列化、HTML 导出
- `cargo test` 全绿
- 列出 Phase 1 已知限制（无索引、无侧栏、无点击跳转）

## 建议 commit

`feat(export): HTML 导出行内 #tag 并补齐 Phase 1 测试`
```

---

## 步骤 4/5：工作区标签索引引擎

```markdown
# 步骤 4/5：工作区标签索引引擎

依赖步骤 1 的 `hashtag.rs` 合法性函数。实现 Phase 2：**仅索引逻辑与 Editor 挂钩**，本步**不做**侧栏 Tags Tab UI。

## 任务

### 4.1 共享 Markdown 文件枚举

新建 `src/editor/markdown_files.rs`（推荐）：

- 从 `workspace.rs` 提取 `collect_markdown_files`、`is_markdown_file`
- `workspace.rs` 改为 use 新模块，行为不变

### 4.2 索引模块

新建 `src/editor/tag_index.rs`，在 `editor/mod.rs` 注册。

```rust
pub struct TagOccurrence { path, line, preview, match_start_byte, raw_file_len }
pub struct WorkspaceTagIndex { by_tag, counts, revision }
```

实现：

- `extract_tags_from_markdown(content: &str) -> Vec<(String, TagOccurrence)>`  
  - 跳过 fenced code block（` ``` ` 状态机）
  - 跳过块级标题行 `^#{1,6}\s`
  - 每行用 `locate_hashtag_in_str` / 共享合法性函数
  - canonical key = `normalize_tag_name`
- `build_workspace_tag_index(root: &Path) -> WorkspaceTagIndex`
- `refresh_tag_index_for_file(index, path, content)`
- `remove_file_from_tag_index(index, path)`

### 4.3 Editor 集成

在 `WorkspaceState`（或 `WorkspaceController`）增加：

- `tag_index: Option<WorkspaceTagIndex>`
- `tag_index_busy: bool`（可选）

实现 `sync_workspace_tag_index(cx)`：

- 工作区根变化 → `cx.spawn` 后台全量 `build_workspace_tag_index`，完成后 `cx.notify`
- 当前文件保存 / 自动保存后 → `refresh_tag_index_for_file`（增量）
- 与 `sync_workspace_outline` 调用时机并列

### 4.4 测试

fixture 示例：

```markdown
# Title not a tag
paragraph with #rust and #project/alpha
```python
#not-a-tag
```
color #fff ok
#123 allowed
```

断言：

- `#rust` 计数 1，`#project/alpha` 计数 1
- 代码块内 `#not-a-tag` 不计入
- `#fff` 不计入
- `#123` 计入
- `#Rust` 与 `#rust` canonical 相同

## 验收

- `cargo test` 全绿
- 单元测试覆盖索引，无需 UI 测试
- 打开含标签的工作区后，内存索引正确（可通过测试或临时 debug 断言）

## 建议 commit

`feat(workspace): 扫描 Markdown 构建行内标签索引`
```

---

## 步骤 5/5：侧栏 Tags 面板、跳转与收尾

```markdown
# 步骤 5/5：侧栏 Tags 面板、跳转与收尾

依赖步骤 4 的 `WorkspaceTagIndex`。完成 Phase 3 与整体收尾。

## 任务

### 5.1 侧栏 Tags Tab

文件：`src/editor/workspace.rs`

- `WorkspaceTab::Tags`
- Tab 图标：`assets/icon/workspace/tags.svg` + `src/main.rs` 注册（见 icon-assets 规则）
- 空状态：无工作区 / 无标签时的 i18n 文案

标签列表面板：

- 展示 `#name` + 引用计数
- 排序：默认按计数降序；可选切换按名称升序（MVP 二选一即可，有余力做切换）
- 选中标签 → 下方引用列表

### 5.2 引用列表与跳转

复用 `WorkspaceSearchResult` 行 UI 或抽取共用 row 组件：

- 显示 `path:line` + preview
- 点击 → `open_workspace_file` + 行号定位（复用 `PendingWorkspaceSearchJump` 或抽取 `jump_to_file_line`）

### 5.3 渲染模式点击标签（推荐本步实现）

- `InlineTagHit` hit-test
- 点击标签 → 打开侧栏 Tags Tab 并选中该 canonical tag，展示引用列表
- 新增 `BlockEvent::RequestFilterByTag { name }` 或等价 Editor 回调

### 5.4 i18n

`src/i18n/mod.rs` + locale JSON，至少：

- `workspace_tab_tags`
- `workspace_empty_tags`
- `workspace_tag_sort_by_name` / `workspace_tag_sort_by_count`（若做排序）
- `workspace_tag_occurrences_title`

英文 + zh-CN 齐全。

### 5.5 收尾

| 场景 | 期望 |
| --- | --- |
| 切换 Tab | Files / Outline / Tags 行为一致 |
| 无工作区 | Tags Tab 显示空状态 |
| Esc / 搜索模式 | 不与 workspace search 冲突 |
| 保存文件 | 索引计数更新 |
| 渲染点击 `#tag` | 侧栏选中并列出引用 |

运行：

```bash
cargo test
./scripts/check.sh
```

手动验证：

1. 工作区多文件含 `#rust`
2. Tags Tab 显示 `#rust` 与正确计数
3. 点击引用跳转到对应行
4. 渲染模式点击正文 `#rust` 打开侧栏过滤

### 5.6 队列完成后

- 将 `.cursor/hashtag-queue/state.json` 的 `enabled` 设为 `false`
- 可选：在 `docs/hashtag-tag-implementation.zh-CN.md` 将 Phase 1–3 标为已实现

## 建议 commit

`feat(workspace): 侧栏标签面板与引用跳转`
```

---

## 可选步骤 6：Phase 4 体验增强（不进入自动队列）

以下功能**默认不**由 hook 自动执行，按需手动开任务：

| 项 | 说明 |
| --- | --- |
| 输入 `#` 自动补全 | 参考 `wiki_link_picker.rs` |
| 工具栏插入 `#` | 选区包裹或插入模板 |
| front matter `tags:` | 合并进索引 |
| 标签重命名 | 工作区批量替换 |
| 文件 watcher | 外部变更刷新索引 |
| 源码模式 `#tag` 高亮 | 渲染模式之外的编辑体验 |

---

## 五步标题速查

| 步 | 标题 | Phase |
| --- | --- | --- |
| 1 | inline 数据模型与 hashtag 解析器 | 1 |
| 2 | 序列化、主题色与渲染 pill 样式 | 1 |
| 3 | HTML 导出与 Phase 1 收尾测试 | 1 |
| 4 | 工作区标签索引引擎 | 2 |
| 5 | 侧栏 Tags 面板、跳转与收尾 | 3 |

---

## 架构示意

```
#tag 输入
    ↓
hashtag.rs parse → InlineFragment.tag
    ↓
render pill（text_tag / tag_background）
    ↓
collect_markdown_files → tag_index.rs
    ↓
WorkspaceTagIndex → Tags Tab → jump_to_file_line
```
