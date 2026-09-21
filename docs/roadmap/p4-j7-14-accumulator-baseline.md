# P4-J7-14 normalized stream and terminal baseline

## Scope

P4-J7-14 records the single provider `Accumulator` shared by streaming protocols. It owns the
message/block transition checks, tool-argument buffering, terminal identity and final output
validation; transport EOF is not a model completion signal.

## Implemented source slice

- Anthropic, OpenAI Chat, OpenAI Responses, Ollama and Gemini events enter one accumulator and
  share the same bounded block/argument/text limits and output validation.
- Start/delta/stop ordering, duplicate identities, closed blocks, open-block terminals, late
  deltas, unknown required events and invalid tool JSON fail closed.
- `[DONE]`, `message_stop`, `response.completed`, Ollama `done=true` and Gemini completed or
  requires_action each have explicit protocol semantics; length/refusal/pause/incomplete stops
  still pass through `ModelFinish::require_complete` before a completed reply is accepted.
- Tool calls are only converted to `ModelToolCall` after final validation; duplicate text deltas
  are delivered as separate deltas and are not deduplicated.

## Evidence boundary

GitHub Actions is the test authority. Local tests are intentionally not run. This step does not
claim provider-specific production transport, reasoning replay, usage settlement or live model
quality; those remain later provider cards.
