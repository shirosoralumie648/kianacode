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

## Completion slice (2026-10-17, CI-only)

The 2026-09-26 slice decided dispositions but had no parked case, no re-admission artifact and no
reconciliation closure, and nothing tied the decision to a committed outbox record. This slice adds
all three, still as pure contracts:

- every `AwaitReconciliation` plan now carries a typed `NotificationDeadLetter` case
  (`kiana.notification-dead-letter.v1`) with `kind`, a bounded frozen `attempt`, a deterministic
  `case_key()` for dedup across restarts, `auto_retry_allowed=false` and
  `requires_reconciliation=true` (both enforced by `validate()`), plus the evidence a case needs:
  `provider_receipt` and/or `operator_decision`;
- `NotificationRetryAdmission` (`kiana.notification-retry-admission.v1`) is the only way to express
  "try again": `validate()` refuses a reused delivery key
  (`notification_retry_admission_delivery_key_reused`) or an authority epoch that did not advance
  (`notification_retry_admission_epoch_not_advanced`), and `authorize(&plan)` binds it to the exact
  `RetryRequiresReAdmission` plan (disposition/notification/previous key/previous epoch), so a
  dead-lettered or dispatched record cannot be retried through it;
- `NotificationRecoveryPlan::reconcile(outcome, evidence_digest, at)` closes a case only with a
  `sha256:` evidence digest and only for a plan that has one: `delivered` →
  `TerminalAcknowledged` (terminal, nothing is re-sent), `not_delivered` →
  `RetryAdmissionRequired` (still needs the fresh admission), `unknown` → `RemainsDeadLettered`.
  The observed outcome is typed as `NotificationReconciliationOutcome` and the closure as
  `NotificationRecoveryResolution`. A cancel/revoke/expiry-before-send plan has no case, so
  `notification_reconciliation_no_case` refuses any attempt to rewrite a refused send as delivered;
- revoked/expired states that may already have sent (`delivery_started=true`) now park for
  reconciliation instead of being reported as `TerminalNoSend`, so a post-send revocation cannot be
  silently classified as "nothing happened";
- `kiana-core::plan_notification_outbox_recovery` derives the gate input from one committed
  NM-08 record plus its subscription: identity/revision/scope drift is a caller error, a live claim
  another worker owns is refused (`notification_recovery_foreign_live_claim`), revocation/expiry
  become `TerminalNoSend` or a parked case, and the caller's
  `NotificationAttemptObservation::{NeverStarted,PossiblyStarted}` decides whether a reported
  failure may be treated as pre-send at all. The bridge never dispatches, calls a channel or writes
  a fact.

Deny-first fixtures cover cancel-not-delivered/no-case, in-flight/Unknown no resend, post-send
revocation/expiry not reclassified, key reuse, epoch non-advance, missing evidence and
Unknown-not-retried; success fixtures cover pending dispatch, bounded pre-send re-admission,
dead-letter parking and the three reconciliation closures. The lane is
`.github/workflows/nm18-notification-recovery.yml` (manual, `workflow_dispatch`-only); the automatic
coverage is the unified `ci.yml` shard `kiana-core-s4/6`, which lists `nm18_notification_recovery`
and `nm18_notification_recovery_guard`.

Known limits after the completion slice: the dead-letter case and the reconciliation resolution are
source contracts — there is still no durable dead-letter store, no committed reconciliation fact, no
provider/channel query, no scheduler, and no live/physical delivery evidence. The retry admission is
an operator/ControlPlane artifact this module validates; it does not issue one, and it never calls a
connector.
