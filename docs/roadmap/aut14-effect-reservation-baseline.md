# AUT-14 effect reservation baseline

AUT-14 adds a strict reservation fact binding execution/action digest, capability, budget/path
scope, approval and authority/config/policy revisions. The bounded ledger replays identical
idempotency payloads and rejects revision drift or payload conflict before any effect path.

`kiana-core` exposes validation only; no Broker, provider, EventLog or worker call is made. CI runs
`kiana-domain/tests/automation_effect_reservation.rs` and the Core source guard in
`.github/workflows/aut14-effect-reservation.yml`. Local Cargo tests/build/check/clippy/smoke were
not run. The slice remains `feature_status=partial`, `proof_level=source`; durable CAS, permit
consumption, worker dispatch and live/physical effects remain AUT-15+.
