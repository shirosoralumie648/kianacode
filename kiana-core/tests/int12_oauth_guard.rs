#[test]
fn int12_account_store_keeps_provider_secret_boundary_and_cas() {
    let domain = include_str!("../../kiana-domain/src/oauth_accounts.rs");
    let oauth_domain = include_str!("../../kiana-domain/src/oauth.rs");
    let ports = include_str!("../../kiana-ports/src/oauth_accounts.rs");
    let daemon = include_str!("../../kiana-daemon/src/oauth_accounts.rs");
    let query = include_str!("../../kiana-query/src/oauth_accounts.rs");
    let provider = include_str!("../../kiana-provider/src/oauth.rs");
    let provider_lib = include_str!("../../kiana-provider/src/lib.rs");

    for marker in [
        "OAuthAccountRecord",
        "OAuthAccountStatus",
        "secret_ref_digest",
        "redirect_uri_valid",
        "credential_generation",
        "oauth_account_generation_conflict",
        "oauth_account_status_fenced",
        "oauth_account_rotation_binding_mismatch",
        "oauth_account_secret_ref_binding_mismatch",
        "oauth_account_provider_binding_mismatch",
        "scope_downgrade",
        "require_reauth",
        "pub fn revoke",
        "pub fn require_reauth",
        "effective_status",
        "oauth_account_status_mismatch",
        "#[serde(deny_unknown_fields)]",
    ] {
        assert!(domain.contains(marker), "domain marker missing: {marker}");
    }
    for marker in [
        "pub trait OAuthAccountStore",
        "OAUTH_ACCOUNT_STORE_PORT_SCHEMA",
        "read_oauth_account",
        "upsert_oauth_account",
        "rotate_oauth_account",
        "require_oauth_reauth",
        "revoke_oauth_account",
        "generation",
        "never returns access/refresh token material",
    ] {
        assert!(ports.contains(marker), "port marker missing: {marker}");
    }
    let daemon_flat: String = daemon.split_whitespace().collect();
    for marker in [
        "InMemoryOAuthAccountStore",
        "RwLock",
        "oauth_account_expected_missing",
        "oauth_account_revision_conflict",
        "oauth_account_mutation_requires_cas",
        "oauth_account_not_found",
        "current.rotate(",
        "current.require_reauth(",
        "current.revoke(",
    ] {
        assert!(
            daemon.contains(marker) || daemon_flat.contains(marker),
            "daemon marker missing: {marker}"
        );
    }
    for marker in [
        "OAuthAccountQueryPage",
        "project_oauth_accounts",
        "access/refresh token",
        "source_cursor",
        "oauth_account_projection_conflict",
        "source_cursor_is_adapter_supplied",
        "metadata_only_no_provider_request",
        "stale",
    ] {
        assert!(query.contains(marker), "query marker missing: {marker}");
    }
    for marker in [
        "canonical_redirect_uri",
        "oauth_redirect_origin_not_loopback",
        "oauth_state_mismatch",
        "oauth_redirect_mismatch",
        "oauth_flow_replay",
        "consumed_code_digests",
        "oauth_code_replay",
        "oauth_code_replay_window_full",
        "oauth_scope_insufficient",
        "OAUTH_REFRESH_SKEW_MS",
        "needs_refresh",
        "refreshing: bool",
        "refresh_notify",
        "RefreshFlightGuard",
        "finish_refresh",
        "GenerationConflict",
        "RefreshFailure::Transient",
        "RefreshFailure::Permanent",
        "RefreshFailure::Revoked",
        "OAuthSubject::Workload",
        "Proactive refresh is an optimization",
    ] {
        assert!(
            provider.contains(marker),
            "provider marker missing: {marker}"
        );
    }
    assert!(provider_lib.contains("mod oauth;"));
    assert!(oauth_domain.contains("OAuthTokenMetadata"));

    for source in [domain, ports, daemon, query] {
        for forbidden in [
            "access_token: String",
            "refresh_token: Option<String>",
            "Authorization: Bearer",
        ] {
            assert!(
                !source.contains(forbidden),
                "raw credential marker crossed account boundary: {forbidden}"
            );
        }
    }
    assert!(!query.contains("SecretRef {"));
    assert!(!query.contains("access_token"));
    assert!(!query.contains("refresh_token"));
}

#[test]
fn int12_refresh_single_flight_re_reads_the_generation_under_the_lock() {
    let provider = include_str!("../../kiana-provider/src/oauth.rs");
    let access = provider
        .split("pub(crate) async fn access_token<F, Fut>(")
        .nth(1)
        .expect("access_token source");

    // The waiter must be registered before the state lock, otherwise a leader notification can be
    // lost between the check and the await.
    let waiter = access
        .find("let notified = self.refresh_notify.notified();")
        .expect("waiter registration");
    let state_lock = access
        .find("let mut state = self.state.lock().unwrap();")
        .expect("state lock");
    let leader_claim = access
        .find("state.refreshing = true;")
        .expect("single-flight claim");
    assert!(waiter < state_lock, "waiter must be registered first");
    assert!(
        state_lock < leader_claim,
        "the single-flight claim must happen under the state lock"
    );
    assert!(
        access.contains("stored.metadata.needs_refresh(now_unix_ms, skew_ms)"),
        "proactive refresh must use the bounded skew window"
    );

    // The leader must re-read the current state under the lock before accepting or discarding its
    // out-of-lock refresh outcome for every terminal branch.
    let finish = provider
        .split("async fn finish_refresh(")
        .nth(1)
        .expect("finish_refresh source");
    let re_read = finish
        .find("let current = state.tokens.as_ref()")
        .expect("lock re-read");
    let compare = finish
        .find("current.metadata.generation != generation")
        .expect("generation comparison");
    assert!(
        re_read < compare,
        "the refresh outcome must be compared against the lock re-read"
    );
    assert_eq!(
        finish
            .matches("current.metadata.generation != generation")
            .count(),
        4,
        "every refresh outcome branch must re-check the generation"
    );
    assert!(provider.contains("impl Drop for RefreshFlightGuard"));
    assert!(provider.contains("flight.disarm();"));
    assert!(provider.contains("state.refresh_cooldown_until = 0;"));
}

#[test]
fn int12_boundaries_do_not_create_a_second_authority_surface() {
    let provider = include_str!("../../kiana-provider/src/oauth.rs");
    let daemon = include_str!("../../kiana-daemon/src/oauth_accounts.rs");
    let query = include_str!("../../kiana-query/src/oauth_accounts.rs");

    // The provider owns raw material and flows; it must not gain the account projection or any
    // authorization/event surface.
    for forbidden in [
        "OAuthAccountRecord",
        "CapabilityBroker",
        "ControlPlane",
        "EventStore",
    ] {
        assert!(
            !provider.contains(forbidden),
            "provider oauth gained authority marker: {forbidden}"
        );
    }
    // The local account store is a CAS metadata store, not an execution engine and not a network
    // client.
    for forbidden in [
        "CapabilityBroker",
        "ControlPlane",
        "EventStore",
        "reqwest",
        "TcpStream",
        "access_token",
        "refresh_token",
    ] {
        assert!(
            !daemon.contains(forbidden),
            "daemon account store gained forbidden marker: {forbidden}"
        );
    }
    // The query projection is read-only: it never reads the store or drives a lifecycle mutation.
    for forbidden in [
        "OAuthAccountStore",
        "rotate_oauth_account",
        "revoke_oauth_account",
        "require_oauth_reauth",
        "access_token",
        "refresh_token",
    ] {
        assert!(
            !query.contains(forbidden),
            "query projection gained lifecycle marker: {forbidden}"
        );
    }
}

#[test]
fn int12_fixtures_are_wired_into_the_manual_lane_and_the_auto_shards() {
    let lane = include_str!("../../.github/workflows/int12-oauth-account.yml");
    assert!(lane.contains("workflow_dispatch"));
    assert!(lane.contains("contents: read"));
    assert!(!lane.contains("pull_request"));
    assert!(!lane.contains("push:"));
    assert!(!lane.contains("continue-on-error"));
    assert!(!lane.contains("#[ignore]"));
    assert!(lane.contains("cargo fetch --locked"));
    assert!(lane.contains("cargo fmt --all --check"));
    for target in [
        "cargo test -p kiana-provider --lib oauth::tests",
        "cargo test -p kiana-domain --test int12_oauth_account",
        "cargo test -p kiana-daemon --test int12_oauth_store",
        "cargo test -p kiana-query --test int12_oauth_projection",
        "cargo test -p kiana-core --test int12_oauth_guard",
    ] {
        assert!(
            lane.contains(target),
            "manual lane missing target: {target}"
        );
    }
    assert_eq!(
        lane.matches("--no-run").count(),
        5,
        "every INT-12 target needs a compile-only step"
    );

    let shards = include_str!("../../scripts/ci/test-shards.json");
    for target in ["int12_oauth_guard", "int12_oauth_account"] {
        assert!(
            shards.contains(&format!("\"{target}\"")),
            "automatic shards missing {target}"
        );
    }
    for krate in ["kiana-provider", "kiana-daemon", "kiana-query"] {
        assert!(
            shards.contains(&format!("\"{krate}\"")),
            "automatic shards missing whole-crate shard {krate}"
        );
    }

    let baseline = include_str!("../../docs/roadmap/int12-oauth-account-baseline.md");
    for fixture in [
        "pkce_state_and_redirect_are_single_use_and_fail_closed",
        "callback_origin_and_authorization_code_replay_are_fenced",
        "concurrent_refresh_is_single_flight_and_transient_keeps_valid_token",
        "generation_cas_and_permanent_reauth_fence_old_refresh",
        "rotation_wins_over_an_in_flight_old_refresh",
        "invalidate_if_current_is_generation_cas_and_denies_stale_callers",
        "proactive_refresh_starts_before_expiry_and_keeps_a_still_valid_token",
        "refresh_re_reads_the_current_generation_under_the_lock_before_falling_back",
        "store_is_insert_or_idempotent_and_refuses_non_cas_replacement",
        "stale_lifecycle_fences_never_revive_an_old_generation",
        "binding_drift_and_expired_status_fail_closed",
        "query_projection_rejects_conflicting_snapshots_and_keeps_expired_rows_redacted",
    ] {
        assert!(
            baseline.contains(fixture),
            "baseline missing fixture: {fixture}"
        );
    }
}
