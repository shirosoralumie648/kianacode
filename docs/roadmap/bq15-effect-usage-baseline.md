# BQ-15 tool/effect/resource usage baseline

> Snapshot date: 2026-09-25; completion slice: 2026-10-17. This slice adds deny-first source
> contracts and read-only Receipt projection. Runtime fixtures run in GitHub Actions; no local
> Cargo tests/build/check/clippy or smoke command is executed.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-15`](../roadmap.md#step-bq-15) |
| source snapshot | `3bfb9eef` plus the 2026-09-25 BQ-15 source slice, then `c232aefe` on `master` plus this 2026-10-17 completion slice |
| feature_status | `implemented` for bounded usage facts, admitted-envelope metering, Broker result binding and Receipt projection |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | ControlPlane permit (+ admitted envelope) → Broker/handler terminal fact → bounded model/tool/effect usage → Receipt |

`EffectUsageObservation` binds each measurement to server-owned Run/Invocation/Execution/attempt,
owner scope, resource digest, lease digest and source cursor. It separates model, tool (shell/MCP)
and effect (artifact/log/storage) layers. Rejected and not-started observations must have zero
started count, success count, bytes and wall time; they may not claim an admitted envelope either.

Every started/failed/unknown observation must also carry the server-admitted
`EffectUsageEnvelope` it was measured against. The envelope is derived by the ControlPlane from
the BQ-09 admission estimate (`tool_calls_upper`, `effects_upper`, `storage_bytes_upper`, wall
time) and bound into the authorized request; the adapter may only repeat it, and the
`envelope_digest` covers every field so a widened envelope is detectable. Per-observation and
aggregate output, artifact, log, storage and wall-time limits fail closed; usage is never
inferred from model text or UI state.

The Broker measures invocation wall time around the existing handler call and validates optional
handler usage immediately after execution. A usage payload with a
different request/run/owner/lease/resource/attempt, without the admitted envelope, above that
envelope, or a successful result without a successful started observation, is rejected. An
admitted envelope also requires usage evidence unless the result is explicitly not executed. Core
accepts usage only from committed terminal invocation facts, checks source event/cursor, terminal
effect state and the envelope, and embeds the resulting read-only summary in the existing Receipt
aggregation. No new execution loop, EventLog writer, quota authority or provider transport is
introduced.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| denied/not-started | zero started/effect/success/bytes/wall-time; denied cannot become success and cannot claim an envelope |
| binding fence | run, invocation, owner, lease, resource and attempt drift fail before projection |
| bounded usage | output/artifact/log/storage/wall-time overflow is rejected per dimension; aggregate overflow is rejected |
| admitted envelope | an envelope wider than the hard limits, zero started admissions or a tampered digest is refused; a started fact without one cannot validate or project |
| flood / quota bypass | a self-consistent fact whose bytes exceed the admitted envelope is refused (`effect_usage_envelope_exceeded:output_bytes`); a self-made wider envelope is a digest mismatch |
| per-fact counts | one fact can never report more than one started invocation; two facts for the same attempt/layer/resource are refused as double counting |
| layer separation | model/tool/effect summaries remain distinct; shell/MCP and artifact/log/storage map to the right layer |
| replay | duplicate usage identity with the same digest is idempotent; conflicting digest/source is rejected |
| Broker boundary | handler result usage is validated after permit/handler dispatch; missing lease/resource binding or missing admitted envelope fails closed; measured wall time replaces the adapter claim and over-time fails closed |
| Receipt projection | only terminal committed facts count; source event/cursor, terminal kind/state and envelope binding must agree; no Broker/model/network authority |

## 2026-10-17 completion slice (CI-only)

- `kiana-domain/src/effect_usage.rs`: added `EffectUsageEnvelope` (server-admitted
  run/invocation/attempt/lease/resource-bound byte, count and wall-time bounds with a self-sealed
  digest), `EffectUsageObservation::bind_envelope` / `validate_against_envelope`, mandatory
  envelope binding for started states, per-fact `started_count <= 1`, and
  `EffectUsageReceipt::envelope_digests` plus the duplicate-attempt fence.
- `kiana-capability-broker/src/lib.rs`: decodes the admitted envelope from the authorized request,
  requires usage evidence once an envelope was admitted, and rechecks the handler's observation
  against that envelope after the Broker's own wall-time measurement.
- `kiana-core/src/effect_usage_projection.rs`: refuses an unmetered started fact and terminal
  kinds that disagree with the usage state (`completed_usage_not_success`, `failed_usage_success`).
- Fixtures: `kiana-domain/tests/bq15_effect_usage.rs` (9 cases), `kiana-core/tests/bq15_effect_usage.rs`
  (5 cases), new `kiana-capability-broker/tests/bq15_effect_usage.rs` (2 behavioural cases over a
  sealed broker), extended core/broker source guards, and a rebuilt manual
  `.github/workflows/bq15-effect-usage.yml` lane. Automatic coverage stays with the unified
  `ci.yml` shards (`kiana-domain-s1/4`, `kiana-core-s1/6`, whole-crate `kiana-capability-broker`).

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the BQ-15
domain/core/Broker fixtures and source guards, then `cargo check --workspace --tests --locked`.
Local Cargo tests, builds, checks, clippy and smoke commands are intentionally not run; CI results
are not awaited.

Limitations: usage remains an immutable source observation and read-only Receipt projection. This
slice does not append/flush EventLog facts, reserve or settle quota, reconcile unknown effects,
persist cross-process counters, validate physical storage writes, contact a shell/MCP/provider, or
prove live/durable/physical accounting. The admitted envelope is bound into the authorized request
by the ControlPlane; this slice does not yet derive it from a live `ReservationPlan`, and the
Receipt only lists envelope digests (not the envelopes themselves). BQ-16 capacity/backpressure,
BQ-19 allocation and later recovery/rollup steps remain open.
