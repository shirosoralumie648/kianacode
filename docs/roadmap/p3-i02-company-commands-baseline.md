# P3-I-02 Company 命令与事件冻结基线

> 快照日期：2026-09-18。本文记录 Company 生命周期九个基础命令/事件对的 versioned contract；运行时验收由 GitHub Actions 负责，本地不运行测试。

## Frozen lifecycle pairs

以下九个基础对固定在 `CompanyCommand::event_name()` 与 `COMPANY_COMMAND_SCHEMA`/`COMPANY_EVENT_SCHEMA`：

| command | committed event |
|---|---|
| `propose_objective` | `ObjectiveProposed` |
| `approve_project` | `ProjectApproved` |
| `create_milestone` | `MilestoneCreated` |
| `approve_packet` | `PacketApproved` |
| `start_run` | `RunStartRequested` → `RunStarted` observation |
| `request_acceptance` | `AcceptanceRequested` |
| `decide_acceptance` | `AcceptanceDecided` |
| `close_project` | `ProjectClosed` |
| `record_outcome` | `OutcomeRecorded` |

`start_run` deliberately has a two-phase Company fact: the command commits a durable `RunStartRequested` reservation before the runtime is launched; ControlPlane later records `RunStarted` only after matching run evidence. This preserves the roadmap shorthand while preventing a requested run from being presented as started without observation.

`CompanyCommandRequest` and `CompanyEvent` use strict `deny_unknown_fields` DTOs and schema version strings. Every committed event uses `company.<event_name>` aggregate stream metadata, stable idempotency key and expected revision; `CompanyReplayReducer` validates aggregate/owner/root, stream sequence, command/event identity, schema migration and state transition before rebuilding. Unknown schema, gaps, duplicate commands, revision/idempotency conflicts or unauthorized roles fail-closed.

## ControlPlane boundary

Company command policy derives allowed roles/decision purpose from the typed command. `ControlPlane::handle_company_command` validates context, schema, idempotency, revision and policy before `CompanyState::transition`, then commits one CompanyEvent; only explicit post-commit dispatch intents use the existing runtime spine. Protocol/CLI/Web/Workbench submit the same versioned request; no model text or UI label defines a new command/event.

## CI-only 验收

`company_commands_are_frozen_and_versioned` core source guard checks all nine pairs, schema/version/strict DTOs, event stream/idempotency/replay fences and ControlPlane authority. Existing `co08_replay` and `co08_replay_guard` verify deterministic replay, legacy v0 migration and unknown schema/gap/duplicate rejection; `co05_policy` covers role/decision policy.

```text
cargo fmt --all --check
cargo test -p kiana-core --test p3_i02_company_commands --locked -- --test-threads=1
cargo test -p kiana-domain --test co08_replay --locked -- --test-threads=1
cargo test -p kiana-core --test co08_replay_guard --locked -- --test-threads=1
cargo test -p kiana-domain --test co05_policy --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行 `cargo fmt --all`、`cargo fmt --all --check`、`cargo check --workspace --tests --locked --offline` 和 `git diff --check`；不执行测试或 smoke binary，也不等待 GitHub CI。

## 限制与交接

- 本切片冻结基础命令/event names and versioned replay contracts; the larger CompanyCommand enum remains extensible only by additive versioned variants and already contains later change/risk/incident/delivery operations.
- Company stream and replay are local EventLog projections; durable cross-process aggregate index, full Objective→Project→Packet→Run→Acceptance→Delivery rebuild and power-loss proof remain P3-I-03/ER/PD.
- `RunStartRequested` is a reservation, not proof of runtime start; `RunStarted` requires observed run evidence. No Company event or receipt proves external business outcome, acceptance delivery or live/physical effect.
