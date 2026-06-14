# AI 知识库 — 方案设计

本文档描述 Markman 中 **工作区 Markdown 知识库与侧栏 AI 对话集成** 的功能设计：将当前工作区全部笔记作为本地知识源，在 AI 对话框中支持按需检索、引用、分析、总结，且回答可追溯到具体文件与行号。

[Hook 自动开发步骤](knowledge-base-ai-hook-steps.zh-CN.md) | [侧栏 AI 对话](ai-chat-implementation.zh-CN.md) | [知识图谱](knowledge-graph-implementation.zh-CN.md) | [开发与构建](development.zh-CN.md)

---

## 需求概述

| 场景 | 期望行为 |
| --- | --- |
| **知识库范围** | 工作区根目录下全部 `.md` / `.markdown`（与 `collect_markdown_files` 一致） |
| **智能检索** | 用户提问时，按问题**自动**从全库检索相关片段，而非固定塞入前 8 个文件摘要 |
| **显式引用** | 输入 `@文件名` / `@#标签` 限定检索范围；选区可 pinned 加入对话 |
| **常用能力** | 总结、对比、检索、关联笔记（利用 tag / wiki 链接 / 图谱索引） |
| **可溯源** | 回复标注 `path/to/note.md L42-58`，点击跳转编辑器对应行 |
| **上下文预算** | 注入内容有字符/token 上限，按相关度截断并提示已检索篇数 |
| **增量索引** | 文件保存、工作区切换时后台更新，不阻塞 UI |
| **隐私提示** | 检索片段会随请求发往用户配置的 LLM API，偏好中明确说明 |

本功能 **不** 引入 WebView；Phase 1–2 不依赖 embedding 模型；网络层继续走 `src/net/ai.rs` 的 OpenAI-compatible streaming。

---

## 现状与差距

侧栏 AI 对话（`ai_chat.rs`）已支持多轮对话与多种上下文模式。其中 **「引用工作区」** 当前实现为：

```rust
// src/editor/controllers/ai.rs — 简化说明
const WORKSPACE_CONTEXT_FILE_LIMIT: usize = 8;
const WORKSPACE_CONTEXT_BYTES_PER_FILE: usize = 1200;
// 按路径排序取前 8 个文件，每文件截断 1200 字节
```

| 已有能力 | 局限 |
| --- | --- |
| 多轮对话、流式回复 | `context_markdown` 仅在**首轮**注入 |
| 工作区关键词搜索（Files 面板） | 未接入 AI 对话 |
| `tag_index` / `link_index` / 知识图谱 | 未用于 AI 检索 |
| `AiContextSnapshot` 含文件名、行号 | 无 citation 列表与跳转闭环 |

**核心矛盾**：用户需要「按需检索全库」，现有是「固定塞一小段摘要」。文档规模增大后必然漏检、答不准、超 token。

---

## 与现有架构的映射

```
┌─────────────────────────────────────────────────────────────────┐
│ Editor                                                          │
│  ├─ ai_chat: AiChatPanelState     ← 多轮对话、上下文模式        │
│  ├─ workspace: tag_index          ← 标签 → 候选文档扩展         │
│  ├─ workspace: link_index         ← Wiki 链接 → 相关文档扩展    │
│  └─ kb_* (新建)                   ← 分块索引 + 检索 + 编排       │
└─────────────────────────────────────────────────────────────────┘
         │                              │
         ▼                              ▼
┌─────────────────┐            ┌──────────────────────┐
│ ai_context.rs   │            │ ai_chat.rs           │
│ AiContextSnapshot│           │ collect/send 流程    │
│ + citations     │            │ citation UI          │
└─────────────────┘            └──────────────────────┘
         │                              │
         └──────────┬───────────────────┘
                    ▼
         ┌──────────────────────┐
         │ src/net/ai.rs        │
         │ complete_chat_streaming│
         └──────────────────────┘
                    │
                    ▼
         ┌──────────────────────┐
         │ Markdown 文件（工作区）│
         └──────────────────────┘
```

### 现有代码复用清单

| 模块 | 路径 | 复用方式 |
| --- | --- | --- |
| 文件枚举 | `src/editor/markdown_files.rs` | 知识库文档列表 |
| 工作区搜索 | `src/editor/workspace.rs` — `search_markdown_files` | 关键词检索核心 |
| 标签索引 | `src/editor/tag_index.rs` | 按 `#tag` 过滤与扩展候选 |
| 链接索引 | `src/editor/link_index.rs` | 反向链接、相关文档 |
| 图谱模型 | `src/editor/graph_model.rs` | 「相关笔记」推荐（可选加分） |
| AI 上下文 | `src/editor/ai_context.rs` | 扩展 `AiContextSnapshot` |
| 侧栏 AI | `src/editor/ai_chat.rs` | 发送流程、消息 UI |
| 跳转 | `workspace.rs` — `PendingWorkspaceSearchJump` | citation 点击打开文件行 |
| AI 偏好 | `src/config/store.rs` — `AiPreferences` | 知识库开关与预算 |

### 需新建模块

| 文件 | 职责 |
| --- | --- |
| `src/editor/kb_index.rs` | 文档元数据 + 按标题/段落分块 + revision |
| `src/editor/kb_retriever.rs` | 混合检索：关键词 + tag + link + 打分排序 |
| `src/editor/kb_orchestrator.rs` | 解析 query / @引用 → 检索 → 组装 `context_markdown` + citations |
| `src/editor/kb_citation.rs`（或合入 `ai_context.rs`） | `KbCitation` 类型与 prompt 格式化 |

---

## 图模型与数据类型

### KbChunk — 知识库分块

```rust
pub struct KbChunk {
    pub path: PathBuf,           // 工作区相对路径
    pub start_line: usize,
    pub end_line: usize,
    pub heading: Option<String>, // 所属 ATX 标题（若有）
    pub text: String,
}
```

**分块策略（MVP）**：

1. 按 ATX 标题（`#` … `######`）切 section；section 过长（> ~800 字）再按段落二次切分。
2. 跳过 fence 内代码块不参与分块边界（与 `tag_index` 扫描规则一致）。
3. 每块保留 `{path, start_line, end_line, heading}` 元数据供 citation。

### KbCitation — 引用来源

```rust
pub struct KbCitation {
    pub path: PathBuf,
    pub start_line: usize,
    pub end_line: usize,
    pub preview: String,         // 单行摘要，UI 展示用
    pub score: f32,              // 检索相关度（可选展示）
}
```

扩展 `AiContextSnapshot`：

```rust
pub struct AiContextSnapshot {
    pub context_markdown: String,
    pub target_label: String,
    pub source_file_name: Option<String>,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    /// 知识库检索命中的来源列表（步骤 4 起）
    pub citations: Vec<KbCitation>,
}
```

### WorkspaceKbIndex — 工作区索引

```rust
pub struct WorkspaceKbIndex {
    pub chunks: Vec<KbChunk>,
    pub by_path: BTreeMap<PathBuf, Vec<usize>>, // path → chunk indices
    pub revision: u64,
}
```

- 全量构建：`build_workspace_kb_index(root)`
- 增量：`refresh_kb_index_for_file(index, path, content)` / `remove_file_from_kb_index`
- 与 `tag_index` / `link_index` 共用 revision 触发策略（文件保存时 refresh）

---

## 检索层设计

### 混合打分（Phase 1，无 embedding）

```
score(chunk) =
    keyword_hits(title × 3 + body × 1)
  + tag_match(+5)           // query 或 @#tag 命中
  + link_neighbor(+3)       // 与当前文档 / 命中文档有 wiki 边
  + heading_match(+2)       // query token 出现在 heading
  + recency_decay(可选)     // 最近修改文件略加分
```

**检索流程**：

1. 解析用户输入：`plain query` + 可选 `@path` / `@#tag` 过滤器。
2. 关键词候选：`search_markdown_files(root, query)` 扩展至 chunk 级。
3. Tag 扩展：query 含 `#tag` 或 `@#tag` 时，从 `tag_index.by_tag` 拉取文件。
4. Link 扩展：当前打开文档的出链 / 入链邻居加入候选（「相关笔记」类问题）。
5. 对候选 chunk 打分排序，在 `max_context_chars` 预算内贪心选取 Top-K。
6. 输出 `Vec<KbCitation>` + 格式化的 `context_markdown`。

### context_markdown 格式

```markdown
[KB-1] notes/rust-gpui.md L12-45 · ## GPUI 基础

（chunk 正文）

---

[KB-2] notes/meeting.md L3-8 · ## 周会

（chunk 正文）
```

System prompt 要求模型引用 `[KB-N]` 或 `[path:Lx-Ly]`，且库中无依据时不编造。

### 显式引用语法（Phase 2）

| 语法 | 含义 |
| --- | --- |
| `@notes/foo.md` | 限定检索/读取该文件 |
| `@#rust` | 限定含该 tag 的文档 |
| `@.` | 当前打开文档（快捷） |

解析在 `kb_orchestrator::parse_kb_query(draft)` 完成，未识别 `@` 时走全库检索。

---

## 对话集成

### 上下文模式

在现有 `AiChatContextMode` 基础上：

| 模式 | 行为变化 |
| --- | --- |
| `Workspace` | **改为** query 驱动知识库检索（传入 `draft` 作为 query） |
| `Blank` | 仍为空；用户可纯聊天 |
| `Selection` / `FullDocument` / `Command` | 不变；可与知识库结果**叠加**（编排层合并，预算内截断） |

偏好 `allow_workspace_context` 语义升级为「允许知识库检索」；UI 文案改为「引用知识库」。

### 多轮策略

| 策略 | 说明 |
| --- | --- |
| **A（MVP 推荐）** | 每轮 user 发送时，若模式含知识库，按**本轮** query 重新检索 |
| B | 首轮检索 + 后续仅在新 `@引用` 时追加 |

MVP 采用 **策略 A**，实现简单且追问时上下文跟得上。

### System Prompt 增强

在 `sidebar_chat_system_prompt()` 追加：

```
You have access to excerpts from the user's local Markdown knowledge base, labeled [KB-N].
Answer using only provided excerpts when they are relevant.
Cite sources as [KB-N] or path:Lx-Ly.
If the excerpts do not contain enough information, say so clearly.
```

---

## UI 设计

### 输入区增强

```
┌──────────────────────────────────────┐
│ [总结知识库] [找相关] [分析选区]      │  ← 快捷按钮（步骤 6）
├──────────────────────────────────────┤
│ 多行输入（支持 @ 补全）               │
├──────────────────────────────────────┤
│ [引用知识库 ▼]              [发送]    │
└──────────────────────────────────────┘
```

### 来源引用（Citation Chips）

助手消息下方展示命中的来源：

```
来源：notes/a.md L12-45 · notes/b.md L3-8
```

- 点击 chip → `open_workspace_search_result` 跳转
- hover 显示 preview 单行

### 检索摘要（可选折叠）

用户消息发送后、模型回复前，可插入一条 `AiChatRole::Context` 消息：

> 已从知识库检索 12 篇笔记，注入 4 段相关内容（约 6.2k 字符）

---

## 配置扩展

`AiPreferences` 新增字段（步骤 7 落地）：

```rust
pub struct AiPreferences {
    // ...existing...
    pub allow_knowledge_base: bool,      // 默认 true（或沿用 allow_workspace_context）
    pub kb_max_context_chars: usize,     // 默认 12000
    pub kb_max_chunks: usize,            // 默认 12
    pub kb_include_link_neighbors: bool, // 默认 true
}
```

偏好 UI 增加说明：「启用后，检索到的笔记片段将发送至 AI 服务商」。

---

## 网络层

**不修改** `complete_chat_streaming` 签名。知识库内容仍通过 `context_markdown` 或拼入 user content 注入，与现网侧栏行为一致。

弹出式 AI 对话框（`controllers/ai.rs`）在步骤 4 可选同步改用 `kb_orchestrator`（与侧栏共用），避免两套工作区逻辑；MVP 优先侧栏，弹出框可作为步骤 7 收尾项。

---

## 分阶段范围（本队列 vs 后续）

| 阶段 | 内容 | 本 Hook 队列 |
| --- | --- | --- |
| **Phase 1** | 分块索引 + 关键词/tag/link 混合检索 + 侧栏集成 | 步骤 1–5 |
| **Phase 2** | Citation UI + @ 引用 + 快捷指令 | 步骤 5–6 |
| **Phase 3** | 本地 embedding + 向量检索 + RRF 融合 | 可选后续 |
| **Phase 4** | LLM function calling 多步 search/read | 可选后续 |

---

## 文件规划

| 操作 | 路径 |
| --- | --- |
| 新建 | `src/editor/kb_index.rs` |
| 新建 | `src/editor/kb_retriever.rs` |
| 新建 | `src/editor/kb_orchestrator.rs` |
| 修改 | `src/editor/ai_context.rs` — `citations` 字段 |
| 修改 | `src/editor/ai_chat.rs` — 发送流程、Context 消息、citation UI |
| 修改 | `src/editor/controllers/ai.rs` — 替换 `ai_workspace_context` |
| 修改 | `src/config/store.rs` — 偏好字段 |
| 修改 | `src/i18n/mod.rs` + locales |
| 修改 | `src/editor/mod.rs` — 声明新模块 |

---

## 测试策略

| 层级 | 内容 |
| --- | --- |
| 单元测试 | 分块边界、fence 跳过、打分排序、预算截断 |
| 集成测试 | 模拟工作区目录 + query → 期望 citation 顺序 |
| 手动验证 | 打开含多文件/tag/wiki 的工作区，侧栏提问并点击来源跳转 |

---

## 风险与缓解

| 风险 | 缓解 |
| --- | --- |
| Token 超限 | `kb_max_context_chars` + Top-K 截断 |
| 幻觉 | System prompt 约束 + UI 展示检索片段 |
| 隐私 | 偏好说明 + 默认需用户开启 |
| 大库性能 | 关键词路径先筛文件再分块；全量索引后台构建 |
| 与弹出框行为分叉 | 共用 `kb_orchestrator`，废弃 `WORKSPACE_CONTEXT_FILE_LIMIT` 逻辑 |

---

## 实现状态

| 模块 | 状态 |
| --- | --- |
| `kb_index.rs` | 待实现 |
| `kb_retriever.rs` | 待实现 |
| `kb_orchestrator.rs` | 待实现 |
| 侧栏知识库集成 | 待实现 |
| Citation UI | 待实现 |
| @ 引用 / 快捷指令 | 待实现 |
| 偏好与 i18n | 待实现 |

完成 Hook 队列后更新上表。
