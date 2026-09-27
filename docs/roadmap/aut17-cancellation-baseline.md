# AUT-17 cancellation generation baseline

AUT-17 adds an automation cancellation fact with generation and authority epoch fencing. Stopped
requires confirmed stop; ResultUnknown preserves an effect-started uncertainty; a late result is
only valid after explicit Fenced state. Core validates facts only and does not signal workers or
retry effects.

GitHub CI runs the domain fixture and Core guard in `.github/workflows/aut17-cancellation.yml`.
Local Cargo tests/build/check/clippy/smoke were not run. This slice is
`feature_status=partial`, `proof_level=source`; real daemon drain, supervisor stop and durable
recovery remain open.
