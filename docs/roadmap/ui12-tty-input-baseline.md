# UI-12 TTY input state machine baseline

> Snapshot date: 2026-09-25. The bounded input contract and deny-first fixtures are wired into
> GitHub Actions. Local tests, builds and checks are intentionally not run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-12`](ui-entrypoints.md#step-ui-12) |
| source snapshot | `fe9336cf` plus this UI-12 source slice |
| feature_status | `implemented` (bounded decoder/state contract, Workbench adapter, PTY fixture and CI wiring) |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | Crossterm event/PTY decoder → bounded `TtyInputState` → immutable `CommittedInput` → existing `WorkbenchView::interpret_line` → DaemonHost-backed action handler |

The input layer owns a disposable draft, IME composition and bounded history. It does not parse or
execute shell commands. An explicit Enter is the only transition that produces `CommittedInput`;
the committed text is copied out before later cancellation or editing events. `Event::Paste` is
inserted as one text payload, so embedded newlines, slash text and terminal controls cannot
synthesize a command or a shell action.

## Input state graph

```text
PTY bytes
  ├─ normal UTF-8/key bytes ────────────────► Crossterm Event
  ├─ partial UTF-8 / escape ─► pending (no action)
  ├─ ESC [ 200~ ... ESC [ 201~ ─────────────► one Paste(text) event
  └─ dangling prefix at finish ─────────────► rejected (no submit)

InputMode::NonTty ─► reject interactive events before draft mutation
InputMode::Tty
  Draft ── Enter(non-empty) ─► CommittedInput + bounded history ─► Draft
    │                                   └─ later Ctrl-C/Esc cannot rewrite the copy
    ├─ Paste / Unicode / Ctrl-J ─────────► Draft (bounded bytes and scalar count)
    ├─ Up / Down ────────────────────────► Draft (bounded history, restores saved draft)
    ├─ Resize ───────────────────────────► Draft + TerminalSize
    ├─ IME start/update ─────────────────► Composition (not draft)
    ├─ IME commit ───────────────────────► Draft
    └─ Ctrl-C/Ctrl-D/EOF ────────────────► Cancel or Quit by running/draft state
```

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| PTY chunk decoder | split UTF-8 and escape prefixes remain pending; `finish` rejects dangling escape; bracketed paste closes before emission |
| paste boundary | pasted slash text/newlines become one bounded draft payload; no paste byte is interpreted as Enter, Escape or shell input |
| Unicode and IME | Chinese, combining marks and emoji remain scalar-safe; IME candidate is separate; Ctrl-C/Esc during composition cancels composition, not a running turn |
| draft/commit separation | Enter creates one immutable `CommittedInput`; later cancel, resize or editing cannot mutate the committed text |
| history and multiline | Ctrl-J/newline is draft data; Up/Down is bounded and restores the pre-history draft; empty/whitespace submission is a no-op that preserves the draft |
| resize/EOF/SIGINT/no-TTY | resize is a presentation transition; Ctrl-D quits only on an empty draft; nonempty draft is retained; NonTty rejects interactive events |
| bounds/control denial | control characters, oversized byte/scalar payloads and oversized paste are rejected atomically without partial draft mutation |
| entrypoint boundary guard | input module has no DaemonHost, ControlPlane, Broker, Harness, filesystem, network, process spawn or model loop; Workbench still delegates actions through existing adapter |

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the UI-12 PTY/Unicode/IME/
history/resize/EOF/SIGINT/bounds fixtures, the entrypoint source guard and
`cargo check --workspace --tests --locked`. Local tests, builds, checks, clippy and smoke commands
are not run; CI results are not awaited.

Limitations: the decoder is a deterministic source fixture for PTY chunks and Crossterm events,
not an OS-backed PTY or terminal emulator; terminal-specific IME composition is exposed through an
adapter API and remains provider/terminal dependent; cursor editing beyond bounded history and
backspace is not claimed; input state is process-local and not durable; this slice does not change
ControlPlane authorization, command execution, protocol transport, receipt truth, cross-entrypoint
parity, or live/physical effects.
