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

`kiana-daemon` now owns a `SecretLeaseAuthority` that holds only per-slot generation/status/revision
metadata and issues opaque, one-shot, TTL-bound `CredentialLease`s. `consume_lease` re-checks the
server-side status and generation immediately before injection, `rotate` advances the slot with
compare-and-swap on the observed generation and rejects rollback, and `revoke` fences the slot so
every later issue/consume/rotate is denied, each returning a secret-free receipt. `export_journal` /
`restore_from_journal` rebuild slot state from a closed, secret-free journal; a restore payload that
carries a raw value is rejected before any state is rebuilt, so a crash/restore does not copy
plaintext.

## Evidence and limits

- `kiana-daemon/tests/sc19_secret_lease.rs` covers the daemon lease authority end to end: expired
  lease denied before injection, one-shot replay denied, revoked slot denies further
  issue/consume/rotate, rotation receipt generation CAS with stale/rollback denial,
  secret-free leases/receipts, and a plaintext-free journal round trip that rejects an injected
  secret field and a generation rollback.
- `kiana-domain/tests/ci09_oauth_contracts.rs` covers OAuth state/PKCE and metadata generation CAS,
  expiry/status/revoke and secret-free serialization.
- `kiana-core/tests/ci09_oauth_guard.rs` and `kiana-core/tests/sc19_secret_rotation_guard.rs` pin
  provider/ports/lease/daemon generation and no-raw-token boundaries, including the daemon secret
  lease markers and fixture names.
- GitHub Actions runs OAuth fixtures, the source guard, the daemon lease fixture and their
  `--no-run` compiles; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: the authority is an in-process
composition fixture, so durable, cross-process revocation propagation, crash/restore key hygiene
beyond the closed journal schema (HSM/OS keyring guarantees) and external/live/physical proof remain
SC-20+ / ER work.
