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
- `gemini_thought_steps_require_protected_replay` locks the step ⑤ hook: a `thought` /
  `thought_summary` step is refused with `provider_private_replay_not_configured` in both the
  streaming and non-streaming decoders instead of being dropped or resent as public text.
- `gemini_hosted_or_unknown_steps_never_execute_locally` refuses hosted (`google_search`,
  `code_execution`) and unknown step types with `provider_private_or_hosted_item_unsupported`
  (stream) / `provider_content_block_unsupported` (body) rather than treating them as local
  function calls.
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

Local tests/build/check/clippy/smoke were not run. Historical provider CI was blocked before the
fixture job by the repository-wide missing `kiana-domain/src/memory_workbench.rs` module; CM-36 now
supplies that module, and the dedicated workflow path filter includes it so a fresh remote run is
triggered by this source update. That run is pending/unobserved; the step remains `source` / 🔄 until
the dedicated fixture and source-guard jobs execute. No live Gemini endpoint, billing, durable replay
storage, or provider-side effect is claimed. The API
reference describes optional interaction/step usage fields; missing usage remains unknown rather
than being fabricated as zero. Cost-vector mapping remains P4-J7-24/BQ-10.

## 2026-10-17 completion slice (CI-only)

The card's exit conditions are covered at the source level and the closing slice adds:

- `gemini_thought_steps_require_protected_replay` and
  `gemini_hosted_or_unknown_steps_never_execute_locally` close the remaining fail-closed gap around
  step ⑤: private thought material and hosted/unknown steps are refused in both the streaming and
  non-streaming decoders, and neither path can silently drop or locally execute them.
- The statelessness claim is now asserted rather than only documented:
  `gemini_stateless_function_result_round_trip` pins the ordered three-step local history and
  asserts that the wire body carries neither `previous_interaction_id` nor `interaction_id`.
- The Core source guard additionally binds `kiana-provider/src/config.rs` so the implemented API
  version stays pinned to the `https://generativelanguage.googleapis.com/v1beta` +
  `interactions` endpoint for the `gemini` / `gemini-interactions` aliases, and asserts that no
  Gemini path in request/response/config mentions `generateContent`.
- The dedicated manual workflow now runs the `cargo fmt --all --check` step its evidence text had
  already been claiming.

Status: roadmap §1.1 row 396 `🔄` → `✅` (CI pending) with proof_level `source`. The unified
`ci.yml` shards remain the automatic lane (`kiana-provider` whole-crate plus the
`kiana-core-s5/6` shard that lists `p4_j7_19_gemini_interactions_guard`); the dedicated workflow is
dispatch-only by the 2026-09-27 fan-out consolidation rule. No GitHub receipt had been observed
when this slice was written, so nothing above is `local_behavior`, `durable`, `live` or `physical`.
