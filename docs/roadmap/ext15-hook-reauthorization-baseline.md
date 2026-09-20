# EXT-15 Hook Reauthorization Baseline

This source slice makes Hook input updates a new authorization material:

- updated arguments receive a new `RequestId` and new args/scope digests;
- old execution scope, cell, capability grant and budget lease bindings are cleared;
- the material records approval invalidation, recursion depth and `broker_not_called=true`;
- recursion beyond the bounded depth is rejected; callers must run the returned request back
  through the normal identity/scope/policy/gate/approval/Hook/Broker spine.

The source guard is `kiana-core/tests/ext15_hook_reauthorization_guard.rs`. Local tests are
intentionally not run; GitHub Actions is the validation surface.
