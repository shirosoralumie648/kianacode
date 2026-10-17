# UI-09 schema 资产、生成和兼容门 baseline

> 快照日期：2026-09-24。UI-09 的 JSON Schema lock、生成 catalog、兼容矩阵和 fixture gate
> 由 GitHub Actions 执行；本地不运行测试、构建或检查。

## 1. 范围与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`UI-09`](ui-entrypoints.md#step-ui-09) |
| source snapshot | `0ed70bb1`（UI-08 集成后的 UI-09 分支基线，提交后绑定本提交） |
| feature_status | `implemented`（七个 UI DTO schema、schema lock、静态 Rust catalog、兼容矩阵、脱敏 fixture gate） |
| proof_level | `source`；不提升为 local_behavior/durable/live/physical |
| canonical path | versioned UI DTO → `docs/schemas/ui/*.schema.json` → `schema-lock.v1.json` → generated `kiana-client` metadata; runtime authority remains `kiana-protocol`/ControlPlane |

UI-09 fixes JSON Schema as the wire boundary for the UI-01/07/08 contracts: handshake request and
response, snapshot, feed frame, action, action result and entity-store snapshot. The lock binds each
id to its Rust owner, strict unknown-field policy, byte ceiling, deprecated-field list, redacted
examples and SHA-256. The generated Rust catalog only exposes bounded metadata; it does not parse a
request or authorize an effect.

## 2. Failure-first fixture matrix

| Fixture / gate | 断言 |
|---|---|
| schema hash and lock ordering | lock entries are sorted, schema `$id`/version/owner match and content hashes are current |
| valid examples | every DTO has a bounded redacted example accepted by the local JSON Schema subset |
| unknown-field examples | strict `additionalProperties: false` rejects an explicit unknown field before decode |
| byte ceilings and redaction scan | schema/example paths stay bounded and contain no token, secret or absolute real path |
| compatibility matrix | same-major changes are additive-only; required/type changes require a major; unknown major/command fail closed |
| generated catalog fixture | checked-in Rust metadata is byte-for-byte generated from the lock |
| `ui09_schema_gate_is_data_only_and_generated_catalog_is_bound` | source guard rejects Broker/ControlPlane/DaemonHost/model loop, network or filesystem execution |

## 3. Generation and compatibility contract

```text
JSON Schema (canonical)
  → schema-lock.v1 (owner/version/hash/limits/examples)
  → validate-ui09-schemas.py --write
  → kiana-client/src/ui_schema_generated.rs (static metadata only)
```

The canonical schemas use draft 2020-12, local `$ref` only, strict top-level unknown-field policy,
explicit max sizes and synthetic examples. Rust DTOs remain hand-written and are bound by source
path/type in the lock; TypeScript generation waits for a browser package. The compatibility matrix
records old-client/new-server read directions and rejects unknown schema majors and command names.
It is a source contract, not evidence of live cross-version interoperability.

## 4. Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `python3 scripts/validate-ui09-schemas.py`, `cargo fetch --locked`,
`cargo fmt --all --check`, the generated catalog fixture, the core source guard and
`cargo check --workspace --tests --locked`. Local tests, builds, checks, clippy and smoke are not
run; CI result is not awaited.

Limitations: the JSON Schema corpus covers the first seven UI DTO boundaries, not every existing
domain/event payload or future TypeScript package; semantic digest/cursor invariants remain enforced
by Rust validators; no live old/new client interoperability, browser transport, provider behavior,
durable schema migration or physical proof is claimed. Later schema versions require a new major or
an explicit migration/upcaster entry.

## 5. 2026-10-17 收口切片（第二次执行）

本次在既有 lock/matrix/fixture 之上补齐三类缺口，并把跨语言 round-trip 的另一半接进 lane：

| 新增 gate | 断言 / 失败码 |
|---|---|
| 资产覆盖 | 目录中任何未登记的 `*.schema.json`、`examples/*.json`、`negative/*.json` 都会被 `unlocked_schema_file` / `unreferenced_example_fixture` / `unreferenced_negative_fixture` 拒绝；新增文件不能绕过版本合同 |
| deprecated 绑定 | lock 的 `deprecated_fields` 必须逐字等于 schema 自己标记 `"deprecated": true` 的字段集合；每项还必须是 `field`/`since`/`remove_in` 的评审记录，否则 `contract_deprecated_fields_unbound` / `contract_deprecated_field_unreviewed` |
| Rust 发射 → JSON Schema | `cargo run -p kiana-client --example ui09_schema_emit` 用 Rust wire type 解码 7 个 fixture 再编码成报告，`validate-ui09-schemas.py --emitted` 再对 canonical schema 校验；漂移即 `emitted_schema_mismatch`，集合缺失即 `emitted_contract_set_mismatch` |

- `frozen_shape` 与 `schema_sha256` 不变：同版本形状改动仍然先失败，必须人工评审并升版本；
  `rust_field_drift` / `schema_optional_field_rust_required` / `rust_required_field_schema_optional`
  继续挡住 Rust 与 schema 的字段漂移；`negative_case_not_rejected` 等负例仍然先拒绝。
- 验收目标：`kiana-client/tests/ui09_schema_round_trip.rs`（4 条 typed round-trip，含
  `ui09_locked_examples_round_trip_through_the_rust_wire_types`）、
  `kiana-client/examples/ui09_schema_emit.rs` 与 `kiana-client/tests/ui09_schema_catalog.rs`；
  source guard 仍是 `kiana-core/tests/ui09_schema_guard.rs`，lane 仍是
  `.github/workflows/ui09-schema-contract.yml`（并按 `scripts/ci/test-shards.json` 进
  `kiana-core-s6/6` 自动分片）。
- proof_level 仍为 `source`：本地不运行测试/构建，lane 是 dispatch-only，CI 回执未等待；本页不声称
  live 跨版本互操作、浏览器侧 Schema 生成或 durable 迁移。
