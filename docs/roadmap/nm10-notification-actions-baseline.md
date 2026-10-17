# NM-10 notification action refs and HumanTask action command baseline

> 快照日期：2026-09-25（完成切片 2026-10-17，CI-only）。测试只由 GitHub Actions 执行；本步骤
> 不在本地运行测试、build、check、clippy 或 smoke，且不等待 CI。

`NotificationActionCommand` is a strict, redaction-safe command candidate. It binds one registered
`ActionRef` to an exact notification scope/id, target revision/digest, recipient/decider, authority
epoch, committed source cursor, opaque payload and idempotency key. `NotificationActionGate` compares
the command against the server-authoritative ref and current target before returning a typed
ControlPlane admission.

The gate returns `control_plane_required=true` and `direct_effect=false`; it does not mutate
HumanTask/Approval, append EventLog facts, consume a permit, invoke Broker, retry, or start a worker.
Stale ref/target/epoch/cursor, wrong decider, expiry, scope widening, unknown action and secret
payload fail closed. `feature_status=implemented`; `proof_level=source`.

## Completion slice (2026-10-17, CI-only)

The card acceptance is now covered end to end at the source level:

- `NotificationActionKind::from_command` is the closed registry of the eight registered actions
  (approve/acknowledge/review/reconcile/snooze/escalate/delegate/withdraw); an unknown ref command
  name fails closed with `notification_action_unknown` before any target comparison.
- `NotificationActionCommand` binds an optional `expected_criteria_digest`, and the gate compares it
  exactly (`notification_action_criteria_conflict`). A payload carrying `approved`/`decision`/
  `task_status`/`delivered`/`acknowledged` keys is refused at validation
  (`notification_action_payload_forged_decision`), so an ACK can never smuggle an approval.
- `NotificationActionTarget` carries the server-authoritative revision, target digest, criteria
  digest, authority epoch and source cursor; the gate rejects stale revision/digest/criteria/epoch/
  cursor separately and enforces the `[created_at, expires_at)` freshness window.
- `HumanTaskActionLedger` is the ControlPlane-side single-consume authority. `consume` re-runs the
  gate (TOCTOU) and accepts only the exact admission it recomputes
  (`notification_action_admission_mismatch`). Replaying the same command digest is idempotent and
  returns the first decision; any other second command for the same action ref is refused
  (`notification_action_already_consumed`), and a reused idempotency key with a different digest is
  `notification_action_idempotency_conflict`.
- `NotificationActionDecision` records `applied`/`failed`/`unknown` (with a required code for
  failed/unknown and no code for applied) plus the derived authority effect
  (`presentation`/`governance`/`decision`). It carries no delivery/read/task-status field; forged
  delivery ACK or read fields fail at the `deny_unknown_fields` boundary, and an ACK decision can
  never decode as a task decision (`notification_action_decision_authority_effect_mismatch`).
- `NotificationActionWaitKey` binds one durable wait key to one action ref and one exact target
  revision/digest/epoch/cursor. One decision wakes exactly one wait key: the first consumption
  returns the wake, replays return none, a second distinct key for the same ref is ambiguous
  (`notification_action_wait_key_ambiguous`), a consumed ref cannot register another key
  (`notification_action_wait_key_consumed`), and a key whose binding drifted from the current
  target refuses the decision (`notification_action_wait_key_binding_mismatch`).
- `ControlPlane::admit_notification_action` routes the command through the original ControlPlane
  surface (pure, no append/dispatch) instead of a second execution loop.

Deny-first fixtures (`kiana-core/tests/nm10_notification_action.rs`, 6 named cases):
`gate_refuses_stale_target_wrong_decider_criteria_epoch_cursor_and_unknown_action`,
`gate_accepts_only_authoritative_fresh_target_and_returns_no_effect`,
`ledger_consumes_once_and_replays_the_first_decision`,
`ledger_wakes_exactly_one_wait_key_per_decision`,
`delivery_and_read_ack_never_forge_an_authority_decision` and
`control_plane_facade_admits_only_bound_notification_actions`. Domain fixtures
(`kiana-domain/tests/nm10_notification_actions.rs`, 7 named cases) cover ref/target/payload
binding, forged decision fields, unknown actions, criteria digest binding, typed round trips,
outcome shape and wait-key self-validation. The source guard
(`kiana-core/tests/nm10_notification_action_guard.rs`) binds the markers, the forbidden
Broker/EventStore/filesystem/network surface, the fixture names, the shard registration
(`kiana-core-s4/6` and `kiana-domain-s3/4` already list the three targets) and the manual lane
`.github/workflows/nm10-notification-actions.yml` (`workflow_dispatch`-only, `contents: read`,
`cargo fetch --locked` + fmt gate + three targets + three `--no-run`).

## Known limits

This remains a source/CI admission + in-process ledger slice. No EventLog append of the decision
fact, no durable ledger/checkpoint, no cross-process idempotency or CAS, no delivery-worker
settlement, and no external/live/physical action evidence is claimed. `Applied/Failed/Unknown` are
recorded values provided by the caller of `consume`, not observed provider effects. Actual
registered CommandRequest routing beyond the `ControlPlane::admit_notification_action` facade and
the wiring into every entrypoint remain later NM-13/NM-14/NM-15 work. The local run is CI-only and
the receipt is not awaited.
