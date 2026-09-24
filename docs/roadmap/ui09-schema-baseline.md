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
