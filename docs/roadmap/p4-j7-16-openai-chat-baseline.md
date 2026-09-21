# P4-J7-16 OpenAI Chat native streaming baseline

## Scope

P4-J7-16 closes the OpenAI Chat Completions dialect on top of the shared request compiler,
bounded transport and normalized Accumulator. The route remains explicitly Chat Completions and
does not infer Responses or hosted tools from a compatible endpoint.

## Implemented source slice

- Chat requests use `max_completion_tokens`, `stream_options.include_usage` for streaming and
  `n=1`; the five server-owned local tools are the only advertised function tools.
- Empty choices usage-only chunks are retained, choice/index cardinality is strict, `[DONE]`
  requires a prior finish reason, and late deltas cannot follow a terminal finish.
- Interleaved tool calls are keyed by provider index and cannot change call IDs or names; malformed
  JSON arguments are rejected rather than converted to `{}` or another fallback object.
- Refusal, hosted-tool items, duplicate/conflicting finish markers and incomplete terminals remain
  deny-first through the shared model finish contract.

## Evidence boundary

GitHub Actions is the test authority. Local tests are intentionally not run. This step does not
claim OpenAI Responses, vendor-specific Chat dialect parity, usage settlement or live network
behavior; those remain separate provider cards.
