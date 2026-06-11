# 行内代码执行 — AI 实施提示词

本文档为 Velotype 添加**行内代码执行**功能的分步实施指南，可直接复制各步骤内容交给 AI Agent 执行。

[开发与构建](development.zh-CN.md)

## 方案概述

采用**子进程代理**（复用现有 `code_runner`），不内嵌 PTY/终端模拟器：

| 方案 | 资源占用 | 适用场景 |
| --- | --- | --- |
| 子进程代理（推荐主路径） | 轻：执行时短暂子进程 + 字符串缓冲 | 单行/短命令，结果回显到编辑器 |
| 系统终端兜底（可选） | 应用本身零开销 | 交互式命令、`vim`、`top` 等 |
| 内嵌终端模拟器 | 重：依赖大、常驻内存 | **不采用** |

行内代码与围栏代码块的区别：

- 行内代码**无语言标签**，默认按 shell（bash/sh）执行
- 结果用**紧凑浮层（popover）**展示，而非块级输出面板
- 复用现有安全门：`allow_code_execution`、首次确认、未保存确认

## 现有代码参考

| 模块 | 路径 | 职责 |
| --- | --- | --- |
| 执行核心 | `src/code_runner/mod.rs` | `spawn_code_run`、`resolve_runner`、`CodeRunProgress` |
| 编辑器状态 | `src/editor/code_run.rs` | `CodeBlockRunState`、`request_code_block_run`、确认对话框 |
| 围栏块 UI | `src/components/block/render.rs` | 运行按钮、输出面板 |
| 块事件 | `src/components/block/state.rs` | `BlockEvent::RequestRunCodeBlock` 等 |
| 事件分发 | `src/editor/code_run.rs` | `handle_block_code_run_event` |
| 偏好设置 | `src/config/preferences.rs` | `allow_code_execution`、`code_execution_confirm_shown` |
| 行内 span | `src/components/block/runtime/mod.rs` | `inline_spans()`、`collapsed_caret_inherits_inline_code_style()` |

## 使用说明

1. **按顺序执行**步骤 1 → 5，每步完成并 `cargo test` / 手动验证后再进行下一步
2. 每步开头附上「总览」块作为上下文
3. 若 AI 试图内嵌完整终端，明确拒绝并要求回到子进程方案
4. 步骤 4（系统终端兜底）可跳过，先完成 1 → 2 → 3 → 5 也能交付 MVP

---

## 总览（每步开头附上）

```markdown
# Velotype 功能：行内代码执行

## 项目背景
Velotype 是 Rust + GPUI 的 Markdown 编辑器。已有围栏代码块执行能力，采用轻量子进程代理（非内嵌终端）：

- 执行核心：`src/code_runner/mod.rs`（`spawn_code_run`、`resolve_runner`、`CodeRunProgress`）
- 编辑器状态：`src/editor/code_run.rs`（`CodeBlockRunState`、`request_code_block_run`、确认对话框）
- 围栏块 UI：`src/components/block/render.rs`（运行按钮、输出面板）
- 事件：`BlockEvent::RequestRunCodeBlock` 等，经 `handle_block_code_run_event` 处理
- 偏好：`allow_code_execution`、`code_execution_confirm_shown`（`src/config/preferences.rs`）

## 目标
为**行内代码**（`` `code` `` / `InlineStyle.code`）添加执行能力：
- **主路径**：复用 `code_runner` 子进程代理，默认 shell（bash/sh）执行单行/短命令
- **结果展示**：紧凑浮层（popover），非完整终端模拟器
- **可选**：偏好设置「在系统终端中打开」（macOS 兜底）
- **不重写**围栏代码块已有逻辑，尽量复用

## 约束
- 不引入 PTY / xterm / alacritty_term 等终端模拟器依赖
- 改动范围最小，匹配现有代码风格
- 中英文 i18n 都要补（`src/i18n/mod.rs` + locale JSON）
- 新增 SVG 图标须按 `.cursor/rules/icon-assets.mdc` 在 `src/main.rs` 注册
- 提交消息用简体中文（`.cursorrules` 规范）
- 每步结束：`cargo test` 通过，并说明如何手动验证

## 行内代码识别参考
- `Block::inline_spans()` → `InlineSpan { range, style: InlineStyle { code, ... } }`
- `Block::collapsed_caret_inherits_inline_code_style()` 可判断光标是否在行内代码内
- `Block::inline_style_at(offset)` 可按偏移取样式
- 行内代码**无语言标签**，默认按 shell 执行
```

---

## 步骤 1/5：扩展 code_runner

```markdown
# 步骤 1/5：扩展 code_runner

在 Velotype 中为行内代码执行扩展 `src/code_runner/mod.rs`，**不破坏**现有围栏块 API。

## 任务

1. 新增 `pub const DEFAULT_INLINE_CODE_LANGUAGE: &str = "shell";`（或等价常量）

2. 新增函数 `pub fn resolve_inline_code_runner() -> RunnerSpec`，返回 bash/sh 解释器

3. 新增 `pub fn spawn_inline_shell_run(...)` 或在 `spawn_code_run` 增加模式：
   - 对 shell 语言：用 `bash -c <source>` 或写临时 `.sh` 后执行（与现有临时文件方式二选一，优先与现有风格一致）
   - 工作目录、取消、stdout/stderr 流式回调与 `spawn_code_run` 一致

4. 新增辅助函数：
   ```rust
   pub fn extract_inline_code_source(display_text: &str, span_range: &Range<usize>) -> String
   ```
   从可见文本中截取行内代码内容（trim 首尾空白，保留内部空格）

5. 为行内执行增加合理限制（常量即可）：
   - `INLINE_CODE_RUN_MAX_OUTPUT_CHARS`（如 8_192）
   - 可选超时（若现有 code_runner 无超时，本步可只截断输出，超时留到后续）

## 测试
在 `src/code_runner/mod.rs` 或独立 test 模块添加单元测试：
- `resolve_inline_code_runner` 返回有效 spec
- `extract_inline_code_source` 正确截取
- （可选）集成测试 `echo hello` 能拿到 stdout（注意 CI 环境）

## 验收
- `cargo test` 通过
- 现有围栏代码块执行行为不变
- 列出本步新增/修改的 public API
```

---

## 步骤 2/5：编辑器状态与事件管线

```markdown
# 步骤 2/5：行内代码执行的 Editor 状态层

依赖步骤 1 的 `code_runner` API。在编辑器层接入行内代码执行，复用 `code_run.rs` 模式。

## 任务

### 2.1 定义行内执行定位键
在 `src/editor/code_run.rs`（或新建 `src/editor/inline_code_run.rs` 再由 `code_run.rs` re-export）定义：

```rust
pub(crate) struct InlineCodeRunTarget {
    pub block_id: EntityId,
    pub span_range: Range<usize>,  // display_text 中的可见范围
}
```

用 `(block_id, span_range)` 作为 `HashMap` 键，存储各 span 的运行状态（可复用 `CodeBlockRunState` 或薄封装）。

### 2.2 Editor 字段（`src/editor/mod.rs`）
新增（命名可微调，但需一致）：
- `inline_code_runs: HashMap<(EntityId, Range<usize>), InlineCodeRunState>`
- `active_inline_code_run: Option<ActiveInlineCodeRunControl>`
- 扩展 `code_run_dialog` 或新增 `inline_code_run_dialog`，支持 `FirstTimeConfirm { target }` 等

### 2.3 Block 侧能力（`src/components/block/runtime/mod.rs`）
新增方法：
```rust
pub(crate) fn inline_code_span_at_cursor(&self) -> Option<InlineSpan>
pub(crate) fn inline_code_span_for_range(&self, range: Range<usize>) -> Option<InlineSpan>
pub(crate) fn inline_code_source_at_cursor(&self) -> Option<String>
```
逻辑：根据 `cursor_offset()` 或选区，在 `inline_spans()` 中找到 `style.code == true` 的 span，用步骤 1 的 `extract_inline_code_source` 取源码。

### 2.4 新增 BlockEvent（`src/components/block/state.rs`）
```rust
RequestRunInlineCode,
RequestStopInlineCode,
RequestCloseInlineCodeRunOutput,
```
在 `interactions.rs` 增加对应 handler（可先占位，UI 在步骤 3 接线）。

### 2.5 Editor 执行流程（仿 `request_code_block_run`）
实现：
- `request_inline_code_run(block_id, span_range, cx)`
- `start_inline_code_run(...)`：调用 `spawn_inline_shell_run`，`work_dir` 复用 `code_run_work_dir()`
- `finish_inline_code_run`、`stop_active_inline_code_run`
- **安全门**：复用 `allow_code_execution`、`code_execution_confirm_shown`、未保存确认（与围栏块一致）
- 语言：固定 `DEFAULT_INLINE_CODE_LANGUAGE`，不走 `resolve_runner(language标签)`

### 2.6 事件分发
- `handle_block_code_run_event` 扩展处理新 BlockEvent
- `src/editor/events.rs` 的 `on_block_event` match 补全新事件分支

## 测试
在 `src/editor/tests.rs` 或 `src/components/block/runtime/tests.rs` 添加：
- 光标位于 `` `echo hi` `` 内时 `inline_code_span_at_cursor` 返回正确 span
- 光标在普通文本时返回 `None`
- 选区覆盖部分行内代码时能定位 span

## 验收
- `cargo test` 通过
- 可通过测试或临时 `println!` 验证能发起执行并收到 `Finished` outcome
- 暂不要求 UI，但 Editor API 可被步骤 3 调用
```

---

## 步骤 3/5：触发方式 + 紧凑结果浮层 UI

```markdown
# 步骤 3/5：行内代码执行 UI

依赖步骤 2 的 Editor API。实现用户可触发的执行入口与紧凑结果展示。

## 交互设计（按此实现）

### 触发方式（至少实现前两项）
1. **快捷键**：`Cmd+Enter`（macOS）/ `Ctrl+Enter`（其他），仅当光标在行内代码 span 内时生效
   - 在 `src/components/` 的 shortcut 定义中注册新 `ShortcutCommand`
   - 在 `src/editor/events.rs` 或 keybinding handler 中调用 `request_inline_code_run`
2. **行内代码 hover 小按钮**：光标/hover 在行内代码上时，在 span 右侧显示小型「运行」图标（参考围栏块 `ICON_CODE_BLOCK_RUN`，可复用或新增 `ICON_INLINE_CODE_RUN`）
3. （可选）渲染模式右键菜单项：「执行行内代码」

### 结果展示：Popover 浮层（非块级输出面板）
- 定位：紧贴当前行内代码 span 下方或右侧
- 内容：
  - Running：旋转/加载态 + 「停止」按钮
  - Done：stdout 单行优先展示；多行默认折叠为 3 行（复用 `CODE_RUN_OUTPUT_COLLAPSED_VISIBLE_LINES`）
  - Failed：stderr 或 `error_message`，红色
  - 元信息：exit code、耗时（复用 `code_run_meta_template` 文案模式）
- 关闭：Esc、点击外部、关闭按钮
- **不要**在段落下方插入大块 panel（与围栏块区分）

### 渲染接入点
- 优先在 `src/components/block/element.rs` 或 `src/components/block/render.rs` 的行内 text run 渲染处挂载 hover 按钮
- Popover 可在 `src/editor/render.rs` 作为 overlay 渲染（类似 `render_code_run_dialog_overlay`），由 Editor 持有 `inline_code_run_popover: Option<InlineCodeRunPopoverState>`

### 状态同步
- 执行开始/进度/结束时 `cx.notify()` 刷新浮层
- 块失焦或 span 被编辑删除时，清理对应 `inline_code_runs` 条目

## i18n（本步至少补英文默认 + zh-CN）
新增 key 示例：
- `inline_code_run_tooltip`
- `inline_code_run_output_title`
- `inline_code_run_no_code_at_cursor`（光标不在行内代码时的提示，可用 toast 或静默忽略）

## 图标
若新增 SVG：
1. 放 `assets/icon/...`
2. `src/main.rs` 的 `VelotypeAssets::load()` 注册
3. UI path 与注册 key 一致

## 测试
- GPUI 测试：在行内代码中触发快捷键，浮层出现且状态变为 Done（可 mock 简单 `echo`）
- 光标不在行内代码时快捷键无效果

## 验收
- `./scripts/dev.sh` 下手动验证：
  1. 输入 `` `echo hello` ``
  2. 光标移入，按 Cmd+Enter 或点运行按钮
  3. 浮层显示 `hello`
- `cargo test` 通过
```

---

## 步骤 4/5：系统终端兜底 + 偏好设置（可选）

```markdown
# 步骤 4/5：系统终端兜底与偏好

依赖步骤 3 的基本 UI。增加「在系统终端中执行」备选路径，保持应用轻量。

## 任务

### 4.1 偏好项（`src/config/preferences.rs`）
新增持久化字段：
```rust
pub(crate) inline_code_run_in_system_terminal: bool  // 默认 false
```
偏好窗口增加开关，文案如：「行内代码在系统终端中执行」

### 4.2 系统终端启动（新建 `src/code_runner/system_terminal.rs` 或放 `mod.rs`）
实现 `pub fn open_in_system_terminal(command: &str, work_dir: &Path) -> Result<()>`：

**macOS**（优先）：
```rust
// 方案 A：osascript 告诉 Terminal.app
// 方案 B：open -a Terminal <script>
```
命令需：`cd <work_dir> && <user_command>`，注意 shell 转义

**非 macOS**：本步可返回明确错误并在 UI 提示「当前平台不支持」，或留 `#[cfg]` 桩

### 4.3 执行路径分支
在 `start_inline_code_run` 中：
- 若 `inline_code_run_in_system_terminal == true`：调用 `open_in_system_terminal`，**不**启动子进程代理，浮层仅显示「已在系统终端中打开」
- 否则：走步骤 1 子进程代理

### 4.4 右键菜单（若步骤 3 未做）
渲染模式右键：当光标在行内代码内时显示：
- 「执行行内代码」
- 「在系统终端中执行」（可忽略偏好，强制走系统终端一次）

## i18n
- `preferences_inline_code_system_terminal_label`
- `inline_code_run_opened_in_terminal`

## 测试
- 单元测试：macOS 下 `open_in_system_terminal` 构造的命令字符串转义正确（不真开终端）
- 偏好序列化/反序列化测试

## 验收
- 偏好开关切换后行为正确
- macOS 手动验证能打开 Terminal 并 cd 到文档目录执行命令
- `cargo test` 通过
```

---

## 步骤 5/5：收尾 — 测试、边界情况、文档

```markdown
# 步骤 5/5：收尾与质量

依赖步骤 1–4 全部完成。补齐边界情况、测试与最小文档。

## 边界情况（必须处理）

| 场景 | 期望行为 |
| --- | --- |
| 行内代码为空 `` `` | 不执行，静默或轻提示 |
| 光标在行内代码边界外 | 不触发 |
| 选区覆盖多个 span 仅部分是 code | 仅当唯一 code span 可确定时执行，否则提示 |
| 行内代码含多行（硬换行） | 允许执行，popover 多行折叠 |
| 同时有围栏块 run 与行内 run | 互斥或允许并行（优先：**互斥**，停止另一个前先停当前 active run） |
| `allow_code_execution == false` | 复用 Disabled 对话框 |
| 文档未保存 | 复用 UnsavedConfirm 流程 |
| 输出超长 | 截断并提示「输出已截断」 |
| 块被删除/编辑导致 span 失效 | 自动清理状态，关闭 popover |

## 测试清单
确保存在并通过：
1. `inline_code_span_at_cursor` 定位
2. shell 执行 `echo` 拿到 stdout
3. 快捷键仅在 code span 内生效
4. 确认对话框流程（首次、未保存、禁用）
5. 输出截断逻辑
6. （如有）系统终端命令转义

运行：`cargo test`

## 文档
在 `docs/development.zh-CN.md` 增加简短小节「行内代码执行」：
- 快捷键
- 默认 shell 行为
- 偏好项说明
- 与围栏代码块执行的区别

## 代码整理
- 删除调试 `println!`
- 确认无未使用的 import
- 确认 i18n 英文与 zh-CN 条目齐全

## 最终验收（手动）
1. `` `date` `` → 浮层显示日期
2. `` `ls` `` → 显示目录列表（折叠）
3. `` `sleep 5` `` → Running 态可停止
4. 关闭代码执行偏好 → 弹出禁用提示
5. 开启「系统终端」偏好 → Terminal 打开（macOS）
6. 围栏代码块执行仍正常

## 输出
完成后请给出：
- 改动文件列表
- 新增快捷键
- 已知限制（如无 TTY、交互式命令不支持等）
- 建议的 git commit 消息（简体中文，符合 `.cursorrules`）
```

---

## 架构示意

```
行内代码点击执行 / Cmd+Enter
        ↓
Block::inline_code_span_at_cursor()
        ↓
Editor::request_inline_code_run()
        ↓
安全门（偏好 / 首次确认 / 未保存）
        ↓
┌─────────────────────────────────────┐
│ inline_code_run_in_system_terminal? │
└─────────────────────────────────────┘
        ↓ 否                    ↓ 是
spawn_inline_shell_run()   open_in_system_terminal()
        ↓                         ↓
stdout/stderr 流式回调      「已在系统终端中打开」
        ↓
Popover 浮层展示结果
```

## 已知限制（实施后预期）

- 无 PTY：不支持 `vim`、`top` 等交互式命令（需走系统终端兜底）
- 行内代码默认 shell，无法像围栏块那样指定 `python` / `node` 语言
- 系统终端兜底当前仅完整支持 macOS
