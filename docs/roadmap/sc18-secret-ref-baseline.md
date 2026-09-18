# SC-18 SecretRef / SecretStore Baseline

## Scope

SC-18 closes the opaque secret-reference boundary. `SecretRef` is a versioned, digest-bound value
containing only store/key/purpose/audience/generation; `CredentialLease` binds the same reference
to provider account, purpose, audience, endpoint and a short one-shot expiry. Core/Runner/ports
carry only opaque references and safe status/digests. Provider-side SecretStore adapters resolve
material at the final transport boundary, consume it once, and drop it; configuration snapshots,
catalogs, receipts and event paths remain secret-free. Recursive redaction rejects sentinel/raw
secret values rather than returning the original data.

Unknown provider/account/purpose/endpoint or expired/replayed lease is denied. This slice does not
make a SecretRef transferable, does not expose SecretStore bytes to ControlPlane/EventLog, and does
not treat a credential digest as proof of an external provider outcome.

## Evidence and limits

- `kiana-domain/tests/ci07_credential_lease.rs` covers one-shot/expiry/binding drift, opaque JSON
  and unknown-field denial.
- `kiana-provider/tests/ci07_secret_store.rs` covers opaque config snapshots and missing env
  resolution fail-closed; `kiana-core/tests/ci07_secret_store.rs` pins the cross-layer boundary.
- `kiana-core/tests/sc18_secret_ref_guard.rs` pins SecretRef/lease/ports/provider/Broker/redaction
  markers and rejects raw secret/passthrough fields.
- GitHub Actions runs fixtures, source guard and workspace compile; local tests are intentionally
  not executed.

This slice is `feature_status=implemented`, `proof_level=source`: daemon lease rotation/revocation,
crash recovery, full secret egress audit and external/live/physical proof remain SC-19+ / OA / ER
work.
