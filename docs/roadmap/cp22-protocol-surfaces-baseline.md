# CP-22 protocol / action-card / surface baseline

## Scope

CP-22 binds CLI, Workbench, Web and the Electron desktop shell to one versioned
`RequestEnvelope → DaemonHost → ControlPlane → ResponseEnvelope` path.  Pending approvals,
approval proof, cancel/resume/receipt/status and action-card preconditions are server-owned DTOs;
the UI stream is a disposable projection with an epoch/cursor and terminal replay.

## Evidence gate

- `cp_all_surfaces_resolve_the_same_pending_once` checks that every surface lists the server-owned
  pending set and answers it with the same challenge proof/version path, while the Desktop shell
  only launches the Web/DaemonHost surface.
- `cp_reconnect_replays_terminal_without_replaying_action` checks snapshot + stream epoch/cursor,
  gap handling and late terminal replay; reconnect/display code contains no model, broker or run
  execution call.
- `cp_noninteractive_cli_never_autoapproves_unknown_scope` checks AwaitingApproval/status/error
  mapping, explicit proof-bound approval and the interactive/non-interactive boundary.

The GitHub workflow runs the existing protocol sequence, UI projection, Web sync, approval-surface,
client and CLI/Workbench fixtures, then this guard and workspace test-target compilation.  Local
runtime tests are not run for this step.

## 2026-10-17 closure: behavioural layer

The three named acceptance tests no longer rely on source markers alone.  Each pairs its markers
with behaviour built from the same versioned DTOs the four surfaces share:

- `cp_all_surfaces_resolve_the_same_pending_once` builds four `UiActionSubmissionV1` with one
  `command_id`/`idempotency_key` and the same CAS cursor, rejects a submission whose cursor is zero
  (a surface that has seen nothing cannot answer), and requires the CLI/Workbench/Web/Desktop +
  daemon `ProtocolObservation` set to compare equal while a surface that applied the pending
  locally is refused (`protocol_parity_approval_applied_locally`).
- `cp_reconnect_replays_terminal_without_replaying_action` plans a terminal disconnect replay
  (`ReplayTerminal`, `new_effect_allowed=false`), forces a snapshot on a feed gap, drives
  `RecoveryFence` through accepted/duplicate/late-terminal/old-epoch, and re-advances the retained
  `RunStreamEnvelope::Terminal` cursor to show the replay is idempotent.
- `cp_noninteractive_cli_never_autoapproves_unknown_scope` maps `AwaitingApproval` to
  `UiErrorCode::ApprovalRequired` / exit 4 / HTTP 409 and `ResultUnknown` to `UiErrorCode::Unknown`
  with `QueryOriginal` retry / exit 8, never to success.

The workflow now also runs the disconnect fixture (`kiana-client --test ui33_recovery`), the
four-surface protocol parity fixture (`sc36_protocol_parity`) and trace parity
(`p4_j7_30_surface_parity`), and the same-id resend/reconcile fixture (`ui07_typed_clients`).  Exit
code and HTTP status continue to come from the single `CapabilityErrorCode::policy()` table.

## Limits

Evidence is source plus CI-fixture coverage only.  It does not claim a remote authenticated
transport, cross-device delivery, durable UI stream retention, live provider effects or physical
desktop packaging proof.  CP-23+ own Company/workflow commands; ER/PD own durable cursor and
pagination guarantees.
