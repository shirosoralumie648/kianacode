# UI-13 Workbench timeline and result rendering baseline

> Snapshot date: 2026-09-25. Completion slice: 2026-10-17 (CI-only). The bounded typed timeline
> projection, Workbench wiring and deny-first fixtures are wired into GitHub Actions. Local tests,
> builds and checks are intentionally not run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-13`](ui-entrypoints.md#step-ui-13) |
| source snapshot | `1e648a08` plus the UI-13 source slices |
| feature_status | `implemented` (bounded renderer, shared display roles, Workbench projection, fixture and CI wiring) |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | `DaemonHost` additive `RunStreamEnvelope` → cursor/run checks → `TimelineRenderer` → disposable Workbench messages |

The renderer is a display projection. EventLog, Receipt, ResponseEnvelope and ControlPlane remain
the authority for lifecycle, authorization and effect facts. A tool call, approval, usage record or
error is rendered as its own typed item and is never presented as assistant text or an instruction to
execute. Unknown event types remain visible as an explicit refresh marker.

## Item kinds, display roles and state markers

| Stream input | Item kind | Display role | Bound/behavior |
|---|---|---|---|
| `Delta` | `Delta` | assistant | plain text, ANSI/OSC/control filtering and item bound |
| `Terminal` | `Terminal` | assistant | terminal status/error/final text; terminal fences later events |
| `Usage` | `Usage` | system | JSON is redacted and capped at `MAX_TIMELINE_JSON_BYTES`; collapsible |
| `ToolCall` | `ToolCall` | system | committed request projection only; collapsible, never executable |
| `ApprovalRequested` | `Approval` | system | collapsible pending-action display; decision still goes through ControlPlane |
| `Artifact` | `Artifact` | changed | derived from terminal `files_changed`/`artifacts`; labelled, collapsible, diff/text capped |
| `Error` | `Error` | system | visible structured error; never cleared by a later delta |
| unknown event | `Unknown` | system | explicit refresh marker; no guessed schema or action |
| sequence gap | `Gap` | system | requires snapshot hydration; later events remain blocked |
| item/byte budget | `Limit` | system | explicit bounded degradation marker |

`TimelineItemKind::display_role()` is the single authority for every entrypoint: only `Delta` and
`Terminal` are `Assistant`, `Artifact` is `Changed`, and usage/tool/approval/error/unknown/gap/limit
rows are `System`. `workbench_chat` consumes that method and does not match item kinds itself, so a
tool result cannot be re-labelled as assistant prose at the entrypoint.

The projection accepts a duplicate sequence as `IgnoredDuplicate`, rejects foreign run IDs before
advancing the cursor, marks out-of-order sequences as `GapRequiresSnapshot`, and returns
`IgnoredAfterTerminal` after a terminal item. An epoch change is rejected as
`stream_epoch_changed` before the cursor moves. Both `apply` (typed envelopes) and `apply_event`
(legacy adapter delivery) keep the gap and terminal fences. `reset(run_id)` is the explicit
snapshot/run boundary. `toggle_collapsed` and `set_loading` only alter disposable view flags.

Required completion invariants (fixture `required` list):

- `arrival-order-never-overwrites-facts` — items are append-only, first write wins per item id, and
  a replayed or later event cannot rewrite an earlier row.
- `tool-results-are-never-assistant-text` — tool calls, usage rows, approvals and errors keep the
  `System` display role; only delta/terminal content is assistant text.
- `errors-are-never-cleared` — a recorded error row survives later deltas, duplicates and terminal
  responses; the renderer exposes no clear/remove/retain/drain API.
- `delta-json-and-diff-are-bounded` — delta and terminal bodies use the item cap, structured JSON
  rows use the JSON cap, and text/diff artifacts use the diff cap.
- `binary-mime-is-never-inlined` — binary MIME and encoded payloads are summarized, never decoded,
  inlined or echoed; payloads with no declared text shape are treated as unspecified.
- `terminal-fences-late-events` — delta, error, approval and tool-call frames after the terminal row
  are classified `IgnoredAfterTerminal` and add no item.
- `unknown-and-limit-markers-are-explicit` — unknown event kinds and budget degradation stay visible
  as their own typed rows instead of being silently dropped.
- `markdown-is-not-interpreted` — markdown, HTML and link syntax stay inert plain text.

## Per-kind truncation policy

```text
MAX_TIMELINE_ITEMS                 = 512 items, with one reserved Limit marker slot
MAX_TIMELINE_BYTES                 = 512 KiB aggregate item body bytes including the marker
MAX_TIMELINE_ITEM_BYTES            = 32 KiB per Delta/Terminal/Unknown/Gap/Limit item
MAX_TIMELINE_JSON_BYTES            = 16 KiB for Usage/ToolCall/Approval/Error JSON views
MAX_TIMELINE_DIFF_BYTES            = 16 KiB for text/diff artifact bodies
MAX_TIMELINE_ARTIFACTS_PER_RESPONSE = 64 projected artifact rows per terminal response
```

Text is treated as plain text. ANSI CSI and OSC sequences are discarded, remaining control bytes
are replaced, and common authorization/bearer/token/api-key/secret values are redacted before the
per-item bound. Authorization headers consume the complete scheme-plus-credential line so a Bearer
credential cannot remain after the scheme is redacted. Binary artifacts keep only a bounded
`id`/`path`/`name` label plus the MIME marker. Truncation is explicit on the item.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| typed snapshot fixture | Delta/Terminal/Usage/ToolCall/Approval/Artifact/Error/Unknown map to stable kinds; terminal response files/artifacts produce separate items |
| duplicate/ordering | duplicate cursor is ignored; gap emits one Gap marker and blocks later stream data; foreign run/epoch leaves cursor/items unchanged; schema drift is rejected |
| terminal fence | terminal is visible; late delta/error/approval/tool-call cannot append or clear the timeline |
| arrival order | an error row is byte-identical after a replayed delta; a later delta appends instead of overwriting |
| display roles | only Delta/Terminal are assistant; usage/tool/approval/error/unknown/gap/limit are system; artifact is changed |
| text safety | ANSI/OSC/control bytes are removed; complete Authorization scheme-plus-credential lines and other secret-shaped values are redacted; markdown/HTML stay plain text |
| binary MIME | `application/octet-stream`, `image/*`, base64/hex encodings and undeclared payloads are never inlined |
| bounded output | oversized delta/JSON/diff is truncated; item, artifact-burst and aggregate budgets emit Limit without exceeding bounds |
| presentation flags | collapse/expand and loading flags are mutable without changing item identity/body |
| entrypoint boundary guard | renderer contains no process, network, filesystem, model loop, ControlPlane or capability execution authority, and no clear/remove API |

The declarative matrix lives in
[`kiana-entrypoints/tests/fixtures/ui13-render-snapshot.json`](../../kiana-entrypoints/tests/fixtures/ui13-render-snapshot.json)
(`kiana.ui13-render-snapshot.v1`): budgets, the primary typed stream, five deny-first scenarios
(arrival order/error retention, binary MIME artifact, labelled text diff, terminal fence,
gap blocking), fixture names, required markers, denied markers, protocol-propagated denial codes
and limitations. Behaviour fixtures live in `kiana-entrypoints/tests/ui13_workbench_render.rs`;
the boundary and completion-slice guards live in
`kiana-entrypoints/tests/ui13_workbench_render_guard.rs`.

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the UI-13 snapshot and
deny-first renderer fixtures, the source/completion guards, and
`cargo test -p kiana-entrypoints --no-run --locked`. Local tests, builds, checks, clippy and smoke
commands are not run; CI results are not awaited.

Limitations: the timeline is process-local and disposable, not a durable EventLog projector or
cross-process replay cache; no OS-backed PTY, terminal emulator, browser, live provider, external
tool effect, receipt correctness, or physical/live proof is claimed. Gap recovery requires a future
snapshot/feed client contract; this slice does not add a second execution loop or change daemon
authorization. Binary detection is MIME/encoding/field-shape based, so a text MIME carrying binary
bytes is rendered as bounded plain text; markdown is intentionally not rendered as markup. The
manual lane is `workflow_dispatch`-only.
