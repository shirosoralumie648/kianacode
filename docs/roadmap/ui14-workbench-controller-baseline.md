# UI-14 Workbench controller and command palette baseline

> Snapshot date: 2026-09-25. Completion slice: 2026-10-17 (CI-only). The controller, keymap, session
> switcher, command palette, disconnect/lost-response fences, restart recovery and deny-first source
> fixtures are wired into GitHub Actions. Local tests, builds and checks are intentionally not run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-14`](ui-entrypoints.md#step-ui-14) |
| source snapshot | `b424cd6c24db` (`origin/master` after UI-13) plus this UI-14 completion slice |
| feature_status | `implemented` (pure controller contract, fail-closed delivery/recovery fences, fixtures and CI wiring) |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | Workbench intent/keymap/palette → bounded `WorkbenchController` → `WorkbenchUiAction` → typed client `UiActionV1` → DaemonHost/ControlPlane |

The controller is a disposable intent adapter. It does not call a daemon, spawn work, cancel a run,
resume a session, or execute a capability. `WorkbenchUiAction::into_protocol` carries the
server-facing `UiActionV1` shape after the typed client supplies the canonical payload digest;
authorization, CAS, idempotency and effect evidence remain server/client contract responsibilities.

## Controller state and action mapping

| User intent | Stable command | Payload/target | Local guard |
|---|---|---|---|
| open workspace | `workbench.open` | workspace path | capability and text bound (≤256 bytes) |
| attach session | `workbench.attach` | session ID | capability; clears old draft boundary |
| new session | `workbench.new` | optional title | capability; clears old draft boundary |
| continue | `workbench.continue` | run ID + prompt, active session target | capability and draft revision |
| status | `workbench.status` | active session | capability |
| run | `workbench.run` | prompt, active session target | capability and draft revision |
| cancel | `workbench.cancel` | optional run ID | capability; idle run without ID denied |
| resume | `workbench.resume` | optional run ID | capability; no implicit resume on close |
| receipt | `workbench.receipt` | optional run ID | capability |

Every prepared action has a fresh command ID, bounded idempotency key
(`workbench:<command>:<sequence>`), actor, target, expected epoch/cursor/revision and structured
payload. The controller tracks `submission` separately from `run`: `Preparing/Accepted/Applied/
Rejected/Unknown` never masquerades as a run lifecycle state, and a terminal run observation does
**not** clear an unreconciled submission. Unknown or in-flight mutation blocks a second mutation
until the original command is reconciled; there is no automatic retry with a new ID. The audit trace
is bounded (`MAX_CONTROLLER_AUDIT_ENTRIES` = 128) and records prepared/result/lost/recovered
dispositions without claiming EventLog facts.

The `Open` workspace target is capped at the protocol's 256-byte `UiActionV1.target_id` bound, and
the idempotency key uses only the controller command and monotonic sequence. A long target cannot
make a prepared action fail later at typed-client validation or inflate the key past its wire bound.

## Delivery loss, termination signals and restart recovery

| Boundary | Contract |
|---|---|
| connection leaves `Attached` | `set_connection` returns `ConnectionTransition { previous, current, in_flight }`; an unreconciled submission becomes `SubmissionStatus::Unknown` and its original `SubmissionIdentity` is handed back. No retry, no new command ID, no cancel/resume, run dimension untouched |
| connection returns to `Attached` | nothing is re-sent; the unreconciled identity still blocks new mutations |
| `Degraded` dispatch | refused locally (`workbench_controller_offline`): the controller owns no send queue, so "prepare now, send later" cannot exist |
| SIGINT/SIGTERM | `note_termination_signal` records the signal, marks the in-flight submission unknown and refuses further intents (`workbench_controller_terminating`). A signal is not a window close and never synthesizes cancel/resume; `WindowClose` still finalizes |
| stale card | a result whose command ID does not match the pending identity is rejected (`action_result_command_mismatch`); a result for an already settled card is rejected (`action_result_without_submission`) |
| restart | `recover(RecoveryObservation)` rebuilds from server-observed coordinates only. The observation carries no draft, no submission claim and no action; the unreconciled identity and the idempotency sequence are the only things that cross the process boundary |

`recover` refuses, in a fixed order: a non-`Attached` observation
(`workbench_recovery_offline`), blank scope (`workbench_controller_scope_invalid`), zero
cursor/revision (`workbench_controller_cursor_invalid`), a different workspace/principal
(`workbench_recovery_scope_changed`), a different epoch (`workbench_recovery_epoch_mismatch`),
regressing coordinates (`workbench_recovery_cursor_stale`, `workbench_recovery_revision_stale`),
and a session switch smuggled in as recovery (`workbench_recovery_session_changed`). The session
set and capabilities still pass `SessionSwitcher::new` and `UiCapability::validate`. A successful
recovery returns `WorkbenchRecovery { controller, reconcile }`: the new controller starts with an
empty draft whose revision is ahead of the previous process, continues the idempotency sequence so
no fresh key can collide with a key the dead process already used, and stays blocked on
`SubmissionStatus::Unknown` until `record_action_result` settles the original command ID.

## Keymap, palette and session boundaries

| Surface | Contract |
|---|---|
| keymap | `Keymap` rejects two commands bound to one `KeyChord` (`duplicate_shortcut`) and one command bound to two chords (`duplicate_command_binding`); the default map has deterministic single-command shortcuts |
| command palette | `CommandPalette::visible` returns only entries backed by an enabled advertised `UiCapability`; empty or disabled capability sets expose no actions and direct dispatch returns `capability_unavailable:<command>` before any sequence/audit change |
| session switcher | `SessionSwitcher` rejects duplicate/missing IDs and the 256-session limit; controller selection clears the draft and bumps its revision before a new session can be used |
| close | `WindowClose` yields `WindowClosed { run_pending }`, preserves observed run status, and emits no cancel/resume `UiAction` |

The draft revision is incremented on every edit and session boundary. A run/continue intent carrying
an older revision fails with `stale_draft` before action construction, and every prepared action
targets the currently selected session, so a session switch or late key event cannot submit text
that belongs to another session.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| action mapping | open/attach/new/continue/status/run/cancel/resume/receipt produce stable command names, target, payload keys, CAS fields and wire-valid `UiActionV1` |
| stale draft | an intent with an older draft revision is rejected before an action is created |
| session switch | switching sessions clears the old draft, advances the revision boundary and re-targets later actions; the previous revision cannot be re-submitted |
| duplicate shortcut/submission | duplicate chords, duplicate command bindings, duplicate palette commands and in-flight/unknown mutations cannot create a second action |
| capability visibility | no advertised capability means no visible palette action and direct dispatch returns `capability_unavailable` |
| denied intents | offline, closed, terminating, capability-less and out-of-bounds intents leave no action, no sequence and no audit entry |
| disconnect | leaving `Attached` marks the submission unknown with its original identity, survives reconnect and blocks new mutations |
| response loss | a terminal run observation does not clear an unknown submission; reconciliation uses the original command ID only |
| stale card | late/foreign results are rejected and never resurrect a settled submission |
| safe close | closing while a run is pending does not synthesize cancel/resume and keeps run/submission dimensions distinct |
| termination | SIGINT/SIGTERM records the signal, keeps the identity, refuses new intents and never emits cancel/resume |
| restart recovery | stale/foreign/widened observations are refused; a successful recovery carries only coordinates, the unreconciled identity and the idempotency sequence |
| bounded trace | a hostile control loop cannot grow the audit trace, and retained rows are whole identity pairs |
| source boundary | controller has no daemon, client, process, filesystem, network, broker, model loop or side-effect authority |

The declarative matrix lives in
[`kiana-entrypoints/tests/fixtures/ui14-controller-actions.json`](../../kiana-entrypoints/tests/fixtures/ui14-controller-actions.json)
(`kiana.ui14-workbench-controller.v1`): budgets, the nine commands, the default keymap, the intent
table (target/payload/mutating/clears-draft), twelve deny-first scenarios with their refusal codes,
the recovery contract (observation fields, what is not carried, what is carried), 21 fixture names,
required markers, denied markers, denial templates, protocol-propagated wire bounds and limitations.
Behaviour fixtures live in `kiana-entrypoints/tests/ui14_workbench_controller.rs`; the boundary and
completion-slice guards live in `kiana-entrypoints/tests/ui14_workbench_controller_guard.rs`.

Required completion invariants (fixture `required` list):

- `session-switch-cannot-reuse-the-previous-draft` — the pre-switch revision is rejected and later
  actions target the newly selected session.
- `duplicate-shortcut-cannot-yield-two-commands` — one chord maps to exactly one command and one
  command to exactly one chord.
- `missing-capability-hides-and-refuses-actions` — an unadvertised or disabled capability is neither
  shown nor dispatchable.
- `window-close-never-cancels-or-resumes` — closing emits a local `WindowClose` record only.
- `multi-session-targets-stay-bound-to-the-selected-session` — targets and audit identities stay
  per-session; `attach` does not silently move the local selection.
- `duplicate-submit-cannot-mint-a-second-command` — a second mutation needs the original identity
  reconciled first.
- `disconnect-marks-the-submission-unknown-with-its-original-identity` — the identity survives the
  connection boundary byte for byte.
- `lost-response-is-reconciled-by-the-original-command-id-only` — no new command ID, no auto-retry.
- `stale-card-results-are-rejected` — mismatched or settled-card results never become success.
- `sigterm-is-not-a-window-close-or-a-cancel` — signals only mark the unknown outcome.
- `restart-recovery-carries-only-server-coordinates` — the observation type has no room for local
  draft/submission/action claims.
- `restart-recovery-continues-the-idempotency-sequence` — fresh keys cannot collide with keys the
  dead process used.
- `action-audit-trace-is-bounded-and-append-only` — the trace is trimmed from the front only.
- `submission-state-and-run-state-stay-separate` — run observations never settle a submission.
- `controller-owns-no-transport-or-effect-authority` — the module has no daemon/client/broker,
  process, filesystem or network surface.

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the UI-14
controller/keymap/session/palette/recovery fixtures, the source/completion guards, and
`cargo test -p kiana-entrypoints --no-run --locked`. The manual lane is
[`.github/workflows/ui14-workbench-controller.yml`](../../.github/workflows/ui14-workbench-controller.yml)
(`workflow_dispatch`-only, `permissions: contents: read`, `--test-threads=1` for both targets); the
automatic coverage stays the unified `ci.yml` `kiana-entrypoints` whole-crate shard
(`scripts/ci/test-shards.json` is unchanged, so both targets are discovered). Local tests, builds,
checks, clippy and smoke commands are not run; CI results are not awaited.

Limitations: this is a process-local source contract, not a wired Workbench event loop or durable
session switcher; no live typed-client transport, server authorization, EventLog append, response
loss/reconnect over a socket, cross-process persistence, OS signal handler, terminal close,
provider/tool effect, receipt correctness or physical/live proof is claimed. The client boundary must
compute and validate the canonical payload digest before sending `UiActionV1`; the audit trace is a
bounded local view, not a durable journal; and recovery is a read-only contract over server-observed
coordinates, so a real restarted process still depends on the client to query the original command
and hand the result back.
