# UI-14 Workbench controller and command palette baseline

> Snapshot date: 2026-09-25. The controller, keymap, session switcher, command palette and
> deny-first source fixtures are wired into GitHub Actions. Local tests, builds and checks are
> intentionally not run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-14`](ui-entrypoints.md#step-ui-14) |
| source snapshot | `ad6f8000` (`origin/master` after UI-13) plus this UI-14 source slice |
| feature_status | `implemented` (pure controller contract, fixtures and CI wiring) |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | Workbench intent/keymap/palette → bounded `WorkbenchController` → `WorkbenchUiAction` → typed client `UiActionV1` → DaemonHost/ControlPlane |

The controller is a disposable intent adapter. It does not call a daemon, spawn work, cancel a run,
resume a session, or execute a capability. `WorkbenchUiAction::into_protocol` carries the
server-facing `UiActionV1` shape after the typed client supplies the canonical payload digest;
authorization, CAS, idempotency and effect evidence remain server/client contract responsibilities.

## Controller state and action mapping

| User intent | Stable command | Payload/target | Local guard |
|---|---|---|---|
| open workspace | `workbench.open` | workspace path | capability and text bound |
| attach session | `workbench.attach` | session ID | capability; clears old draft boundary |
| new session | `workbench.new` | optional title | capability; clears old draft boundary |
| continue | `workbench.continue` | run ID + prompt, active session target | capability and draft revision |
| status | `workbench.status` | active session | capability |
| run | `workbench.run` | prompt, active session target | capability and draft revision |
| cancel | `workbench.cancel` | optional run ID | capability; idle run without ID denied |
| resume | `workbench.resume` | optional run ID | capability; no implicit resume on close |
| receipt | `workbench.receipt` | optional run ID | capability |

Every prepared action has a fresh command ID, bounded idempotency key, actor, target, expected
epoch/cursor/revision and structured payload. The controller tracks `submission` separately from
`run`: `Preparing/Accepted/Applied/Rejected/Unknown` never masquerades as a run lifecycle state.
Unknown or in-flight mutation blocks a second mutation until the original command is reconciled;
there is no automatic retry with a new ID. The audit trace is bounded and records prepared/result
dispositions without claiming EventLog facts.

## Keymap, palette and session boundaries

| Surface | Contract |
|---|---|
| keymap | `Keymap` rejects two commands bound to one `KeyChord`; the default map has deterministic single-command shortcuts |
| command palette | `CommandPalette::visible` returns only entries backed by an enabled advertised `UiCapability`; empty or disabled capability sets expose no actions |
| session switcher | `SessionSwitcher` rejects duplicate/missing IDs; controller selection clears the draft and bumps its revision before a new session can be used |
| close | `WindowClose` yields `WindowClosed { run_pending }`, preserves observed run status, and emits no cancel/resume `UiAction` |

The draft revision is incremented on every edit and session boundary. A run/continue intent carrying
an older revision fails with `stale_draft` before action construction. This prevents a session
switch or late key event from submitting text that belongs to another session.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| action mapping | open/attach/new/continue/status/run/cancel/resume/receipt produce stable command names, target, payload and CAS fields |
| stale draft | an intent with an older draft revision is rejected before an action is created |
| duplicate shortcut/submission | duplicate key bindings are rejected; an in-flight/unknown mutation cannot create a second action |
| capability visibility | no advertised capability means no visible palette action and direct dispatch returns `capability_unavailable` |
| session switch | switching sessions clears the old draft and advances the revision boundary |
| safe close | closing while a run is pending does not synthesize cancel/resume and keeps run/submission dimensions distinct |
| source boundary | controller has no daemon, process, filesystem, network, broker, model loop or side-effect authority |

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the UI-14 controller/keymap/
session/palette fixtures, the source guard and `cargo check --workspace --tests --locked`. Local
tests, builds, checks, clippy and smoke commands are not run; CI results are not awaited.

Limitations: this is a process-local source contract, not a wired Workbench event loop or durable
session switcher; no live typed-client transport, server authorization, EventLog append, response
loss/reconnect, cross-process recovery, OS SIGTERM, terminal close, provider/tool effect, receipt
correctness or physical/live proof is claimed. The client boundary must compute and validate the
canonical payload digest before sending `UiActionV1`.
