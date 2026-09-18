# EQ-12 Evaluation DaemonHost Spine Baseline

## Scope

EQ-12 adds `kiana-daemon::eval_runtime::EvalTarget`, a thin evaluation wrapper that owns an
existing `DaemonHost` and delegates protocol requests to `DaemonHost::handle`.  It can be built
from an already composed `ControlPlane` via `DaemonHost::new`; the evaluation layer does not
construct a runner, decide policy, register capabilities, or create a second async loop.

The target still carries the EQ-09 sandbox as data so later fixture/store work can bind initial
state to the same target without discovering operator paths.  Environment setup remains an
explicit caller concern because a process-wide environment lock cannot safely be held across an
async request.

## Evidence and limits

- `kiana-daemon/tests/eq12_daemon_spine.rs` pins the public host-wrapper constructor and delegated
  `handle` call.
- `kiana-core/tests/eq12_daemon_spine_guard.rs` protects the no-second-runner/no-second-authority
  boundary.  GitHub Actions runs the fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it does not yet materialize
initial state, policy/role snapshots, fixture stores, EventLog/Receipt capture, fault plans or
quality evaluation results.  Those remain EQ-13+.
