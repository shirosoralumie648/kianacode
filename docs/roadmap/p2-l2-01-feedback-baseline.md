# P2-L2-01 反馈与候选改进基线

> 快照日期：2026-09-18。本文记录反馈的候选化、证据和质量复核边界；运行时验收由 GitHub Actions 负责，本地不运行测试。

## Candidate-only contract

ControlPlane 的 `feedback.submit` 只接受有界 category/observation/proposed change、唯一 candidate id 和已提交 EventLog evidence，服务端把 actor/session、evidence refs 和提交时间写入 `FeedbackCandidate`，追加 `feedback.candidate_created`。候选从历史重建并按 idempotency key、request digest 和 platform revision CAS 防止重复或冲突。

`feedback.review` 只能由独立 reviewer/sponsor 在不同 session 提交，要求 evidence 和 `quality_approved`/`rejected` 结果；重复 review、缺候选、缺证据、同 session 自审和字段越界均拒绝。review 事实明确标记 `promotion="candidate_only"`、`authority_changes_applied=false`，不会直接修改 Role、Grant、Policy、Approval、Receipt 或既有 EventLog。

GoldenTrace/quality contracts 可作为候选的证据来源，但本切片不把 feedback 当成 quality promotion authority；后续质量评估、Candidate patch、quality gate、promote/rollback 仍须经过独立 EQ/ER/SC 流程。

## CI-only 验收

`feedback_cannot_mutate_policy_or_history` core source guard 覆盖 submit/review candidate lifecycle、evidence/independent-reviewer/duplicate/CAS/idempotency fences、candidate-only promotion 标记和无 Model/Provider/Broker/Policy mutation 边界；P2-K3 Human Inbox guard 作为入口回归。

```text
cargo fmt --all --check
cargo test -p kiana-core --test p2_l2_01_feedback --locked -- --test-threads=1
cargo test -p kiana-core --test p2_k3_01_human_inbox --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行 `cargo fmt --all`、`cargo fmt --all --check`、`cargo check --workspace --tests --locked --offline` 和 `git diff --check`；不执行测试或 smoke binary，也不等待 GitHub CI。

## 限制与交接

- FeedbackCandidate 当前存于受保护 human-operations platform stream 并由 projection 重建；没有独立 FeedbackStore、跨进程 queue、自动诊断或自动 promotion worker。
- candidate-only review 不等于质量通过、Policy/Grant 变更或生产发布；真实质量效果、漂移和回滚须由 EQ/ER/DEP/SC 的受控 evaluator/gate/receipt 证明。
- evidence refs 只证明提交的本地事实关联，不证明外部业务 outcome、live provider 效果或任何现实世界改进。
