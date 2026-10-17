# P4-J7-18 Ollama NDJSON baseline

## Scope

Ollama Chat uses the shared request compiler and bounded NDJSON transport. Model streaming is
enabled in the request body; response frames emit incremental text before the terminal `done`
frame. The adapter maps local tool call IDs back to their advertised function names when building
the next request, since Ollama does not supply a wire ID for each tool invocation.

## Implemented source slice

- Every response frame must contain an assistant message with string content and a boolean `done`.
  A successful reply additionally requires `done=true`, `done_reason`, and complete tool arguments.
- Synthetic tool IDs are derived from the admitted ModelCallId and tool ordinal. Text frames do not
  change the ordinal, so the ID remains stable across repeated decoding of the same response.
- Prompt/evaluation token counts are retained as reported usage. `load_duration` and
  `eval_duration` stay separate and are added to the committed ModelTurn metadata as
  `provider_timing`; they are observations, not admission or cost authority.
- Partial or malformed usage and timing fields are rejected instead of being silently treated as
  missing observations.
- An Ollama profile may set `ollama_load_timeout_ms` from 1,000 through 150,000. The value sets the
  first semantic event timeout and leaves room for the 30-second response-header limit within the
  transport's 180-second total request limit. Supplying it for another provider or through
  `inherit_default` is rejected.
- The provider HTTP client disables reqwest's protocol NACK retry behavior, so a single admitted
  model attempt cannot be silently resent by the HTTP library.
- Unknown tool names and orphan tool results fail closed. The adapter never downloads, creates,
  or deletes a local Ollama model.

## GitHub-only evidence

The dedicated GitHub Actions runs `35832638463` (PR) and `35832756821` (post-merge) both stopped at
`cargo fmt --all --check` because `kiana-domain/src/memory_workbench.rs` was missing while
`kiana-domain/src/lib.rs` declared the module; provider fixtures and the core source guard were
skipped. CM-36 now supplies that declared module on current master. Those two runs never executed
the provider fixtures and are not a pass for this step.

At the 2026-09-26 live status check, GitHub run `36162666095` for head `5f12d89c` was `queued`.
The check was read-only and not polled further; provider fixtures and the source guard remain
unobserved.

The 2026-09-27 `ci: consolidate 704 per-slice workflows into one unified gate` commit removed
push/PR path triggers from every slice workflow, so this lane is manual-only
(`workflow_dispatch`). Automatic coverage now comes from the unified `ci.yml`: the whole-crate
`kiana-provider` shard runs `cargo test -p kiana-provider` (lib fixtures included) and the
`kiana-core-s5/6` shard claims `p4_j7_18_ollama_guard`. Local tests remain intentionally not run;
no live Ollama endpoint or physical model behavior is claimed.

## 2026-10-17 completion slice (CI-only)

`kiana-provider/tests/p4_j7_18_ollama_stream.rs` replays captured `/api/chat` bytes through the real
`Framer` + `Accumulator` boundary with 1/2/3/5/7/11/13/31/64/127-byte chunk widths, so line-by-line
decoding is exercised at the same boundary the transport uses:

- `ollama_ndjson_split_frames_stream_text_before_the_terminal_done_frame` — two text frames stream
  before the terminal frame; terminal `prompt_eval_count`/`eval_count` and
  `load_duration`/`eval_duration` are retained as separate observations.
- `ollama_ndjson_eof_without_done_never_completes_a_run` — EOF with `done=false` (with and without
  the trailing newline) stays `provider_stream_incomplete`, `retry_class=never`,
  `request_sent=true`, `side_effect_state=unknown`.
- `ollama_ndjson_unterminated_terminal_line_completes_within_frame_limit` — an unterminated final
  line is flushed by the bounded framer and only then may complete the run.
- `ollama_ndjson_error_frame_never_completes_and_keeps_no_partial_reply` — an `{"error": ...}`
  frame fails closed after dispatch.
- `ollama_ndjson_done_true_without_done_reason_is_not_a_terminal_completion` — `done=true` without
  `done_reason` is not a completion (`provider_stop_reason_missing`).
- `ollama_tool_ordinals_stay_stable_across_frame_boundaries` — same-name tools keep distinct
  `ollama:<call_id>:<ordinal>` IDs across frame splits, while a wire-provided id is preserved.
- `ollama_tool_arguments_fail_closed_before_any_call_is_usable` — non-object arguments and
  duplicate JSON keys are rejected before any tool call can be used.

The core guard now also asserts that `/api/pull`, `/api/create`, `/api/delete`, `/api/copy` and
`/api/push` appear in none of `kiana-provider/src/{request,response,transport,config}.rs`, and the
manual lane runs `cargo fmt --all --check` plus
`cargo test -p kiana-provider --test p4_j7_18_ollama_stream --locked -- --test-threads=1`.

Fixture names: `ollama_eof_without_done_is_incomplete`,
`ollama_same_name_tools_keep_distinct_invocations`,
`ollama_unknown_tools_support_is_not_assumed`, `ollama_streams_before_model_completion`,
`ollama_tool_result_continuation_uses_stable_local_ids`,
`ollama_load_latency_is_distinct_from_generation_latency`,
`ollama_load_timeout_is_provider_scoped_and_bounded`.
