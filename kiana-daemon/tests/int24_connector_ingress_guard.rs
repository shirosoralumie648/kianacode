//! INT-24 source guard.
//!
//! The signed ingress slice is a *verified occurrence producer*. It must never grow into a second
//! execution path: no Broker/connector/adapter call, no EventLog append, no process spawn and no
//! harness loop may appear in the verifier, the durable replay adapter or the daemon route. The
//! route is allowed exactly one execution edge: `DaemonHost::handle`, the existing command path
//! into `ControlPlane`.
//!
//! Two tests:
//! - `connector_ingress_verifier_stops_at_authentication_and_dedupe`: markers and forbidden
//!   effect edges across all four modules.
//! - `connector_ingress_route_only_reaches_control_plane_through_daemon_handle`: the route's
//!   single execution edge and its refusal ordering are present, and duplicate/denial outcomes
//!   cannot reach a broker.

const DOMAIN: &str = include_str!("../../kiana-domain/src/connector_ingress.rs");
const VERIFIER: &str = include_str!("../src/connector_ingress.rs");
const REPLAY: &str = include_str!("../src/connector_ingress_replay.rs");
const ROUTE: &str = include_str!("../src/connector_ingress_route.rs");

#[test]
fn connector_ingress_verifier_stops_at_authentication_and_dedupe() {
    for marker in [
        "ConnectorIngressProtocol",
        "tenant_id",
        "nonce",
        "signature_digest",
        "connector_ingress_dedupe_payload_conflict",
        "ConnectorIngressVerifier",
        "hmac::verify",
    ] {
        assert!(
            DOMAIN.contains(marker) || VERIFIER.contains(marker),
            "INT-24 marker missing: {marker}"
        );
    }
    // The durable replay adapter and the route are part of the guard surface; the guard was
    // extended rather than narrowed when those modules were added.
    for marker in [
        "ConnectorIngressReplayStore",
        "admit_loaded",
        "connector_ingress_replay_capacity_exceeded",
        "ReplayFileLock",
        "connector_ingress_replay_store_invalid",
    ] {
        assert!(
            REPLAY.contains(marker),
            "INT-24 replay marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "ConnectorAdapter::invoke",
        "auto_success",
        "append_idempotent",
        "append_expected",
        "broker.register",
    ] {
        for (module, source) in [
            ("kiana-domain/src/connector_ingress.rs", DOMAIN),
            ("kiana-daemon/src/connector_ingress.rs", VERIFIER),
            ("kiana-daemon/src/connector_ingress_replay.rs", REPLAY),
            ("kiana-daemon/src/connector_ingress_route.rs", ROUTE),
        ] {
            assert!(
                !source.contains(forbidden),
                "INT-24 ingress widened effect boundary: {forbidden} in {module}"
            );
        }
    }
}

#[test]
fn connector_ingress_route_only_reaches_control_plane_through_daemon_handle() {
    // Exactly one execution edge: the existing daemon command path. Any direct adapter/broker
    // call would bypass `ControlPlane` and the policy/gate/approval chain.
    assert!(
        ROUTE.contains("self.handle(envelope)"),
        "the route must submit the derived intent through DaemonHost::handle"
    );
    assert!(
        ROUTE.contains("ConnectorIngressIntent::from_verified_occurrence"),
        "the connector account must be server-resolved, not wire-selected"
    );
    assert!(
        ROUTE.contains("connector_ingress_source_route_mismatch")
            && ROUTE.contains("connector_ingress_payload_digest_mismatch")
            && ROUTE.contains("project_untrusted"),
        "the route must keep its deny-first refusals"
    );
    assert!(
        ROUTE.contains("verify_durable"),
        "the production route must admit through the durable replay store"
    );
    assert!(
        ROUTE.contains("broker_calls: 0"),
        "a duplicate or denied route result must stay zero-broker-call"
    );
    for forbidden in [
        "CapabilityBroker",
        "CapabilityHandler",
        "register_static",
        "KianaHarness",
        "ScriptedModel",
        "std::process::Command",
        "tokio::spawn",
        "EventStorePort",
        "connector.dispatch.lifecycle",
    ] {
        assert!(
            !ROUTE.contains(forbidden),
            "INT-24 route gained a second execution edge: {forbidden}"
        );
    }
}
