---
status: testing
phase: 01-baseline-evidence-governance
source: [01-VERIFICATION.md]
started: 2026-07-26T02:24:44Z
updated: 2026-07-26T02:24:44Z
---

## Current Test

number: 1
name: 运行新鲜进程生产验证回归测试，确认 Plan 01-13 修复生效
expected: |
  cargo test linux_production_slice_has_fresh_process_headroom 通过；
  两个独立样本 elapsed_seconds < 25；offline=true；无 deadline_exceeded；无网络追踪；无运行时根残留。
awaiting: user response

## Tests

### 1. 新鲜进程生产验证回归（HV-1 + HV-2）
expected: |
  两个独立的 bwrap/Bash/Python 链各在 25 秒内完成，Rust 报告 elapsed_seconds < 25，offline=true，
  无网络 syscall，sandbox/receipt/cleanup 全部通过。
  Python 封闭报告测试（14-check schema）全部通过。
  capability-governance-smoke.sh production 退出 0。
commands:
  - "cargo test -p kiana-capability-governance-supervisor --test supervisor_linux linux_production_slice_has_fresh_process_headroom --locked --offline -- --exact --test-threads=1"
  - "python3 scripts/tests/test_generate_capability_governance.py"
  - "bash scripts/capability-governance-smoke.sh production"
result: [待测试]

### 2. 49 能力分解可信度人工判断（HV-3）
expected: |
  维护者确认：docs/agent-program/kiana-completion/governance/ 中的 49 项能力分解
  忠实诠释了捕获的官方公开资料，没有超出或不足。
  （01-08 D3 — 自动化无法替代此判断）
result: [待测试]

### 3. 许可证解读与借用理由人工判断（HV-4）
expected: |
  维护者确认：governance evidence 和 reference registry 中的
  source-specific 许可证解读及借用理由准确且符合项目策略。
  （01-09 D3 — 自动化无法替代此判断）
result: [待测试]

## 摘要

total: 3
passed: 0
issues: 0
pending: 3
skipped: 0
blocked: 0

## Gaps
