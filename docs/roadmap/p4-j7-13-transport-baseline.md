# P4-J7-13 bounded transport baseline

## Scope

P4-J7-13 freezes the provider transport boundary after request admission: connect/header,
first-semantic-event, read-idle, attempt-total and the prepared call deadline remain separate;
SSE and NDJSON are framed incrementally with bounded buffers and no hidden SDK retry.

## Implemented source slice

- Header, first semantic event, idle read and total attempt deadlines are applied at distinct
  layers, while the prepared request deadline remains the outer authority.
- SSE supports CRLF, multi-line `data:` records, comments/metadata, arbitrary UTF-8 chunking and
  bounded incomplete-tail rejection; NDJSON permits one bounded final line without a newline.
- Decompressed body and per-frame limits are enforced before unbounded accumulation; invalid
  Content-Type, invalid UTF-8, oversized frames, truncated streams and non-JSON success bodies
  fail closed.
- Connection capacity is held by an owned semaphore permit and dropping the async request future
  remains the cancellation boundary; no reqwest retry policy is enabled.

## Evidence boundary

GitHub Actions is the test authority. Local tests are intentionally not run. This step does not
claim protocol-specific terminal correctness, usage settlement, retry policy or live provider
behavior; those remain P4-J7-14 and later.
