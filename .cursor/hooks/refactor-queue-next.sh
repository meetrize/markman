#!/usr/bin/env bash
# Cursor stop hook: enqueue the next refactor step via followup_message.
# Enable/disable in .cursor/refactor-queue/state.json ("enabled": true/false).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
STATE="$ROOT/.cursor/refactor-queue/state.json"
STEPS="$ROOT/.cursor/refactor-queue/steps.json"

python3 - "$STATE" "$STEPS" <<'PY'
import json
import sys

state_path, steps_path = sys.argv[1], sys.argv[2]

try:
    with open(state_path, encoding="utf-8") as f:
        state = json.load(f)
except OSError:
    sys.exit(0)

if not state.get("enabled"):
    sys.exit(0)

next_step = int(state.get("nextStep", 1))
total = int(state.get("totalSteps", 22))
plan_doc = state.get("planDoc", "docs/refactor-execution-plan.zh-CN.md")

with open(steps_path, encoding="utf-8") as f:
    steps = json.load(f)

if next_step > total:
    print(
        json.dumps(
            {
                "followup_message": (
                    f"重构执行队列已完成（{total}/{total}）。"
                    "请在 .cursor/refactor-queue/state.json 将 enabled 设为 false。"
                )
            },
            ensure_ascii=False,
        )
    )
    sys.exit(0)

step_key = str(next_step)
step = steps.get(step_key)
if not step:
    sys.exit(0)

title = step["title"]
prompt = f"""请阅读 `{plan_doc}` 中的 **步骤 {next_step}/{total}：{title}** 及文首「总览块」。

要求：
1. 只做该步骤范围内的改动
2. 行为等价，不顺手改其他模块
3. 完成后运行 `cargo test` 和 `cargo build --release`
4. 用中文总结：改了哪些文件、如何验证、建议的 commit message

开始执行。"""

step["status"] = "in_progress"
state["nextStep"] = next_step + 1
with open(steps_path, "w", encoding="utf-8") as f:
    json.dump(steps, f, indent=2, ensure_ascii=False)
    f.write("\n")
with open(state_path, "w", encoding="utf-8") as f:
    json.dump(state, f, indent=2, ensure_ascii=False)
    f.write("\n")

print(json.dumps({"followup_message": prompt}, ensure_ascii=False))
PY
