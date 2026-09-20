# EXT-13 Hook Process Baseline

This source slice tightens the existing Hook executor boundary:

- every Hook shell uses a fixed working directory, cleared inherited environment and an explicit
  minimal `PATH`;
- Unix Hook processes are placed in their own process group and retain `kill_on_drop` behavior;
- timeout and abort continue through the existing cancellation path, while output is bounded and
  oversized UTF-8 output is explicitly marked `truncated`;
- no secret-bearing host environment or arbitrary workspace write set is inherited by the Hook
  process.

The product-path source guard is `kiana-core/tests/ext13_hook_process_guard.rs`. Local tests are
intentionally not run; GitHub Actions is the validation surface for this step.
