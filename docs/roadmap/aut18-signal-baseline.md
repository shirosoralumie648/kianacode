# AUT-18 approval/signal pause-resume baseline

AUT-18 adds a strict signal fact binding execution owner, action/path scope, authority epoch and
checkpoint sequence. Paused, Resumed and Consumed are explicit states and `consumed` is derived
from the terminal state, preventing duplicate signal use. The Core facade validates only; no UI
context, approval consumer, runner or EventLog effect is executed.

GitHub CI runs `.github/workflows/aut18-signal.yml`; local Cargo tests/build/check/clippy/smoke were
not run. This slice is `feature_status=partial`, `proof_level=source`; durable approval/signal
consumption, restart checkpoints and live execution remain open.
