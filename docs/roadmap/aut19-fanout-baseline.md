# AUT-19 bounded fan-out/fan-in baseline

AUT-19 adds a strict parent-child fan-out plan with unique child IDs, depth/concurrency/TTL bounds,
conservative child budget, scope subset, cycle fence, fail-fast and required fan-in. Core validates
the plan only and does not create children or execute a workflow.

GitHub CI runs `.github/workflows/aut19-fanout.yml`; local Cargo tests/build/check/clippy/smoke were
not run. The slice is `feature_status=partial`, `proof_level=source`; durable child execution,
result merge and live effects remain unproven.
