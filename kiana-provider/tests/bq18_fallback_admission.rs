//! BQ-18 provider-side fallback boundary fixtures (deny first).
//!
//! The provider never chooses a fallback, never mints a permit and never calls EventLog.  It only
//! re-checks the route/credential/permit bindings ControlPlane admitted right before the existing
//! gateway path, so these fixtures keep that boundary validation-only and prove that every
//! binding drift is refused before any provider request is prepared.

use kiana_domain::*;
use kiana_provider::validate_fallback_attempt;

fn digest(seed: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (seed as u32) & 0xffff).repeat(16)
    )
}

fn route(provider: &str, model: &str, connection: &str) -> ModelRoute {
    ModelRoute {
        provider_id: provider.to_owned(),
        protocol: ModelProtocol::OpenAiChat,
        connection_id: connection.to_owned(),
        model_id: model.to_owned(),
        profile: connection.to_owned(),
        configuration_revision: "config.v1".to_owned(),
        streaming: true,
    }
}

fn capabilities() -> ModelCapabilities {
    ModelCapabilities {
        tools: CapabilitySupport::Supported,
        streaming: CapabilitySupport::Supported,
        structured_output: CapabilitySupport::Supported,
        images: CapabilitySupport::Supported,
        reasoning_replay: CapabilitySupport::Supported,
        context_window: 128_000,
        max_output: 4_096,
        source: "fixture".to_owned(),
        revision: "catalog.v1".to_owned(),
    }
}

fn card() -> RateCard {
    let mut card = RateCard::new(
        RateCardId::new(),
        "provider-b",
        "model-b",
        "USD",
        Some(2),
        Some(3),
        1_000,
        Some(10_000),
        2,
        "bq18-provider-fixture",
    )
    .expect("card");
    card.request_price = Some(5);
    card.rate_card_digest = card.digest();
    card.validate().expect("valid card");
    card
}

fn requirements() -> FallbackCapabilityRequirements {
    FallbackCapabilityRequirements {
        require_tools: true,
        require_streaming: true,
        require_structured_output: true,
        require_images: true,
        require_reasoning_replay: false,
        min_context_window: 64_000,
        min_output_tokens: 1_024,
    }
}

fn request(original: &ModelRoute) -> FallbackAdmissionRequest {
    FallbackAdmissionRequest::new(
        RunId::new(),
        AttemptId::new(),
        original,
        digest('a'),
        7,
        digest('b'),
        digest('c'),
        digest('d'),
        digest('e'),
        requirements(),
        2_000,
    )
    .expect("request")
}

fn candidate() -> FallbackAdmissionCandidate {
    FallbackAdmissionCandidate::new(
        route("provider-b", "model-b", "backup"),
        capabilities(),
        digest('a'),
        7,
        digest('b'),
        Some(digest('g')),
        digest('c'),
        Some(digest('f')),
        Some(card()),
        digest('h'),
    )
}

fn admission() -> FallbackAttemptAdmission {
    let original = route("provider-a", "model-a", "primary");
    let candidate = candidate();
    let allowlist = FallbackRouteAllowlist::from_routes(std::slice::from_ref(&candidate.route))
        .expect("allowlist");
    let request = request(&original);
    admit_fallback_attempt(&request, &candidate, &allowlist, AttemptId::new())
        .expect("admitted fallback")
}

#[test]
fn provider_fallback_boundary_is_validation_only_and_route_bound() {
    let fallback = include_str!("../src/fallback.rs");
    let gateway = include_str!("../src/lib.rs");
    let transport = include_str!("../src/transport.rs");
    for marker in [
        "FallbackAttemptAdmission",
        "validate_fallback_attempt",
        "fallback_route_admission_drift",
        "fallback_credential_revision_drift",
        "fallback_permit_admission_drift",
    ] {
        assert!(
            fallback.contains(marker) || gateway.contains(marker),
            "BQ-18 provider marker missing: {marker}"
        );
    }
    for forbidden in [
        "choose_fallback",
        "fallback_loop",
        "tokio::spawn(fallback",
        "send_fallback",
        "fallback.unwrap()",
        "raw_credential",
    ] {
        assert!(
            !fallback.contains(forbidden) && !transport.contains(forbidden),
            "provider fallback boundary widened: {forbidden}"
        );
    }
    assert!(gateway.contains("ProviderGateway"));
    assert!(transport.contains("send_inner_attempt"));
}

#[test]
fn provider_fallback_rejects_connection_identity_drift() {
    let fallback = include_str!("../src/fallback.rs");
    assert!(fallback.contains("route.connection_id != admission.route.connection_id"));
}

#[test]
fn provider_accepts_only_the_exact_admitted_route_credential_and_permit() {
    let admission = admission();
    let credential = admission.credential_revision.clone();
    let permit = admission.permit_digest.clone();

    // The exact admitted route/credential/permit triple is the only accepted shape.
    validate_fallback_attempt(
        &admission,
        &admission.route,
        Some(credential.as_str()),
        permit.as_str(),
    )
    .expect("exact bindings accepted");

    let mut changed_model = admission.route.clone();
    changed_model.model_id = "model-c".to_owned();
    assert_eq!(
        validate_fallback_attempt(
            &admission,
            &changed_model,
            Some(credential.as_str()),
            permit.as_str(),
        )
        .unwrap_err()
        .code,
        "fallback_route_admission_drift"
    );

    let mut changed_connection = admission.route.clone();
    changed_connection.connection_id = "other-connection".to_owned();
    assert_eq!(
        validate_fallback_attempt(
            &admission,
            &changed_connection,
            Some(credential.as_str()),
            permit.as_str(),
        )
        .unwrap_err()
        .code,
        "fallback_route_admission_drift"
    );

    let mut changed_provider = admission.route.clone();
    changed_provider.provider_id = "provider-c".to_owned();
    assert_eq!(
        validate_fallback_attempt(
            &admission,
            &changed_provider,
            Some(credential.as_str()),
            permit.as_str(),
        )
        .unwrap_err()
        .code,
        "fallback_route_admission_drift"
    );

    // A configuration revision or streaming change keeps every name but changes the route digest.
    let mut changed_revision = admission.route.clone();
    changed_revision.configuration_revision = "config.v2".to_owned();
    assert_eq!(
        validate_fallback_attempt(
            &admission,
            &changed_revision,
            Some(credential.as_str()),
            permit.as_str(),
        )
        .unwrap_err()
        .code,
        "fallback_route_admission_drift"
    );
    let mut changed_streaming = admission.route.clone();
    changed_streaming.streaming = false;
    assert_eq!(
        validate_fallback_attempt(
            &admission,
            &changed_streaming,
            Some(credential.as_str()),
            permit.as_str(),
        )
        .unwrap_err()
        .code,
        "fallback_route_admission_drift"
    );

    // A missing or rotated credential revision never reaches the transport.
    assert_eq!(
        validate_fallback_attempt(&admission, &admission.route, None, permit.as_str())
            .unwrap_err()
            .code,
        "fallback_credential_revision_drift"
    );
    let rotated = digest('z');
    assert_eq!(
        validate_fallback_attempt(
            &admission,
            &admission.route,
            Some(rotated.as_str()),
            permit.as_str(),
        )
        .unwrap_err()
        .code,
        "fallback_credential_revision_drift"
    );

    // A permit from another admission cannot be presented for this attempt.
    let other_permit = digest('y');
    assert_eq!(
        validate_fallback_attempt(
            &admission,
            &admission.route,
            Some(credential.as_str()),
            other_permit.as_str(),
        )
        .unwrap_err()
        .code,
        "fallback_permit_admission_drift"
    );

    // A tampered admission is refused by the admission's own self-check first.
    let mut tampered = admission;
    tampered.permit_digest = other_permit.clone();
    assert_eq!(
        validate_fallback_attempt(
            &tampered,
            &tampered.route,
            Some(credential.as_str()),
            other_permit.as_str(),
        )
        .unwrap_err()
        .code,
        "fallback_attempt_admission_invalid"
    );
}
