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

## Limits

Evidence is source plus CI-fixture coverage only.  It does not claim a remote authenticated
transport, cross-device delivery, durable UI stream retention, live provider effects or physical
desktop packaging proof.  CP-23+ own Company/workflow commands; ER/PD own durable cursor and
pagination guarantees.
