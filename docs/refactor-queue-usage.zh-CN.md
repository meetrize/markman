# 重构执行队列 — 使用说明

本目录配合 [refactor-execution-plan.zh-CN.md](refactor-execution-plan.zh-CN.md) 使用，在 Cursor Agent 中**自动链式执行** 22 步重构。

## 机制

| 文件 | 作用 |
| --- | --- |
| `.cursor/refactor-queue/steps.json` | 22 步标题与状态（`pending` / `in_progress` / `done`） |
| `.cursor/refactor-queue/state.json` | 队列开关 `enabled`、下一待执行步 `nextStep` |
| `.cursor/hooks/refactor-queue-next.sh` | Agent **stop** 时读取状态，通过 `followup_message` 投递下一步提示词 |
| `.cursor/hooks.json` | 注册 stop hook（`loop_limit: 25`） |

## 启用 / 暂停

```json
// .cursor/refactor-queue/state.json
{
  "enabled": true,
  "nextStep": 3,
  "totalSteps": 22
}
```

- `enabled: false` — 暂停自动链式执行（Agent 正常结束，不投递下一步）
- `nextStep` — 从第 N 步开始（步骤 1 已完成时可设为 `2`）

修改 `state.json` 或 `steps.json` 后无需重启 Cursor；若 hook 未生效，重启 IDE 或检查 **Hooks** 输出通道。

## 手动入队（Cursor 聊天队列）

Agent 运行中，将下方提示词粘贴到输入框后按 **Enter**（非 Cmd+Enter）即可加入 **Queued messages**，当前任务结束后顺序执行：

```markdown
请阅读 `docs/refactor-execution-plan.zh-CN.md` 中的 **步骤 {N}/22：{标题}** 及文首「总览块」。

要求：
1. 只做该步骤范围内的改动
2. 行为等价，不顺手改其他模块
3. 完成后运行 `cargo test` 和 `cargo build --release`
4. 用中文总结：改了哪些文件、如何验证、建议的 commit message

开始执行。
```

## 全部 22 步标题

见 `.cursor/refactor-queue/steps.json` 或 [refactor-execution-plan.zh-CN.md](refactor-execution-plan.zh-CN.md) 阶段总览。

## 完成后的清理

全部 22 步完成后：

1. 将 `state.json` 中 `enabled` 设为 `false`
2. 可选：从 `.cursor/hooks.json` 移除 `stop` 中的 `refactor-queue-next.sh` 条目
