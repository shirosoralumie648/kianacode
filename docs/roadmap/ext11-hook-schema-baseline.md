# EXT-11 Hook Schema Baseline

This source slice adds a normalized Hook descriptor adapter on top of the existing strict and
legacy manifest parser:

- normalized events cover SessionStart, UserPromptSubmit, BeforeModel, PreToolUse, PostToolUse,
  PostToolFailure, Compaction, Stop, SessionEnd and Terminal;
- every descriptor carries version, matcher, guard/observer phase, timeout, input/output schemas,
  source digest, effect, required scope and stable deny/ask/update semantics;
- Claude/Gemini/local legacy event names normalize into the same typed event set, while unknown
  events fail closed;
- `update_requires_reauthorization` remains true for every adapter result that could change tool
  input; adapters cannot turn deny/ask into allow.

Focused fixtures live in `kiana-skills/tests/ext11_hook_schema.rs`; the product-path source guard
is `kiana-core/tests/ext11_hook_schema_guard.rs`. Local tests are intentionally not run;
GitHub Actions is the validation surface.
