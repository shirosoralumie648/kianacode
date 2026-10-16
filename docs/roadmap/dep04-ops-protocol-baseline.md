# DEP-04 · Ops protocol baseline

> Snapshot date: 2026-09-27. This slice registers versioned `ops.*` command/query/event/error/
> unknown envelopes and idempotency evidence. Current local protocol/client focused verification
> is recorded below; cross-surface admission remains open.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-04`](../roadmap.md#step-dep-04) |
| source snapshot | historical implementation anchor `825596c7`; focused verification on current dirty worktree (2026-10-05) |
| feature_status | `partial` for wire contracts and typed client transport; cross-surface admission remains open |
| proof_level | `local_behavior` for protocol/client fixtures; no durable/live/physical promotion |
| canonical path | surface DTO → versioned ops envelope → DaemonHost/Core authority comparison → later operation journal |

`kiana-protocol/src/ops.rs` keeps operator commands and read-only queries on one versioned wire
shape. Commands carry operation/idempotency keys, actor/authority epoch and a scoped snapshot;
Core must compare those values to server-owned authority before any admission. Events use an
explicit name-to-kind registry; structured errors and unknown payloads retain digests and remain
non-executable. Reusing either an operation id or idempotency key with a different payload is an
explicit conflict; exact replay is a replay disposition. `RequestEnvelope::ops_command` and
`ops_query` encode the same DTO on the existing shared wire spine.

`ResultUnknown` is non-retryable and must be reconciled by querying the original operation. Event
names `accepted`, `state_changed`, `completed`, `failed`, `cancelled` and `unknown` each map to one
matching `OpsEventKind`; a digest-valid name/kind contradiction is rejected.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| forged actor | server snapshot comparison rejects actor drift |
| forged epoch | stale/future authority epoch is rejected |
| scope mismatch | request cannot widen project/storage scope |
| unknown command | unregistered `ops.*` command is rejected |
| idempotency | same operation/key/digest replays; different digest conflicts |
| envelope | command/query/event/error/unknown bodies round-trip with digest |
| event kind | every registered event name maps to its exact kind; mismatches are rejected |
| unknown retry | constructor, recomputed-digest DTO validation and envelope validation reject retryable `ResultUnknown` |
| unknown identity | constructor, recomputed-digest DTO validation and envelope validation reject `ResultUnknown` without an `operation_id` |
| source boundary | protocol contains no Broker/EventLog/process/supervisor effect |

## CI and limitations

GitHub Actions is wired through `.github/workflows/dep04-ops-protocol.yml` with protocol fixtures,
a Core source guard and affected-target compilation. The local receipt below is current focused
proof; remote workflow results were not awaited.

These DTOs do not authenticate an actor, consume approval, enforce a filesystem scope, append a
journal, dispatch a command or establish an operation outcome. The typed RequestEnvelope helpers
are wire constructors only; DaemonHost/Core ops admission and the four-surface command/query
route remain later work. Those decisions remain in Core, EventLog and later DEP steps; Unknown
never becomes success by decoding alone.

## Local focused verification receipt (2026-10-05)

```text
source_snapshot: current dirty worktree after DEP-00 through DEP-03 verification
worktree_status: DEP-04 remains 🔄; typed client ops methods added on the shared RequestEnvelope spine; no automatic commit/push/merge/rebase
command_argv: `CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 RUST_MIN_STACK=33554432 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_STRIP=symbols CARGO_INCREMENTAL=0 cargo test -p kiana-protocol --test dep04_ops -p kiana-core --test dep04_ops_protocol_guard -p kiana-client --test client_methods --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; serial Cargo/test threads; 32 MiB test-thread stack; stripped, non-incremental test profile; combined resource receipt `/tmp/kiana-roadmap-20261005-evidence/dep04-05-client.resources` recorded 2.35s user, 0.41s system, 436268 KiB max RSS, 0 swaps
fixture·cassette: protocol 2/2; Core no-effect guard 1/1; typed client methods 3/3 including shared command envelope decoding and transport-error preservation
exit_code: 0
status_change: no whole-card promotion; DEP-04 local protocol/client slice advanced to local_behavior
proof-level_change: focused wire/client behavior `source` → `local_behavior`; full card remains partial/open
limitations: DaemonHost/Core still treats ops.* as generic command input and has no typed authority/admission path; Web/Workbench/Desktop do not route these typed methods; no EventLog operation journal, approval, Broker dispatch, durable/live/physical effect or successful remote CI evidence
reviewer: integration agent with read-only DEP-04 audit; focused client/protocol reviewer
```

## 2026-10-06 deny-first protocol contract receipt

```text
source_snapshot: current dirty worktree; kiana-protocol/src/ops.rs and tests/dep04_ops.rs; existing Core guard
worktree_status: DEP-04 remains 🔄 / partial; protocol contract corrections only, typed Core/DaemonHost admission still absent
command_argv: bounded serial cargo test -p kiana-protocol --test dep04_ops --locked --offline -- --test-threads=1; cargo test -p kiana-core --test dep04_ops_protocol_guard --locked --offline -- --test-threads=1
cwd·environment: repository root Linux/bash; CARGO_BUILD_JOBS=1, RUST_TEST_THREADS=1, RUST_MIN_STACK=33554432, stripped non-incremental profile; protocol max RSS 456376 KiB, Core 130528 KiB, swaps 0
fixture·cassette: protocol 3/3 including all six event-kind pairs, digest-valid failed/Accepted contradiction, retryable ResultUnknown rejection at constructor/validation/envelope boundaries; Core guard 1/1
exit_code: both commands 0; logs/resources `/tmp/kiana-roadmap-20261006-dep04-protocol-final.{log,resources}` and `/tmp/kiana-roadmap-20261006-dep04-core-final.{log,resources}`; RED reproduction before implementation exit 101 at mismatched event kind
status_change: selected DEP-04 protocol contracts are local_behavior; whole card remains partial/source-or-local wire boundary, no roadmap row promotion
proof-level_change: focused protocol behavior source → local_behavior; no durable/live/physical promotion
limitations: no typed DaemonHost/Core authority admission, EventLog operation journal, approval/Broker dispatch, cross-surface route, durable/live/physical effect or successful remote CI evidence
reviewer: integration agent with DEP-04 read-only audit; focused protocol reviewer
```

## 2026-10-06 query authority receipt

`OpsCommandRequest` already compared client-declared authority with a server snapshot, but
`OpsQueryRequest` only self-validated its actor, epoch and scope. The protocol now exposes the same
pure `validate_authority` comparison for queries; the wire constructor remains snapshot-free and
does not perform admission or execution.

```text
source_snapshot: current dirty worktree; kiana-protocol/src/ops.rs and tests/dep04_ops.rs
worktree_status: DEP-04 remains 🔄 / partial; selected query authority checks are local_behavior; no DaemonHost/Core admission or second execution path
command_argv: RED `cargo test -p kiana-protocol --test dep04_ops --locked --offline -- --test-threads=1`; GREEN same command; `cargo test -p kiana-core --test dep04_ops_protocol_guard --locked --offline -- --test-threads=1`; `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=1`, `RUST_MIN_STACK=33554432`, stripped/non-incremental profile; one Cargo worker; protocol GREEN max RSS 461576 KiB (~450.8 MiB), Core guard 131424 KiB (~128.3 MiB), 0 swaps
fixture·cassette: RED compile failure 4 missing query methods; GREEN protocol 3/3 with forged query actor/epoch/scope denial and Core no-effect guard 1/1; logs/resources `/tmp/kiana-roadmap-20261006-dep04/{red,green,core}.{log,resources}`
exit_code: RED 101 expected; GREEN/Core/fmt/diff 0
status_change: selected query authority comparison source → local_behavior; whole card remains feature_status=partial, proof_level=source/local_behavior at focused wire boundary
proof-level_change: query deny-first contract source → local_behavior; no durable/live/physical promotion
limitations: no typed DaemonHost/Core admission, EventLog operation journal, approval/Broker dispatch, cross-surface route, durable/live/physical effect or successful remote CI evidence
reviewer: root integration agent with DEP-04 read-only audit; focused protocol reviewer
```

## 2026-10-08 ResultUnknown operation identity receipt

`ResultUnknown` now requires an original `operation_id` at every protocol boundary. A missing
identifier cannot be reconciled against the operation journal, so construction, digest-valid DTO
validation and `OpsEnvelope` validation all return `ops_result_unknown_operation_invalid`.

```text
source_snapshot: current dirty worktree; kiana-protocol/src/ops.rs; kiana-protocol/tests/dep04_ops.rs; kiana-core/tests/dep04_ops_protocol_guard.rs
worktree_status: DEP-04 row 609 remains 🔄 / feature_status=partial / proof_level=source; selected ResultUnknown identity behavior now local_behavior; no typed DaemonHost/Core ops admission or second execution path
command_argv: RED `cargo test -p kiana-protocol --test dep04_ops --locked --offline -- --test-threads=1`; GREEN protocol `cargo test -p kiana-protocol --test dep04_ops --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test dep04_ops_protocol_guard --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; one Cargo job/test thread, `RUST_MIN_STACK=33554432`, stripped/non-incremental tests; RED max RSS 83568 KiB / 67% CPU; protocol GREEN max RSS 457460 KiB / 95% CPU; Core guard max RSS 430924 KiB / 86% CPU; all measured runs reported 0 swaps; logs/resources `/tmp/kiana-roadmap-20261008-dep04-{red,protocol,core}.{log,resources}`
fixture·cassette: RED accepted digest-valid `ResultUnknown` with `operation_id=None`; GREEN protocol 3/3 and Core guard 1/1; retryable Unknown remains rejected separately
exit_code: RED 101 expected; GREEN protocol/Core 0
status_change: selected DEP-04 ResultUnknown identity moves source → local_behavior; whole card remains feature_status=partial, proof_level=source
proof-level_change: protocol identity denial source → local_behavior; no durable/live/physical promotion
limitations: typed ops query is still encoded on the generic command spine and DaemonHost/Core admission, operation journal reconciliation, approval/Broker effects and cross-process durable/live proof remain open
reviewer: root integration agent with DEP-04 read-only audit; focused ops protocol reviewer
```
