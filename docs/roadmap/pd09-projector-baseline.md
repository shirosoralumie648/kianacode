# PD-09 Projector Checkpoint Baseline

## Scope

PD-09 adds a pure `ProjectionDriver` around the existing `ReplayProjection`. It applies a committed
tail only after fold success, materializes a digest-bound checkpoint, pauses on fold failure,
requires explicit retry, and supports rebuild-from-zero. Checkpoint state is an optimization and
never an authority fact; the driver has no EventStore/Broker/Runner access.

## Evidence and limits

- `kiana-core/tests/pd09_projection_driver.rs` covers checkpoint/tail apply, rebuild, fold failure,
  pause and explicit retry.
- `kiana-core/tests/pd09_projector_guard.rs` protects replay-only/checkpoint-first boundaries.
  GitHub Actions runs fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no durable checkpoint store,
concurrent projector lease, process restart worker or cross-process rebuild proof is claimed.
