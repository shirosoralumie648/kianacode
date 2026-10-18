# AUT-14 effect reservation baseline

AUT-14 adds a strict reservation fact binding execution/action digest, capability, budget/path
scope, approval and authority/config/policy revisions. The bounded ledger replays identical
idempotency payloads and rejects revision drift or payload conflict before any effect path.

`kiana-core` exposes validation only; no Broker, provider, EventLog or worker call is made. CI runs
`kiana-domain/tests/automation_effect_reservation.rs` and the Core source guard in
`.github/workflows/aut14-effect-reservation.yml`. Local Cargo tests/build/check/clippy/smoke were
not run. The slice remains `feature_status=partial`, `proof_level=source`; durable CAS, permit
consumption, worker dispatch and live/physical effects remain AUT-15+.

## CI wiring gap closed (2026-09-28)

This baseline previously stated that "CI runs `kiana-domain/tests/automation_effect_reservation.rs`
and the Core source guard in `.github/workflows/aut14-effect-reservation.yml`". That workflow file
did not exist in the repository, so the claim was an overstatement: the fixtures had no CI wiring
and could never have produced evidence.

The source slice itself was real and is unchanged — `kiana-domain/src/automation_effect_reservation.rs`,
`kiana-core/src/automation_effect_reservation.rs`, `kiana-domain/tests/automation_effect_reservation.rs`
and `kiana-core/tests/automation_effect_reservation_guard.rs` were all already present.

`.github/workflows/aut14-effect-reservation.yml` is now added, running the format gate, the domain
fixture and the core source guard. `scripts/ci/validate-workflows.sh` passes with it registered
(`workflow check passed: 42 files, 5 automatic, 37 manual`).

AUT-14 remains 🔄 with `proof_level=source`: creating the workflow makes evidence *obtainable*, it does
not constitute evidence. No run of this workflow has been observed, because GitHub Actions has not
executed any job for this account since 2026-09-27T11:16Z.

## 2026-10-17 completion slice (CI-only)

The earlier slice stopped at an in-memory idempotency map that compared only authority/config/policy
revisions, and the 2026-09-28 note explicitly kept permit consumption and CAS open. This slice
completes the card's source-level exit conditions in `kiana-domain`, `kiana-core` and
`kiana-eventlog`; no local Cargo test/build/check/clippy/smoke was run and no CI receipt is awaited.
`feature_status` moves to implemented-at-source and `proof_level` stays `source`.

- `kiana-domain/src/automation_effect_reservation.rs`: adds `AutomationExecutionBinding`
  (per-execution action/capability/budget/path/approval digests), `AutomationEffectPermit`
  (`Issued` -> `Consumed`, at most one per reservation), `AutomationEffectAttempt` (at most one per
  reservation, `attempt = 1`) and `AutomationEffectAttemptBegin::{Started, AlreadyStarted}`.
  `reserve()` now fails closed for unregistered executions
  (`automation_effect_reservation_execution_unknown`) and for any scope/approval/budget/path drift
  (`automation_effect_reservation_scope_drift`) before writing anything; the `fetch_reservation*`,
  `fetch_permit` and `fetch_attempt` reads are the safe re-fetch path after an uncertain commit.
- `kiana-core/src/automation_effect_reservation.rs`: adds `issue_automation_effect_permit` and
  `dispatch_reserved_automation_effect`, the gate that runs the Domain `begin_attempt` check before
  the injected broker effect and refuses to call it for any denial, replay or epoch drift
  (`automation_effect_attempt_already_started`, `automation_effect_permit_fence_drift`).
- `kiana-eventlog/src/automation_effect_reservation.rs` (new module):
  `MemoryAutomationEffectReservationStore` gives the reservation/attempt facts CAS semantics on a
  per-execution stream (`commit_reservation`/`commit_attempt` with `expected_revision`, idempotent
  safe re-fetch by key or ID, one attempt per reservation, concurrent commits leaving exactly one
  winner). It declares itself CI-semantics: no fsync, no crash recovery, no cross-process fencing,
  so it cannot be cited as durable.

Fixtures (deny-first; local run not performed): `kiana-domain/tests/automation_effect_reservation.rs`
(5 tests), `kiana-core/tests/automation_effect_reservation.rs` (new, 4 tests: every denial keeps the
injected broker counter at 0 and the one allowed dispatch reaches it exactly once),
`kiana-core/tests/automation_effect_reservation_guard.rs` (2 source guards, including the
effect-after-check ordering and the rejection-code matrix) and
`kiana-eventlog/tests/aut14_effect_reservation_store.rs` (new, 3 tests). The manual lane
`.github/workflows/aut14-effect-reservation.yml` runs all four targets serially plus `--no-run`
compiles; `kiana-core-s1/6` in `scripts/ci/test-shards.json` now claims
`automation_effect_reservation` (the eventlog crate is a whole-crate shard, so its new target is
discovered automatically).

Known limits that remain open: the ledger and the store are in-process only (CI semantics); no
production ControlPlane path consults them yet; there is no durable EventLog-backed reservation
index, no crash recovery, no worker dispatch/observation separation and no live/physical effect
evidence — those remain AUT-15+/AUT-16 work. The module map's `guard_workflow_capability`
unknown-execution fail-closed gap in the production dispatch path is not changed by this slice;
deny behavior is enforced at the reservation boundary and the gap stays listed for AUT-15.
