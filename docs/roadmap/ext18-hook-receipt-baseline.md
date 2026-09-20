# EXT-18 Hook Receipt Baseline

This source slice defines receipt-only Hook replay and recovery:

- `HookReceipt` retains snapshot/input/output digests, decision, approval ref, timeout/cancel,
  patch before/after digests and process cleanup state;
- replay returns a read-only `HookReplayView` with `executed=false` and never starts a process,
  reads a script or touches an external resource;
- cleanup Unknown or outcome Unknown fences the receipt and requires a ControlPlane decision;
- replay-safe receipts are rejected by this conservative recovery helper until an explicit replay
  contract is supplied.

The source guard is `kiana-core/tests/ext18_hook_receipt_guard.rs`. Local tests are intentionally
not run; GitHub Actions is the validation surface.
