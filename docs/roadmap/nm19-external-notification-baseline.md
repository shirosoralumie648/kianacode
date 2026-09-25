# NM-19 external notification/webhook contract baseline

> Snapshot date: 2026-09-26. Tests are GitHub Actions-only; this step does not run local tests,
> build, check, clippy or smoke commands and does not wait for CI.

NM-19 adds a default-off, source-only external notification contract. `ExternalNotificationPolicy`
uses exact canonical HTTPS origins, a server-owned signing-key reference, authority/data epochs and
an explicit enable bit. `ExternalNotificationEnvelope` binds transport, recipient, delivery ID,
nonce, expiry, payload/signature digests and source event IDs. The core gate checks policy,
allowlist, nonce, epochs and expiry but returns only `NotSupported` or an explicit
`ReadyForExplicitConnector` handoff with `direct_effect=false`.

Provider receipts are observation DTOs only. A binding-matched `Acknowledged` receipt is observed;
`Unknown` becomes `ReconcileRequired`; drift or invalid receipt is rejected. No HTTP/A2A client,
socket, secret signer, connector dispatch or receipt application was added, and the default policy
remains disabled.

`feature_status=implemented`; `proof_level=source`.

Known limits: no network transport, provider signature verification, nonce store, durable receipt or
live/physical external delivery evidence is claimed. External connector enablement remains a later
explicitly approved scope.
