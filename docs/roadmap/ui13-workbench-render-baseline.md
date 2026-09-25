# UI-13 Workbench timeline and result rendering baseline

> Snapshot date: 2026-09-25. The bounded typed timeline projection, Workbench wiring and
> deny-first fixtures are wired into GitHub Actions. Local tests, builds and checks are
> intentionally not run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-13`](ui-entrypoints.md#step-ui-13) |
| source snapshot | `1e648a08` plus this UI-13 source slice |
| feature_status | `implemented` (bounded renderer, Workbench projection, fixture and CI wiring) |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | `DaemonHost` additive `RunStreamEnvelope` → cursor/run checks → `TimelineRenderer` → disposable Workbench messages |

The renderer is a display projection. EventLog, Receipt, ResponseEnvelope and ControlPlane remain
the authority for lifecycle, authorization and effect facts. A tool call, approval, usage record or
error is rendered as its own typed item and is never presented as assistant text or an instruction to
execute. Unknown event types remain visible as an explicit refresh marker.

## Item kinds and state markers

| Stream input | Item kind | Display role | Bound/behavior |
|---|---|---|---|
| `Delta` | `Delta` | assistant | plain text, ANSI/OSC/control filtering and item bound |
| `Terminal` | `Terminal` | assistant | terminal status/error/final text; terminal fences later events |
| `Usage` | `Usage` | system | JSON is redacted and capped at `MAX_TIMELINE_JSON_BYTES`; collapsible |
| `ToolCall` | `ToolCall` | system | committed request projection only; collapsible, never executable |
| `ApprovalRequested` | `Approval` | system | collapsible pending-action display; decision still goes through ControlPlane |
| `Artifact` | `Artifact` | changed | derived from terminal `files_changed`/`artifacts`; collapsible |
| `Error` | `Error` | system | visible structured error; never cleared by a later delta |
| unknown event | `Unknown` | system | explicit refresh marker; no guessed schema or action |
| sequence gap | `Gap` | system | requires snapshot hydration; later events remain blocked |
| item/byte budget | `Limit` | system | explicit bounded degradation marker |

The projection accepts a duplicate sequence as `IgnoredDuplicate`, rejects foreign run IDs before
advancing the cursor, marks out-of-order sequences as `GapRequiresSnapshot`, and returns
`IgnoredAfterTerminal` after a terminal item. `reset(run_id)` is the explicit snapshot/run boundary.
`toggle_collapsed` and `set_loading` only alter disposable view flags.

## Resource and text limits

```text
MAX_TIMELINE_ITEMS      = 512 items, with one reserved Limit marker slot
MAX_TIMELINE_BYTES      = 512 KiB aggregate item body bytes including the marker
MAX_TIMELINE_ITEM_BYTES = 32 KiB per item
MAX_TIMELINE_JSON_BYTES = 16 KiB for Usage/ToolCall/Approval/Error JSON views
```

Text is treated as plain text. ANSI CSI and OSC sequences are discarded, remaining control bytes
are replaced, and common authorization/bearer/token/api-key/secret values are redacted before the
per-item bound. Authorization headers consume the complete scheme-plus-credential line so a Bearer
credential cannot remain after the scheme is redacted. Markdown is not parsed or rendered as markup.
Truncation is explicit on the item.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| typed snapshot fixture | Delta/Terminal/Usage/ToolCall/Approval/Artifact/Error/Unknown map to stable kinds; terminal response files/artifacts produce separate items |
| duplicate/ordering | duplicate cursor is ignored; gap emits one Gap marker and blocks later stream data; foreign run leaves cursor/items unchanged |
| terminal fence | terminal is visible; late delta/error cannot append or clear the timeline |
| text safety | ANSI/OSC/control bytes are removed; complete Authorization scheme-plus-credential lines and other secret-shaped values are redacted; markdown is left as plain text |
| bounded output | oversized delta/JSON is truncated; item and aggregate budgets emit Limit without exceeding bounds |
| presentation flags | collapse/expand and loading flags are mutable without changing item identity/body |
| entrypoint boundary guard | renderer contains no process, network, filesystem, model loop, ControlPlane or capability execution authority |

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the UI-13 snapshot and
deny-first renderer fixtures, the source guard, and `cargo check --workspace --tests --locked`.
Local tests, builds, checks, clippy and smoke commands are not run; CI results are not awaited.

Limitations: the timeline is process-local and disposable, not a durable EventLog projector or
cross-process replay cache; no OS-backed PTY, terminal emulator, browser, live provider, external
tool effect, receipt correctness, or physical/live proof is claimed. Gap recovery requires a future
snapshot/feed client contract; this slice does not add a second execution loop or change daemon
authorization.
