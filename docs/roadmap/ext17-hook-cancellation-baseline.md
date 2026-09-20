# EXT-17 Hook Cancellation Baseline

This source slice defines Hook cancellation and recursion boundaries:

- invocation identity binds run/hook/invocation IDs and a stable idempotency key;
- parent invocation state carries bounded recursion depth and a visited set, rejecting cycles;
- cancellation distinguishes confirmed Cancelled from Unknown, and only observer Unknown may be
  retried; guard retries remain disallowed;
- the contract is read-only metadata and leaves process stop confirmation to the supervisor.

The source guard is `kiana-core/tests/ext17_hook_cancellation_guard.rs`. Local tests are
intentionally not run; GitHub Actions is the validation surface.
