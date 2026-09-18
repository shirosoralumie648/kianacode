# CP-19 Explicit Resume / Fact Rebuild Baseline

## Scope

CP-19 makes resume an explicit, authorized command rather than startup behavior. ControlPlane reads
committed EventLog facts, filters the run and rebuilds invocation projections/pending approvals;
missing memory does not erase facts. Resume validates the snapshot digest, owner/session/project/
role/trust/sandbox, authority revision, data epoch, pending invocation binding and stale events,
then CAS-claims `run.resume_prepared` before restoring the Runner. The restored Runner re-enters
the existing `drive_run` path; approval waits remain pending, committed results are delivered only,
and uncommitted/ambiguous dispatch remains for CP-20 reconciliation.

No resume path starts a model or handler while scanning or validating. Repeated claims conflict or
return the existing committed recovery state; there is no second model loop.

## Evidence and limits

- `kiana-core/tests/p0_g03_resume_guard.rs` pins the explicit protocol/DaemonHost/core/drive path
  and no-second-loop boundary.
- `kiana-domain/tests/h14_invocation_resume.rs` covers invocation binding digest/owner/parameter/
  catalog/sandbox checks; `kiana-core/tests/cp19_explicit_resume_guard.rs` adds recovery/projection/
  port/daemon fact-rebuild markers.
- GitHub Actions runs fixtures, guards and workspace compile; local tests are intentionally not
  executed.

This slice is `feature_status=implemented`, `proof_level=source`: two-host durable lease recovery,
full crash matrix and Unknown reconciliation/retry/compensation remain CP-20+ / ER / PD work.
