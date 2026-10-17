//! INT-18 source and wiring guard: the effect-time connector permit must stay on the single
//! ControlPlane → Broker → daemon spine, deny before any adapter effect, and keep its fixtures
//! wired to CI.  `include_str!` assertions are source-level evidence only; they are not runtime
//! receipts.

const DOMAIN_SOURCE: &str = include_str!("../../kiana-domain/src/connector_effect_permit.rs");
const RESERVATION_SOURCE: &str = include_str!("../../kiana-domain/src/connector_reservation.rs");
const CORE_SOURCE: &str = include_str!("../src/connector_reservation.rs");
const BROKER_SOURCE: &str = include_str!("../../kiana-capability-broker/src/lib.rs");
const DAEMON_SOURCE: &str = include_str!("../../kiana-daemon/src/connectors.rs");
const DOMAIN_FIXTURES: &str = include_str!("../../kiana-domain/tests/int18_effect_permit.rs");
const BROKER_FIXTURES: &str =
    include_str!("../../kiana-capability-broker/tests/int18_effect_permit.rs");
const DAEMON_FIXTURES: &str = include_str!("../../kiana-daemon/tests/int18_effect_permit.rs");
const MANUAL_LANE: &str = include_str!("../../.github/workflows/int18-effect-permit.yml");
const SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");
const BASELINE: &str = include_str!("../../docs/roadmap/int18-effect-permit-baseline.md");

#[test]
fn effect_permit_fence_stays_on_the_control_plane_broker_daemon_spine() {
    for marker in [
        "ConnectorEffectFence",
        "ConnectorEffectPermit",
        "connector_effect_scope_digest",
        "connector_effect_fence_invalid",
        "connector_effect_fence_command_mismatch",
        "connector_effect_scope_epoch_mismatch",
        "connector_effect_authority_epoch_stale",
        "connector_effect_configuration_epoch_stale",
        "connector_effect_policy_epoch_stale",
        "connector_effect_credential_epoch_stale",
        "connector_effect_data_epoch_stale",
        "connector_effect_permit_expired",
        "connector_effect_permit_reservation_mismatch",
        "connector_effect_binding_revoked",
        "connector_effect_binding_revision_stale",
        "connector_effect_binding_snapshot_stale",
        "connector_effect_scope_digest_mismatch",
        "scope_digest",
        "permit_digest",
        "fence_digest",
        "expires_at_unix_ms",
        "validate_for_effect",
        "issue_effect_permit",
        "admit_effect_permit",
        "ConnectorEffectPermit::issue",
        "CONNECTOR_EFFECT_PERMIT_SCHEMA",
        "CONNECTOR_EFFECT_FENCE_SCHEMA",
        "validate_connector_effect_boundary",
        "connector_effect_permit_required",
        "connector_effect_permit_invalid",
        "connector_effect_scope_required",
        "connector_configuration_epoch_required",
        "connector_policy_epoch_required",
        "connector_binding_snapshot_required",
    ] {
        assert!(
            DOMAIN_SOURCE.contains(marker)
                || RESERVATION_SOURCE.contains(marker)
                || CORE_SOURCE.contains(marker)
                || BROKER_SOURCE.contains(marker)
                || DAEMON_SOURCE.contains(marker),
            "INT-18 marker missing: {marker}"
        );
    }

    // The domain stays a pure value model: no Broker, no EventStore, no runtime spawn.
    assert!(!DOMAIN_SOURCE.contains("CapabilityBroker"));
    assert!(!DOMAIN_SOURCE.contains("EventStorePort"));
    assert!(!DOMAIN_SOURCE.contains("tokio::spawn"));
    // Core owns the durable reservation commit, not the adapter or a second permit store.
    assert!(CORE_SOURCE.contains("ConnectorEffectPermit::issue"));
    assert!(CORE_SOURCE.contains("validate_for_effect"));
    assert!(!CORE_SOURCE.contains("tokio::spawn"));
    assert!(!CORE_SOURCE.contains("CapabilityBroker"));
    // The Broker revalidates the effect envelope but cannot write events or spawn transports.
    assert!(BROKER_SOURCE.contains("validate_connector_effect_boundary(&request"));
    assert!(!BROKER_SOURCE.contains("EventStore::append"));
    assert!(!BROKER_SOURCE.contains("tokio::spawn(connector"));
    // The daemon adapter stays local: no network transport and no provider gateway.
    assert!(!DAEMON_SOURCE.contains("reqwest::Client"));
    assert!(!DAEMON_SOURCE.contains("ProviderGateway"));
}

#[test]
fn stale_or_rotated_effect_material_is_denied_before_effect() {
    for marker in [
        "connector_effect_permit_reservation_not_committed",
        "connector_effect_permit_expiry_invalid",
        "connector_effect_credential_epoch_required",
        "connector_invocation_command_invalid",
        "connector_reservation_binding_revision_mismatch",
        "reservation.state != crate::ConnectorReservationState::Committed",
        "current.validate()",
        "binding.status != \"active\"",
        "binding.revision != self.binding_revision",
        "require_credential_epoch",
    ] {
        assert!(
            DOMAIN_SOURCE.contains(marker) || RESERVATION_SOURCE.contains(marker),
            "INT-18 domain deny marker missing: {marker}"
        );
    }

    // The Broker checks the effect fence before the one-shot execution permit is consumed and
    // before the handler runs; the daemon repeats it before the fixture is read.
    let fence = BROKER_SOURCE
        .find("validate_connector_effect_boundary(&request, connector_now_unix_ms()?)?;")
        .expect("broker effect fence");
    let verifier = BROKER_SOURCE
        .find("verify_and_consume(&request)")
        .expect("execution permit");
    assert!(
        fence < verifier,
        "the effect fence must precede the one-shot execution permit"
    );
    let daemon_fence = DAEMON_SOURCE
        .find("validate_for_effect(&reservation, &snapshot, &current, now)")
        .expect("daemon effect fence");
    let fixture = DAEMON_SOURCE
        .rfind("let fixture = load_fixture")
        .expect("fixture adapter");
    assert!(
        daemon_fence < fixture,
        "the effect fence must precede the fixture adapter read"
    );
    assert!(DAEMON_SOURCE.contains("connector_effect_permit_required"));
    assert!(DAEMON_SOURCE.contains("connector_effect_permit_invalid"));
    assert!(DAEMON_SOURCE.contains("connector_effect_scope_required"));
    assert!(DAEMON_SOURCE.contains("connector_configuration_epoch_required"));
    assert!(DAEMON_SOURCE.contains("connector_policy_epoch_required"));
}

#[test]
fn effect_permit_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shards() {
    for fixture in [
        // Domain fence fixtures: deny-first, success last.
        "effect_permit_binds_scope_digests_epochs_and_lease_expiry",
        "stale_authority_configuration_policy_credential_and_data_epochs_never_reach_the_adapter",
        "revoked_rotated_or_reconfigured_bindings_and_old_snapshots_are_refused",
        "permit_cannot_be_minted_from_uncommitted_or_mismatched_snapshot",
        "tampered_or_replayed_effect_material_is_refused_before_any_effect",
        "fence_and_permit_digests_cover_scope_digests_and_every_revocable_epoch",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(fixture),
            "INT-18 domain fixture missing: {fixture}"
        );
    }
    for fixture in [
        // Broker effect boundary: recording handler and verifier stay at zero calls.
        "current_effect_permit_reaches_the_handler_exactly_once",
        "stale_revoked_rotated_and_expired_effect_material_never_reaches_the_handler",
    ] {
        assert!(
            BROKER_FIXTURES.contains(fixture),
            "INT-18 Broker fixture missing: {fixture}"
        );
    }
    for fixture in [
        // Daemon spine: wire smuggling refused, no permit material persisted or echoed.
        "wire_supplied_effect_permit_is_refused_before_any_adapter_effect",
        "effect_permit_and_fence_material_is_never_persisted_or_echoed",
    ] {
        assert!(
            DAEMON_FIXTURES.contains(fixture),
            "INT-18 daemon fixture missing: {fixture}"
        );
    }
    for fixtures in [DOMAIN_FIXTURES, BROKER_FIXTURES, DAEMON_FIXTURES] {
        assert!(!fixtures.contains("#[ignore"));
        assert!(!fixtures.contains("ignore ="));
    }

    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test int18_effect_permit --locked -- --test-threads=1",
        "cargo test -p kiana-capability-broker --test int18_effect_permit --locked -- --test-threads=1",
        "cargo test -p kiana-daemon --test int18_effect_permit --locked -- --test-threads=1",
        "cargo test -p kiana-core --test int18_effect_permit_guard --locked -- --test-threads=1",
    ] {
        assert!(
            MANUAL_LANE.contains(step),
            "missing INT-18 lane step: {step}"
        );
    }
    assert_eq!(MANUAL_LANE.matches("--no-run").count(), 4);
    assert!(MANUAL_LANE.contains("workflow_dispatch:"));
    assert!(MANUAL_LANE.contains("permissions:\n  contents: read"));
    for forbidden in [
        "push:",
        "pull_request:",
        "schedule:",
        "continue-on-error",
        "|| true",
    ] {
        assert!(
            !MANUAL_LANE.contains(forbidden),
            "INT-18 lane must not contain {forbidden}"
        );
    }

    // Automatic coverage: the domain/core targets stay in their registered chunked shards and the
    // new Broker/daemon fixtures are picked up by their whole-crate shards.
    let shards: serde_json::Value = serde_json::from_str(SHARDS).expect("test shards parse");
    let shard_map = shards.as_object().expect("shard map");
    let owner = |target: &str| -> Option<String> {
        shard_map.iter().find_map(|(name, entry)| {
            let targets = entry["targets"].as_array()?;
            targets
                .iter()
                .any(|value| value.as_str() == Some(target))
                .then(|| name.clone())
        })
    };
    assert_eq!(
        owner("int18_effect_permit").as_deref(),
        Some("kiana-domain-s3/4"),
        "the domain fixture must stay in its registered shard"
    );
    assert_eq!(
        owner("int18_effect_permit_guard").as_deref(),
        Some("kiana-core-s4/6"),
        "the core guard must stay in its registered shard"
    );
    assert!(shards["kiana-capability-broker"]["targets"].is_null());
    assert!(shards["kiana-daemon"]["targets"].is_null());

    assert!(BASELINE.contains("proof_level: source"));
    assert!(BASELINE.contains("connector_effect_credential_epoch_required"));
}
