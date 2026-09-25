# NM-16 Desktop local notification adapter baseline

> Snapshot date: 2026-09-26. Electron/Node and Rust verification run in GitHub Actions only;
> this step does not run local tests, build, check, clippy or smoke commands and does not wait for CI.

The Electron main process now uses an explicit `DesktopNotificationAdapter` around the existing
server-fact `NotificationBridge`. Permission is one of `granted`, `denied`, `unknown` or
`unavailable`; only `granted` may construct/show an OS notification. Denied, unknown, unsupported,
or constructor failure remains an explicit non-delivery disposition, while the server fact still
updates the bounded desktop attention projection.

OS copy remains fixed and redacted: title/body point back to the workbench and never include the
notification payload, path, token, secret or run identity. Workspace binding and replay fences are
preserved. Tray actions continue to be typed workspace intents, and close/detach/reattach reuse the
existing no-implicit-cancel/resume policy and metadata-only persistence.

`feature_status=implemented`; `proof_level=source`.

Known limits: Electron/OS permission prompts, actual toast display, tray accessibility, reopen and
cross-process cursor recovery are not physically exercised; durable inbox/read state and external or
live delivery receipts remain server-side/later proof. A local `shown` disposition is not a physical
delivery receipt.
