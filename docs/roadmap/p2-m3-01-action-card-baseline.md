# P2-M3-01 人工动作卡基线

> 快照日期：2026-09-18。本文记录 Approval/Review/Acceptance/Incident action card 的服务端元数据和跨入口边界；运行时验收由 GitHub Actions 负责，本地不运行测试。

## Shared action metadata

Core `HumanAction` 是 Human Inbox 的唯一 action 描述，包含稳定 action id、label/command、server-selected arguments 和 required fields；protocol `HumanActionCard` 为 versioned UI projection，绑定 target、command、expected revision/expiry、allowed decisions 和 payload digest。审批 challenge 同样由服务端提供 approval id、request hash、nonce、available decisions 和 expected version。

Approval、Review、Acceptance、Incident 的 action list 由原 Approval/Company/Failure authority 计算，再投影到 `human.inbox`；`human.resolve` 只接受 inbox digest、item/action id、所需字段和幂等键，并回到原 ControlPlane command。Web、Workbench/TTY 和 CLI 的审批/Inbox 路径只消费这些服务端字段，不从本地按钮或命令参数扩展 decision set。

## Deny-first boundaries

stale inbox revision、未知 item/action、缺 required field、额外字段、错误 target/owner/session/role、过期 approval、重复 idempotency 和 scope/epoch drift 均在服务端拒绝。UI action cursor/epoch 是额外的乐观 precondition；实际 capability、workspace、approval、company 和 incident effect 仍必须再次通过 ControlPlane，动作卡不拥有权限。

## CI-only 验收

`action_card_is_shared_by_all_surfaces` 由 P2-M2 daemon source guard、P2-K3 Human Inbox guard 与 protocol UI DTO 回归组成，覆盖 shared metadata、server decision set、required fields、revision/epoch/digest/idempotency 和 no-second-authority 边界。

```text
cargo fmt --all --check
cargo test -p kiana-daemon --test p2_m2_01_ui_projection --locked -- --test-threads=1
cargo test -p kiana-core --test p2_k3_01_human_inbox --locked -- --test-threads=1
cargo test -p kiana-protocol --test ui01_dto --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行 `cargo fmt --all`、`cargo fmt --all --check`、`cargo check --workspace --tests --locked --offline` 和 `git diff --check`；不执行测试或 smoke binary，也不等待 GitHub CI。

## 限制与交接

- 当前 `HumanAction`/`HumanActionCard` 和 approval challenge 是 EventLog/platform projection 或受控响应，不是跨进程 durable action-card/read-state/notification store。
- CLI 的旧 approval prompt、TTY Workbench、Web endpoint 和 Inbox actions 仍有不同呈现层；它们共享 server metadata/authority，但完整三入口 runtime/UI visual parity、external human auth/delivery 和 live/physical evidence 留 UI/NM/PD/SC。
- 动作卡的 available decision 不等于已批准或已执行；没有外部 receipt，不能声称业务 outcome、incident closed 或 acceptance delivered。
