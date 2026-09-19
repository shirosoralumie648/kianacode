# CAP-24 Invocation recovery, bounded retry and reconciliation baseline

CAP-24 uses the existing EventLog as the authority for Invocation state. `project_invocations`
rebuilds request, approval, dispatch, attempt and terminal facts by stable request/call identity;
duplicate or conflicting terminal evidence fails closed, and a dispatch without a terminal fact
projects as `Unknown` rather than success.

Resume is an explicit claim over a committed run snapshot. The binding rechecks owner, session,
project, role prompt, parameter/action/catalog digests, sandbox, pending batch and authority/data
epochs. Redacted snapshot material is display-only and makes the snapshot non-resumable; an
unavailable continuation does not mint executable input. A successful resume restores the existing
Runner/Cell state through ControlPlane and never directly invokes a handler.

Model retries are new bounded attempts with their own identity, deadline, retry class, budget
reservation and settlement. Unknown, cancelled, denied, or effectful/non-idempotent outcomes are
not automatically retried. Reconciliation accepts only explicit scoped evidence, appends a
correction/evidence fact, keeps the original runtime outcome unchanged, and never acts as a retry;
compensation or a new request must obtain its own authorization.

GitHub Actions runs replay, resume, invocation, retry, reliability and reconciliation fixtures,
source guards and workspace compilation. No local runtime tests or smoke commands were run.
