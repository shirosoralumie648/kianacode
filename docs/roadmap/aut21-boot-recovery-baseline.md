# AUT-21 boot recovery baseline

AUT-21 adds a strict boot recovery fact binding source/projection cursors, projection generation,
authority epoch, pending Unknown count and stale-index state. Ready is allowed only when projection
is current and no Unknown/stale index remains; otherwise reconciliation stays visible. Core validates
facts only and does not rebuild a projection or append recovery events.

GitHub CI runs `.github/workflows/aut21-boot-recovery.yml`; local Cargo tests/build/check/clippy/smoke
were not run. This slice is `feature_status=partial`, `proof_level=source`; real restart/rebuild,
durable EventLog and worker recovery remain unproven.
