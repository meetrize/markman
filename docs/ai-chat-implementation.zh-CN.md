# 侧栏 AI 对话 — 方案设计

本文档描述 Markman 左侧面板中 **独立 AI 对话** 的功能设计：复用现有 AI 配置与弹出式对话框能力，在工作区侧栏提供可持续的多轮对话，并支持从面板顶部直接打开 AI 配置。

[Hook 自动开发步骤](ai-chat-hook-steps.zh-CN.md) | [开发与构建](development.zh-CN.md) | [知识图谱方案](knowledge-graph-implementation.zh-CN.md) | [AI 知识库方案](knowledge-base-ai.zh-CN.md)

---

## 需求概述

| 场景 | 期望行为 |
| --- | --- |
| **入口** | 工作区左侧面板新增 **AI** Tab，与 Files / Outline / Tags / Graph 并列 |
| **配置** | 复用 `AiPreferences`（provider、base_url、model、api_key_env、上下文开关） |
| **对话** | 多轮消息流：用户输入 → 流式回复 → 历史保留，可「新建对话」清空 |
| **上下文** | 对齐弹出式 AI 对话框：引用选中文本 / 引用全文 / 全新对话；偏好允许时附加工作区摘要、代码块上下文 |
| **设置** | 面板顶部工具栏提供 **AI 配置** 按钮，调用 `open_preferences_window_to_ai` |
| **与弹出框关系** | 弹出框保留「选中即问 + 预览插入/替换」；侧栏专注长对话，不替代预览流 |

本功能 **不** 引入 WebView；网络层继续走 `src/net/ai.rs` 的 OpenAI-compatible streaming。

---

## 与现有架构的映射

```
┌─────────────────────────────────────────────────────────────────┐
│ Editor                                                          │
│  ├─ ai: AiController          ← 弹出对话框、选区工具栏、预览插入   │
│  └─ ai_chat: AiChatPanelState ← 侧栏多轮对话（新增）              │
└─────────────────────────────────────────────────────────────────┘
         │                              │
         ▼                              ▼
┌─────────────────┐            ┌──────────────────────┐
│ overlays.rs     │            │ workspace.rs         │
│ 浮动 AI 对话框   │            │ WorkspaceTab::Ai     │
│ AI 预览 overlay │            │ render_ai_chat_panel │
└─────────────────┘            └──────────────────────┘
         │                              │
         └──────────┬───────────────────┘
                    ▼
         ┌──────────────────────┐
         │ src/net/ai.rs        │
         │ 单轮 + 多轮 streaming │
         └──────────────────────┘
                    │
                    ▼
         ┌──────────────────────┐
         │ config/store.rs      │
         │ AiPreferences        │
         └──────────────────────┘
```

### 现有代码复用清单

| 模块 | 路径 | 复用方式 |
| --- | --- | --- |
| AI 偏好 | `src/config/store.rs` | `read_app_preferences().ai` |
| 打开 AI 设置 | `src/config/ui/preferences/window.rs` | `open_preferences_window_to_ai(cx)` |
| 流式请求 | `src/net/ai.rs` | 扩展多轮 `messages` API |
| 上下文收集 | `src/editor/controllers/ai.rs` | 抽取 `collect_custom_ai_context` / `collect_ai_context` 为共享函数 |
| 弹出输入框 | `src/editor/controllers/ai.rs` | `AiPromptTextAreaElement`、上下文下拉 UI 模式 |
| 侧栏 Tab | `src/editor/workspace.rs` | `WorkspaceTab`、tab 栏、`render_workspace_panel` |
| 设置图标 | `assets/icon/toolbar/settings-2.svg` | 面板头部「AI 配置」 |
| AI 图标 | `assets/icon/toolbar/sparkles.svg` | Tab 图标（复制到 `workspace/ai-chat.svg` 并注册） |

---

## 数据模型

### 消息与角色

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AiChatRole {
    User,
    Assistant,
    /// 系统注入的上下文摘要（可选展示为折叠块）
    Context,
}

#[derive(Clone, Debug)]
pub struct AiChatMessage {
    pub id: String,
    pub role: AiChatRole,
    pub content: String,
    /// 流式生成中仅 assistant 最后一条为 true
    pub streaming: bool,
}
```

### 上下文模式（与弹出框对齐）

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AiChatContextMode {
    #[default]
    FullDocument,
    Selection,
    Blank,
    /// 偏好 `allow_workspace_context` 时可选
    Workspace,
    /// 偏好 `allow_command_context` 且焦点在代码块时可选
    Command,
}
```

### 面板状态

```rust
pub struct AiChatPanelState {
    pub messages: Vec<AiChatMessage>,
    pub draft: String,
    pub context_mode: AiChatContextMode,
    pub context_dropdown_open: bool,
    /// 打开面板或切换模式时快照的编辑器上下文
    pub pinned_selection_context: Option<AiContextSnapshot>,
    pub in_flight: bool,
    pub error: Option<String>,
    pub scroll_handle: ScrollHandle,
    pub input_focus: FocusHandle,
    // 输入框选区、光标闪烁等同 AiController.prompt_* 字段
}
```

`AiContextSnapshot` 为从 `AiController` 抽出的轻量结构：`target` 描述 + `context_markdown` 字符串，供侧栏发送时拼入首条 user 消息或独立 context 块。

---

## 网络层扩展

当前 `complete_markdown_streaming` 固定 `system + user(instruction + context)`。侧栏多轮需要：

```rust
pub struct AiChatTurn {
    pub role: &'static str,  // "user" | "assistant"
    pub content: String,
}

pub struct AiChatCompletionRequest {
    pub preferences: AiPreferences,
    pub system_prompt: String,
    pub turns: Vec<AiChatTurn>,
    /// 本轮附带的 Markdown 上下文（选区/全文/工作区等）
    pub context_markdown: Option<String>,
}

pub fn complete_chat_streaming(
    request: AiChatCompletionRequest,
    mut on_delta: impl FnMut(String),
) -> anyhow::Result<String>;
```

**策略**：

- 首条 user 消息：`{用户输入}\n\nMarkdown context:\n\n{context}`（与现网行为一致）
- 后续轮次：仅发送 `turns` 中的 user/assistant 交替历史
- `system_prompt` 沿用 `DEFAULT_SYSTEM_PROMPT`，侧栏可追加「你正在侧栏对话模式」一句

弹出框 **继续** 调用原 `complete_markdown_streaming`，避免大范围回归。

---

## UI 结构

### Tab 栏

- `WorkspaceTab::Ai`
- 图标：`assets/icon/workspace/ai-chat.svg`（自 `sparkles.svg` 复制，按 icon-assets 规则注册）
- Tab id：`workspace-tab-ai`

### 面板布局（`render_ai_chat_panel`）

```
┌──────────────────────────────────────┐
│ [⚙ AI 配置]  [＋ 新建对话]     (header) │
├──────────────────────────────────────┤
│                                      │
│  ▌ 用户消息气泡                       │
│                                      │
│       助手回复气泡（支持流式追加）      │
│                                      │
│  （空态：提示配置 API 或开始提问）      │
│                                      │
├──────────────────────────────────────┤
│ ┌──────────────────────────────────┐ │
│ │ 多行输入区（复用 prompt 输入模式）  │ │
│ └──────────────────────────────────┘ │
│ [引用全文 ▼]              [发送]      │
└──────────────────────────────────────┘
```

| 区域 | 行为 |
| --- | --- |
| **Header** | 左：设置按钮 → `open_preferences_window_to_ai`；右：新建对话清空 `messages` + `error` |
| **消息区** | `ScrollHandle` 滚动；assistant 流式时自动滚到底；简单 Markdown 纯文本展示（MVP 不做块级渲染） |
| **输入区** | Enter 发送、Shift+Enter 换行；与弹出框相同的剪贴板/选区快捷键 |
| **上下文下拉** | 选项与弹出框一致；`Selection` 在无选区时 disabled |
| **错误** | 首选项未配置 API key 时显示引导 + 设置按钮 |

### 与弹出对话框的功能对照

| 能力 | 弹出对话框 | 侧栏 AI |
| --- | --- | --- |
| 自定义 prompt | ✓ | ✓ |
| 上下文模式 | Selection / Full / Blank | 同上 + Workspace / Command |
| 流式输出 | ✓（预览 overlay） | ✓（消息气泡内） |
| 多轮历史 | ✗ | ✓ |
| 插入编辑器 | ✓（预览 Insert/Replace） | 步骤 7：单条回复「插入」「复制」 |
| 拖拽定位 | ✓ | ✗（侧栏固定布局） |
| AI 配置入口 | 工具栏设置图标 | 面板 header 设置按钮 |

---

## 状态与生命周期

1. **初始化**：`Editor::new` 中 `AiChatPanelState::new(cx)`
2. **Tab 切换**：切到 `WorkspaceTab::Ai` 时 focus 输入框；离开不丢历史
3. **发送流程**：
   - `read_app_preferences()` → 校验 API
   - `collect_ai_chat_context(mode)` → 复用编辑器上下文逻辑
   - append user message → `request_ai_chat_completion`（模式同 `request_ai_completion` 的 mpsc + spawn）
   - 流式更新最后一条 assistant message
4. **新建对话**：清空 messages；`context_mode` 保持或重置为 `Blank`（产品默认 `Blank`）
5. **工作区切换**：可选在 `workspace_root` 变化时提示「上下文已变」；MVP 不清空历史

---

## 文件规划

| 文件 | 职责 |
| --- | --- |
| `src/editor/ai_chat.rs` | `AiChatPanelState`、渲染、事件处理 |
| `src/editor/ai_context.rs` | 从 `ai.rs` 抽取的上下文类型与收集函数 |
| `src/net/ai.rs` | `complete_chat_streaming` |
| `src/editor/workspace.rs` | `WorkspaceTab::Ai`、tab 渲染分支 |
| `src/editor/controllers/ai.rs` | 改为调用 `ai_context` 共享函数（小范围重构） |
| `src/i18n/mod.rs` + `locales/*.jsonc` | 侧栏文案 |
| `assets/icon/workspace/ai-chat.svg` | Tab 图标 |
| `src/main.rs` | 注册 SVG |

---

## 分步实施（摘要）

完整提示词见 [ai-chat-hook-steps.zh-CN.md](ai-chat-hook-steps.zh-CN.md)。

| 步骤 | 内容 | 状态 |
| --- | --- | --- |
| 1 | 抽取 `ai_context` + 扩展 `net/ai` 多轮 API | ✅ 已完成 |
| 2 | `AiChatPanelState` 数据模型与 Editor 挂载 | ✅ 已完成 |
| 3 | 侧栏 streaming 请求管线 | ✅ 已完成 |
| 4 | `WorkspaceTab::Ai` 壳层 + header | ✅ 已完成 |
| 5 | 消息列表 + 输入区 + 发送/新建对话 | ✅ 已完成 |
| 6 | 上下文下拉 + 选区/全文/工作区/代码块集成 | ✅ 已完成 |
| 7 | i18n、图标、复制/插入、错误态与收尾 | ✅ 已完成 |

---

## 风险与约束

- **侧栏宽度有限**（180–360px）：消息气泡使用较小字号、自动换行；不做复杂 Markdown 块渲染
- **AiController 与 AiChatPanelState 输入状态重复**：步骤 1–2 只抽取上下文与网络层；输入组件可步骤 5 后再考虑共享 `AiPromptTextAreaElement`
- **并发**：侧栏 in_flight 时禁用发送；与弹出框 in_flight **独立**（允许同时仅一侧请求，或全局互斥——建议 **全局互斥** `Editor.ai_request_active` 避免双请求）
- **隐私**：工作区上下文遵守 `allow_workspace_context` 与 `WORKSPACE_CONTEXT_FILE_LIMIT`

---

## 验收标准（整体）

1. 左侧面板可见 AI Tab，点击可切换
2. 面板顶部可打开 AI 配置窗口并定位到 AI 页
3. 配置有效时可多轮对话，流式显示回复
4. 上下文模式与弹出框行为一致（在有选区/全文/偏好开关时）
5. `cargo test` 与 `./scripts/check.sh` 通过
6. 中英文 i18n 完整

---

## 可选后续（不在本队列）

| 功能 | 说明 |
| --- | --- |
| 会话持久化 | 按工作区保存 `messages` 到本地 JSON |
| Markdown 渲染 | 助手回复用块级预览组件 |
| 弹出框与侧栏同步 | 从选区工具栏「在侧栏继续」 |
| 模型切换下拉 | 侧栏临时覆盖 preferences.model |
| 全局快捷键 | 聚焦侧栏 AI 输入 |
