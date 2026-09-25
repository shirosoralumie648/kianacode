# NM-18 notification cancellation, revocation, expiry and reconciliation baseline

> Snapshot date: 2026-09-26. Tests are GitHub Actions-only; this step does not run local tests,
> build, check, clippy or smoke commands and does not wait for CI.

`NotificationRecoveryInput` and `NotificationRecoveryPlan` are a pure, digest-bound recovery gate
over the NM-08 delivery attempt boundary. Cancel-requested, revoked, expired and acknowledged
states cannot become a delivered result. In-flight, submitted, Unknown, post-send cancellation,
authority epoch drift and inactive subscriptions return `AwaitReconciliation`; they never auto-retry.

Only a known pre-send failure within a bounded attempt limit can produce
`RetryRequiresReAdmission`, and that plan explicitly requires a fresh authority and delivery key.
Pending active work can produce a dispatch plan, but this module does not call a connector, send,
retry, write EventLog or synthesize a receipt.

`feature_status=implemented`; `proof_level=source`.

Known limits: no durable dead-letter store, provider/connector query, committed reconciliation fact,
delivery ACK, scheduler or live/physical effect evidence is claimed. Actual retry/admission remains
under ControlPlane and later connector/reconciliation steps.
