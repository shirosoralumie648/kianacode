# P4-J7-17 OpenAI Responses baseline

## Scope

P4-J7-17 keeps OpenAI Responses as a separate protocol route. The request is stateless by
default, re-sends instructions/tools and local function history, and does not rely on hosted
tools or `previous_response_id` for the controlled tool loop.

## Implemented source slice

- Responses request encoding uses `instructions`, typed `input` items,
  `function_call_output`, flattened local function tools, `max_output_tokens` and `store=false`.
- Non-streaming decode requires `status=completed`, preserves output text and function call
  `call_id`, validates arguments against the server-owned tool schema and rejects reasoning or
  hosted tool items without protected replay authorization.
- Streaming tracks `output_index`, `item_id` and `call_id`; final output-item arguments and call
  identity must match the accumulated fragments before `response.completed` can finish the turn.
- Incomplete/failed responses never complete the run, and no `previous_response_id` shortcut is
  introduced into the local stateless history path.

## Evidence boundary

GitHub Actions is the test authority. Local tests are intentionally not run. This step does not
claim reasoning replay, hosted tool support, durable usage settlement or live OpenAI Responses
behavior; those remain later cards.
