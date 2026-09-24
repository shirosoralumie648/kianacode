# BQ-15 tool/effect/resource usage baseline

> Snapshot date: 2026-09-25. This slice adds deny-first source contracts and read-only Receipt
> projection. Runtime fixtures run in GitHub Actions; no local Cargo tests/build/check/clippy or
> smoke command is executed.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-15`](../roadmap.md#step-bq-15) |
| source snapshot | `3bfb9eef` plus this BQ-15 source slice |
| feature_status | `implemented` for bounded usage facts, Broker result binding and Receipt projection |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | ControlPlane permit → Broker/handler terminal fact → bounded model/tool/effect usage → Receipt |

`EffectUsageObservation` binds each measurement to server-owned Run/Invocation/Execution/attempt,
owner scope, resource digest, lease digest and source cursor. It separates model, tool (shell/MCP)
and effect (artifact/log/storage) layers. Rejected and not-started observations must have zero
started count, success count, bytes and wall time. Per-observation and aggregate output, artifact,
log, storage and wall-time limits fail closed; usage is never inferred from model text or UI state.

The Broker measures invocation wall time around the existing handler call and validates optional
handler usage immediately after execution. A usage payload with a
different request/run/owner/lease/resource/attempt, or a successful result without a successful
started observation, is rejected. Core accepts usage only from committed terminal invocation facts,
checks source event/cursor and terminal effect state, and embeds the resulting read-only summary in
the existing Receipt aggregation. No new execution loop, EventLog writer, quota authority or
provider transport is introduced.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| denied/not-started | zero started/effect/success/bytes/wall-time; denied cannot become success |
| binding fence | run, invocation, owner, lease, resource and attempt drift fail before projection |
| bounded usage | output/artifact/log/storage/wall-time overflow is rejected; aggregate overflow is rejected |
| layer separation | model/tool/effect summaries remain distinct; shell/MCP and artifact/log/storage map to the right layer |
| replay | duplicate usage identity with the same digest is idempotent; conflicting digest/source is rejected |
| Broker boundary | handler result usage is validated after permit/handler dispatch; missing lease/resource binding fails closed |
| Receipt projection | only terminal committed facts count; source event/cursor and effect_started state must agree; no Broker/model/network authority |

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the BQ-15 domain/core/Broker
fixtures and source guards, then `cargo check --workspace --tests --locked`. Local Cargo tests,
builds, checks, clippy and smoke commands are intentionally not run; CI results are not awaited.

Limitations: usage remains an immutable source observation and read-only Receipt projection. This
slice does not append/flush EventLog facts, reserve or settle quota, reconcile unknown effects,
persist cross-process counters, validate physical storage writes, contact a shell/MCP/provider,
or prove live/durable/physical accounting. BQ-16 capacity/backpressure, BQ-19 allocation and
later recovery/rollup steps remain open.
