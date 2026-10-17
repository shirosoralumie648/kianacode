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

## 2026-10-17 completion slice (CI-only)

The 2026-09-25 slice implemented the provider/account boundary; this slice closes the card's
acceptance list (proactive refresh, CAS rotate, reauth/revoked fences and the in-lock re-read) and
makes the manual lane the authority for all five crates.

- `kiana-provider/src/oauth.rs`: proactive refresh is explicitly best effort. A still-valid access
  token without refresh material is returned until it actually expires instead of failing early
  with `oauth_reauth_required`; only an expired credential is `oauth_token_expired`. The leader
  registers its waiter before the state lock, claims the single-flight slot under the lock, and
  `finish_refresh` re-reads `state.tokens` after reacquiring the lock before comparing
  `current.metadata.generation != generation` in every outcome branch (success, transient,
  permanent, revoked).
- Fixtures added: `proactive_refresh_starts_before_expiry_and_keeps_a_still_valid_token` (cached
  outside the skew window, one leader exchange inside it, transient cooldown, valid-without-refresh
  and expired-without-refresh paths) and
  `refresh_re_reads_the_current_generation_under_the_lock_before_falling_back` (a replacement
  grant lands while the old refresh is in flight; the stale transient outcome becomes
  `oauth_generation_conflict` instead of falling back to its pre-lock snapshot, and the persisted
  token file keeps the replacement bytes). Existing provider fixtures remain:
  `pkce_state_and_redirect_are_single_use_and_fail_closed`,
  `callback_origin_and_authorization_code_replay_are_fenced`,
  `concurrent_refresh_is_single_flight_and_transient_keeps_valid_token`,
  `generation_cas_and_permanent_reauth_fence_old_refresh`,
  `rotation_wins_over_an_in_flight_old_refresh` and
  `invalidate_if_current_is_generation_cas_and_denies_stale_callers`.
- `kiana-domain/src/oauth_accounts.rs`: `require_reauth` now refuses a `Revoked` record with
  `oauth_account_status_fenced`, so revocation cannot be downgraded to the softer reauth state.
  Fixture `binding_drift_and_expired_status_fail_closed` covers SecretRef purpose/audience drift,
  provider binding drift, generation mismatch, redirect query/fragment/userinfo/scheme drift,
  digest tampering, expired projections and stale revoke/reauth fences.
- `kiana-daemon/tests/int12_oauth_store.rs`: `store_is_insert_or_idempotent_and_refuses_non_cas_replacement`
  (missing-record insert with an expected revision, idempotent replay, non-CAS replacement and
  revision conflict) and `stale_lifecycle_fences_never_revive_an_old_generation` (stale
  rotate/revoke after reauth, fenced rotate on a reauth record, revoke, and reauth refusal after
  revocation).
- `kiana-query/tests/int12_oauth_projection.rs`:
  `query_projection_rejects_conflicting_snapshots_and_keeps_expired_rows_redacted` rejects two
  digests for one account ID, emits `Expired` for an elapsed row and never encodes token material
  or SecretRef keys.
- `kiana-core/tests/int12_oauth_guard.rs` keeps the secret-boundary/CAS markers and adds
  `int12_refresh_single_flight_re_reads_the_generation_under_the_lock`,
  `int12_boundaries_do_not_create_a_second_authority_surface` and
  `int12_fixtures_are_wired_into_the_manual_lane_and_the_auto_shards`.
- `.github/workflows/int12-oauth-account.yml` now fetches locked dependencies, gates formatting,
  runs the provider oauth fixtures plus the domain/daemon/query/core targets and compiles each
  target with `--no-run` (manual `workflow_dispatch` only, `contents: read`). Automatic coverage
  stays with `ci.yml`: `kiana-domain-s3/4` (`int12_oauth_account`), `kiana-core-s4/6`
  (`int12_oauth_guard`) and the whole-crate `kiana-provider`/`kiana-daemon`/`kiana-query` shards.

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
