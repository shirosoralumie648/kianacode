# INT-12 OAuth PKCE / account lifecycle baseline

## Scope

INT-12 composes the CI-09 provider owned OAuth manager with the connector account boundary. Authorization
requests use S256 PKCE, one time state and exact callback origin/redirect binding; callbacks reject state,
redirect and code replay before token exchange. The provider retains raw access and refresh material in
its protected SecretStore adapter only. Domain, account store, and query projections carry provider/account
identity, scopes, generation, expiry, status and digests only.

## Implemented source slice

- `kiana-provider/src/oauth.rs` validates callback redirects as HTTPS or loopback HTTP, checks the exact
  origin and path, consumes pending state once, records bounded authorization-code digests to reject cross-flow reuse, and keeps code/PKCE exchange inside the provider boundary.
  Existing CI-09 strict response parsing rejects unknown/oversized responses, bad bearer metadata and
  scope downgrade. Refresh uses one in-process single-flight with waiter registration before the state lock and re-reads current metadata before the
  generation CAS; transient failures retain a still-valid token, permanent failures fence to reauth and
  provider revocation fences to revoked.
- `kiana-domain/src/oauth_accounts.rs` adds a strict `OAuthAccountRecord` with provider account identity,
  callback URI, scope set, opaque `SecretRef` digest, credential generation, token metadata and account
  revision. Scope downgrade, provider/subject/redirect drift, stale revision/generation, reauth and
  revoked records fail closed. Reauth and revoke advance the generation fence. `OAuthAccountProjection`
  intentionally excludes SecretRef keys and token material.
- `kiana-ports/src/oauth_accounts.rs` defines the server-owned account store CAS port. It returns metadata
  records only and exposes explicit rotate, reauth and revoke generation fences.
- `kiana-daemon/src/oauth_accounts.rs` provides an in-process store for local composition and CI fixtures;
  it is not a second execution loop or authorization engine. `kiana-query` emits a deterministic,
  redaction-safe account page with source cursor and limitations.
- Domain, provider, daemon, query and core source fixtures cover state/redirect mismatch, one-time flow,
  scope downgrade, stale generation, reauth/revoked fencing, projection redaction, CAS replay and
  unknown fields. `.github/workflows/int12-oauth-account.yml` is the test authority.

## Evidence boundary

```text
feature_status: implemented
proof_level: source
```

Local Cargo tests, builds, checks, clippy and smoke commands are intentionally not run. GitHub Actions
CI is triggered by the commit and is not awaited. The provider exchange remains closure-backed and no
real IdP/token endpoint, browser callback listener, durable cross-process account database, keyring/HSM,
external connector effect, or live/physical proof is claimed. Account projections and EventLog-facing
contracts contain only opaque references/digests; raw token handling remains provider/SecretStore-only.
