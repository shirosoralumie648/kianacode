# AUT-16 retry classifier baseline

AUT-16 adds a pure bounded retry classifier. Effect-started but unknown outcomes become
ReconcileUnknown; retry requires pre-send transient failure, idempotent capability, valid approval,
remaining budget/deadline and an attempt below the bound. The Core facade never creates a new
attempt or dispatches an effect.

GitHub CI runs the domain fixture and Core guard in `.github/workflows/aut16-retry.yml`. Local Cargo
tests/build/check/clippy/smoke were not run. This slice is `feature_status=partial`,
`proof_level=source`; durable attempts, leases, cancel races and live effects remain AUT-17+.
