# SC-19 Secret Lease Rotation / Revocation Baseline

## Scope

SC-19 closes generation and lease fencing above SC-18. Credential leases are short-lived and
one-shot; provider account/purpose/audience/endpoint/credential-revision drift, expiry and replay
fail before header injection. `CredentialRotationPort` exposes only opaque references and observed
generation, while OAuth metadata rotates with compare-and-swap generation, rejects stale refreshes,
advances generation on revoke and persists only digest/status metadata. The provider refresh manager
uses single-flight and refresh cooldown state so an old in-flight refresh cannot reinstall a rotated
or revoked token. Protected daemon ingress keeps credential refs opaque.

The raw access/refresh token remains provider-side material and is never part of domain metadata,
EventLog, configuration snapshots or the rotation receipt. Unsupported storage backends fail closed.

## Evidence and limits

- `kiana-domain/tests/ci09_oauth_contracts.rs` covers OAuth state/PKCE and metadata generation CAS,
  expiry/status/revoke and secret-free serialization.
- `kiana-core/tests/ci09_oauth_guard.rs` and `kiana-core/tests/sc19_secret_rotation_guard.rs` pin
  provider/ports/lease/daemon generation and no-raw-token boundaries.
- GitHub Actions runs OAuth fixtures, source guard and workspace compile; local tests are
  intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: daemon-wide durable secret
revocation propagation, crash/restore key hygiene, HSM/OS keyring guarantees and external/live/
physical proof remain SC-20+ / ER work.
