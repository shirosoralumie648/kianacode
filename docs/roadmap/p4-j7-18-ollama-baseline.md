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

The `p4-j7-18-ollama` workflow runs provider response/config fixtures and a core source guard.
Local tests are intentionally not run. The step's proof level remains `source` until the remote
workflow reports success; no live Ollama endpoint or physical model behavior is claimed.

Fixture names: `ollama_eof_without_done_is_incomplete`,
`ollama_same_name_tools_keep_distinct_invocations`,
`ollama_unknown_tools_support_is_not_assumed`, `ollama_streams_before_model_completion`,
`ollama_tool_result_continuation_uses_stable_local_ids`,
`ollama_load_latency_is_distinct_from_generation_latency`,
`ollama_load_timeout_is_provider_scoped_and_bounded`.
