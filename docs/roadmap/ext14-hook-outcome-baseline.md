# EXT-14 Hook Outcome Baseline

This source slice defines a closed, fail-closed Hook outcome contract:

- `Allow`, `Block`, `Ask`, `UpdateInput`, `AdditionalContext`, `Timeout`, `Cancelled` and
  `Unknown` are explicit variants;
- stdout parsing rejects unknown fields, malformed JSON, missing approval refs, oversized patches
  and oversized additional context;
- `UpdateInput` always carries `reauthorize=true`; observers cannot silently turn a failed guard
  into allow;
- timeout/cancel/unknown remain distinct outcomes for later runtime and receipt aggregation.

The product-path source guard is `kiana-core/tests/ext14_hook_outcome_guard.rs`. Local tests are
intentionally not run; GitHub Actions is the validation surface.
