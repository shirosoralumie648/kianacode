# P3-I-04 Acceptance 快照与独立 Review 基线

> 快照日期：2026-09-18。本文记录验收标准快照、逐条独立 Review 和 Builder 原始事实保护边界；运行时验收由 GitHub Actions 负责，本地不运行测试。

## Frozen criteria

`CompanyCommand::RequestAcceptance` 只在 project active、相关 run/packet 完成且 evidence 可核验时创建 Acceptance；它从 project/milestone/packet 当前版本和 criteria 生成 immutable `CriteriaSnapshot`，保存所有 criteria/version maps 与 Builder run/session/evidence refs。后续 `RecordReview`/`DecideAcceptance` 只能比较和消费这个快照，不能从当前可变项目重算。

Review 必须使用不同于 `Acceptance.author_session_id` 的 reviewer session，criterion result keys 必须与 snapshot 完全一致，并引用 Acceptance 已有 evidence。Decision 再次校验 review/acceptance ID、snapshot、author run、reviewer session 和 role；accept/reject/waive 的状态转移由 CompanyState 唯一执行，Reviewer 不能改写 Builder run、output、evidence、criteria 或历史 CompanyEvent。

Business review adapter 同样把 frozen target/artifacts 当不可信 evidence，要求独立 reviewer assignment/session、completed reviewer run、matching acceptance/evidence digest 和 strict result schema；review/close artifacts 只追加 facts，不能覆盖原始 Builder receipt。Company governance projection 将 session overlap、criteria mismatch、missing review/evidence 标为 limitation。

## CI-only 验收

`reviewer_cannot_rewrite_builder_facts` core source guard 覆盖 CriteriaSnapshot capture/validation、独立 session、逐条 criterion/evidence、review identity/role checks、CompanyState transition 与 business review adapter；domain object、Company governance 和 ControlPlane fresh reviewer fixtures 作为回归。

```text
cargo fmt --all --check
cargo test -p kiana-core --test p3_i04_acceptance_review --locked -- --test-threads=1
cargo test -p kiana-domain --test p3_i01_company_objects --locked -- --test-threads=1
cargo test -p kiana-core --test oa27_company_governance --locked -- --test-threads=1
cargo test -p kiana-core --test control_plane review_author_run_uses_a_fresh_reviewer_session --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行 `cargo fmt --all`、`cargo fmt --all --check`、`cargo check --workspace --tests --locked --offline` 和 `git diff --check`；不执行测试或 smoke binary，也不等待 GitHub CI。

## 限制与交接

- CriteriaSnapshot/CompanyReview facts are local EventLog aggregate data; no cross-process durable AcceptanceStore, external review service or power-loss proof is claimed.
- Reviewer/Builder independence is enforced by session/role/assignment/evidence checks, but a successful local review does not prove semantic correctness, real-world acceptance or external delivery; EQ/CO/ER/PD/SC remain responsible for quality and business outcome gates.
- This slice protects immutable Builder facts and frozen criteria; later change requests must create a new version/Acceptance rather than mutate an existing snapshot.
