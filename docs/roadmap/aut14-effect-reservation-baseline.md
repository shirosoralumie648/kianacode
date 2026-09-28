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
