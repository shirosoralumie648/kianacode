# P4-J7-12 Request compilation baseline

## Scope

P4-J7-12 freezes the final request shape before admission/send: the server-owned five-tool
catalog is mapped once, every protocol uses the same reversible wire name map, history pairing is
validated before encoding, and the final wire body is included in the prepared request hash.

## Implemented source slice

- `ToolNameMap` rejects unknown tools, duplicate internal names and wire collisions while keeping
  forward and reverse maps plus an explicit strictness flag.
- Anthropic, OpenAI Chat, OpenAI Responses, Ollama and Gemini request encoders use the same map;
  provider credentials/options are not accepted from model request content.
- The admission budget is split across final wire messages, system material, tool schemas and
  reserved output. `PreparedModelCall::request_hash` freezes the post-encoding payload.
- Existing `validate_model_history` rejects orphan or incomplete tool-result batches before send;
  it does not invent missing results or IDs.

## Evidence boundary

GitHub Actions is the test authority. Local tests are intentionally not run. This step does not
claim live transport, provider-specific stream completeness, response accumulation, durable
usage settlement or physical model effects; those remain P4-J7-13+ work.
