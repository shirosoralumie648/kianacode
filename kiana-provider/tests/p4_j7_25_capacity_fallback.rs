//! P4-J7-25 provider boundary fixtures.
//!
//! The bounded waiter pool, the RPM/TPM window and the circuit breaker live inside the provider
//! transport, above the ControlPlane permit and below the Runner's retry driver.  These fixtures
//! pin the two things the provider owns here:
//!
//! - a retry backoff never holds a capacity slot (the only network exit contains no sleep), and
//!   aliases that resolve to the same provider/origin/credential scope share one waiter pool;
//! - a fallback admission that drifted is refused before the route can change, and the provider
//!   never chooses a fallback itself.
//!
//! Nothing here opens a socket, mints a permit or selects a route.

use kiana_domain::{
    AttemptId, FallbackAttemptAdmission, ModelProtocol, ModelRoute, RateCardId, RunId,
    SchemaVersion,
};
use kiana_provider::validate_fallback_attempt;

fn digest(seed: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (seed as u32) & 0xffff).repeat(16)
    )
}

fn admission() -> FallbackAttemptAdmission {
    let route = ModelRoute {
        provider_id: "provider-b".to_owned(),
        protocol: ModelProtocol::OpenAiChat,
        connection_id: "backup".to_owned(),
        model_id: "model-b".to_owned(),
        profile: "backup".to_owned(),
        configuration_revision: digest('r'),
        streaming: true,
    };
    let mut admission = FallbackAttemptAdmission {
        schema: kiana_domain::FALLBACK_ADMISSION_SCHEMA.to_owned(),
        version: SchemaVersion::new(1, 0),
        run_id: RunId::new(),
        original_attempt_id: AttemptId::new(),
        attempt_id: AttemptId::new(),
        route: route.clone(),
        route_digest: route.digest(),
        authority_digest: digest('a'),
        authority_epoch: 1,
        data_boundary_digest: digest('b'),
        budget_digest: digest('c'),
        budget_reservation_digest: digest('d'),
        credential_revision: digest('e'),
        rate_card_id: RateCardId::new(),
        rate_card_version: 1,
        rate_card_digest: digest('f'),
        permit_digest: digest('g'),
        capability_digest: digest('h'),
        admitted_at_unix_ms: 1,
        admission_digest: String::new(),
    };
    admission.admission_digest = admission.digest();
    admission
}

#[test]
fn backoff_never_holds_a_capacity_slot_and_aliases_share_one_waiter_pool() {
    let transport = include_str!("../src/transport.rs");
    let config = include_str!("../src/config.rs");

    // The non-blocking waiter gate is taken before the network slot and returned as soon as the
    // network slot is acquired, so queued work cannot flood the pool while waiting.
    let waiter_gate = transport
        .find(".try_acquire_owned()")
        .expect("bounded waiter gate");
    let network_slot = transport
        .find(".acquire_owned()")
        .expect("network capacity slot");
    let waiter_release = transport
        .find("drop(queue_slot)")
        .expect("waiter slot release");
    assert!(
        waiter_gate < network_slot && network_slot < waiter_release,
        "provider capacity gates must be taken and released in order"
    );
    for marker in [
        "provider_capacity_queue_full",
        "provider_capacity_closed",
        "provider_capacity_tpm_exceeded",
        "provider_capacity_quota_exhausted",
    ] {
        assert!(
            transport.contains(marker),
            "provider capacity marker missing: {marker}"
        );
    }

    // Retry backoff belongs to the Runner: the only provider network exit contains no sleep, so a
    // backoff can never hold the capacity slot, and the transport never retries on its own.
    for forbidden in [
        "tokio::time::sleep",
        "std::thread::sleep",
        "retry_delay",
        "retry_after_backoff",
    ] {
        assert!(
            !transport.contains(forbidden),
            "provider transport must not wait inside the capacity scope: {forbidden}"
        );
    }

    // Aliases that resolve to one provider/origin/credential scope share the same pooled state;
    // a profile name is never a way around the connection-level quota.
    for marker in [
        "capacity_scope",
        "capacity_window",
        "queue_slots",
        "KIANA_MODEL_QUEUE_LIMIT",
    ] {
        assert!(
            config.contains(marker),
            "provider capacity alias marker missing: {marker}"
        );
    }
}

#[test]
fn drifted_fallback_admission_is_refused_before_the_route_changes() {
    let admission = admission();
    let route = admission.route.clone();
    validate_fallback_attempt(
        &admission,
        &route,
        Some(admission.credential_revision.as_str()),
        &admission.permit_digest,
    )
    .expect("matching fallback binding");

    let mut model_drift = route.clone();
    model_drift.model_id = "model-c".to_owned();
    assert_eq!(
        validate_fallback_attempt(
            &admission,
            &model_drift,
            Some(admission.credential_revision.as_str()),
            &admission.permit_digest,
        )
        .unwrap_err()
        .code,
        "fallback_route_admission_drift"
    );
    assert_eq!(
        validate_fallback_attempt(&admission, &route, None, &admission.permit_digest)
            .unwrap_err()
            .code,
        "fallback_credential_revision_drift"
    );
    assert_eq!(
        validate_fallback_attempt(
            &admission,
            &route,
            Some(admission.credential_revision.as_str()),
            &digest('z'),
        )
        .unwrap_err()
        .code,
        "fallback_permit_admission_drift"
    );

    // The provider validates a ControlPlane decision; it never chooses or spawns a fallback.
    let fallback = include_str!("../src/fallback.rs");
    for forbidden in [
        "choose_fallback",
        "fallback_loop",
        "fallback.unwrap()",
        "raw_credential",
        "tokio::spawn",
    ] {
        assert!(
            !fallback.contains(forbidden),
            "provider fallback boundary widened: {forbidden}"
        );
    }
    assert!(fallback.contains("FALLBACK_PROVIDER_BOUNDARY_SCHEMA"));
}
