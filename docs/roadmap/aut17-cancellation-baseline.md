# AUT-17 cancellation generation baseline

AUT-17 adds an automation cancellation fact with generation and authority epoch fencing. Stopped
requires confirmed stop; ResultUnknown preserves an effect-started uncertainty; a late result is
only valid after explicit Fenced state.

## 2026-09-27 source slice (historical)

The first slice added only a value object and a validation-only Core facade
(`kiana-domain/src/automation_cancellation.rs`, `kiana-core/src/automation_cancellation.rs`): a
single `AutomationCancellationFact` with `Requested`/`Stopping`/`Stopped`/`ResultUnknown`/`Fenced`
states, a partial field-combination check, and a documented gap ("Stopping does not require
stop_requested; no history check"). That baseline claimed GitHub CI ran
`.github/workflows/aut17-cancellation.yml`, but the file was later removed by the workflow
consolidation commit `08552ada`, so the claim was stale: nothing persisted a generation, nothing
signalled a worker, and no late result was ever fenced. The manual lane is restored by this slice.

## 2026-10-17 completion slice (CI-only)

This slice completes the card's source-level exit conditions across `kiana-domain`,
`kiana-ports`, `kiana-eventlog`, `kiana-core` and `kiana-daemon`. No local Cargo
test/build/check/clippy/smoke was run and no CI receipt is awaited; `proof_level` stays `source`.

**1. Closed cancel-state matrix (`kiana-domain/src/automation_cancellation.rs`).**
`AutomationCancelState` gains `NotExecuted`; `AutomationCancellationFact` gains
`stop_evidence_digest`. `validate()` now enforces the whole matrix instead of a few local rules:

| state | stop_requested | stop_confirmed | effect_started | late_result | stop_evidence |
|---|---|---|---|---|---|
| `Requested` | false | false | any | false | none |
| `Stopping` | true | false | true | false | required |
| `NotExecuted` | any | false | false | false | iff a stop was requested |
| `Stopped` | true | true | true | false | required |
| `ResultUnknown` | any | false | true | false | iff a stop was requested |
| `Fenced` | any | any | any | true | any |

Consequences: `Stopped` without `stop_confirmed` (the "stop unconfirmed but marked cancelled"
case) is structurally impossible, `NotExecuted` must prove that the effect never started,
`ResultUnknown` must keep the started-effect uncertainty, and `late_result` only exists on
`Fenced`. Two pure fences make the generation explicit: `cancel_generation` must strictly
increase per execution/attempt, and `authority_epoch` must equal the committed command's epoch.

**2. Cancel request, stop report and late result (`kiana-domain`).**
`AutomationCancelRequest` (generation + epoch + reason + self digest), `AutomationStopReport`
(`NotStarted`/`Stopping`/`Stopped`/`Unknown` + mandatory `evidence_digest` + self digest) and
`AutomationLateResult` (result digest + self digest) are separate versioned DTOs. Three pure
functions carry the contract:

| function | rule |
|---|---|
| `begin_automation_cancellation` | command `Prepared` → `NotExecuted` and **no** stop; `Claimed` → `Requested` + stop required; terminal `Observed` → `automation_cancel_execution_terminal`; terminal `Unknown` → `ResultUnknown` and no stop; execution/attempt drift, stale epoch and stale/conflicting generations are refused; identical same-generation requests replay safely |
| `bind_automation_stop_report` + `fold_automation_stop_report` | the report must match execution/attempt/generation/epoch of both the command and the request; only a `Stopped` report yields `Stopped` (with the report's evidence digest), `NotStarted` yields `NotExecuted`, `Stopping`/`Unknown` stay unconfirmed; a closed fact cannot fold a second stop |
| `fence_automation_late_result` | a result whose generation/epoch is not ahead can only become `Fenced`; `Fenced` is terminal (`automation_cancel_already_fenced`), and the stop history (`stop_confirmed`, evidence digest) is preserved, never rewritten |

`automation_cancel_transition_allowed` is the shared same-generation whitelist used by the store:
`Fenced` has no outgoing edge, and `Stopped`/`NotExecuted` can only move to `Fenced`.

**3. Ports (`kiana-ports/src/lib.rs`).** `AutomationCancellationStore` (fetch by
execution/attempt, CAS commit on the current fact digest with generation monotonicity and the
transition whitelist; fail-closed defaults) and `AutomationWorkerStopController` (the only stop
boundary; returns a `AutomationStopReport` or fails, with no default that could invent a stop).

**4. Core coordinator (`kiana-core/src/automation_cancellation.rs`).** `cancel_automation_worker_command`
re-fetches the committed command, fetches the current cancel fact, asks the domain for the next
step, commits the fact **before** requesting a stop, calls `request_stop` exactly once, folds the
report and CAS-commits the result. `AutomationCancellationOutcome` has four variants:
`NotExecuted`, `Stopped`, `RecoveryRequired`, `Replayed`; a controller error, a drifted report or
an uncertain commit returns `RecoveryRequired` and never `Stopped`. `record_automation_late_result`
refuses to fence without a recorded cancel fact (`automation_cancel_fact_unknown`) and never
reopens a closed fact. Verification: `README.md`/`CURRENT_STATUS.md` remain the only status
authority; the coordinator touches no EventStore, Broker, model or file system.

**5. Store and daemon (`kiana-eventlog`, `kiana-daemon`).**
`MemoryAutomationCancellationStore` implements the CAS semantics in-process (content CAS,
generation monotonicity, same-generation whitelist, identical replay). `AutomationCancellationService`
is owned by `DaemonHost`, is explicitly started, runs a single worker behind a bounded 64-message
channel, and reports `stopped_count`/`not_executed_count`/`recovery_required_count` on shutdown
(in-flight, unknown and fenced facts count as recovery-required).

## Fixtures and CI wiring

| target | tests | coverage |
|---|---|---|
| `kiana-domain/tests/automation_cancellation.rs` | 3 → 12 | deny-first state matrix (queued/claimed/unknown/terminal), generation+epoch fences, stop-report evidence and state conflicts, unconfirmed-stop and fencing semantics, unknown-field/digest denial |
| `kiana-core/tests/automation_cancellation_worker.rs` (new) | 7 | not_executed with 0 stop calls, exactly one stop per claimed attempt, recovery-only for unconfirmed/unavailable/drifted stops, terminal/unknown fail-closed, replay and stale/conflicting requests with unchanged stop counts, late-result fencing |
| `kiana-core/tests/automation_cancellation_guard.rs` | 1 → 2 | boundary markers, rejection-code matrix, coordinator ordering (re-fetch → commit → stop → fold → CAS), fixture names, manual lane and shard binding |
| `kiana-eventlog/tests/aut17_cancellation_store.rs` (new) | 7 | absent-fact/digest CAS, identical replay, generation staleness, closed-state reopening refusal, epoch drift within a generation, invalid fact denial, pure fetch keyed by attempt |
| `kiana-daemon/tests/aut17_cancellation.rs` (new) | 4 | DaemonHost ownership and fail-closed-before-start, queued vs claimed cancels, recovery counting for unconfirmed/unknown, late-result fencing never counted as stopped |
| `kiana-daemon/tests/aut17_cancellation_guard.rs` (new) | 2 | bounded single-worker service markers, forbidden-boundary negative assertions, fixture binding |

The manual lane `.github/workflows/aut17-cancellation.yml` (`workflow_dispatch` only,
`permissions: contents: read`) fetches locked dependencies, runs the fmt gate, runs all six targets
serially (`--test-threads=1`) and compiles each with `--no-run`. Automatic coverage: the unified
`ci.yml` shards — `automation_cancellation` in `kiana-domain-s1/4`, the new
`automation_cancellation_worker` plus `automation_cancellation_guard` in `kiana-core-s1/6`, and
whole-crate discovery for `kiana-eventlog` and `kiana-daemon`.

## Known limits

- The store and the service are in-process only (CI semantics): no fsync, no crash recovery, no
  cross-process fencing. "Re-fetch after a crash" is proven against the same process' store, not
  against a real power loss, and the generation counter has no durable home yet.
- The stop controller is the only stop boundary in these fixtures; no real supervisor, process
  group drain, provider cancel or worker was signalled, and nothing here is `durable`, `live` or
  `physical` evidence.
- Cancellation is a separate fact stream: the dispatch command itself is not moved to a cancelled
  state, so a cancelled-but-claimed attempt still needs AUT-15 observation or AUT-21 reconcile to
  close the command. No production ControlPlane path calls the coordinator yet.
- The daemon shutdown counts only facts this worker has seen; it is not a store-wide census.
- `NotExecuted` for an unclaimed command trusts the committed command state; it is not a proof that
  no external effect happened through some other path. Boot recovery, parent→child cancellation and
  typed child results remain AUT-21/SW-10/SW-12.
