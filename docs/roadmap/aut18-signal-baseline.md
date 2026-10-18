# AUT-18 approval/signal pause-resume baseline

AUT-18 adds a strict signal fact binding execution owner, action/path scope, authority epoch and
checkpoint sequence. Paused, Resumed and Consumed are explicit states and `consumed` is derived
from the terminal state, preventing duplicate signal use.

## 2026-09-27 source slice (historical)

The first slice added only a value object and a validation-only Core facade
(`kiana-domain/src/automation_signal.rs`, `kiana-core/src/automation_signal.rs`): a single
`AutomationSignalFact` with `Paused`/`Resumed`/`Consumed` states, a partial field-combination
check and 2 domain fixtures. That baseline claimed GitHub CI ran
`.github/workflows/aut18-signal.yml`, but the file was removed by the workflow consolidation
commit `08552ada`, so the claim was stale: no request/approval/checkpoint DTOs, no transition
planner or ledger, no store, no runner boundary and no single-use approval consumption existed.
The manual lane is restored by this slice.

## 2026-10-17 completion slice (CI-only)

This slice completes the card's source-level exit conditions across `kiana-domain`, `kiana-ports`,
`kiana-eventlog`, `kiana-core` and `kiana-runner`. No local Cargo test/build/check/clippy/smoke was
run and no CI receipt is awaited; `proof_level` stays `source`.

**1. Domain contract and pure planner (`kiana-domain/src/automation_signal.rs`).** Four versioned,
self-digested, `deny_unknown_fields` schemas: the signal request, the resume approval, the
checkpoint and the committed fact. The request carries owner/definition/action/path identity,
authority epoch, checkpoint sequence and `signal_sequence`; a pause must not carry an approval
digest while resume/consume must, and any `ui_context_digest` is refused with
`automation_signal_ui_context_forbidden` — server-side state only, UI/model-supplied context is
never merged. `AutomationSignalApproval` admits only `Operator`/`Supervisor` (owner self-approval →
`automation_signal_approval_self_approval`, read-only roles → `automation_signal_approver_unauthorized`),
requires a positive window checked strictly at both endpoints (`is_live_at`) and a `Resume` kind.
`AutomationCheckpoint` binds execution/owner/definition/action/path/epoch/sequence.

`plan_automation_signal` is the single pure planner with a fixed deny-first order: `signal_id`
digest replay (`Replayed`, no new fact) → contiguous sequence (`automation_signal_sequence_regression` /
`automation_signal_sequence_gap`) → owner/definition/action/path drift and authority-epoch fences
(epoch may only advance on a `Resume`) → kind-specific state checks (pause: required bound
checkpoint, no approval, no regression; resume: paused predecessor, unconsumed live approval bound
to that pause, checkpoint not regressing; consume: resumed predecessor quoting the exact resume
approval digest, terminal). `AutomationSignalLedger::apply`/`record_recovered` re-check the same
gates against the pause→resume→consume transition whitelist; `Consumed` is terminal (no
resurrection), and `approval_reuse_allowed` exempts exactly one case — the consume fact quoting
the digest of its own resume — so an approval is otherwise consumed exactly once. `dispatch_permitted`
is always `false`: the signal contract expresses pause/resume/consume authorization, never dispatch.

**2. Ports and store (`kiana-ports`, `kiana-eventlog`).** `AutomationSignalStore` fetches committed
history and CAS-commits a fact against the expected digest with fail-closed defaults.
`MemoryAutomationSignalStore` implements digest CAS, `signal_id` dedupe (identical replay returns
the stored copy), contiguous sequences, the transition whitelist, monotonic authority epoch
(advance only on restart resume) and single approval consumption. It is in-process CI semantics:
no fsync, no crash recovery, no cross-process fencing, and it must not be called durable.

**3. Core coordinator (`kiana-core/src/automation_signal.rs`).** `apply_automation_signal` rebuilds
the ledger from the store's committed history, re-runs the domain planner and CAS-commits the
result; outcomes are `Paused`/`Resumed`/`Consumed`/`Replayed`. Shape and drift refusals leave the
store untouched, and an uncertain commit is never reported as success. The coordinator only calls
the `AutomationSignalStore` port — no dispatch, no broker, no runner.

**4. Runner boundary (`kiana-runner/src/signal.rs`).** `RunnerSignalGate::fold` is a pure fold from
the committed facts plus the runner-held identity to `RunnerSignalDirective::{Continue, Hold,
Resume{authority_epoch, checkpoint_sequence}, Closed}`. Identity/execution drift, non-contiguous
sequences, digest conflicts, whitelist violations, epoch regressions and checkpoint mismatches
(`runner_signal_checkpoint_unknown`) are refused; stable dedupe skips identical signals.
`Hold`/`Closed` make `model_calls_permitted()` false; the module calls no model, executes no tool,
writes no EventStore, consumes no approval and creates no checkpoint.

## Fixtures and CI wiring

| target | tests | coverage |
|---|---|---|
| `kiana-domain/tests/automation_signal.rs` | 2 → 17 | deny-first: DTO/role/self-approval/window/kind refusals, UI context, checkpoint-bound pause, owner/definition/action/path drift, stable sequence order and dedupe, epoch fences, restart identity/checkpoint match, approval epoch/digest binding, strict expiry boundary, checkpoint regression, single consume, paused/resumed predecessor requirements, recovery conflicts, consumed-digest reuse, never-dispatch |
| `kiana-core/tests/automation_signal_worker.rs` (new) | 9 | shape failures touch nothing, checkpoint-bound pause, identity drift, duplicate replay without a second commit, restart rebuilt from committed history, expiry + single use, terminal consume, unreplayable history fail-closed, uncertain commits never reported as success |
| `kiana-core/tests/automation_signal_guard.rs` | 1 → 2 | boundary markers and forbidden effects, coordinator order (shape → fetch → rebuild → plan → CAS), fixture names, manual-lane and shard literals, no `#[ignore]` |
| `kiana-eventlog/tests/aut18_signal_store.rs` (new) | 8 | absent-history first commit, identical replay, sequence regression/gap, transition whitelist, identity drift, epoch advance only on restart resume, single approval consumption, pure fetch |
| `kiana-runner/tests/aut18_signal_gate.rs` (new) | 12 | pure fold: identity shape, empty history, resume/consume mapping, restart epoch, drift, contiguous sequences, dedupe, exact checkpoint binding, whitelist, epoch monotonicity, invalid facts, source-boundary negative assertions |

The original two domain fixtures were superseded by the 17-test matrix (the fact-shape,
consumed-flag and unknown-field refusals live in `request_and_fact_shapes_fail_closed`; the
single-consume assertions in `sequence_order_and_dedupe_are_stable` and
`resumed_signal_is_consumed_exactly_once`), and the completion session fixed the
consume/second-consume/pause-after-consume/resume-after-consume and second-pause fixtures so they
exercise the contiguous-sequence gate instead of failing on `automation_signal_sequence_gap`.

The manual lane `.github/workflows/aut18-signal.yml` (`workflow_dispatch` only,
`permissions: contents: read`) fetches locked dependencies, runs the fmt gate, runs all five
targets serially (`--test-threads=1`) and compiles each with `--no-run`. Automatic coverage: the
unified `ci.yml` shards — `automation_signal` in `kiana-domain-s1/4`, `automation_signal_worker`
plus `automation_signal_guard` in `kiana-core-s1/6` — and whole-crate discovery for
`kiana-eventlog` and `kiana-runner`.

## Known limits

- The store and the ledger are in-process only (CI semantics): no fsync, no crash recovery, no
  cross-process fencing. "Restart" is replayed against committed in-process history, not a real
  process restart, and the checkpoint is a referenced metadata value — this contract does not prove
  a checkpoint was durably written or claimed.
- Approval liveness uses a caller-supplied `now_tick`; the approval is a self-digested value in the
  request, not a record in a durable approval store, and nothing here consumes it outside the
  signal ledger.
- No production ControlPlane caller drives `apply_automation_signal` yet; the coordinator and the
  runner fold are exercised by fixtures only. The runner gate is a pure function and is not wired
  into the harness model loop; no live signal streaming exists.
- `ui_context_digest` has no accepted non-empty path: the only behavior is refusal.
- `bash scripts/ci/validate-workflows.sh` requires ruby and was not run locally; the workflow shape
  was re-checked with an equivalent in-memory script. Live/physical evidence remains open.
