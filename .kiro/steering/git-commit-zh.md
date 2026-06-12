---
inclusion: always
---

# Git 提交消息规范

生成 Git 提交消息时（Source Control ✨ 按钮、Agent 提交、PR 标题建议）**必须使用简体中文**。

## 格式

`<type>(<scope>): <简短中文描述>`

## type

- `feat` 新功能
- `fix` 修复
- `docs` 文档
- `style` 格式
- `refactor` 重构
- `test` 测试
- `chore` 构建/工具

## 要求

1. 标题一行，≤ 50 字，说明改动目的
2. 即使 `git log` 近期为英文，仍输出中文
3. scope 可选，如 `toolbar`、`editor`、`i18n`

## 示例

- `feat(toolbar): 添加 Markdown 格式工具栏`
- `fix(block): 修复工具栏加粗标记内输入光标位置`
- `docs: 更新开发指南`

## Kiro Git Commit 按钮专用规则

当用户点击 Git 面板的 "generate commit message" 按钮时：

1. **强制要求**：必须使用简体中文生成提交消息
2. **不要**询问用户或提供英文选项
3. **直接输出**符合格式的中文提交消息
4. 如果变更内容不明确，基于 git diff 推断后生成中文消息
