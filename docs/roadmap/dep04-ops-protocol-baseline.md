# DEP-04 · Ops protocol baseline

> Snapshot date: 2026-09-27. This slice registers versioned `ops.*` command/query/event/error/
> unknown envelopes and idempotency evidence. Local Cargo test/build/check/clippy/smoke commands
> are intentionally not run; GitHub Actions owns fixtures and affected-target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-04`](../roadmap.md#step-dep-04) |
| source snapshot | `825596c7` plus this DEP-04 source slice |
| feature_status | `partial` for wire contracts and admission evidence |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | surface DTO → versioned ops envelope → DaemonHost/Core authority comparison → later operation journal |

`kiana-protocol/src/ops.rs` keeps operator commands and read-only queries on one versioned wire
shape. Commands carry operation/idempotency keys, actor/authority epoch and a scoped snapshot;
Core must compare those values to server-owned authority before any admission. Events, structured
errors and unknown payloads retain digests and remain non-executable. Duplicate operation/key with
a different payload is an explicit conflict; exact replay is a replay disposition.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| forged actor | server snapshot comparison rejects actor drift |
| forged epoch | stale/future authority epoch is rejected |
| scope mismatch | request cannot widen project/storage scope |
| unknown command | unregistered `ops.*` command is rejected |
| idempotency | same operation/key/digest replays; different digest conflicts |
| envelope | command/query/event/error/unknown bodies round-trip with digest |
| source boundary | protocol contains no Broker/EventLog/process/supervisor effect |

## CI and limitations

GitHub Actions runs `.github/workflows/dep04-ops-protocol.yml` with protocol fixtures, a Core
source guard and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke
commands are intentionally not run, and CI results are not awaited.

These DTOs do not authenticate an actor, consume approval, enforce a filesystem scope, append a
journal, dispatch a command or establish an operation outcome. Those decisions remain in Core,
EventLog and later DEP steps; Unknown never becomes success by decoding alone.
