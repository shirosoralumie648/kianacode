# P3-I-03 Company 全链重建基线

> 快照日期：2026-09-18。本文记录从 EventLog/Artifact refs 重建 Objective→Project→Packet→Run→Acceptance→Receipt 的边界；运行时验收由 GitHub Actions 负责，本地不运行测试。

## Event-sourced chain

`ControlPlane::load_company` 只读取受保护 `company` aggregate stream，按 stream version 交给 `CompanyReplayReducer`；reducer 校验 aggregate/root/owner、schema/migration、command/event name、expected revision、idempotency、sequence、state transition 后返回 `CompanyState`。`company_snapshot`/`company_view` 从该 state 投影十类对象和 scheduling，任何 UI/缓存/模型文本都不是第二事实源。

基础链的对象和引用在同一 state 中可定位：Objective/Initiative → Project → Milestone/WorkPacket → CompanyRun（匹配 execution request/session/status/evidence）→ Acceptance/CompanyReview → Delivery/artifact refs → ClosingReceipt；runtime receipt 通过 `events_for_persisted_run`/`receipt_from_events` 继续按 run/event/invocation/source cursor 重建。Artifact 注册/读取校验 path、hash、immutable content、revocation 和 evidence owner，Receipt/CompanyProof 只保留 redacted refs/digests。

## Deny-first recovery

新进程遇到 unknown schema/field、aggregate mismatch、cursor/version gap、duplicate/idempotency、revision/role/owner 冲突、未批准 project/packet、过期 claim/approval、artifact 变化/撤销、evidence 不归属、terminal conflict 或 `ResultUnknown` 时保持 blocked/Unknown，不执行 Runner/Provider/Broker，不把 reservation 当 started、不把 runtime Completed 当业务 Outcome。重复 command 返回原 CompanyCommandReceipt，原 EventLog 保持不可变。

## CI-only 验收

`new_process_rebuilds_the_company_chain` core source guard 覆盖 reducer/load/snapshot/view、全链对象、artifact/evidence/receipt refs 和所有 deny markers；现有 co08 replay、co06 artifact、co07 receipt 与 OA-27 Company governance fixtures 负责行为回归。

```text
cargo fmt --all --check
cargo test -p kiana-core --test p3_i03_company_chain --locked -- --test-threads=1
cargo test -p kiana-domain --test co08_replay --locked -- --test-threads=1
cargo test -p kiana-domain --test co06_artifact --locked -- --test-threads=1
cargo test -p kiana-domain --test co07_receipt --locked -- --test-threads=1
cargo test -p kiana-core --test oa27_company_governance --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行 `cargo fmt --all`、`cargo fmt --all --check`、`cargo check --workspace --tests --locked --offline` 和 `git diff --check`；不执行测试或 smoke binary，也不等待 GitHub CI。

## 限制与交接

- Company aggregate/replay is local EventLog projection with explicit v0 migration; no independent cross-process aggregate index, durable snapshot checkpoint, power-loss recovery or full historical upcast is claimed.
- Artifact refs validate governed local files and hashes; external Provider/MCP/DB/notification effects, generated artifacts and physical business outcomes require separate receipts/reconciliation and remain ER/PD/DEP/INT/SC work.
- This slice verifies source wiring and CI fixtures, not a live end-to-end Company lifecycle; runtime/live/physical proof is intentionally not promoted.
