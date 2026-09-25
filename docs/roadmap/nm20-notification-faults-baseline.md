# NM-20 notification fault, capacity and security matrix baseline

> Snapshot date: 2026-09-26. Tests are GitHub Actions-only; this step does not run local tests,
> build, check, clippy or smoke commands and does not wait for CI.

`NotificationFaultMatrix` is a deterministic replay-only classification over nine notification
failure scenarios: duplicate, out-of-order, cursor gap, crash-after-claim, slow consumer, queue
full, disk full, secret sentinel and projection loss. Each case binds source cursor/event IDs and
records a bounded disposition, critical-fact preservation, duplicate suppression, snapshot/reconcile
requirements, `effect_started=false` and `secret_free=true` invariants.

The matrix does not inject a crash, fill a disk, drop a queue item, write EventLog, call a provider,
or send a notification. It is a safety contract to drive later EventLog/Projector/Worker/Channel/UI
fault tests; critical/terminal/approval facts are never silently downgraded to successful empty
state.

`feature_status=implemented`; `proof_level=source`.

Known limits: no real kill-9/power-loss/disk-full/network/slow-browser/provider/connector test,
durable checkpoint, capacity benchmark or live/physical receipt evidence is claimed.
