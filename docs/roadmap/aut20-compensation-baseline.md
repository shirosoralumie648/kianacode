# AUT-20 compensation baseline

AUT-20 adds a strict compensation plan binding the original execution/attempt and action digest to
a distinct compensation execution/action and fresh authorization epoch. Reusing the original
permit is rejected. Core validates plans only; no compensation workflow or effect runs.

GitHub CI runs `.github/workflows/aut20-compensation.yml`; local Cargo tests/build/check/clippy/smoke
were not run. This slice is `feature_status=partial`, `proof_level=source`; durable compensation,
external side effects and live outcomes remain unproven.
