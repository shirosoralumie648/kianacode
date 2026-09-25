# NM-17 notification severity, digest, reminder and escalation baseline

> Snapshot date: 2026-09-26. Tests are GitHub Actions-only; this step does not run local tests,
> build, check, clippy or smoke commands and does not wait for CI.

`NotificationPolicyDecision` is a server-owned, source-bound plan over the existing NM-11 priority
projection. It maps low/medium/high/critical severity to bounded routes: low may be suppressed,
medium may enter a digest, high is immediate or same-day after quiet hours, and critical/explicit
Unknown bypass digest suppression. Quiet hours are UTC policy metadata, not client time authority.

Every decision retains owner, source event IDs, digest group, deadline and a bounded next action. High
and critical decisions carry an `NotificationEscalationFact` with the same bindings. The planner is
pure and returns a decision only; it does not send, schedule a task, approve, retry, close, call a
connector or write EventLog. External channels remain disabled by default through the existing
channel catalog.

`feature_status=implemented`; `proof_level=source`.

Known limits: no durable reminder scheduler, digest worker, quiet-hours timezone service, escalation
fact commit, connector delivery or live/physical receipt evidence is claimed. Repeated-reminder
prevention is bounded by the policy interval input; execution/reconciliation remains later NM-18+.
