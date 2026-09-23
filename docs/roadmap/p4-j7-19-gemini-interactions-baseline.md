# P4-J7-19 Gemini Interactions baseline

## Official protocol boundary

The codec targets Google's Interactions API at the configured `.../v1beta/interactions`
endpoint, not the legacy `generateContent` endpoint or candidate/chunk event format. The request
is stateless (`store=false`), streams lifecycle events, resends `system_instruction`, declared
function tools, and `generation_config.max_output_tokens` on every turn. Google documents these
request fields, function result steps, lifecycle events, status values and usage fields in the
[Interactions API reference](https://ai.google.dev/api/interactions-api) and the
[Interactions overview](https://ai.google.dev/gemini-api/docs/interactions-overview).

## Implemented source slice

- The shared request compiler forces the prepared route and wire request to stream SSE,
  `store=false`, and
  `generation_config.max_output_tokens` for each Gemini call. It sends ordered user/model/function
  steps and uses local history rather than server-side `previous_interaction_id` state.
- Function results use the matching prior call ID and advertised tool name. Failed local
  `kiana.tool-observation.v1` results retain the Interactions `is_error` signal. Only local function declarations are
  sent; the provider codec does not execute functions or enable hosted tools.
- The SSE accumulator validates interaction identity and status transitions, requires contiguous
  step indexes and start/delta/stop pairing, accumulates text and JSON argument fragments, and
  waits for `interaction.completed` before returning a `ModelReply`. `requires_action` becomes
  `tool_use`; it is not a completed model turn. Tool arguments are parsed and schema-validated
  only when the terminal response is assembled.
- Optional step usage is accumulated with overflow checks and compared to terminal usage only when
  every step has a complete observation. Partial/malformed usage and inconsistent totals fail
  closed; a terminal total takes precedence when present. The current `ModelUsage` contract
  normalizes total provider tokens into input/output totals; it does not preserve every modality or
  thought/tool-use component required for cost settlement.
- Thought/replay items and non-text modalities remain unsupported until the protected replay and
  content steps add their scoped contracts. EOF before the completed event is incomplete.

## GitHub-only fixtures

- `gemini_requires_action_is_not_run_completion` covers function call accumulation, a
  nonterminal `requires_action` status update, and terminal-only delivery of the validated call.
- `gemini_incomplete_function_arguments_never_dispatch` covers malformed partial function JSON.
- `gemini_steps_cannot_follow_terminal_status_update` denies new steps after a terminal
  `requires_action`/`completed` status update.
- `gemini_requires_action_status_must_match_function_steps` denies a function-required terminal
  status with no local function call step.
- `gemini_generate_content_events_are_not_accepted_as_interactions` rejects GenerateContent
  candidate responses and events.
- `gemini_stateless_function_result_round_trip` covers full local history, call ID/name pairing,
  result text, and known error status.
- `gemini_interaction_parameters_are_resubmitted_each_turn` covers streaming, storage opt-out,
  and per-turn generation configuration.
- `gemini_malformed_or_inconsistent_usage_fails_closed` covers partial, mistyped and contradictory
  token reports; `gemini_optional_step_usage_does_not_override_terminal_usage` covers optional
  per-step usage; the Core source guard binds these fixtures and the wire boundary.

## Verification and evidence boundary

All tests belong to GitHub Actions. The dedicated workflow runs:

```text
cargo fmt --all --check
cargo test -p kiana-provider --lib --locked gemini_ -- --test-threads=1
cargo test -p kiana-core --test p4_j7_19_gemini_interactions_guard --locked -- --test-threads=1
```

Local tests/build/check/clippy/smoke were not run. The source branch has not yet run in GitHub CI;
the step remains `source` / 🔄 until the dedicated fixture and source-guard jobs execute. No live
Gemini endpoint, billing, durable replay storage, or provider-side effect is claimed. The API
reference describes optional interaction/step usage fields; missing usage remains unknown rather
than being fabricated as zero. Cost-vector mapping remains P4-J7-24/BQ-10.
