# P4-J7-15 Anthropic Messages baseline

## Scope

P4-J7-15 closes the native Anthropic Messages path on top of the shared request compiler and
Accumulator. The native dialect keeps system/content/tool-result shapes separate and requires a
complete message terminal before a tool call or final answer is accepted.

## Implemented source slice

- Anthropic request encoding preserves the separate system field, assistant text/tool-use blocks
  and user tool-result blocks, including the explicit `is_error` result flag.
- Non-streaming decode validates required message identity, content blocks, tool schemas,
  stop_reason and usage; private thinking/replay blocks remain denied until P4-J7-20.
- Streaming accepts message_start, content block lifecycle, ping and message_delta events only in
  the native order, requires closed blocks plus message_stop, and rejects usage regression.
- Native capability remains distinct from any compatibility dialect; no DeepSeek behavior is
  promoted by this card.

## Evidence boundary

GitHub Actions is the test authority. Local tests are intentionally not run. This step does not
claim reasoning replay, live Anthropic calls, durable usage settlement or compatibility-provider
parity; those remain later provider cards.
