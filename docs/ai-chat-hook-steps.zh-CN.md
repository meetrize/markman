# 侧栏 AI 对话 — Hook 自动开发步骤

本文档为 Markman 左侧面板 **独立 AI 对话** 的分步实施指南，配合 `.cursor/ai-chat-queue/` 与 stop hook **自动链式执行**。也可手动复制各步骤内容交给 Agent。

[方案设计](ai-chat-implementation.zh-CN.md) | [Hook 使用说明](#hook-自动链式执行) | [开发与构建](development.zh-CN.md)

---

## 方案概述

| 层次 | 内容 |
| --- | --- |
| **步骤 1** | 抽取 `ai_context` + 扩展 `net/ai` 多轮 streaming |
| **步骤 2** | `AiChatPanelState` 数据模型与 Editor 挂载 |
| **步骤 3** | 侧栏 streaming 请求管线 |
| **步骤 4** | `WorkspaceTab::Ai` 壳层 + header（含 AI 配置按钮） |
| **步骤 5** | 消息列表、输入区、发送与新建对话 |
| **步骤 6** | 上下文下拉与编辑器/工作区上下文集成 |
| **步骤 7** | i18n、图标、复制/插入、错误态收尾 |

## 现有代码参考

| 模块 | 路径 | 职责 |
| --- | --- | --- |
| AI 控制器 | `src/editor/controllers/ai.rs` | 弹出对话框、流式、上下文、预览 |
| AI 网络 | `src/net/ai.rs` | OpenAI-compatible streaming |
| AI 偏好 | `src/config/store.rs` | `AiPreferences` |
| 偏好窗口 | `src/config/ui/preferences/window.rs` | `open_preferences_window_to_ai` |
| 侧栏 | `src/editor/workspace.rs` | `WorkspaceTab`、面板渲染 |
| 图谱侧栏参考 | `src/editor/graph_workspace.rs` | 工具栏 + 面板模式 |
| Hook 参考 | `.cursor/hooks/knowledge-graph-queue-next.sh` | stop hook 链式投递 |

---

## Hook 自动链式执行

### 机制

| 文件 | 作用 |
| --- | --- |
| `.cursor/ai-chat-queue/steps.json` | 7 步标题与状态 |
| `.cursor/ai-chat-queue/state.json` | 队列开关、下一待执行步 |
| `.cursor/hooks/ai-chat-queue-next.sh` | Agent **stop** 时投递下一步 |
| `.cursor/hooks.json` | 注册 stop hook（**同时只启用一个开发队列**） |
| 本文档 | 每步任务说明与验收标准 |

### 启用

1. 将其他队列（`refactor-queue`、`hashtag-queue`、`knowledge-graph-queue`）的 `state.json` 中 `enabled` 设为 `false`
2. 确认 `.cursor/hooks.json` 的 `stop` 数组已包含：

```json
{
  "command": ".cursor/hooks/ai-chat-queue-next.sh",
  "loop_limit": 10
}
```

3. 设置 `.cursor/ai-chat-queue/state.json`：

```json
{
  "enabled": true,
  "nextStep": 1,
  "totalSteps": 7,
  "planDoc": "docs/ai-chat-hook-steps.zh-CN.md"
}
```

4. 在 Cursor 对话中发送：

```markdown
请阅读 `docs/ai-chat-hook-steps.zh-CN.md` 中的 **步骤 1/7：抽取 ai_context 与多轮 AI API** 及「总览块」，开始执行。
```

之后 Agent 每轮结束，stop hook 会自动投递下一步。

### 暂停 / 从某步继续

- `enabled: false` — 暂停
- `nextStep: N` — 从第 N 步开始（同时将 `steps.json` 中第 N 步 `status` 改回 `pending`）

### 手动入队

```markdown
请阅读 `docs/ai-chat-hook-steps.zh-CN.md` 中的 **步骤 {N}/7：{标题}** 及「总览块」。

要求：
1. 只做该步骤范围内的改动
2. 复用现有 `AiPreferences` 与 `open_preferences_window_to_ai`，不重复造配置 UI
3. 完成后运行 `cargo test`（必要时 `./scripts/check.sh`）
4. 用中文总结：改了哪些文件、如何验证、建议的 commit message

开始执行。
```

---

## 总览（每步开头附上）

```markdown
# Markman 功能：侧栏 AI 对话

## 项目背景
Markman 是 Rust + GPUI 块级 Markdown 编辑器。已有编辑器内弹出式 AI 对话框（选区上下文、流式预览、插入替换）与 `AiPreferences` 配置。需在左侧面板新增独立 **AI Tab**，提供多轮对话，并复用同一套 AI 配置与网络层。

核心模块：
- `src/editor/controllers/ai.rs` — 现有弹出 AI
- `src/net/ai.rs` — streaming 客户端
- `src/editor/workspace.rs` — 侧栏 Tab
- `docs/ai-chat-implementation.zh-CN.md` — 完整方案

## 目标
1. 侧栏 AI Tab + 多轮消息流
2. 复用 AiPreferences / open_preferences_window_to_ai
3. 上下文模式对齐弹出对话框
4. 流式回复、新建对话、面板内打开 AI 配置

## 约束
- 不新增 WebView；网络层 OpenAI-compatible
- 弹出框行为不因侧栏开发而回归
- 中英文 i18n（步骤 7 集中补，步骤 4 起可用硬编码占位）
- 新 SVG 按 `.cursor/rules/icon-assets.mdc` 在 `src/main.rs` 注册
- 提交消息简体中文
- 每步结束 `cargo test` 通过

## 设计文档
详见 `docs/ai-chat-implementation.zh-CN.md`
```

---

## 步骤 1/7：抽取 ai_context 与多轮 AI API

```markdown
# 步骤 1/7：抽取 ai_context 与多轮 AI API

为侧栏与弹出框共享上下文逻辑，并扩展网络层支持多轮对话。

## 任务

### 1.1 新建 `src/editor/ai_context.rs`

导出（从 `ai.rs` 迁移或复用）：

- `AiContextMode` — 合并 `AiPromptContextMode` 与侧栏扩展（Workspace、Command 可先留 stub）
- `AiContextSnapshot` — `context_markdown: String` + 简要 `target_label`
- `collect_editor_ai_context(editor, mode, selection_override, window, cx) -> Result<AiContextSnapshot, String>`

逻辑从 `collect_custom_ai_context` / `collect_ai_context` 抽取，**行为不变**。

在 `src/editor/mod.rs` 声明 `mod ai_context;`。

### 1.2 重构 `src/editor/controllers/ai.rs`

- 改为调用 `ai_context::collect_editor_ai_context`
- 删除或 thin-wrap 原私有收集函数
- 确保现有 `cargo test` 与手动弹出 AI 流程无行为变化

### 1.3 扩展 `src/net/ai.rs`

新增：

```rust
pub struct AiChatTurn {
    pub role: &'static str,
    pub content: String,
}

pub struct AiChatCompletionRequest { ... }

pub fn complete_chat_streaming(...) -> anyhow::Result<String>;
```

- 保留原 `complete_markdown_streaming` 签名与行为
- 单轮场景可内部转调新 API
- 为 `complete_chat_streaming` 添加单元测试：消息序列序列化、空 turns 错误处理

## 验收

- `cargo test` 通过
- `ai.rs` 弹出 AI 仍编译且上下文逻辑单一来源在 `ai_context.rs`

## 建议 commit

`refactor(ai): 抽取 AI 上下文模块并扩展多轮 streaming API`
```

---

## 步骤 2/7：AiChatPanelState 数据模型

```markdown
# 步骤 2/7：AiChatPanelState 数据模型

新建侧栏 AI 状态结构并挂到 `Editor`。

## 任务

### 2.1 新建 `src/editor/ai_chat.rs`

定义：

- `AiChatRole`, `AiChatMessage`, `AiChatContextMode`
- `AiChatPanelState` — messages、draft、context_mode、in_flight、error、scroll_handle、input_focus 及输入选区字段
- `AiChatPanelState::new(cx)`、`clear_conversation(&mut self)`

消息 `id` 可用递增 `u64` 或 `uuid` 简化实现。

### 2.2 挂载到 Editor

- `src/editor/mod.rs`：`mod ai_chat;`，`Editor` 增加 `ai_chat: AiChatPanelState`
- `Editor::new` 初始化
- 可选：`Editor::ai_chat_request_active()` 全局互斥标志（与 `ai.in_flight` 联动，步骤 3 实现）

### 2.3 暂不渲染 UI

本步仅数据结构与 `clear_conversation` 单元测试（消息清空、默认 context_mode）。

## 验收

- `cargo test ai_chat` 通过
- 编译通过，无 UI 变化

## 建议 commit

`feat(ai): 添加侧栏 AI 对话面板状态模型`
```

---

## 步骤 3/7：侧栏 streaming 请求管线

```markdown
# 步骤 3/7：侧栏 streaming 请求管线

实现侧栏发送 AI 请求与流式更新，复用 `ai.rs` 的 mpsc + `cx.spawn` 模式。

## 任务

### 3.1 `Editor::request_ai_chat_completion`

参数：`user_prompt: String`、`context: AiContextSnapshot`、`history: Vec<AiChatTurn>`（不含本轮 user）。

流程：

1. `read_app_preferences().ai`
2. 设置 `ai_chat.in_flight = true`，append 空 assistant 占位消息 `streaming: true`
3. 后台线程调用 `complete_chat_streaming`
4. 主线程合并 delta 到最后一条 assistant `content`
5. 完成或失败时 `in_flight = false`，`streaming = false`

### 3.2 全局互斥

侧栏请求进行中时：

- `ai.in_flight` 或统一 `ai_request_mutex` 阻止弹出框并发请求（反之亦然）

### 3.3 错误处理

API key 缺失、网络错误写入 `ai_chat.error`，不 panic。

## 验收

- 新增 `#[cfg(test)]` 或集成测试验证 history 传入 `complete_chat_streaming`（可 mock 层测消息构建）
- `cargo test` 通过
- 仍无侧栏 UI（可用临时 dev 调用验证编译）

## 建议 commit

`feat(ai): 实现侧栏 AI 对话流式请求管线`
```

---

## 步骤 4/7：WorkspaceTab::Ai 壳层与配置入口

```markdown
# 步骤 4/7：WorkspaceTab::Ai 壳层与配置入口

侧栏增加 AI Tab 与基础面板框架，顶部可打开 AI 配置。

## 任务

### 4.1 WorkspaceTab

`src/editor/workspace.rs`：

- `WorkspaceTab` 新增 `Ai`
- Tab id：`workspace-tab-ai`
- 图标路径常量：`AI_TAB_ICON = "icon/workspace/ai-chat.svg"`

### 4.2 图标资源

- 复制 `assets/icon/toolbar/sparkles.svg` → `assets/icon/workspace/ai-chat.svg`
- `src/main.rs` 注册 `icon/workspace/ai-chat.svg`

### 4.3 `render_ai_chat_panel`

新建渲染函数（可放在 `ai_chat.rs` 或 `workspace.rs`）：

- Header 行：
  - 设置按钮 → `crate::config::open_preferences_window_to_ai(cx)`（参考 `ai.rs` 中 `OpenAiPreferences`）
  - 「新建对话」按钮 → `clear_conversation`
- Body：占位文案「AI 对话即将可用」或空态
- 在 `render_workspace_panel` 的 `match active_tab` 增加 `WorkspaceTab::Ai` 分支

### 4.4 Tab 切换

`select_workspace_tab` 支持 `Ai`；切到 AI Tab 时 `window.focus(&ai_chat.input_focus)`（若已创建）。

## 验收

- 编译运行后左侧面板可见 AI Tab
- 点击设置按钮打开偏好窗口并定位 AI 页
- 其他 Tab 不受影响

## 建议 commit

`feat(workspace): 添加侧栏 AI Tab 与配置入口`
```

---

## 步骤 5/7：消息列表、输入区与发送

```markdown
# 步骤 5/7：消息列表、输入区与发送

完成侧栏核心对话 UI：历史消息、多行输入、发送与流式展示。

## 任务

### 5.1 消息列表

- `ScrollHandle` 滚动区域
- 用户/助手气泡：不同背景色（复用 `dialog_surface` / `editor_background` token）
- 流式时更新最后一条 assistant 内容并 `scroll_to_bottom`
- 空态：提示输入或检查 AI 配置

### 5.2 输入区

复用弹出框模式（二选一）：

- **A**：抽出 `AiPromptTextAreaElement` 到 `src/editor/ai_prompt_input.rs` 供两侧使用
- **B**：侧栏实现简化多行 `textarea`（`input_focus` + `replace` 逻辑）

底部：

- 上下文下拉占位（步骤 6 接选项）
- 发送按钮：非空 draft 且非 in_flight 可点
- Enter 发送、Shift+Enter 换行

### 5.3 发送流程

1. 取 `draft.trim()`，清空 draft
2. append user message
3. 用当前 `context_mode` + `collect_editor_ai_context` 收集上下文
4. 从历史构建 `Vec<AiChatTurn>`
5. 调用 `request_ai_chat_completion`

### 5.4 新建对话

Header 按钮清空 messages + error，保留 context_mode。

## 验收

- 手动测试：配置 API 后可发送并看到流式回复
- 多轮连续提问，历史保留
- `cargo test` 通过

## 建议 commit

`feat(ai): 实现侧栏 AI 消息列表与输入发送`
```

---

## 步骤 6/7：上下文下拉与编辑器集成

```markdown
# 步骤 6/7：上下文下拉与编辑器集成

对齐弹出式 AI 对话框的上下文能力。

## 任务

### 6.1 上下文下拉 UI

选项（与 `ai.rs` 一致）：

| 模式 | 标签 | 启用条件 |
| --- | --- | --- |
| Selection | 引用选中文本 | 当前有选区 |
| FullDocument | 引用全文 | `allow_full_document_context` 或始终可选（与弹出框一致） |
| Blank | 全新对话 | 始终 |
| Workspace | 引用工作区 | `allow_workspace_context` 且工作区已打开 |
| Command | 引用代码块 | `allow_command_context` 且焦点在代码块 |

下拉定位与样式参考 `render_ai_prompt_context_dropdown`。

### 6.2 上下文收集

发送时调用 `collect_editor_ai_context`，`Blank` 传空 context。

`Selection` 模式：打开下拉时快照 `pinned_selection_context`，避免发送瞬间选区丢失。

### 6.3 无选区 / 无配置提示

- Selection 不可用时灰色 + tooltip
- 未配置 API：空态显示错误与「打开 AI 配置」按钮

### 6.4 与弹出框一致性检查

对比 `AiPromptContextMode` 行为，必要时在 `ai_context.rs` 统一枚举。

## 验收

- 选中文本后选「引用选中文本」发送，模型收到选区内容
- 「引用全文」「全新对话」行为正确
- 偏好开关控制 Workspace / Command 选项显示

## 建议 commit

`feat(ai): 侧栏 AI 支持上下文模式与编辑器选区集成`
```

---

## 步骤 7/7：i18n、图标、复制插入与收尾

```markdown
# 步骤 7/7：i18n、图标、复制插入与收尾

国际化、助手消息操作与整体收尾。

## 任务

### 7.1 i18n

`src/i18n/mod.rs` + `locales/en.jsonc` + `locales/zh-CN.jsonc`：

- `workspace_ai_title`
- `workspace_ai_new_chat`
- `workspace_ai_settings`
- `workspace_ai_send`
- `workspace_ai_empty`
- `workspace_ai_context_selection` / `_full` / `_blank` / `_workspace` / `_command`
- `workspace_ai_error_no_api`
- `workspace_ai_copy` / `workspace_ai_insert`

替换步骤 4–6 中的硬编码中文。

### 7.2 助手消息操作

每条 assistant 消息 hover 显示：

- **复制** — 写入剪贴板
- **插入** — 在当前光标后插入 Markdown（复用 `ai.rs` insert 逻辑或简化 `insert_markdown_at_cursor`）

### 7.3 收尾

- 移除临时占位与 dead code
- `./scripts/check.sh` 或 `cargo test` 全绿
- 更新 `docs/ai-chat-implementation.zh-CN.md` 实现状态（若有偏差）

## 验收

- 中英文界面切换文案正确
- 复制/插入可用
- 队列全部 done

## 建议 commit

`feat(ai): 完善侧栏 AI 对话 i18n 与消息操作`
```

---

## 队列完成后

1. `.cursor/ai-chat-queue/state.json` → `"enabled": false`
2. 手动验证：AI Tab、多轮对话、上下文模式、打开配置、复制插入
3. 按需提交 PR

---

## 可选后续（不在本队列）

| 功能 | 说明 |
| --- | --- |
| 会话持久化 | 工作区本地保存对话历史 |
| 助手回复 Markdown 渲染 | 块级预览 |
| 从选区工具栏「在侧栏继续」 | 弹出框与侧栏联动 |
| 侧栏临时切换模型 | 不改全局 preferences |
