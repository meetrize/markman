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
