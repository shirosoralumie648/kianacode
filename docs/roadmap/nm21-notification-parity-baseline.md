# NM-21 cross-entry notification/action/result parity baseline

> Snapshot date: 2026-09-26. Tests are GitHub Actions-only; this step does not run local tests,
> build, check, clippy or smoke commands and does not wait for CI.

`NotificationEntrypointSnapshot` and `NotificationEntrypointParity` compare the four surfaces
CLI/TTY, Web, Desktop and Workbench against one server-owned source cursor and authority epoch.
The comparator requires equal notification, action, terminal-result, pending and delivery-attempt
ID sets and records whether each view was rebuilt in a fresh process. Any drift or missing rebuild is
`Unknown` with `query_original_source_and_rebuild_snapshot`, never a completed run or successful
action.

The contract compares opaque IDs/digests and bounded state only; message text cannot change run
state. It does not create a bus, runner loop, action, retry, receipt or connector effect.

`feature_status=implemented`; `proof_level=source`.

Known limits: no four-process/browser/Electron/PTY E2E, durable cross-process inbox store, actual
action CAS, provider/model fake run, or live/physical receipt evidence is claimed.
