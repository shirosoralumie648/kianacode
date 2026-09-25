# NM-15 Web notification snapshot/page and SSE baseline

> Snapshot date: 2026-09-26. Tests are GitHub Actions-only; this step does not run local tests,
> build, check, clippy or smoke commands and does not wait for CI.

`/api/notifications` is a loopback Web page adapter over the authenticated daemon principal's
committed `NotificationStore` projection. It requires the existing Host/Origin/web-token and
session/tab fences, bounds the page to 30 items and 16 action labels per item, and removes detail,
source payload and action arguments before returning JSON.

`/api/notifications/events` reuses the existing `RunStreamFeedSubscription`. It emits a snapshot
boundary first, accepts a versioned `UiFeedCursorV1` through `Last-Event-ID` or the query fallback,
redacts feed payloads, closes on gap/unknown for snapshot hydration, and preserves terminal as a
non-retryable display state. It never submits an action, retries a run, or creates a second bus.

`feature_status=implemented`; `proof_level=source`.

Known limits: notification read/ACK projection state is rebuilt for this slice and is not claimed as
durable; SSE cursor/subscription state is process-local; no browser/PTY, cross-process, external
connector, provider/live timing or physical delivery receipt evidence is claimed.
