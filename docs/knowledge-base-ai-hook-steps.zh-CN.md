# AI 知识库 — Hook 自动开发步骤

本文档为 Markman **工作区知识库与侧栏 AI 对话集成** 的分步实施指南，配合 `.cursor/knowledge-base-ai-queue/` 与 stop hook **自动链式执行**。也可手动复制各步骤内容交给 Agent。

[方案设计](knowledge-base-ai.zh-CN.md) | [Hook 使用说明](#hook-自动链式执行) | [侧栏 AI 对话](ai-chat-implementation.zh-CN.md) | [开发与构建](development.zh-CN.md)

---

## 方案概述

| 层次 | 内容 |
| --- | --- |
| **步骤 1** | `kb_index.rs` — 文档分块索引 |
| **步骤 2** | `kb_retriever.rs` — 混合检索与打分 |
| **步骤 3** | `kb_orchestrator.rs` — 查询解析与 context 组装 |
| **步骤 4** | 替换 `ai_workspace_context` 并接入侧栏发送流程 |
| **步骤 5** | Citation 数据模型扩展与来源跳转 UI |
| **步骤 6** | `@` 引用补全与快捷指令按钮 |
| **步骤 7** | 偏好项、i18n、System Prompt 与收尾 |

## 现有代码参考

| 模块 | 路径 | 职责 |
| --- | --- | --- |
| 侧栏 AI | `src/editor/ai_chat.rs` | 多轮对话、上下文模式、发送 |
| AI 上下文 | `src/editor/ai_context.rs` | `AiContextSnapshot` |
| 工作区 AI | `src/editor/controllers/ai.rs` | `ai_workspace_context`（待替换） |
| 工作区搜索 | `src/editor/workspace.rs` | `search_markdown_files` |
| 标签索引 | `src/editor/tag_index.rs` | tag 扫描与增量 |
| 链接索引 | `src/editor/link_index.rs` | wiki 链接索引 |
| 文件枚举 | `src/editor/markdown_files.rs` | `collect_markdown_files` |
| AI 偏好 | `src/config/store.rs` | `AiPreferences` |
| Hook 参考 | `.cursor/hooks/ai-chat-queue-next.sh` | stop hook 链式投递 |

---

## Hook 自动链式执行

### 机制

| 文件 | 作用 |
| --- | --- |
| `.cursor/knowledge-base-ai-queue/steps.json` | 7 步标题与状态 |
| `.cursor/knowledge-base-ai-queue/state.json` | 队列开关、下一待执行步 |
| `.cursor/hooks/knowledge-base-ai-queue-next.sh` | Agent **stop** 时投递下一步 |
| `.cursor/hooks.json` | 注册 stop hook（**同时只启用一个开发队列**） |
| 本文档 | 每步任务说明与验收标准 |

### 启用

1. 将其他队列（`refactor-queue`、`hashtag-queue`、`ai-chat-queue`、`knowledge-graph-queue`）的 `state.json` 中 `enabled` 设为 `false`
2. 确认 `.cursor/hooks.json` 的 `stop` 数组已包含：

```json
{
  "command": ".cursor/hooks/knowledge-base-ai-queue-next.sh",
  "loop_limit": 10
}
```

3. 设置 `.cursor/knowledge-base-ai-queue/state.json`：

```json
{
  "enabled": true,
  "nextStep": 1,
  "totalSteps": 7,
  "planDoc": "docs/knowledge-base-ai-hook-steps.zh-CN.md"
}
```

4. 在 Cursor 对话中发送：

```markdown
请阅读 `docs/knowledge-base-ai-hook-steps.zh-CN.md` 中的 **步骤 1/7：kb_index 文档分块索引** 及「总览块」，开始执行。
```

之后 Agent 每轮结束，stop hook 会自动投递下一步。

### 暂停 / 从某步继续

- `enabled: false` — 暂停
- `nextStep: N` — 从第 N 步开始（同时将 `steps.json` 中第 N 步 `status` 改回 `pending`）

### 手动入队

```markdown
请阅读 `docs/knowledge-base-ai-hook-steps.zh-CN.md` 中的 **步骤 {N}/7：{标题}** 及「总览块」。

要求：
1. 只做该步骤范围内的改动
2. 解析规则与 `tag_index` / `link_index` 扫描行为一致（跳过 fence、ATX 标题行）
3. 完成后运行 `cargo test`（必要时 `./scripts/check.sh`）
4. 用中文总结：改了哪些文件、如何验证、建议的 commit message

开始执行。
```

---

## 总览（每步开头附上）

```markdown
# Markman 功能：AI 知识库

## 项目背景
Markman 是 Rust + GPUI 块级 Markdown 编辑器。侧栏 AI 对话已支持多轮流式对话；「引用工作区」目前仅为固定 8 文件 × 1200 字节摘要。需将工作区全部 Markdown 构成知识库，在 AI 对话中按需检索、引用、分析、总结，且回答可追溯到文件行号。

核心模块：
- `src/editor/ai_chat.rs` — 侧栏 AI 对话
- `src/editor/ai_context.rs` — 上下文快照
- `src/editor/tag_index.rs` / `link_index.rs` — 结构化索引
- `src/editor/workspace.rs` — 关键词搜索与跳转
- `docs/knowledge-base-ai.zh-CN.md` — 完整方案

## 目标
1. 工作区 Markdown 分块索引
2. 关键词 + tag + link 混合检索
3. 侧栏「引用知识库」模式 query 驱动注入
4. 来源 citation 展示与跳转
5. @ 引用与快捷指令（步骤 6）
6. 偏好、i18n、System Prompt 收尾

## 约束
- 不新增 WebView；Phase 1 不引入 embedding 依赖
- 弹出框 AI 行为不因本队列回归（步骤 4 起共用 orchestrator）
- 扫描规则与 `tag_index` 单一来源一致
- 中英文 i18n（步骤 7 集中补，步骤 5 起可用硬编码占位）
- 提交消息简体中文
- 每步结束 `cargo test` 通过

## 设计文档
详见 `docs/knowledge-base-ai.zh-CN.md`
```

---

## 步骤 1/7：kb_index 文档分块索引

```markdown
# 步骤 1/7：kb_index 文档分块索引

实现 `src/editor/kb_index.rs`，为工作区 Markdown 建立可分块检索的索引。

## 任务

### 1.1 类型定义

- `KbChunk` — path（相对工作区）, start_line, end_line, heading, text
- `WorkspaceKbIndex` — chunks, by_path, revision

### 1.2 分块算法

- 读取 UTF-8 Markdown，按 ATX 标题切 section
- section 超过 ~800 字符时按空行段落二次切分
- 跳过 fence 内内容（复用 `tag_index` 的 fence 状态机模式）
- 记录每块起止行号（1-based，与编辑器跳转一致）

### 1.3 API

```rust
pub fn extract_chunks_from_markdown(content: &str, path: &Path) -> Vec<KbChunk>;
pub fn build_workspace_kb_index(root: &Path) -> WorkspaceKbIndex;
pub fn refresh_kb_index_for_file(index: &mut WorkspaceKbIndex, root: &Path, path: &Path, content: &str);
pub fn remove_file_from_kb_index(index: &mut WorkspaceKbIndex, path: &Path);
```

在 `src/editor/mod.rs` 声明 `mod kb_index;`。

### 1.4 单元测试

- 多标题文档 → 多块，heading 正确
- fence 内 `# not heading` 不切分
- 长 section 二次切分
- refresh 替换旧块、remove 删除

## 验收

- `cargo test kb_index` 通过
- 不修改 UI 与 AI 发送流程

## 建议 commit

`feat(kb): 添加工作区 Markdown 分块索引`
```

---

## 步骤 2/7：kb_retriever 混合检索

```markdown
# 步骤 2/7：kb_retriever 混合检索

实现 `src/editor/kb_retriever.rs`，在索引上做 query 驱动的候选筛选与打分。

## 任务

### 2.1 检索输入/输出

```rust
pub struct KbRetrieveQuery {
    pub text: String,
    pub tag_filters: Vec<String>,      // 来自 @#tag，步骤 3 解析；本步可手动传入
    pub path_filters: Vec<PathBuf>,    // 来自 @path
    pub anchor_path: Option<PathBuf>,  // 当前文档，用于 link 扩展
}

pub struct KbRetrieveResult {
    pub chunks: Vec<KbChunk>,
    pub scores: Vec<f32>,
}

pub struct KbRetrieveConfig {
    pub max_chunks: usize,
    pub max_context_chars: usize,
    pub include_link_neighbors: bool,
}
```

### 2.2 打分逻辑

- 关键词：复用或封装 `search_markdown_files` 思路，标题 token ×3、正文 ×1
- Tag：`tag_index.by_tag` 命中文件内 chunk 加分
- Link：`link_index` 邻居 chunk 加分（`include_link_neighbors` 控制）
- 按 score 降序，贪心选取直至 `max_context_chars` / `max_chunks`

### 2.3 依赖注入

函数签名接受 `&WorkspaceKbIndex`、`&WorkspaceTagIndex`、`&WorkspaceLinkIndex`，不直接读 Editor，便于单测。

### 2.4 单元测试

- 固定小索引 + query → 期望 chunk 顺序
- 预算截断：超长 chunk 只取 Top-K
- tag 过滤缩小候选集

## 验收

- `cargo test kb_retriever` 通过

## 建议 commit

`feat(kb): 实现知识库混合检索与打分排序`
```

---

## 步骤 3/7：kb_orchestrator 查询编排

```markdown
# 步骤 3/7：kb_orchestrator 查询编排

实现 `src/editor/kb_orchestrator.rs`，连接检索结果与 AI prompt 格式。

## 任务

### 3.1 查询解析

```rust
pub struct ParsedKbQuery {
    pub plain_text: String,
    pub tag_filters: Vec<String>,
    pub path_filters: Vec<PathBuf>,
}

pub fn parse_kb_query(input: &str) -> ParsedKbQuery;
```

- `@path/to.md` → path_filters（支持无扩展名模糊匹配工作区文件）
- `@#tag` → tag_filters（normalize 同 `tag_index`）
- `@.` → 当前打开文件（调用方传入 anchor）
- 去掉 @ 片段后的剩余文本为 plain_text

### 3.2 Context 组装

```rust
pub fn build_kb_context_markdown(chunks: &[KbChunk], citations: &[KbCitation]) -> String;
pub fn chunks_to_citations(chunks: &[KbChunk], scores: &[f32]) -> Vec<KbCitation>;
```

- 每段前缀 `[KB-N] path Lx-Ly · heading`
- 段间 `---` 分隔

### 3.3 高层 API

```rust
pub fn retrieve_and_format(
    root: &Path,
    kb_index: &WorkspaceKbIndex,
    tag_index: &WorkspaceTagIndex,
    link_index: &WorkspaceLinkIndex,
    parsed: &ParsedKbQuery,
    anchor_path: Option<&Path>,
    config: &KbRetrieveConfig,
) -> (String, Vec<KbCitation>);
```

### 3.4 单元测试

- parse `@notes/a.md 总结 rust` → filters + plain_text
- format 输出含 `[KB-1]` 标签
- 空检索 → 空 context + 空 citations

## 验收

- `cargo test kb_orchestrator` 通过

## 建议 commit

`feat(kb): 添加知识库查询解析与 context 组装`
```

---

## 步骤 4/7：接入侧栏 AI 发送流程

```markdown
# 步骤 4/7：接入侧栏 AI 发送流程

用知识库检索替换固定 8 文件摘要，接入 `ai_chat` 发送路径。

## 任务

### 4.1 Editor 状态

`WorkspaceState` 或 `Editor` 增加：

- `kb_index: WorkspaceKbIndex`
- `kb_index_busy: bool`（可选，与 tag_index 同步模式）

`sync_workspace_kb_index` — 工作区打开/刷新时全量或增量构建。

文件保存时 `refresh_kb_index_for_file`。

### 4.2 替换 ai_workspace_context

`src/editor/controllers/ai.rs`：

- 删除或废弃 `WORKSPACE_CONTEXT_FILE_LIMIT` / `first_markdown_excerpt` 路径
- `ai_workspace_context()` 改为 thin wrapper 或删除，统一走 `kb_orchestrator`

### 4.3 修改 collect_ai_chat_send_context

`src/editor/ai_chat.rs` — 当 `context_mode == Workspace` 且 `allow_workspace_context`（或新 `allow_knowledge_base`）：

1. `parse_kb_query(draft)`
2. `retrieve_and_format(...)`，query 用 `plain_text`（发送时的 user prompt）
3. 写入 `snapshot.context_markdown` 与 `snapshot.citations`

### 4.4 多轮策略

每轮发送均重新检索（MVP 策略 A）；`build_ai_chat_completion_request` 中 knowledge context 拼入方式：

- 方案：每轮将 `context_markdown` 附在本轮 user turn（若不为空），或扩展 `AiChatCompletionRequest` 支持 `context_markdown: Option` 每轮传递（优选后者，需小改 `net/ai.rs`）

### 4.5 System Prompt

`sidebar_chat_system_prompt()` 追加知识库引用说明（见设计文档）。

## 验收

- 手动：打开多文件工作区，侧栏选「引用工作区/知识库」，提问「哪些笔记提到 X」能收到相关片段
- `cargo test` 通过
- 弹出框 AI 仍可用（若已改共用 orchestrator，验证无回归）

## 建议 commit

`feat(ai): 侧栏 AI 接入工作区知识库检索`
```

---

## 步骤 5/7：Citation 展示与来源跳转

```markdown
# 步骤 5/7：Citation 展示与来源跳转

扩展数据模型并在侧栏 AI 消息区展示可点击来源。

## 任务

### 5.1 扩展 AiContextSnapshot

`src/editor/ai_context.rs`：

```rust
pub struct KbCitation { ... }  // 若未在 kb_orchestrator 定义则集中到 kb_citation 或 ai_context
pub struct AiContextSnapshot {
    // ...existing...
    pub citations: Vec<KbCitation>,
}
```

发送成功后，将 citations 挂到对应 user 消息或单独 `AiChatRole::Context` 消息。

### 5.2 AiChatMessage 扩展

可选字段 `citations: Vec<KbCitation>` 或关联 map `message_id → citations`。

### 5.3 UI — Citation Chips

`render_ai_chat_panel` 中 assistant（或 context）消息下方：

- 展示 `path Lx-Ly` truncate 标签
- 点击 → `open_workspace_search_result(path, Some(start_line), preview, ...)`

### 5.4 检索摘要（可选）

发送后插入 Context 消息：「已从知识库检索 N 段内容」。

## 验收

- 手动：发送后可见来源 chips，点击跳转到正确文件行
- `cargo test` 通过

## 建议 commit

`feat(ai): 侧栏 AI 展示知识库引用来源并可跳转`
```

---

## 步骤 6/7：@ 引用补全与快捷指令

```markdown
# 步骤 6/7：@ 引用补全与快捷指令

输入 `@` 触发文件/标签补全；输入区上方添加快捷按钮。

## 任务

### 6.1 @ 补全 UI

复用 `file_search` 或 `wiki_link_picker` 模式：

- 输入 `@` 后弹出候选：工作区 `.md` 文件 + 标签列表（来自 `tag_index`）
- 选择后插入 `@path` 或 `@#tag` 到 draft
- 与 `parse_kb_query` 语法一致

### 6.2 快捷按钮

输入框上方（或 context 行旁）：

| 按钮 | 行为 |
| --- | --- |
| 总结知识库 | draft = 「请总结知识库中与当前主题相关的内容」+ 模式 Workspace |
| 找相关 | draft = 「与当前文档相关的笔记有哪些？」+ anchor `@.` |
| 分析选区 | 模式 Selection + draft = 「分析以下选中文本」 |

按钮仅填充 draft / 切换 context_mode，不自动发送（或可选一键发送）。

### 6.3 Slash 命令（简化）

识别 draft 以 `/search `、`/summarize ` 开头时改写 query 并强制 Workspace 模式。

## 验收

- `@` 补全可选文件与标签
- 快捷按钮填充预期 prompt
- `cargo test` 通过

## 建议 commit

`feat(ai): 添加知识库 @ 引用补全与快捷指令`
```

---

## 步骤 7/7：偏好、i18n 与收尾

```markdown
# 步骤 7/7：偏好、i18n 与收尾

配置项、国际化、文档更新与整体收尾。

## 任务

### 7.1 AiPreferences

`src/config/store.rs` + 偏好 UI：

- `kb_max_context_chars`（默认 12000）
- `kb_max_chunks`（默认 12）
- `kb_include_link_neighbors`（默认 true）
- 沿用或重命名 `allow_workspace_context` → UI 文案「允许知识库检索」
- 隐私说明：检索内容将发送至 AI 服务商

### 7.2 i18n

`src/i18n/mod.rs` + locales：

- `workspace_ai_context_knowledge_base`（替换原 workspace 文案）
- `workspace_ai_kb_sources`
- `workspace_ai_kb_retrieved_summary`
- `workspace_ai_kb_summarize` / `_related` / `_analyze_selection`
- `workspace_ai_kb_no_results`

### 7.3 弹出框 AI（可选）

`controllers/ai.rs` 中 `allow_workspace_context` 路径改用 `kb_orchestrator`，删除 dead code。

### 7.4 文档

- 更新 `docs/knowledge-base-ai.zh-CN.md` 实现状态表
- `./scripts/check.sh` 或 `cargo test` 全绿

## 验收

- 中英文切换文案正确
- 偏好修改影响检索预算
- 队列全部 done

## 建议 commit

`feat(kb): 完善知识库 AI 偏好、i18n 与收尾`
```

---

## 队列完成后

1. `.cursor/knowledge-base-ai-queue/state.json` → `"enabled": false`
2. 手动验证：多文件工作区、tag/wiki 交叉引用、侧栏提问、citation 跳转、@ 补全
3. 按需提交 PR

---

## 可选后续（不在本队列）

| 功能 | 说明 |
| --- | --- |
| 本地 embedding | `fastembed` + 向量索引，RRF 与关键词融合 |
| LLM tool calling | `kb_search` / `kb_read` 多步 Agent |
| 会话级 pinned 文档 | 多文件固定引用列表 |
| 知识库检索预览 | 发送前展示将注入的片段 |
| YAML front matter 标签 | 与行内 tag 合并索引 |
