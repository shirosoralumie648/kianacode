//! BQ-18 allow-listed fallback admission fixtures (deny first).
//!
//! The fixtures pin the card's refusal surface — capability downgrade, cross data-boundary
//! fallback, missing credential/budget/price, stale authority, allowlist tamper and reuse of the
//! primary permit/reservation — before the success column that gives every fallback attempt its
//! own route, usage observation, pinned rate card and receipt.
//!
//! Every fixture is an in-process value contract: no provider is contacted, no permit is consumed,
//! no EventLog fact is written and no second execution loop starts.

use kiana_domain::*;
use uuid::Uuid;

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

fn card_with(
    id: RateCardId,
    provider: &str,
    model_selector: &str,
    card_version: u64,
    effective_from_unix_ms: u64,
    effective_to_unix_ms: Option<u64>,
) -> RateCard {
    let mut card = RateCard::new(
        id,
        provider,
        model_selector,
        "USD",
        Some(2),
        Some(3),
        effective_from_unix_ms,
        effective_to_unix_ms,
        card_version,
        "bq18-fixture",
    )
    .expect("card");
    card.request_price = Some(5);
    card.rate_card_digest = card.digest();
    card.validate().expect("valid card");
    card
}

fn card() -> RateCard {
    card_with_id(RateCardId::new())
}

fn card_with_id(id: RateCardId) -> RateCard {
    card_with(id, "provider-b", "model-b", 2, 1_000, Some(10_000))
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

fn strict_requirements() -> FallbackCapabilityRequirements {
    FallbackCapabilityRequirements {
        require_reasoning_replay: true,
        ..requirements()
    }
}

fn request_for(
    run_id: RunId,
    original: &ModelRoute,
    requirements: FallbackCapabilityRequirements,
) -> FallbackAdmissionRequest {
    FallbackAdmissionRequest::new(
        run_id,
        AttemptId::new(),
        original,
        digest('a'),
        7,
        digest('b'),
        digest('c'),
        digest('d'),
        digest('e'),
        requirements,
        2_000,
    )
    .expect("request")
}

fn request(run_id: RunId, original: &ModelRoute) -> FallbackAdmissionRequest {
    request_for(run_id, original, requirements())
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

fn allowlist_for(candidate: &FallbackAdmissionCandidate) -> FallbackRouteAllowlist {
    FallbackRouteAllowlist::from_routes(std::slice::from_ref(&candidate.route)).expect("allowlist")
}

fn admission() -> FallbackAttemptAdmission {
    let original = route("provider-a", "model-a", "primary");
    let candidate = candidate();
    let allowlist = allowlist_for(&candidate);
    let request = request(RunId::new(), &original);
    admit_fallback_attempt(&request, &candidate, &allowlist, AttemptId::new()).expect("admit")
}

/// Build a usage observation with explicit route identity so a fixture can prove which field
/// drifts.  All seven token dimensions are present, which keeps the estimate path (rather than the
/// `Partial` unknown path) reachable for the success fixtures.
#[allow(clippy::too_many_arguments)]
fn usage_with(
    admission: &FallbackAttemptAdmission,
    run_id: RunId,
    provider: Option<&str>,
    model: &str,
    connection: &str,
    basis: &str,
    raw_digest: String,
    confidence: UsageConfidence,
) -> NormalizedUsage {
    let unknown = confidence != UsageConfidence::Known;
    NormalizedUsage::new(
        UsageId::new(),
        admission.attempt_id,
        run_id,
        provider.map(str::to_owned),
        model.to_owned(),
        connection.to_owned(),
        UsageVector {
            schema: USAGE_VECTOR_SCHEMA.to_owned(),
            input_tokens: Some(10),
            output_tokens: Some(4),
            ..UsageVector::zero()
        },
        UsageSource::Provider,
        UsageObservation::Final,
        Some(1),
        confidence,
        unknown.then_some(BillingUnknownReason::ResultUnknown),
        basis.to_owned(),
        raw_digest,
    )
    .expect("usage")
}

fn usage(admission: &FallbackAttemptAdmission, confidence: UsageConfidence) -> NormalizedUsage {
    usage_with(
        admission,
        admission.run_id,
        Some(admission.route.provider_id.as_str()),
        admission.route.model_id.as_str(),
        admission.route.connection_id.as_str(),
        "provider fallback fixture",
        digest('u'),
        confidence,
    )
}

fn cost_from_usage(admission: &FallbackAttemptAdmission, usage: &NormalizedUsage) -> CostBreakdown {
    let card = card_with_id(admission.rate_card_id);
    CostBreakdown::from_usage(usage, Some(&card), 2, 2_000).expect("cost")
}

fn receipt(
    admission: &FallbackAttemptAdmission,
    confidence: UsageConfidence,
) -> FallbackAttemptReceipt {
    let usage = usage(admission, confidence);
    let cost = cost_from_usage(admission, &usage);
    FallbackAttemptReceipt::from_admission(admission, usage, cost, None).expect("receipt")
}

/// Copy a serialized contract and add one field that only model text or provider wire data could
/// carry.  A typed decode must reject the shape instead of ignoring the extra value.
fn with_extra_field(value: &serde_json::Value, field: &str) -> serde_json::Value {
    let mut object = value.as_object().expect("object").clone();
    object.insert(field.to_owned(), serde_json::json!("raw model text"));
    serde_json::Value::Object(object)
}

#[test]
fn fallback_rechecks_route_authority_data_budget_credential_price_and_permit() {
    let original = route("provider-a", "model-a", "primary");
    let candidate = candidate();
    let allowlist = allowlist_for(&candidate);
    let request = request(RunId::new(), &original);
    let admitted = admit_fallback_attempt(&request, &candidate, &allowlist, AttemptId::new())
        .expect("fresh admission");
    assert_ne!(admitted.route_digest, request.original_route_digest);
    assert_ne!(admitted.permit_digest, request.original_permit_digest);
    assert_ne!(
        admitted.budget_reservation_digest,
        request.original_budget_reservation_digest
    );

    let mut capability = candidate.clone();
    capability.capabilities.tools = CapabilitySupport::Unsupported;
    assert_eq!(
        admit_fallback_attempt(&request, &capability, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_capability_downgrade_rejected"
    );

    let mut authority = candidate.clone();
    authority.authority_digest = digest('x');
    assert_eq!(
        admit_fallback_attempt(&request, &authority, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_authority_recheck_failed"
    );

    let mut authority_epoch = candidate.clone();
    authority_epoch.authority_epoch = 8;
    assert_eq!(
        admit_fallback_attempt(&request, &authority_epoch, &allowlist, AttemptId::new())
            .unwrap_err(),
        "fallback_authority_epoch_stale"
    );

    let mut data = candidate.clone();
    data.data_boundary_digest = digest('x');
    assert_eq!(
        admit_fallback_attempt(&request, &data, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_data_boundary_mismatch"
    );

    let mut budget = candidate.clone();
    budget.budget_digest = digest('x');
    assert_eq!(
        admit_fallback_attempt(&request, &budget, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_budget_recheck_required"
    );

    let mut no_budget = candidate.clone();
    no_budget.budget_reservation_digest = None;
    assert_eq!(
        admit_fallback_attempt(&request, &no_budget, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_budget_reservation_missing"
    );

    let mut reused_reservation = candidate.clone();
    reused_reservation.budget_reservation_digest =
        Some(request.original_budget_reservation_digest.clone());
    assert_eq!(
        admit_fallback_attempt(&request, &reused_reservation, &allowlist, AttemptId::new())
            .unwrap_err(),
        "fallback_cannot_reuse_original_budget_reservation"
    );

    let mut reused_permit = candidate.clone();
    reused_permit.permit_digest = request.original_permit_digest.clone();
    assert_eq!(
        admit_fallback_attempt(&request, &reused_permit, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_cannot_reuse_original_permit"
    );

    let mut no_credential = candidate.clone();
    no_credential.credential_revision = None;
    assert_eq!(
        admit_fallback_attempt(&request, &no_credential, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_credential_missing"
    );

    let mut no_price = candidate.clone();
    no_price.rate_card = None;
    assert_eq!(
        admit_fallback_attempt(&request, &no_price, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_rate_card_missing"
    );

    let mut expired = candidate.clone();
    expired.rate_card = Some(card_with(
        RateCardId::new(),
        "provider-b",
        "model-b",
        2,
        1_000,
        Some(1_500),
    ));
    assert_eq!(
        admit_fallback_attempt(&request, &expired, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_rate_card_expired"
    );

    // A wildcard card that still covers the route is a valid price re-admission.
    let mut wildcard = candidate.clone();
    let mut wildcard_card = card();
    wildcard_card.model_selector = "*".to_owned();
    wildcard_card.rate_card_digest = wildcard_card.digest();
    wildcard_card.validate().expect("wildcard card");
    wildcard.rate_card = Some(wildcard_card);
    admit_fallback_attempt(&request, &wildcard, &allowlist, AttemptId::new())
        .expect("wildcard price covers the admitted route");
}

#[test]
fn capability_downgrade_is_refused_per_dimension_and_on_the_route_streaming_flag() {
    let original = route("provider-a", "model-a", "primary");
    let base = candidate();
    let allowlist = allowlist_for(&base);
    let request = request_for(RunId::new(), &original, strict_requirements());

    let mut tools = base.clone();
    tools.capabilities.tools = CapabilitySupport::Unsupported;
    assert_eq!(
        admit_fallback_attempt(&request, &tools, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_capability_downgrade_rejected"
    );

    let mut streaming = base.clone();
    streaming.capabilities.streaming = CapabilitySupport::Unknown;
    assert_eq!(
        admit_fallback_attempt(&request, &streaming, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_capability_downgrade_rejected"
    );

    let mut structured = base.clone();
    structured.capabilities.structured_output = CapabilitySupport::Unsupported;
    assert_eq!(
        admit_fallback_attempt(&request, &structured, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_capability_downgrade_rejected"
    );

    let mut images = base.clone();
    images.capabilities.images = CapabilitySupport::Unsupported;
    assert_eq!(
        admit_fallback_attempt(&request, &images, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_capability_downgrade_rejected"
    );

    let mut reasoning = base.clone();
    reasoning.capabilities.reasoning_replay = CapabilitySupport::Unsupported;
    assert_eq!(
        admit_fallback_attempt(&request, &reasoning, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_capability_downgrade_rejected"
    );

    let mut small_context = base.clone();
    small_context.capabilities.context_window = 32_000;
    assert_eq!(
        admit_fallback_attempt(&request, &small_context, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_capability_downgrade_rejected"
    );

    let mut small_output = base.clone();
    small_output.capabilities.max_output = 512;
    assert_eq!(
        admit_fallback_attempt(&request, &small_output, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_capability_downgrade_rejected"
    );

    // A non-streaming fallback route cannot pretend the attempt still streams.
    let mut non_streaming = base.clone();
    non_streaming.route.streaming = false;
    let streaming_allowlist = allowlist_for(&non_streaming);
    assert_eq!(
        admit_fallback_attempt(
            &request,
            &non_streaming,
            &streaming_allowlist,
            AttemptId::new()
        )
        .unwrap_err(),
        "fallback_capability_downgrade_rejected"
    );

    // Zero capability minimums cannot silently disable the whole downgrade fence.
    let mut zero_requirements = strict_requirements();
    zero_requirements.min_context_window = 0;
    assert_eq!(
        FallbackAdmissionRequest::new(
            RunId::new(),
            AttemptId::new(),
            &original,
            digest('a'),
            7,
            digest('b'),
            digest('c'),
            digest('d'),
            digest('e'),
            zero_requirements,
            2_000,
        )
        .unwrap_err(),
        "fallback_capability_requirement_invalid"
    );
}

#[test]
fn missing_credential_budget_or_price_is_refused() {
    let original = route("provider-a", "model-a", "primary");
    let base = candidate();
    let allowlist = allowlist_for(&base);
    let request = request(RunId::new(), &original);

    let mut bad_credential = base.clone();
    bad_credential.credential_revision = Some("plain".to_owned());
    assert_eq!(
        admit_fallback_attempt(&request, &bad_credential, &allowlist, AttemptId::new())
            .unwrap_err(),
        "fallback_candidate_credential_revision_invalid"
    );

    let mut bad_reservation = base.clone();
    bad_reservation.budget_reservation_digest = Some("plain".to_owned());
    assert_eq!(
        admit_fallback_attempt(&request, &bad_reservation, &allowlist, AttemptId::new())
            .unwrap_err(),
        "fallback_candidate_budget_reservation_digest_invalid"
    );

    let mut bad_permit = base.clone();
    bad_permit.permit_digest = "plain".to_owned();
    assert_eq!(
        admit_fallback_attempt(&request, &bad_permit, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_candidate_permit_digest_invalid"
    );

    let mut no_authority = base.clone();
    no_authority.authority_epoch = 0;
    assert_eq!(
        admit_fallback_attempt(&request, &no_authority, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_candidate_authority_invalid"
    );

    let mut bad_capabilities = base.clone();
    bad_capabilities.capabilities.source = String::new();
    assert_eq!(
        admit_fallback_attempt(&request, &bad_capabilities, &allowlist, AttemptId::new())
            .unwrap_err(),
        "fallback_candidate_capabilities_invalid"
    );

    let mut no_provider = base.clone();
    no_provider.route.provider_id = String::new();
    assert_eq!(
        admit_fallback_attempt(&request, &no_provider, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_candidate_provider_invalid"
    );

    let mut no_connection = base.clone();
    no_connection.route.connection_id = String::new();
    assert_eq!(
        admit_fallback_attempt(&request, &no_connection, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_candidate_connection_invalid"
    );

    let mut no_model = base.clone();
    no_model.route.model_id = String::new();
    assert_eq!(
        admit_fallback_attempt(&request, &no_model, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_candidate_model_invalid"
    );

    let mut no_profile = base.clone();
    no_profile.route.profile = String::new();
    assert_eq!(
        admit_fallback_attempt(&request, &no_profile, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_candidate_profile_invalid"
    );

    let mut no_revision = base.clone();
    no_revision.route.configuration_revision = String::new();
    assert_eq!(
        admit_fallback_attempt(&request, &no_revision, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_candidate_configuration_revision_invalid"
    );

    // A card that is not pinned to this provider cannot price the fallback route.
    let mut foreign_provider = base.clone();
    foreign_provider.rate_card = Some(card_with(
        RateCardId::new(),
        "provider-c",
        "model-b",
        2,
        1_000,
        Some(10_000),
    ));
    assert_eq!(
        admit_fallback_attempt(&request, &foreign_provider, &allowlist, AttemptId::new())
            .unwrap_err(),
        "fallback_rate_card_scope_mismatch"
    );

    // A card pinned to a different model cannot be borrowed for this route.
    let mut foreign_model = base.clone();
    foreign_model.rate_card = Some(card_with(
        RateCardId::new(),
        "provider-b",
        "model-c",
        2,
        1_000,
        Some(10_000),
    ));
    assert_eq!(
        admit_fallback_attempt(&request, &foreign_model, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_rate_card_scope_mismatch"
    );

    // A zero-version or corrupted card never prices anything.
    let mut zero_version = base.clone();
    let mut broken_card = card();
    broken_card.card_version = 0;
    broken_card.rate_card_digest = broken_card.digest();
    zero_version.rate_card = Some(broken_card);
    assert_eq!(
        admit_fallback_attempt(&request, &zero_version, &allowlist, AttemptId::new()).unwrap_err(),
        "rate_card_invalid"
    );

    let mut corrupted = base;
    let mut corrupted_card = card();
    corrupted_card.rate_card_digest = digest('z');
    corrupted.rate_card = Some(corrupted_card);
    assert_eq!(
        admit_fallback_attempt(&request, &corrupted, &allowlist, AttemptId::new()).unwrap_err(),
        "rate_card_invalid"
    );
}

#[test]
fn fallback_route_must_change_and_attempt_identity_must_be_fresh() {
    let candidate = candidate();
    let allowlist = allowlist_for(&candidate);

    // Falling back to the same route is not a fallback at all.
    let same_route_request = request(RunId::new(), &candidate.route);
    assert_eq!(
        admit_fallback_attempt(
            &same_route_request,
            &candidate,
            &allowlist,
            AttemptId::new()
        )
        .unwrap_err(),
        "fallback_route_must_change"
    );

    let original = route("provider-a", "model-a", "primary");
    let admission_request = request(RunId::new(), &original);

    // Reusing the primary attempt identity is refused.
    assert_eq!(
        admit_fallback_attempt(
            &admission_request,
            &candidate,
            &allowlist,
            admission_request.original_attempt_id,
        )
        .unwrap_err(),
        "fallback_attempt_identity_invalid"
    );

    let nil_attempt = AttemptId::from_uuid(Uuid::nil());
    assert_eq!(
        admit_fallback_attempt(&admission_request, &candidate, &allowlist, nil_attempt)
            .unwrap_err(),
        "fallback_attempt_identity_invalid"
    );

    let nil_request = FallbackAdmissionRequest::new(
        RunId::new(),
        AttemptId::from_uuid(Uuid::nil()),
        &original,
        digest('a'),
        7,
        digest('b'),
        digest('c'),
        digest('d'),
        digest('e'),
        requirements(),
        2_000,
    );
    assert_eq!(
        nil_request.unwrap_err(),
        "fallback_admission_request_invalid"
    );
}

#[test]
fn admission_records_the_original_route_and_allowlist_digest() {
    let original = route("provider-a", "model-a", "primary");
    let candidate = candidate();
    let allowlist = allowlist_for(&candidate);
    let request = request(RunId::new(), &original);
    let admitted = admit_fallback_attempt(&request, &candidate, &allowlist, AttemptId::new())
        .expect("fresh admission");

    assert!(admitted.is_fresh_attempt());
    assert_eq!(
        admitted.original_route_digest,
        request.original_route_digest
    );
    assert_ne!(admitted.original_route_digest, admitted.route_digest);
    assert_eq!(admitted.allowlist_digest, allowlist.allowlist_digest);
    assert!(admitted.allowlist_digest.starts_with("sha256:"));
    admitted.validate().expect("admission is self-checking");

    // The recorded primary route and allowlist revision are part of the admission digest.
    let mut drifted_original = admitted.clone();
    drifted_original.original_route_digest = digest('z');
    drifted_original.admission_digest = drifted_original.digest();
    assert_ne!(drifted_original.admission_digest, admitted.admission_digest);

    let mut drifted_allowlist = admitted.clone();
    drifted_allowlist.allowlist_digest = digest('z');
    drifted_allowlist.admission_digest = drifted_allowlist.digest();
    assert_ne!(
        drifted_allowlist.admission_digest,
        admitted.admission_digest
    );
}

#[test]
fn allowlist_pins_exact_route_and_model_identity() {
    let original = route("provider-a", "model-a", "primary");
    let candidate = candidate();
    let allowlist = allowlist_for(&candidate);
    let request = request(RunId::new(), &original);

    let mut drifted = candidate.clone();
    drifted.route.model_id = "model-c".to_owned();
    let mut wildcard_card = card();
    wildcard_card.model_selector = "*".to_owned();
    wildcard_card.rate_card_digest = wildcard_card.digest();
    wildcard_card.validate().expect("wildcard card");
    drifted.rate_card = Some(wildcard_card);
    assert_eq!(
        admit_fallback_attempt(&request, &drifted, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_route_not_allowlisted"
    );

    // A different provider with the same model name is a different route.
    let mut foreign_provider = candidate.clone();
    foreign_provider.route.provider_id = "provider-c".to_owned();
    assert_eq!(
        allowlist.contains(&foreign_provider.route).unwrap_err(),
        "fallback_route_not_allowlisted"
    );

    // A configuration revision change is a different route even when every name is unchanged.
    let mut foreign_revision = candidate.clone();
    foreign_revision.route.configuration_revision = "config.v2".to_owned();
    assert_eq!(
        allowlist.contains(&foreign_revision.route).unwrap_err(),
        "fallback_route_not_allowlisted"
    );
}

#[test]
fn allowlist_rejects_connection_identity_drift() {
    let candidate = candidate();
    let allowlist = allowlist_for(&candidate);
    let mut changed = candidate.route.clone();
    changed.connection_id = "different-connection".to_owned();
    assert_eq!(
        allowlist.contains(&changed).unwrap_err(),
        "fallback_route_not_allowlisted"
    );

    let mut legacy = serde_json::to_value(&allowlist).expect("allowlist JSON");
    legacy["entries"][0]
        .as_object_mut()
        .expect("entry object")
        .remove("connection_id");
    assert!(serde_json::from_value::<FallbackRouteAllowlist>(legacy).is_err());
}

#[test]
fn allowlist_can_pin_multiple_connections_for_the_same_provider_and_model() {
    let first = route("provider-b", "model-b", "backup-a");
    let second = route("provider-b", "model-b", "backup-b");
    let allowlist = FallbackRouteAllowlist::from_routes(&[first.clone(), second.clone()])
        .expect("distinct connections are distinct allowlist entries");
    assert_eq!(allowlist.entries.len(), 2);
    allowlist
        .contains(&first)
        .expect("first connection allowed");
    allowlist
        .contains(&second)
        .expect("second connection allowed");

    // Both connections can be admitted as real fallback routes with fresh bindings.
    let original = route("provider-a", "model-a", "primary");
    let request = request(RunId::new(), &original);
    let mut candidate = candidate();
    candidate.route = second;
    candidate.rate_card = Some(card());
    let admitted = admit_fallback_attempt(&request, &candidate, &allowlist, AttemptId::new())
        .expect("second connection admit");
    assert_eq!(admitted.route.connection_id, "backup-b");
}

#[test]
fn allowlist_tamper_is_refused_before_contains_or_admission() {
    let candidate = candidate();
    let original = route("provider-a", "model-a", "primary");
    let request = request(RunId::new(), &original);

    let mut flipped_digest = allowlist_for(&candidate);
    flipped_digest.allowlist_digest = digest('z');
    assert_eq!(
        flipped_digest.contains(&candidate.route).unwrap_err(),
        "fallback_allowlist_invalid"
    );
    assert_eq!(
        admit_fallback_attempt(&request, &candidate, &flipped_digest, AttemptId::new())
            .unwrap_err(),
        "fallback_allowlist_invalid"
    );

    let mut tampered_entry = allowlist_for(&candidate);
    tampered_entry.entries[0].route_digest = digest('z');
    assert_eq!(
        tampered_entry.validate().unwrap_err(),
        "fallback_allowlist_invalid"
    );

    assert_eq!(
        FallbackRouteAllowlist::from_routes(&[]).unwrap_err(),
        "fallback_allowlist_invalid"
    );
    let routed = route("provider-b", "model-b", "backup");
    assert_eq!(
        FallbackRouteAllowlist::from_routes(&[routed.clone(), routed.clone()]).unwrap_err(),
        "fallback_allowlist_duplicate_route"
    );

    let too_many = (0..=MAX_FALLBACK_ROUTES)
        .map(|index| route("provider-b", &format!("model-{index}"), "backup"))
        .collect::<Vec<_>>();
    assert_eq!(
        FallbackRouteAllowlist::from_routes(&too_many).unwrap_err(),
        "fallback_allowlist_invalid"
    );

    // A reordered list is not canonical even when every entry is otherwise valid.
    let second = route("provider-b", "model-b", "backup-b");
    let mut reordered =
        FallbackRouteAllowlist::from_routes(&[routed.clone(), second]).expect("two routes");
    reordered.entries.swap(0, 1);
    reordered.allowlist_digest = reordered.digest();
    assert_eq!(
        reordered.validate().unwrap_err(),
        "fallback_allowlist_noncanonical"
    );
}

#[test]
fn request_and_admission_digests_are_fenced() {
    let original = route("provider-a", "model-a", "primary");
    let candidate = candidate();
    let allowlist = allowlist_for(&candidate);
    let base_request = request(RunId::new(), &original);

    let mut zero_epoch = base_request.clone();
    zero_epoch.authority_epoch = 0;
    zero_epoch.request_digest = zero_epoch.digest();
    assert_eq!(
        admit_fallback_attempt(&zero_epoch, &candidate, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_admission_request_invalid"
    );

    let mut zero_now = base_request.clone();
    zero_now.now_unix_ms = 0;
    zero_now.request_digest = zero_now.digest();
    assert_eq!(
        admit_fallback_attempt(&zero_now, &candidate, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_admission_request_invalid"
    );

    let mut drifted = base_request.clone();
    drifted.original_route_digest = "plain".to_owned();
    drifted.request_digest = drifted.digest();
    assert_eq!(
        admit_fallback_attempt(&drifted, &candidate, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_original_route_digest_invalid"
    );

    let mut drifted = base_request.clone();
    drifted.authority_digest = "plain".to_owned();
    drifted.request_digest = drifted.digest();
    assert_eq!(
        admit_fallback_attempt(&drifted, &candidate, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_authority_digest_invalid"
    );

    let mut drifted = base_request.clone();
    drifted.data_boundary_digest = "plain".to_owned();
    drifted.request_digest = drifted.digest();
    assert_eq!(
        admit_fallback_attempt(&drifted, &candidate, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_data_boundary_digest_invalid"
    );

    let mut drifted = base_request.clone();
    drifted.budget_digest = "plain".to_owned();
    drifted.request_digest = drifted.digest();
    assert_eq!(
        admit_fallback_attempt(&drifted, &candidate, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_budget_digest_invalid"
    );

    let mut drifted = base_request.clone();
    drifted.original_budget_reservation_digest = "plain".to_owned();
    drifted.request_digest = drifted.digest();
    assert_eq!(
        admit_fallback_attempt(&drifted, &candidate, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_original_budget_reservation_digest_invalid"
    );

    let mut drifted = base_request.clone();
    drifted.original_permit_digest = "plain".to_owned();
    drifted.request_digest = drifted.digest();
    assert_eq!(
        admit_fallback_attempt(&drifted, &candidate, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_original_permit_digest_invalid"
    );

    let admitted = admission();

    let mut permit_drift = admitted.clone();
    permit_drift.permit_digest = digest('z');
    assert_eq!(
        permit_drift.validate().unwrap_err(),
        "fallback_attempt_admission_invalid"
    );

    let mut schema_drift = admitted.clone();
    schema_drift.schema = "kiana.fallback-admission.v0".to_owned();
    schema_drift.admission_digest = schema_drift.digest();
    assert_eq!(
        schema_drift.validate().unwrap_err(),
        "fallback_attempt_admission_invalid"
    );

    let mut reused_attempt = admitted.clone();
    reused_attempt.attempt_id = reused_attempt.original_attempt_id;
    reused_attempt.admission_digest = reused_attempt.digest();
    assert_eq!(
        reused_attempt.validate().unwrap_err(),
        "fallback_attempt_admission_invalid"
    );

    let mut same_route = admitted.clone();
    same_route.route_digest = same_route.original_route_digest.clone();
    same_route.admission_digest = same_route.digest();
    assert_eq!(
        same_route.validate().unwrap_err(),
        "fallback_attempt_admission_invalid"
    );

    let mut bad_allowlist_digest = admitted.clone();
    bad_allowlist_digest.allowlist_digest = "plain".to_owned();
    bad_allowlist_digest.admission_digest = bad_allowlist_digest.digest();
    assert_eq!(
        bad_allowlist_digest.validate().unwrap_err(),
        "fallback_admission_allowlist_digest_invalid"
    );

    let mut nil_attempt = admitted;
    nil_attempt.attempt_id = AttemptId::from_uuid(Uuid::nil());
    nil_attempt.admission_digest = nil_attempt.digest();
    assert_eq!(
        nil_attempt.validate().unwrap_err(),
        "fallback_attempt_admission_invalid"
    );
}

#[test]
fn every_fallback_attempt_has_independent_usage_rate_card_and_receipt() {
    let first = admission();
    let rate_card = card_with_id(first.rate_card_id);
    let first_usage = usage(&first, UsageConfidence::Known);
    let first_cost =
        CostBreakdown::from_usage(&first_usage, Some(&rate_card), 2, 2_000).expect("estimate");
    let first_receipt =
        FallbackAttemptReceipt::from_admission(&first, first_usage, first_cost, None)
            .expect("estimate receipt");
    assert!(first_receipt.is_success());
    assert_eq!(first_receipt.provider_id, first.route.provider_id);
    assert_eq!(first_receipt.connection_id, first.route.connection_id);
    assert_eq!(first_receipt.model_id, first.route.model_id);
    assert_eq!(first_receipt.route_digest, first.route_digest);

    let mut second = first.clone();
    second.attempt_id = AttemptId::new();
    second.permit_digest = digest('i');
    second.budget_reservation_digest = digest('j');
    second.admission_digest = second.digest();
    second.validate().expect("fresh second admission");
    let second_usage = usage(&second, UsageConfidence::Known);
    let measured_receipt = ProviderReceiptRef::new("provider-receipt-bq18").expect("receipt");
    let second_cost = CostBreakdown::from_provider_receipt(
        &second_usage,
        Some(&rate_card),
        Money::new("USD", 31).expect("money"),
        measured_receipt.clone(),
        2_000,
    )
    .expect("measured cost");
    let second_receipt = FallbackAttemptReceipt::from_admission(
        &second,
        second_usage,
        second_cost,
        Some(measured_receipt),
    )
    .expect("measured receipt");
    assert_ne!(first_receipt.attempt_id, second_receipt.attempt_id);
    assert_ne!(first_receipt.receipt_digest, second_receipt.receipt_digest);
    assert_eq!(second_receipt.rate_card_version, 2);
    assert_eq!(second_receipt.outcome, FallbackAttemptOutcome::Measured);

    let unknown_admission = admission();
    let unknown_card = card_with_id(unknown_admission.rate_card_id);
    let unknown_usage = usage(&unknown_admission, UsageConfidence::Unknown);
    let unknown_cost = CostBreakdown::from_usage(&unknown_usage, Some(&unknown_card), 1, 2_000)
        .expect("unknown cost");
    let unknown = FallbackAttemptReceipt::from_admission(
        &unknown_admission,
        unknown_usage,
        unknown_cost,
        None,
    )
    .expect("unknown receipt");
    assert!(!unknown.is_success());
    assert_eq!(unknown.outcome, FallbackAttemptOutcome::Unknown);
}

#[test]
fn receipt_refuses_foreign_usage_and_cost_observations() {
    let first = admission();
    let foreign_run = RunId::new();
    let foreign = usage_with(
        &first,
        foreign_run,
        Some("provider-b"),
        "model-b",
        "backup",
        "foreign run fixture",
        digest('v'),
        UsageConfidence::Known,
    );
    let valid_cost = cost_from_usage(&first, &usage(&first, UsageConfidence::Known));
    assert_eq!(
        FallbackAttemptReceipt::from_admission(&first, foreign, valid_cost, None).unwrap_err(),
        "fallback_receipt_usage_identity_mismatch"
    );

    let foreign_attempt = admission();
    let foreign_usage = usage(&foreign_attempt, UsageConfidence::Known);
    let valid_cost = cost_from_usage(&first, &usage(&first, UsageConfidence::Known));
    assert_eq!(
        FallbackAttemptReceipt::from_admission(&first, foreign_usage, valid_cost, None)
            .unwrap_err(),
        "fallback_receipt_usage_identity_mismatch"
    );

    let canonical_usage = usage(&first, UsageConfidence::Known);
    let valid_cost = cost_from_usage(&first, &canonical_usage);
    for (provider, model, connection) in [
        (Some("provider-c"), "model-b", "backup"),
        (None, "model-b", "backup"),
        (Some("provider-b"), "model-c", "backup"),
        (Some("provider-b"), "model-b", "backup-b"),
    ] {
        let drifted = usage_with(
            &first,
            first.run_id,
            provider,
            model,
            connection,
            "drifted route fixture",
            digest('w'),
            UsageConfidence::Known,
        );
        assert_eq!(
            FallbackAttemptReceipt::from_admission(&first, drifted, valid_cost.clone(), None)
                .unwrap_err(),
            if provider.is_none() {
                "fallback_receipt_usage_route_missing"
            } else {
                "fallback_receipt_usage_route_mismatch"
            }
        );
    }

    // A cost breakdown computed from a different usage observation cannot be paired with this
    // usage, even when both describe the same attempt and route.
    let other_usage = usage_with(
        &first,
        first.run_id,
        Some(first.route.provider_id.as_str()),
        first.route.model_id.as_str(),
        first.route.connection_id.as_str(),
        "second observation fixture",
        digest('x'),
        UsageConfidence::Known,
    );
    let other_cost = cost_from_usage(&first, &other_usage);
    assert_ne!(other_cost.usage_digest, canonical_usage.usage_digest);
    assert_eq!(
        FallbackAttemptReceipt::from_admission(&first, canonical_usage.clone(), other_cost, None)
            .unwrap_err(),
        "fallback_receipt_cost_usage_mismatch"
    );

    // A cost whose confidence contradicts the bound usage is refused even when the digest is
    // copied from that usage.
    let contradictory = CostBreakdown::unknown(
        first.run_id,
        Some(first.attempt_id),
        canonical_usage.usage_digest.clone(),
        UsageConfidence::Unknown,
        BillingUnknownReason::ResultUnknown,
    )
    .expect("unknown cost");
    assert_eq!(
        FallbackAttemptReceipt::from_admission(
            &first,
            canonical_usage.clone(),
            contradictory,
            None
        )
        .unwrap_err(),
        "fallback_receipt_cost_usage_mismatch"
    );

    // A receipt is self-checking: tampering with the recorded route identity fails validation even
    // after the digest is recomputed.
    let mut receipt = FallbackAttemptReceipt::from_admission(
        &first,
        canonical_usage.clone(),
        valid_cost.clone(),
        None,
    )
    .expect("valid receipt");
    receipt.connection_id = "backup-b".to_owned();
    receipt.receipt_digest = receipt.digest();
    assert_eq!(
        receipt.validate().unwrap_err(),
        "fallback_receipt_usage_route_mismatch"
    );

    // A cost with no attempt identity cannot be bound to this attempt.
    let attemptless = CostBreakdown::unknown(
        first.run_id,
        None,
        canonical_usage.usage_digest.clone(),
        UsageConfidence::Known,
        BillingUnknownReason::ResultUnknown,
    )
    .expect("attemptless cost");
    assert_eq!(
        FallbackAttemptReceipt::from_admission(&first, canonical_usage.clone(), attemptless, None)
            .unwrap_err(),
        "fallback_receipt_cost_identity_mismatch"
    );

    // The receipt envelope itself is schema-fenced.
    let mut schema_drift =
        FallbackAttemptReceipt::from_admission(&first, canonical_usage, valid_cost, None)
            .expect("valid receipt");
    schema_drift.schema = "kiana.fallback-attempt-receipt.v0".to_owned();
    schema_drift.receipt_digest = schema_drift.digest();
    assert_eq!(
        schema_drift.validate().unwrap_err(),
        "fallback_attempt_receipt_invalid"
    );
}

#[test]
fn receipt_keeps_estimated_measured_and_unknown_separate() {
    let first = admission();
    let card = card_with_id(first.rate_card_id);
    let known_usage = usage(&first, UsageConfidence::Known);
    let estimate =
        CostBreakdown::from_usage(&known_usage, Some(&card), 2, 2_000).expect("estimate");

    // An estimate pinned to another card version cannot masquerade as this admission's price.
    let mut foreign_card_version = first.clone();
    foreign_card_version.rate_card_version = 3;
    foreign_card_version.admission_digest = foreign_card_version.digest();
    foreign_card_version
        .validate()
        .expect("version drift admission");
    assert_eq!(
        FallbackAttemptReceipt::from_admission(
            &foreign_card_version,
            known_usage.clone(),
            estimate.clone(),
            None
        )
        .unwrap_err(),
        "fallback_receipt_cost_outcome_mismatch"
    );

    // Measured requires the opaque provider receipt, and it must be the same receipt that priced
    // the measured amount.
    let provider_receipt = ProviderReceiptRef::new("provider-receipt-bq18-a").expect("receipt ref");
    let measured = CostBreakdown::from_provider_receipt(
        &known_usage,
        Some(&card),
        Money::new("USD", 31).expect("money"),
        provider_receipt.clone(),
        2_000,
    )
    .expect("measured cost");
    assert_eq!(
        FallbackAttemptReceipt::from_admission(&first, known_usage.clone(), measured.clone(), None)
            .unwrap_err(),
        "fallback_receipt_cost_outcome_mismatch"
    );
    let other_receipt = ProviderReceiptRef::new("provider-receipt-bq18-b").expect("receipt ref");
    assert_eq!(
        FallbackAttemptReceipt::from_admission(
            &first,
            known_usage.clone(),
            measured.clone(),
            Some(other_receipt)
        )
        .unwrap_err(),
        "fallback_receipt_cost_outcome_mismatch"
    );
    let accepted = FallbackAttemptReceipt::from_admission(
        &first,
        known_usage.clone(),
        measured,
        Some(provider_receipt.clone()),
    )
    .expect("measured receipt");
    assert_eq!(accepted.outcome, FallbackAttemptOutcome::Measured);
    assert!(accepted.is_success());

    // Measured amounts cannot be built on an unknown usage observation.
    let unknown_usage = usage(&first, UsageConfidence::Unknown);
    assert_eq!(
        CostBreakdown::from_provider_receipt(
            &unknown_usage,
            Some(&card),
            Money::new("USD", 31).expect("money"),
            provider_receipt,
            2_000,
        )
        .unwrap_err(),
        "cost_measured_usage_incomplete"
    );

    // Estimated and unknown stay distinct, and unknown has no success conversion.
    let unknown_cost =
        CostBreakdown::from_usage(&unknown_usage, Some(&card), 1, 2_000).expect("unknown cost");
    let unknown = FallbackAttemptReceipt::from_admission(&first, unknown_usage, unknown_cost, None)
        .expect("unknown receipt");
    assert_eq!(unknown.outcome, FallbackAttemptOutcome::Unknown);
    assert!(!unknown.is_success());
    assert_ne!(
        FallbackAttemptOutcome::Unknown.is_success(),
        FallbackAttemptOutcome::Estimated.is_success()
    );
}

#[test]
fn fallback_receipt_ledger_rejects_cross_attempt_reuse_and_conflict() {
    let first = admission();
    let rate_card = card_with_id(first.rate_card_id);
    let first_usage = usage(&first, UsageConfidence::Known);
    let cost = CostBreakdown::from_usage(&first_usage, Some(&rate_card), 2, 2_000).expect("cost");
    let receipt =
        FallbackAttemptReceipt::from_admission(&first, first_usage, cost, None).expect("receipt");
    let mut ledger = FallbackReceiptLedger::default();
    assert!(ledger.is_empty());
    assert_eq!(
        ledger.apply(receipt.clone()).expect("apply"),
        FallbackReceiptApplyOutcome::Applied
    );
    assert_eq!(ledger.len(), 1);
    assert_eq!(
        ledger.apply(receipt.clone()).expect("replay"),
        FallbackReceiptApplyOutcome::Duplicate
    );
    let mut conflict = receipt;
    conflict.route_digest = digest('z');
    conflict.receipt_digest = conflict.digest();
    assert_eq!(
        ledger.apply(conflict).unwrap_err(),
        "fallback_attempt_receipt_conflict"
    );
    // The conflicting receipt never overwrites the accepted fact.
    assert_eq!(
        ledger
            .get(first.attempt_id)
            .expect("stored receipt")
            .route_digest,
        first.route_digest
    );

    // A receipt that claims another attempt's usage is rejected before it can be stored.
    let foreign = admission();
    let foreign_usage = usage(&foreign, UsageConfidence::Known);
    let foreign_cost = CostBreakdown::from_usage(
        &foreign_usage,
        Some(&card_with_id(foreign.rate_card_id)),
        2,
        2_000,
    )
    .expect("foreign cost");
    let mut tampered =
        FallbackAttemptReceipt::from_admission(&foreign, foreign_usage, foreign_cost, None)
            .expect("foreign receipt");
    tampered.attempt_id = first.attempt_id;
    tampered.receipt_digest = tampered.digest();
    assert_eq!(
        ledger.apply(tampered).unwrap_err(),
        "fallback_receipt_usage_identity_mismatch"
    );
    assert_eq!(ledger.len(), 1);
}

#[test]
fn fallback_contracts_reject_unknown_fields_and_raw_model_text() {
    let first = admission();
    let request = request(RunId::new(), &route("provider-a", "model-a", "primary"));
    let candidate = candidate();
    let allowlist = allowlist_for(&candidate);
    let receipt = receipt(&first, UsageConfidence::Known);

    let request_json = serde_json::to_value(&request).expect("request JSON");
    let candidate_json = serde_json::to_value(&candidate).expect("candidate JSON");
    let allowlist_json = serde_json::to_value(&allowlist).expect("allowlist JSON");
    let admission_json = serde_json::to_value(&first).expect("admission JSON");
    let receipt_json = serde_json::to_value(&receipt).expect("receipt JSON");

    // Model text or provider wire data cannot smuggle extra fields into any fallback contract.
    for (name, value) in [
        ("request", &request_json),
        ("candidate", &candidate_json),
        ("allowlist", &allowlist_json),
        ("admission", &admission_json),
        ("receipt", &receipt_json),
    ] {
        let text = value.to_string();
        assert!(
            !text.contains("transcript")
                && !text.contains("prompt")
                && !text.contains("raw_response"),
            "BQ-18 {name} must not serialize raw model text"
        );
    }

    // Each contract rejects the extra field on its own typed decode.
    assert!(
        serde_json::from_value::<FallbackAdmissionRequest>(with_extra_field(
            &request_json,
            "transcript"
        ))
        .is_err()
    );
    assert!(
        serde_json::from_value::<FallbackAdmissionCandidate>(with_extra_field(
            &candidate_json,
            "prompt"
        ))
        .is_err()
    );
    assert!(
        serde_json::from_value::<FallbackRouteAllowlist>(with_extra_field(
            &allowlist_json,
            "alias"
        ))
        .is_err()
    );
    assert!(
        serde_json::from_value::<FallbackAttemptAdmission>(with_extra_field(
            &admission_json,
            "transcript"
        ))
        .is_err()
    );
    assert!(
        serde_json::from_value::<FallbackAttemptReceipt>(with_extra_field(
            &receipt_json,
            "raw_response"
        ))
        .is_err()
    );

    // Typed round-trip keeps every identity binding intact.
    let json = serde_json::to_value(&first).expect("admission JSON");
    let decoded: FallbackAttemptAdmission =
        serde_json::from_value(json).expect("admission round-trip");
    assert_eq!(decoded, first);
    let json = serde_json::to_value(&receipt).expect("receipt JSON");
    let decoded: FallbackAttemptReceipt = serde_json::from_value(json).expect("receipt round-trip");
    assert_eq!(decoded, receipt);
    assert_eq!(decoded.usage.usage_digest, decoded.cost.usage_digest);
}
