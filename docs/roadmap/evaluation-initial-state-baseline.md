# EQ-13 Evaluation Initial-State Baseline

## Scope

EQ-13 adds a strict `EvalInitialStateBundle` and an in-memory `EvalInitialStateStore` in the
existing evaluation runtime adapter.  The bundle groups policy, role assignment, memory,
workflow and artifact fixture snapshots, validates them as bounded object values, rejects raw
secret material, and computes one canonical `initial_state_digest` that binds all inputs to an
experiment identifier.

The store implements the existing `FixtureStore` port. Reads require the exact bundle digest and
an opaque allow-listed fixture name; path-like references, unknown names and scope drift fail
closed. The store is not an EventLog, policy authority, role grant, artifact projector or project
filesystem, and it has no operator-home or network access.

## Evidence and limits

- `kiana-daemon/tests/eq13_initial_state.rs` covers digest binding, fixture loading, scope/path
  denial, tamper detection, object shape and secret rejection.
- `kiana-core/tests/eq13_initial_state_guard.rs` protects the controlled-store/no-I/O boundary.
  GitHub Actions runs the fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it does not yet persist these
fixtures into a durable EvalStore, bind authenticated ProjectTrust/role snapshots, capture
EventLog/Receipt evidence, or launch/fault/restart a target. Those remain EQ-14+ and PD/ER work.
