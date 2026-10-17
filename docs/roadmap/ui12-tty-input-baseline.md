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
| PTY chunk decoder | split UTF-8, escape and bracketed-paste start prefixes remain pending; `finish` rejects dangling escape; bracketed paste closes before emission |
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

## Completion slice (2026-10-17, CI-only)

The 2026-09 source slice left the card `🔄`: the dedicated lane had been removed by the 08552ada
workflow consolidation, and several deny-first cases (dangling escape plus newline, decoder-level
oversized paste, IME bounds, non-TTY IME, scalar-safe editing, history cursor reset) were only
implied. This slice closes both without widening any existing assertion:

- `kiana-entrypoints/src/tty_input.rs`: the IME API (`begin_ime`/`update_ime`/`commit_ime`/
  `cancel_ime`) now refuses `InputMode::NonTty` before composition state can exist, so
  `non-tty-input-is-inactive` holds for every interactive entry point, not only
  `apply_event`/`eof`.
- `kiana-entrypoints/tests/fixtures/ui12-pty-events.json` is now the declarative matrix
  (`kiana.ui12-pty-events.v1`): six streams with expected transition sequences (`changed`,
  `committed`, `cancel`, `resized`, `ignored`), five atomic denials, budgets
  (65536 bytes / 16384 scalars / 64 history entries) and the `required`/`denied`/`limitations`
  lists.
- `kiana-entrypoints/tests/ui12_tty_input.rs` grew from 11 to 19 fixtures; the new ones are
  `partial_escape_sequences_never_synthesize_enter_across_chunks`,
  `a_dangling_escape_across_chunks_is_rejected_and_never_submits_the_draft`,
  `bracketed_paste_payload_controls_stay_data_until_the_closing_marker`,
  `oversized_bracketed_paste_is_rejected_at_the_decoder_boundary_and_clears_pending`,
  `scalar_safe_editing_never_splits_a_character_and_combining_marks_stay_data`,
  `non_tty_mode_rejects_ime_and_resize_before_any_state_change`,
  `ime_rejects_paste_enter_and_oversized_candidates_without_touching_the_draft` and
  `history_navigation_is_bounded_and_editing_resets_the_cursor`.
- `kiana-entrypoints/tests/ui12_tty_input_guard.rs` gained a second guard
  (`tty_input_completion_slice_stays_wired_to_the_fixture_lane_and_shard`) that binds the fixture
  budgets to the source constants, every declared fixture name to a real test, the required
  markers to this document, the denial codes to the source, the Workbench commit path, the manual
  lane commands and the whole-crate shard.
- `.github/workflows/ui12-tty-input.yml` is rebuilt as the manual lane (`workflow_dispatch`-only,
  `permissions: contents: read`): `cargo fetch --locked`, `cargo fmt --all --check`,
  `cargo test -p kiana-entrypoints --test ui12_tty_input --locked -- --test-threads=1`,
  `cargo test -p kiana-entrypoints --test ui12_tty_input_guard --locked -- --test-threads=1` and
  `cargo test -p kiana-entrypoints --no-run --locked`. Automatic coverage stays the unified
  `ci.yml` `kiana-entrypoints` whole-crate shard (`scripts/ci/test-shards.json` keeps
  `targets: null`), which discovers both targets.

Required markers, each covered by named fixtures:

| Marker | Fixture |
|---|---|
| `partial-escape-never-synthesizes-enter` | `partial_escape_sequences_never_synthesize_enter_across_chunks`, `a_dangling_escape_across_chunks_is_rejected_and_never_submits_the_draft` |
| `paste-payload-is-data-not-shell-input` | `bracketed_paste_payload_controls_stay_data_until_the_closing_marker`, `decoded_pty_event_fixture_preserves_unicode_paste_resize_and_commit_boundary` |
| `cancel-cannot-rewrite-a-committed-command` | `eof_sigint_escape_and_resize_do_not_rewrite_a_committed_command` |
| `oversized-input-is-rejected-atomically` | `deny_first_rejects_terminal_controls_and_oversized_paste_without_partial_mutation`, `oversized_bracketed_paste_is_rejected_at_the_decoder_boundary_and_clears_pending` |
| `ime-candidate-is-separate-from-the-draft` | `ime_candidate_is_separate_and_ctrl_c_cannot_cancel_a_running_turn`, `ime_rejects_paste_enter_and_oversized_candidates_without_touching_the_draft` |
| `non-tty-input-is-inactive` | `non_tty_mode_rejects_interactive_events_before_any_draft_change`, `non_tty_mode_rejects_ime_and_resize_before_any_state_change` |
| `unicode-and-combining-marks-stay-scalar-safe` | `scalar_safe_editing_never_splits_a_character_and_combining_marks_stay_data` |
| `history-is-bounded-and-restores-the-draft` | `history_is_bounded_and_restores_the_uncommitted_draft`, `history_navigation_is_bounded_and_editing_resets_the_cursor` |
| `resize-eof-and-sigint-are-explicit-transitions` | `eof_only_exits_an_empty_editor_and_never_submits_a_partial_draft`, `eof_sigint_escape_and_resize_do_not_rewrite_a_committed_command` |

Known limits of this slice: a carriage return inside a bracketed paste payload is refused as a
terminal control instead of being normalized to a newline; the live Workbench loop still consumes
crossterm's own event stream, so `PtyChunkDecoder` remains a deterministic source fixture rather
than the production PTY reader; no OS-backed PTY, terminal emulator or IME engine is exercised;
`proof_level` stays `source` and no `local_behavior`/`durable`/`live`/`physical` promotion is
claimed.
