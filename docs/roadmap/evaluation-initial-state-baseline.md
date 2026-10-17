# EQ-13 Evaluation Initial-State Baseline

## Scope

EQ-13 adds a strict `EvalInitialStateBundle` and an in-memory `EvalInitialStateStore` in the
existing evaluation runtime adapter.  The bundle groups policy, role assignment, memory,
workflow and artifact fixture snapshots, validates them as bounded object values, rejects raw
secret material, and computes one canonical `initial_state_digest` that binds all inputs to an
experiment identifier.

The store implements the existing `FixtureStore` port. Reads require the exact bundle digest and
an opaque allow-listed fixture name; path-like references, unknown names and scope drift fail
closed. Each fixture also carries a canonical `fixture_digest`, and the sandbox method
`materialize_initial_state` writes the store into `<KIANA_HOME>/eval-initial-state/<scope>/` as a
controlled, digest-bound manifest (`kiana.eval-initial-state-materialized.v1`). The materializer
builds every path from the owned sandbox root, validates the scope/name/digest shape and rejects
`..`/absolute/renamed/drifted entries before returning. The store is not an EventLog, policy
authority, role grant, artifact projector or project filesystem, and it has no operator-home or
network access.

## Evidence and limits

- `kiana-daemon/tests/eq13_initial_state.rs` covers digest binding, fixture loading, scope/path
  denial, unknown-fixture denial, tamper detection, object shape and secret/oversize rejection,
  sandbox materialization and manifest path-escape/digest-drift/scope rejection.
- `kiana-core/tests/eq13_initial_state_guard.rs` protects the controlled-store/no-I/O boundary and
  asserts the materializer never reads the operator home, process cwd or a network dependency.
  GitHub Actions runs the fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: the materialized store lives only
inside a caller-provided isolated sandbox and is not yet a durable `EvalStore`, authenticated
ProjectTrust/role authority, EventLog/Receipt evidence or a launched/faulted/restarted target.
Those remain EQ-14+ and PD/ER work.
