# PD-13 Pending Invocation / Continuation / Checkpoint Baseline

## Scope

PD-13 records the existing recovery-material boundary: RunSnapshot carries pending invocation,
runner state digest, sandbox/context and resumable state; InvocationResumeBinding fixes run/turn/
step/invocation/request/parameter/catalog/authority/owner/session/project/sandbox/pending-batch
identity; workspace checkpoint restore rechecks source authority, data epoch, company scope,
revision and invalidates old approvals/runner context. Recovery rebuilds pending material from
facts and requires explicit re-admission/resume.

Transcript/UI state is not a continuation authority. This step adds the CI source guard and status
handoff; it does not add a second runner loop, persist a new store, or claim cross-process durable
checkpoint/restart proof beyond existing adapters.

## Evidence and limits

- `kiana-core/tests/pd13_recovery_material_guard.rs` protects snapshot/pending/binding/checkpoint
  and no-auto-resume/no-direct-effect boundaries.
- `.github/workflows/pd13-recovery-material.yml` runs the source guard and workspace compilation in
  GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: recovery material source and CI
guard are present; durable checkpoint store, process restart and cross-process resume evidence are
not claimed.
