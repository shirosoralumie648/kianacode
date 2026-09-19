# H21 real request budget and stable prefix baseline

## Delivered source slice

- `WireBudgetInput` accounts for system, history/message, tool schema, attachment and provider
  framing bytes plus one shared output reservation/cap.
- `TokenAccounting::Exact` and explicit `ConservativeUtf8` are distinct; the latter carries its
  bytes-per-token and safety margin instead of pretending to be arbitrary-provider tokenizer
  truth. Unicode, emoji and long-schema boundaries are covered by CI fixtures.
- `StablePrefix` fixes ordered stable segments and keeps dynamic suffix digest out of the prefix
  and cache key. Profile, prompt bundle, catalog and data epoch all participate in cache identity,
  so revocation/data changes cannot reuse a stale prefix.
- Existing runner budget/compaction source is guarded for shared output reservation, prompt source
  provenance and product-owned stable prefix behavior.

## Boundary and proof ceiling

The exact tokenizer/provider adapter remains optional and external; local conservative estimation
is an explicit fallback. This step does not claim a provider-specific billing/tokenizer result,
cache hit rate, live provider behavior or cross-process cache durability.
