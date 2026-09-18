# CP-18 RunSnapshot / Safe Checkpoint Baseline

## Scope

CP-18 binds a quiescent `RunSnapshot` to run/context/sandbox, role prompt hash, authority and data
epochs, runner-state digest, CellRegistry checkpoint and pending invocation/resume bindings. Core
writes the snapshot only at checkpoint boundaries, marks redacted material non-resumable, and
resume rejects missing/changed authority, trust, role, data epoch, scope, stale facts or state
digests before restoring the Runner. Workspace checkpoints bind actor/session/role/project/path
scope, file content revision, data epoch and operator approval; daemon capture/preview/restore uses
file identity and no-follow checks, and revocation invalidates old checkpoints.

Snapshots and checkpoints do not execute a model/handler while being built or restored; they only
re-enter the existing authorization/drive path after validation.

## Evidence and limits

- `kiana-core/tests/p2_k4_01_checkpoint.rs` covers workspace capture/preview/restore, revocation,
  approval/runner invalidation and file identity markers.
- `kiana-domain/tests/h14_invocation_resume.rs` covers server-owned resume binding digest/owner/
  parameter/sandbox checks; `kiana-core/tests/p0_g03_resume_guard.rs` pins the shared drive path.
- `kiana-core/tests/cp18_run_snapshot_guard.rs` pins RunSnapshot/recovery/checkpoint/restore/port
  boundaries and rejects recovery-side Broker/Model execution.
- GitHub Actions runs fixtures, source guard and workspace compile; local tests are intentionally
  not executed.

This slice is `feature_status=implemented`, `proof_level=source`: durable cross-process snapshot
blob guarantees, physical backup retention, full crash-resume matrix and external/live/physical
proof remain CP-19+ / ER / PD work.
