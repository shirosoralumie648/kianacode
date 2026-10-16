# BQ-16 · Provider capacity, RPM/TPM and bounded fair admission baseline

This source slice makes provider capacity an explicit, deny first boundary after ControlPlane
admission and before network dispatch. It binds every request to one canonical UTC quota window
and `QuotaGroupKey` (provider, credential, model and alias), and keeps queueing and backoff out of
the active provider slot.

## Source contract

- `ProviderCapacityController` returns typed `Accept`, `Queue`, `Delay` and `Reject` outcomes.
- RPM and TPM are checked against the immutable policy and UTC window. A quota window failure is
  a `Delay` with a bounded `retry_after_ms`; it never consumes an active semaphore slot.
- The queue is bounded and session-fair. Cancellation removes a queued attempt before dispatch;
  no cancelled entry is handed to transport. A full queue returns structured `Reject`.
- `ProviderCapacityLease` binds owner, attempt, group, window and policy digests. Release checks
  the exact owner, so a different session cannot return another session's permit.
- Queued entries retain the request's absolute deadline in their own digest. Dispatch rejects an
  entry whose deadline has elapsed with `provider_capacity_queue_wait_expired`; queue wait never
  extends the request deadline or creates a lease for expired work.
- Profile aliases share provider/origin/credential capacity policy. Changing alias, credential or
  model therefore cannot create a second quota group without an explicit server policy.
- Provider transport keeps a bounded waiter semaphore and releases the waiter slot before taking
  the active slot. Its shared `CapacityWindow` reserves RPM/TPM in the fixed UTC minute only after
  an active slot is available, so delayed/backpressured work does not consume quota. It only sends
  through the existing `ProviderGateway`/ControlPlane path.

## CI-only fixtures

`bq16-provider-capacity.yml` runs the domain outcome/queue/owner fixtures, provider transport
source fixture, and Core/Daemon source guards on GitHub Actions. Local test, build, check, clippy
and smoke commands are intentionally not run.

## Evidence ceiling and limitations

The slice is `feature_status=implemented` with `proof_level=source` and remote CI wiring. The
capacity controller is an in-process admission contract; a durable cross-process RPM/TPM CAS
store, provider-side capacity receipts, live external requests, and physical proof remain open.
ControlPlane remains the only authorization authority and this slice adds no provider execution
loop or fallback executor.

## 2026-10-08 local deadline evidence

The focused regression first reproduced the bug: a queued request with absolute deadline `60_003`
was accepted when dispatched at `60_100` because dispatch rebuilt a fresh deadline from the current
time. The fix stores `deadline_unix_ms` in `ProviderCapacityQueueEntry`, includes it in the entry
digest, and uses it for the typed expired rejection. The focused test, all four BQ-16 domain tests,
and all three P4-J7-25 capacity/fallback domain tests pass locally. This advances only the selected
boundary to `proof_level=local_behavior`; the controller remains process-local and the step remains
🔄.
