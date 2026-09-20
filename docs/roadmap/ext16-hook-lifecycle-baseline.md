# EXT-16 Hook Lifecycle Baseline

This source slice defines one typed lifecycle dispatch contract for Hook events:

- SessionStart, UserPromptSubmit, BeforeModel, PreToolUse, PostToolUse, PostToolFailure,
  Compaction, Stop, SessionEnd and Terminal share session/run/snapshot identity and sequence;
- payloads are represented by a digest, and each dispatch has a stable event digest;
- PostToolUse/PostToolFailure require a committed result before dispatch;
- the dispatcher only creates a typed event and explicitly sets `capability_dispatch_allowed=false`.

The source guard is `kiana-core/tests/ext16_hook_lifecycle_guard.rs`. Local tests are intentionally
not run; GitHub Actions is the validation surface.
