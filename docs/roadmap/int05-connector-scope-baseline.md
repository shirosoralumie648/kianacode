# INT-05 Connector scope baseline

## Scope

INT-05 binds Connector accounts to owner, project, data class, revision, expiry and data epoch.
Requested scopes can only narrow an active binding; cross-owner/project/epoch and revoked/expired
bindings fail closed.

## Implemented source slice

- `ConnectorScopeBinding` carries owner/project/data epoch/revision/status and read/write/data-class
  scope sets with a stable digest.
- `intersect` rejects scope widening, owner/project/epoch mismatch, expired/revoked status and
  later-expiry or stale-revision child claims.
- The contract remains server-side metadata; no credential read, provider call or adapter effect is
  introduced.

## Evidence boundary

GitHub Actions is the test authority for the domain fixture and core source guard. Local tests are
intentionally not run. This step does not claim account persistence, credential leases, live OAuth
or external provider effects.
