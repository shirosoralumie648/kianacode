# P3-I-05 Delivery / ClosingReceipt / Outcome 基线

> 快照日期：2026-09-18。本文记录项目交付、关闭回执和 Outcome 测量边界；运行时验收由 GitHub Actions 负责，本地不运行测试。

## Closeout contract

Company `CloseProject` 只有在 project `Accepted`、Acceptance 有独立 Review、Delivery 属于同一 project/acceptance 且 `Confirmed`、handoff receipt/evidence 存在、所有 Incident 已解决/关闭并且没有重复 closing receipt 时才创建 `CompanyClosingReceipt`，随后把 project/acceptance/milestone 推进到 Closed。Delivery 生命周期 Prepared→Approved→Delivered→Confirmed；未知投递先进入 Incident/`DeliveryUnknown`，必须带 recipient/evidence 再 Reconcile，不能把 dispatch 或 delivered 自报成 confirmed。

失败关闭/人工豁免走独立 business closeout contract：必须有 human actor、停止/Unknown 已对账、named waiver/residual obligations 和完整 evidence；未解决 effect、incident、delivery obligation、acceptance 或重复 receipt 均拒绝。ClosingReceipt 只记录本机事实链和 artifact/evidence refs，不证明外部业务交付。

## Outcome measurement

`RecordOutcome` 要求 closed project、objective 关联、objective owner、measurement window（`period_start`–`period_end`）、finite observation、持久 evidence 和唯一 outcome id；`OutcomeStatus` 按 frozen objective baseline/target/direction 得到 Realized/PartiallyRealized/NotRealized。`AchieveObjective` 只有在关联 Outcome 已 Realized 时允许，Runtime Completed、Receipt 文本或模型自报永远不能直接宣称 Objective Achieved。

Business closeout additionally freezes a human-owned measurement plan before its window, accepts only bounded/hash-verified metric artifacts within the window, requires enough non-fixture observations after the window, and assesses outcome deterministically. Missing/late/duplicate/non-finite observations stay incomplete/not realized; `business_objective_current_realized_observations_required` remains a human decision gate.

## CI-only 验收

`outcome_cannot_be_claimed_without_measurement` core source guard covers Delivery/ClosingReceipt prerequisites, waiver/Unknown boundaries, measurement window/owner/evidence and runtime-completed-not-outcome guards. Existing domain Company object and OA-27 governance fixtures provide closeout/outcome regression.

```text
cargo fmt --all --check
cargo test -p kiana-core --test p3_i05_closeout_outcome --locked -- --test-threads=1
cargo test -p kiana-domain --test p3_i01_company_objects --locked -- --test-threads=1
cargo test -p kiana-core --test oa27_company_governance --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行 `cargo fmt --all`、`cargo fmt --all --check`、`cargo check --workspace --tests --locked --offline` 和 `git diff --check`；不执行测试或 smoke binary，也不等待 GitHub CI。

## 限制与交接

- Delivery/ClosingReceipt/Outcome are local Company EventLog facts and projections; no external recipient/provider receipt, durable cross-process closeout store, power-loss proof or real-world delivery is claimed.
- Outcome measurement code supports deterministic local observations and explicitly excludes fixture/missing data from objective achievement; semantic KPI validity, external data sources, legal acceptance and live/physical business impact remain CO/EQ/ER/PD/DEP/SC work.
- A closed runtime chain does not automatically imply business Outcome; all claims remain bounded by the stored criteria, evidence, measurement window and human authority.
