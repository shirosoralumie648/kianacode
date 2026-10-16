# P0-A-01b schema registry baseline

> 原始快照日期：2026-09-16。历史验证策略为仅由 GitHub Actions 执行夹具；2026-10-06 用户明确授权本地完整测试后，按有界并发执行本地回归，结果见下方 current-head receipt。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`P0-A-01b`](../roadmap.md#step-p0-a-01b) |
| source snapshot | `4d48220`（CO-08 parent）加本步 evidence/fixtures；最终 commit 记录在 git history |
| feature_status | `implemented`（existing domain registry/event registry verified and CI-wired） |
| proof_level | `source`；静态编译与远程 fixtures 不提升为 local_behavior/durable/live/physical |
| authority path | schema_contract/SchemaLayer + check_schema_compatibility → strict domain serde / wire compatibility → event_kind registry/migration → existing ControlPlane/EventLog |
| this step does | 为已存在的 `SCHEMA_CONTRACTS`、`SchemaLayer`、`SchemaVersion` 和 `EVENT_KIND_SPECS` 建立专用验收与 source guard；确认 wire 可按规则接收 additive minor，domain/runtime unknown field 拒绝，unknown major/schema/event 不执行，已登记 migration 只能显式使用 |
| this step does not | 不另造第二 schema registry、不把 unknown opaque event 当执行事实、不改变已有业务状态迁移语义；具体 upcaster、全量 legacy event coverage 和 durable migration runner 留给 ER/PD/CO-08+ |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Schema contracts | `kiana-domain/src/contracts.rs` | `aa796d275675e113d0a5fa338664fae0d4234357ee833de19acb384e72370403` |
| Event kind/migration registry | `kiana-domain/src/event_contracts.rs` | `5ba2b23e9999bb07ea39d26fe0672a19f1e519c262ae165c222f9cb6be24f4cb` |
| Protocol/core guard | `kiana-protocol/src/lib.rs`, `kiana-core/tests/p0_a01b_schema_guard.rs` | `a5efad3bbff0cd67f82ad79b4d137dfec3424cc1c6f3969ba9eaacc356cc260c`, `91a134ebb6af366c1d1208695f8ec80222268a539202035fa5f910df155ffd34` |
| Fixtures and CI | `kiana-domain/tests/p0_a01b_schema.rs`, `.github/workflows/p0-a01b-schema.yml` | `a88d52e960890ddb85cc3a691ed5f43a69db4daa5cc03f06476e5cd3715d0c95`, `f21f495ae29e7b9b300dee78d5d0c73b10dd01ebe2835ec0e57ce55ca51caea3` |

## 2. Registry rules

`SchemaContract` 为每个公开 schema 固定 owner、layer、当前版本、compatibility 和 unknown-field policy。`check_schema_compatibility` 对未知 schema 直接错误，major 不匹配 fail-closed，minor 在同一 major 内按 contract 规则兼容；domain/runtime contracts 的 `allow_unknown_fields=false` 与 `serde(deny_unknown_fields)` 配对，wire envelope 保留 additive 兼容边界。

`EventKindSpec`/`EVENT_KIND_SPECS` 登记 required IDs、allowed fields、aggregate/terminal/secret policy 和 migration metadata。未知但可读的 opaque event 只可查询；落在 request/run/capability/approval/invocation/execution/action/session family 的未知 kind 直接 `unknown_required_event_kind`。`event_migration` 只返回登记过的 family/from/to upcaster 名称，不猜测未来 major。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `schema_registry_separates_layers_and_fails_closed_on_unknown_major` | wire/domain/runtime layer 与 unknown field policy 清晰，未知 schema/major 拒绝，minor 兼容 |
| `event_unknown_and_migration_rules_are_explicit` | known/opaque/required unknown event 和 migration registry 行为显式 |
| `schema_and_event_registry_are_the_single_compatibility_boundary` | protocol/core 只复用这两个 registry，不新增旁路判断 |

`.github/workflows/p0-a01b-schema.yml` 在 GitHub runner 执行 domain schema/event fixtures、core source guard、fmt 和 domain/protocol/core test-target compile；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- registry 已覆盖当前列出的 contracts/event kinds，但并不证明所有 legacy payload 都有 upcaster；事件 registry 仍需 ER-02+ 全量接线和 durable projector 验收。
- unknown major 的原始数据保留/隔离由具体 EventStore/PD migration adapter 决定；本步只证明 compatibility decision fail-closed。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。

## 5. Current-head registry receipt (2026-10-06)

Full-workspace testing exposed eight public ID types missing from `ID_CONTRACTS` and the serde round-trip list, plus two current schema owners missing from the owner allow-list. `kiana-domain/src/contracts.rs` now registers `WorkflowInstanceId`, `UsageId`, `ReservationId`, `LedgerEntryId`, `CostAllocationId`, `RateCardId`, `CostCorrectionId`, and `QuotaReservationId`, and includes `kiana-policy` and `kiana-skills` in the owner check. The existing registry remains the sole schema authority.

The focused command `cargo test -p kiana-domain --lib contracts::tests --locked --offline -- --test-threads=1` passed 103/103 tests with one Cargo job, one test thread, a 32 MiB test stack, stripped/non-incremental profile, peak RSS 3,580,072 KiB, and 0 swaps. `cargo test -p kiana-domain --test p0_a01b_schema --locked --offline -- --test-threads=1` passed 2/2 (peak RSS 4,085,736 KiB, 0 swaps). The Core single-registry guard initially failed because `kiana-protocol` did not re-export the domain compatibility helper; the protocol now re-exports `check_schema_compatibility` without duplicating the registry, and `cargo test -p kiana-core --test p0_a01b_schema_guard --locked --offline -- --test-threads=1` passed 1/1 (peak RSS 133,000 KiB, 0 swaps). The first contracts run failed on the missing IDs and owners before these registrations were added. Workspace regression is still non-green and current-head remote CI was not run; this receipt proves only focused local registry behavior and does not promote the complete P0-A-01a/01b cards to done.

## 6. P0-A-01a correlation ID registry follow-up (2026-10-07)

A second source audit found that the two public serde IDs in `kiana-domain/src/correlation.rs`, `TraceId` and `SpanId`, were not included in `ID_CONTRACTS` or the round-trip list. The source guard only scanned `ids.rs`, so the earlier 103/103 result did not cover those public types. The registry now records them as string-shaped IDs with their `trace_id` and `span_id` wire names; deterministic valid lowercase hex samples exercise serialization, deserialization and string wire shape. The declaration guard now scans both source files.

RED added the two samples first and failed as expected: 102/105 passed, with the two missing-contract round trips and registry-count assertion failing. An intermediate run exposed that a valid 32-character trace hex string is accepted by the UUID parser; the wire-shape test now requires canonical UUID hyphens before classifying a string as UUID. Final GREEN `cargo test -p kiana-domain --lib contracts::tests --locked --offline -- --test-threads=1` passed 105/105 (38.64s, 95% CPU, peak RSS 3,580,256 KiB, 0 swaps). The Core single-registry guard `cargo test -p kiana-core --test p0_a01b_schema_guard --locked --offline -- --test-threads=1` passed 1/1 (1m30.61s, 86% CPU, peak RSS 3,995,748 KiB, 0 swaps). The RED run was 40.21s at 93% CPU and peak RSS 3,565,620 KiB, also with 0 swaps. Commands used one Cargo job/test thread, a 32 MiB test stack, stripped test debug info and no incremental compilation; exact logs/resources are under `/tmp/kiana-roadmap-20261007-p0a01a/`. Formatting and diff checks passed. This closes only the selected registry coverage gap; full workspace remains non-green, remote CI was not run, and P0-A-01a/01b roadmap rows remain 🔄.

## 7. Breaking compatibility policy receipt (2026-10-08)

`check_schema_compatibility` now consumes `SchemaContract.compatibility`: backward-compatible
contracts continue to accept same-major minor versions, while breaking contracts require the exact
registered version until an explicit migration is selected.

```text
source_snapshot: current dirty worktree; kiana-domain/src/contracts.rs; kiana-domain/tests/p0_a01b_schema.rs; kiana-core/tests/p0_a01b_schema_guard.rs
worktree_status: P0-A-01b row 068 remains 🔄 / feature_status=implemented / proof_level=source; selected Breaking-policy enforcement is locally verified; no migration runner or second registry
command_argv: RED `cargo test -p kiana-domain --test p0_a01b_schema schema_registry_separates_layers_and_fails_closed_on_unknown_major --locked --offline -- --exact --test-threads=1`; GREEN domain `cargo test -p kiana-domain --test p0_a01b_schema --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test p0_a01b_schema_guard --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; one Cargo job/test thread, `RUST_MIN_STACK=33554432`, stripped/non-incremental profile; RED max RSS 221548 KiB / 34% CPU; domain GREEN max RSS 82932 KiB / 15% CPU; Core guard max RSS 85324 KiB / 27% CPU; all measured runs reported 0 swaps
fixture·cassette: RED `kiana.deployment-startup-fact.v1` accepted incoming 1.1 despite `Breaking`; GREEN domain 2/2 preserves protocol same-major minor compatibility and exact Breaking 1.0, rejects Breaking 1.1/unknown major/schema; Core guard 1/1 locks policy consumption
exit_code: RED 101 expected; GREEN domain/Core 0
status_change: selected P0-A-01b Breaking-policy enforcement moves source → local_behavior; whole card remains feature_status=implemented, proof_level=source
proof-level_change: compatibility policy branch source → local_behavior; no durable/live/physical promotion
limitations: explicit migration/upcaster selection, full legacy payload coverage, durable migration runner and workspace/remote CI closure remain open
reviewer: root integration agent with P0-A registry audit; focused schema compatibility reviewer
```

## 8. P0-A-01a ID owner metadata fence (2026-10-08)

`every_public_type_has_one_owner_and_a_conversion_test` now requires every registered ID owner to
equal the package defining the scanned ID declarations. The prior check only required the owner to
be any workspace member, so a valid but foreign crate could be recorded without detection. RED
temporarily assigned `RequestId` to `kiana-protocol`, which is a real workspace member, and the new
assertion rejected it; the registry value was restored to `kiana-domain` before GREEN.

```text
source_snapshot: current dirty worktree; kiana-domain/src/contracts.rs; P0-A-01a ID_CONTRACTS and source scan for ids.rs/correlation.rs
worktree_status: selected owner metadata contract is locally verified; roadmap row 001 remains 🔄; registry data and wire representation unchanged
command_argv: RED exact `cargo test -p kiana-domain --lib contracts::tests::every_public_type_has_one_owner_and_a_conversion_test --locked --offline -- --exact --test-threads=1`; GREEN `cargo test -p kiana-domain --lib contracts::tests --locked --offline -- --test-threads=1`; `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; one Cargo build job/test thread, 32 MiB test stack, stripped/non-incremental test profile, locked offline dependencies; RED max RSS 3577792 KiB / CPU 89%; GREEN max RSS 3581472 KiB / CPU 94%; both commands reported 0 swaps
fixture·cassette: RED 0/1 passed because `RequestId.owner_crate="kiana-protocol"` mismatched package `kiana-domain`; GREEN `contracts::tests` 105/105 including all ID serde round trips, registration scan, owner and schema registry tests
exit_code: RED 101 expected; GREEN and static checks 0
status_change: selected owner metadata assertion promoted to local_behavior; P0-A-01a remains 🔄 due workspace regression and remote CI not closed
proof-level_change: exact ID owner matching source → local_behavior for the focused domain test group; no durable/live/physical promotion
limitations: Rust does not provide runtime reflection for source ownership; current assertion pins the scanned declarations in `ids.rs` and `correlation.rs` to kiana-domain; full workspace regression, remote CI and P0-A-01a downstream acceptance remain open
reviewer: root integration agent with read-only P0-A-01a frontier audit
```

## 9. P0-A-01a focused CI lane wiring (2026-10-13)

The existing ID registry contracts live in `kiana-domain` library unit tests, which the integration-test shard matrix does not select. A manual-only workflow now runs the existing `contracts::tests` module, checks formatting, and compiles the selected unit-test module. It does not duplicate the ID registry or change production code.

```text
source_snapshot: current dirty worktree; .github/workflows/p0-a01a-id-contracts.yml; existing kiana-domain/src/contracts.rs unit tests; scripts/ci/validate-workflows.sh
worktree_status: P0-A-01a row 001 remains 🔄; workflow definition adds a manual CI lane only; no shard/manifest/lockfile/production logic change; no workflow run ID exists yet
command_argv: `bash scripts/ci/validate-workflows.sh`; YAML parse and trigger/resource configuration inspection; `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; workflow sets one Cargo build job, one Rust test thread, 32 MiB test stack, stripped/non-incremental test profile
fixture·cassette: workflow validator accepted 255 workflow files (5 automatic, 250 manual) and verified all 33 workspace members / 41 shards / 1473 integration targets; new manual lane selects the existing `contracts::tests` unit tests and no-run compile
exit_code: workflow structure/shard validation 0; final format/diff checks recorded with this receipt; no remote workflow result observed
status_change: P0-A-01a focused CI path is now defined; roadmap status remains 🔄 until a current-head run and full card exit conditions are evidenced
proof-level_change: workflow wiring is source only; no CI/live/durable promotion
limitations: the lane has not been dispatched and has no run ID/result; full workspace regression still exceeds the bounded local run; unit source scan is limited to current ID declaration macros and files; schema/event migration and downstream card evidence remain separate
reviewer: root integration agent with read-only P0-A-01a CI-wiring audit
```

## 10. P0-A-01a current-head local registry receipt (2026-10-13)

The current dirty source snapshot still satisfies the existing ID registry unit contract suite.
This is a local behavior receipt for the selected registry boundary; the manual workflow above has
not been dispatched, so it is not a remote CI receipt.

```text
source_snapshot: current dirty worktree; kiana-domain/src/contracts.rs; .github/workflows/p0-a01a-id-contracts.yml
worktree_status: P0-A-01a row 001 remains 🔄; existing ID registry source and tests are unchanged by the workflow wiring; no commit/push/merge/rebase
command_argv: bounded `cargo test -p kiana-domain --lib contracts::tests --locked --offline -- --test-threads=1`; workflow structure `bash scripts/ci/validate-workflows.sh`; `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; one Cargo/compiler job, one Rust test thread, 32 MiB test stack, stripped/non-incremental locked/offline profile; no swap
fixture·cassette: `contracts::tests` 105/105; ID declaration scan covers `ids.rs` and `correlation.rs`, owner exactness and all registered round trips; workflow validator accepted 255 workflows / 5 automatic / 250 manual and 41 shards / 1473 integration targets
exit_code: local test 0; max RSS 3594924 KiB / 93% CPU / 36.29s; workflow validator 0; resource receipt `/tmp/kiana-roadmap-20261010-evidence/p0a01a-contracts-current.{log,resources}`
status_change: selected ID registry behavior remains local_behavior; manual CI evidence lane is source-wired; P0-A-01a remains open
proof-level_change: selected registry source → local_behavior; workflow definition source only; no remote CI/durable/live/physical promotion
limitations: no current-head GitHub workflow run ID/result; full workspace regression remains bounded-timeout; source scan is macro/text based and does not prove cross-crate or durable registry coverage; P0-A-01b schema/event migration remains separate
reviewer: root integration agent with read-only P0-A-01a CI-wiring audit; focused contracts test reviewer
```
