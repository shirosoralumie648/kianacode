//! provider/model route, prompt-pack, data policy, credential and MCP fixture coverage.
//!
//! Deny-first: every refusal fixture runs before the single success fixture, which is the
//! last test in this file.  A provider or model supplied route claim is evidence to compare,
//! never an authority; a reused downstream token cannot be re-labelled onto another account,
//! audience or purpose.

use std::collections::BTreeSet;

use kiana_domain::{
    DownstreamTokenAttestation, McpRouteAttestation, McpRouteTransport, ModelProtocol, ModelRoute,
    PromptPackAttestation, PromptPackTrust, ProviderRouteClaim, RouteAttestation,
    RouteAttestationContext, RouteDataPolicyBinding, RouteDataUseDecision, RouteVerificationStatus,
};

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const D: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const E: &str = "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const F: &str = "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

fn route() -> ModelRoute {
    ModelRoute {
        provider_id: "provider-fixture".to_owned(),
        protocol: ModelProtocol::OpenAiResponses,
        connection_id: "connection-fixture".to_owned(),
        model_id: "model-fixture".to_owned(),
        profile: "builder".to_owned(),
        configuration_revision: "config-revision-1".to_owned(),
        streaming: true,
    }
}

fn prompt(trust: PromptPackTrust) -> PromptPackAttestation {
    PromptPackAttestation::new("role-builder", "1.0.0", A, B, trust).expect("prompt fixture")
}

fn policy() -> RouteDataPolicyBinding {
    RouteDataPolicyBinding::new(
        C,
        4,
        7,
        "code-generation",
        BTreeSet::from(["internal".to_owned(), "confidential".to_owned()]),
    )
    .expect("policy fixture")
}

fn mcp_with(transport: McpRouteTransport, enabled: bool) -> McpRouteAttestation {
    McpRouteAttestation::new(
        "mcp-fixture",
        transport,
        D,
        "audience:mcp-fixture",
        "network:mcp-fixture",
        enabled,
    )
    .expect("mcp fixture")
}

fn mcp(transport: McpRouteTransport) -> McpRouteAttestation {
    mcp_with(transport, true)
}

fn downstream_token(account: &str, audience: &str, purpose: &str) -> DownstreamTokenAttestation {
    DownstreamTokenAttestation::new(D, account, audience, purpose).expect("downstream token")
}

fn context(
    trust: PromptPackTrust,
    mcp_route: Option<McpRouteAttestation>,
) -> RouteAttestationContext {
    RouteAttestationContext::new(
        route(),
        prompt(trust),
        mcp_route,
        policy(),
        E,
        "account-fixture",
        "audience:provider-fixture",
        A,
        F,
    )
    .expect("attestation context")
}

fn context_with_token(
    trust: PromptPackTrust,
    mcp_route: Option<McpRouteAttestation>,
) -> RouteAttestationContext {
    context(trust, mcp_route)
        .with_downstream_token(downstream_token(
            "account-fixture",
            "audience:provider-fixture",
            "code-generation",
        ))
        .expect("bound downstream token")
}

fn attestation(
    trust: PromptPackTrust,
    mcp_route: Option<McpRouteAttestation>,
    provider_verified: bool,
) -> RouteAttestation {
    let context = context(trust, mcp_route);
    RouteAttestation::new(
        context.clone(),
        ProviderRouteClaim::from_route(&context.route),
        provider_verified,
    )
    .expect("route attestation")
}

#[test]
fn provider_claim_and_route_drift_are_blocked_before_dispatch() {
    let expected = context(PromptPackTrust::Product, None);

    // Every provider/model field is inside the claim digest.  A provider that re-signs its
    // own claim after changing one field still cannot match the server-owned snapshot.
    for index in 0..7 {
        let mut forged_route = route();
        match index {
            0 => forged_route.provider_id = "other-provider".to_owned(),
            1 => forged_route.protocol = ModelProtocol::AnthropicMessages,
            2 => forged_route.connection_id = "other-connection".to_owned(),
            3 => forged_route.model_id = "other-model".to_owned(),
            4 => forged_route.profile = "other-profile".to_owned(),
            5 => forged_route.configuration_revision = "config-revision-2".to_owned(),
            6 => forged_route.streaming = false,
            _ => unreachable!("bounded loop"),
        }
        let claim = ProviderRouteClaim::from_route(&forged_route);
        let forged = RouteAttestation::new(expected.clone(), claim, true)
            .expect("forged provider claim is still a well-formed claim");
        let report = forged.verify(&expected).expect("blocked provider claim");
        assert_eq!(
            report.status,
            RouteVerificationStatus::Blocked,
            "provider claim field {index} must be blocked"
        );
        assert_eq!(report.reason, "route_attestation_provider_claim_mismatch");
    }

    // A claim that is changed without re-deriving its digest is not even well-formed.
    let mut stale_claim = ProviderRouteClaim::from_route(&route());
    stale_claim.model_id = "forged-model".to_owned();
    assert_eq!(
        stale_claim.validate().unwrap_err(),
        "route_claim_digest_mismatch"
    );

    // A downstream route token cannot be smuggled in as an unknown claim field.
    let mut value =
        serde_json::to_value(ProviderRouteClaim::from_route(&route())).expect("claim JSON");
    value["downstream_route_token"] = serde_json::json!("copied-token");
    assert!(serde_json::from_value::<ProviderRouteClaim>(value).is_err());

    // Route drift itself is refused before the claim is even compared.
    let mut drifted = expected.clone();
    drifted.route.connection_id = "other-connection".to_owned();
    drifted.route_digest = drifted.route.digest();
    let drifted_attestation = RouteAttestation::new(
        drifted.clone(),
        ProviderRouteClaim::from_route(&drifted.route),
        true,
    )
    .expect("route drift attestation");
    assert_eq!(
        drifted_attestation
            .verify(&expected)
            .expect("blocked route report")
            .reason,
        "route_attestation_route_mismatch"
    );

    let mut tampered = expected.clone();
    tampered.route_digest = F.to_owned();
    assert_eq!(
        tampered.validate().unwrap_err(),
        "route_attestation_route_digest_mismatch"
    );
}

#[test]
fn unreviewed_prompt_packs_and_unverified_providers_are_denied_or_unknown() {
    let untrusted = attestation(PromptPackTrust::Untrusted, None, true);
    let report = untrusted
        .verify(&untrusted.context)
        .expect("untrusted report");
    assert_eq!(report.status, RouteVerificationStatus::Blocked);
    assert_eq!(report.reason, "route_attestation_prompt_pack_untrusted");

    let revoked = attestation(PromptPackTrust::Revoked, None, true);
    assert_eq!(
        revoked
            .verify(&revoked.context)
            .expect("revoked report")
            .reason,
        "route_attestation_prompt_pack_untrusted"
    );

    let unknown = attestation(PromptPackTrust::Unknown, None, true);
    let report = unknown.verify(&unknown.context).expect("unknown report");
    assert_eq!(report.status, RouteVerificationStatus::Unknown);
    assert_eq!(report.reason, "route_attestation_prompt_pack_unknown");

    let unverified = attestation(PromptPackTrust::Product, None, false);
    let report = unverified
        .verify(&unverified.context)
        .expect("unverified report");
    assert_eq!(report.status, RouteVerificationStatus::Unknown);
    assert_eq!(report.reason, "route_attestation_provider_unverified");

    // A pack that no longer matches the reviewed content is a hard mismatch, not Unknown.
    let reviewed = context(PromptPackTrust::Signed, None);
    let mut drifted = reviewed.clone();
    drifted.prompt_pack =
        PromptPackAttestation::new("role-builder", "1.0.1", A, B, PromptPackTrust::Signed)
            .expect("drifted prompt pack");
    let drifted_attestation = RouteAttestation::new(
        drifted.clone(),
        ProviderRouteClaim::from_route(&drifted.route),
        true,
    )
    .expect("drifted prompt attestation");
    assert_eq!(
        drifted_attestation
            .verify(&reviewed)
            .expect("blocked prompt report")
            .reason,
        "route_attestation_prompt_pack_mismatch"
    );
}

#[test]
fn downstream_token_mixing_across_account_audience_and_purpose_is_refused() {
    // A token minted for another account, audience or purpose cannot be bound to this route.
    assert_eq!(
        context(PromptPackTrust::Product, None)
            .with_downstream_token(downstream_token(
                "other-account",
                "audience:provider-fixture",
                "code-generation",
            ))
            .unwrap_err(),
        "route_attestation_downstream_token_account_mismatch"
    );
    assert_eq!(
        context(PromptPackTrust::Product, None)
            .with_downstream_token(downstream_token(
                "account-fixture",
                "audience:other",
                "code-generation",
            ))
            .unwrap_err(),
        "route_attestation_downstream_token_audience_mismatch"
    );
    assert_eq!(
        context(PromptPackTrust::Product, None)
            .with_downstream_token(downstream_token(
                "account-fixture",
                "audience:provider-fixture",
                "other-purpose",
            ))
            .unwrap_err(),
        "route_attestation_downstream_token_purpose_mismatch"
    );

    // A token whose own scope digest does not match its fields is refused when bound.
    let mut tampered_token = downstream_token(
        "account-fixture",
        "audience:provider-fixture",
        "code-generation",
    );
    tampered_token.account_id = "other-account".to_owned();
    assert_eq!(
        tampered_token.validate().unwrap_err(),
        "route_downstream_token_digest_mismatch"
    );

    // Mixing is also refused on the wire: a decoded context whose token names another
    // account never validates, even after the token's own scope digest is re-derived.
    let mut context_value =
        serde_json::to_value(context_with_token(PromptPackTrust::Product, None))
            .expect("context JSON");
    context_value["downstream_token"]["account_id"] = serde_json::json!("other-account");
    let mut mixed_token: DownstreamTokenAttestation =
        serde_json::from_value(context_value["downstream_token"].clone()).expect("strict decode");
    mixed_token.token_scope_digest = mixed_token.digest();
    context_value["downstream_token"] =
        serde_json::to_value(&mixed_token).expect("re-signed token JSON");
    let decoded: RouteAttestationContext =
        serde_json::from_value(context_value).expect("strict decode");
    assert_eq!(
        decoded.validate().unwrap_err(),
        "route_attestation_downstream_token_account_mismatch"
    );

    // An attestation that drops the expected token, or presents another token reference for
    // the same scope, is blocked before dispatch.
    let expected = context_with_token(PromptPackTrust::Product, None);
    let without = RouteAttestation::new(
        context(PromptPackTrust::Product, None),
        ProviderRouteClaim::from_route(&route()),
        true,
    )
    .expect("attestation without downstream token");
    let report = without.verify(&expected).expect("missing token report");
    assert_eq!(report.status, RouteVerificationStatus::Blocked);
    assert_eq!(report.reason, "route_attestation_downstream_token_missing");

    let other_token = context(PromptPackTrust::Product, None)
        .with_downstream_token(
            DownstreamTokenAttestation::new(
                C,
                "account-fixture",
                "audience:provider-fixture",
                "code-generation",
            )
            .expect("well-formed other token"),
        )
        .expect("token binding refused");
    let swapped =
        RouteAttestation::new(other_token, ProviderRouteClaim::from_route(&route()), true)
            .expect("swapped token attestation");
    assert_eq!(
        swapped
            .verify(&expected)
            .expect("swapped token report")
            .reason,
        "route_attestation_downstream_token_mismatch"
    );

    // The token contract is strict: no fields may be added and no values extended.
    let value = serde_json::to_value(downstream_token(
        "account-fixture",
        "audience:provider-fixture",
        "code-generation",
    ))
    .expect("token JSON");
    let mut with_unknown = value.clone();
    with_unknown["token"] = serde_json::json!("raw-secret-value");
    assert!(serde_json::from_value::<DownstreamTokenAttestation>(with_unknown).is_err());
    let mut with_bad_header = value;
    with_bad_header["schema"] = serde_json::json!("kiana.route-attestation-downstream-token.v2");
    let decoded: DownstreamTokenAttestation =
        serde_json::from_value(with_bad_header).expect("strict decode");
    assert_eq!(
        decoded.validate().unwrap_err(),
        "route_downstream_token_header_invalid"
    );
}

#[test]
fn data_use_policy_denies_out_of_scope_purpose_and_class_escalation() {
    let binding = policy();
    let before = binding.digest();

    let permitted = binding
        .authorize_use(
            "code-generation",
            &BTreeSet::from(["internal".to_owned(), "confidential".to_owned()]),
        )
        .expect("permitted use");
    assert!(permitted.allowed);
    assert_eq!(permitted.reason, "ok");
    permitted.validate().expect("use decision digest");
    assert_eq!(permitted.policy_revision, 4);
    assert_eq!(permitted.data_epoch, 7);

    // The decision is deterministic and the binding is never mutated by a refusal.
    let again = binding
        .authorize_use(
            "code-generation",
            &BTreeSet::from(["confidential".to_owned(), "internal".to_owned()]),
        )
        .expect("permitted use again");
    assert_eq!(permitted.decision_digest, again.decision_digest);

    let denied = binding
        .authorize_use("marketing", &BTreeSet::from(["internal".to_owned()]))
        .expect("purpose refusal");
    assert!(!denied.allowed);
    assert_eq!(denied.reason, "route_attestation_purpose_not_permitted");
    let empty_purpose = binding
        .authorize_use("  ", &BTreeSet::from(["internal".to_owned()]))
        .expect("empty purpose refusal");
    assert_eq!(
        empty_purpose.reason,
        "route_attestation_purpose_not_permitted"
    );
    let empty_classes = binding
        .authorize_use("code-generation", &BTreeSet::new())
        .expect("empty class refusal");
    assert_eq!(
        empty_classes.reason,
        "route_attestation_data_classes_invalid"
    );
    let unknown_class = binding
        .authorize_use(
            "code-generation",
            &BTreeSet::from(["secret-ish".to_owned()]),
        )
        .expect("unknown class refusal");
    assert_eq!(unknown_class.reason, "route_attestation_data_class_unknown");
    let escalation = binding
        .authorize_use(
            "code-generation",
            &BTreeSet::from(["restricted".to_owned()]),
        )
        .expect("class escalation refusal");
    assert_eq!(
        escalation.reason,
        "route_attestation_data_class_not_permitted"
    );
    let oversized = binding
        .authorize_use(
            "code-generation",
            &BTreeSet::from([
                "public".to_owned(),
                "internal".to_owned(),
                "confidential".to_owned(),
                "restricted".to_owned(),
                "pii".to_owned(),
            ]),
        )
        .expect("oversized class set refusal");
    assert_eq!(oversized.reason, "route_attestation_data_classes_invalid");

    assert_eq!(
        binding.digest(),
        before,
        "refusals must not mutate the policy"
    );
    denied.validate().expect("refusal is still digest-bound");
    assert_eq!(
        binding
            .authorize_use("marketing", &BTreeSet::from(["internal".to_owned()]))
            .expect("purpose refusal again")
            .decision_digest,
        denied.decision_digest
    );

    // A refusal report cannot be re-labelled into an allowance.
    let mut forged_state = permitted.clone();
    forged_state.allowed = false;
    assert_eq!(
        forged_state.validate().unwrap_err(),
        "route_use_decision_state_invalid"
    );
    let mut forged_reason = permitted.clone();
    forged_reason.allowed = false;
    forged_reason.reason = "denied".to_owned();
    assert_eq!(
        forged_reason.validate().unwrap_err(),
        "route_use_decision_reason_invalid"
    );
    let mut forged_digest = permitted.clone();
    forged_digest.decision_digest = D.to_owned();
    assert_eq!(
        forged_digest.validate().unwrap_err(),
        "route_use_decision_digest_mismatch"
    );
    let value = serde_json::to_value(&permitted).expect("decision JSON");
    let mut with_unknown = value;
    with_unknown["prompt_text"] = serde_json::json!("raw prompt");
    assert!(serde_json::from_value::<RouteDataUseDecision>(with_unknown).is_err());
}

#[test]
fn policy_credential_account_audience_and_mcp_transport_drift_is_denied() {
    let stdio = attestation(
        PromptPackTrust::Product,
        Some(mcp(McpRouteTransport::Stdio)),
        true,
    );

    let mut policy_drift = stdio.context.clone();
    policy_drift.data_policy = RouteDataPolicyBinding::new(
        C,
        5,
        8,
        "different-purpose",
        BTreeSet::from(["restricted".to_owned()]),
    )
    .expect("policy drift");
    assert_eq!(
        stdio.verify(&policy_drift).unwrap().reason,
        "route_attestation_data_policy_mismatch"
    );

    let mut credential_drift = stdio.context.clone();
    credential_drift.credential_ref_digest = D.to_owned();
    assert_eq!(
        stdio.verify(&credential_drift).unwrap().reason,
        "route_attestation_credential_mismatch"
    );

    let mut account_drift = stdio.context.clone();
    account_drift.account_id = "other-account".to_owned();
    assert_eq!(
        stdio.verify(&account_drift).unwrap().reason,
        "route_attestation_account_mismatch"
    );

    let mut audience_drift = stdio.context.clone();
    audience_drift.audience = "audience:other".to_owned();
    assert_eq!(
        stdio.verify(&audience_drift).unwrap().reason,
        "route_attestation_audience_mismatch"
    );

    let mut release_drift = stdio.context.clone();
    release_drift.release_signature_attestation_digest = E.to_owned();
    assert_eq!(
        stdio.verify(&release_drift).unwrap().reason,
        "route_attestation_release_binding_mismatch"
    );

    // An MCP descriptor whose tool catalog changed is a different route.
    let mut catalog_drift_context = stdio.context.clone();
    catalog_drift_context.mcp_route = Some(
        McpRouteAttestation::new(
            "mcp-fixture",
            McpRouteTransport::Stdio,
            F,
            "audience:mcp-fixture",
            "network:mcp-fixture",
            true,
        )
        .expect("drifted catalog"),
    );
    let catalog_drift = RouteAttestation::new(
        catalog_drift_context.clone(),
        ProviderRouteClaim::from_route(&catalog_drift_context.route),
        true,
    )
    .expect("catalog drift attestation");
    assert_eq!(
        catalog_drift
            .verify(&stdio.context)
            .expect("catalog drift report")
            .reason,
        "route_attestation_mcp_route_mismatch"
    );

    let http_attestation = attestation(
        PromptPackTrust::Product,
        Some(mcp(McpRouteTransport::Http)),
        true,
    );
    assert_eq!(
        http_attestation
            .verify(&http_attestation.context)
            .unwrap()
            .reason,
        "route_attestation_mcp_transport_unsupported"
    );

    let disabled = attestation(
        PromptPackTrust::Product,
        Some(mcp_with(McpRouteTransport::Stdio, false)),
        true,
    );
    assert_eq!(
        disabled.verify(&disabled.context).unwrap().reason,
        "route_attestation_mcp_disabled"
    );

    let no_mcp = attestation(PromptPackTrust::Product, None, true);
    assert_eq!(
        stdio.verify(&no_mcp.context).unwrap().reason,
        "route_attestation_mcp_route_mismatch"
    );
}

#[test]
fn strict_serde_and_digest_fences_reject_forged_values() {
    let signed = attestation(PromptPackTrust::Signed, None, true);
    let mut value = serde_json::to_value(&signed).expect("attestation JSON");
    value["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<RouteAttestation>(value).is_err());

    let mut forged = signed.clone();
    forged.context.credential_ref_digest = D.to_owned();
    assert_eq!(
        forged.validate().unwrap_err(),
        "route_attestation_digest_mismatch"
    );

    let mut claim = ProviderRouteClaim::from_route(&route());
    claim.model_id = "forged-model".to_owned();
    assert_eq!(claim.validate().unwrap_err(), "route_claim_digest_mismatch");

    let mut mcp_digest = mcp(McpRouteTransport::Stdio);
    mcp_digest.tool_catalog_digest = A.to_owned();
    assert_eq!(
        mcp_digest.validate().unwrap_err(),
        "route_mcp_digest_mismatch"
    );

    let mut policy_use = policy();
    policy_use.purpose = "forged-purpose".to_owned();
    assert_eq!(
        policy_use.validate().unwrap_err(),
        "route_policy_use_digest_mismatch"
    );

    let mut prompt_header = prompt(PromptPackTrust::Signed);
    prompt_header.schema = "kiana.route-attestation-prompt.v2".to_owned();
    assert_eq!(
        prompt_header.validate().unwrap_err(),
        "route_prompt_header_invalid"
    );

    // The report is digest-bound evidence too: a re-labelled reason is refused.
    let report = signed.verify(&signed.context).expect("verified report");
    report.validate().expect("report digest");
    let mut forged_report = report.clone();
    forged_report.route_digest = D.to_owned();
    assert_eq!(
        forged_report.validate().unwrap_err(),
        "route_report_digest_mismatch"
    );

    // A context that carries a downstream token round-trips without degradation.
    let tokenized = context_with_token(PromptPackTrust::Signed, None);
    let value = serde_json::to_value(&tokenized).expect("tokenized context JSON");
    let decoded: RouteAttestationContext =
        serde_json::from_value(value).expect("tokenized context decode");
    assert_eq!(decoded, tokenized);
    decoded.validate().expect("tokenized context validates");
}

#[test]
fn exact_provider_model_prompt_policy_and_mcp_binding_verifies() {
    let context = context_with_token(PromptPackTrust::Signed, Some(mcp(McpRouteTransport::Stdio)));
    let attestation = RouteAttestation::new(
        context.clone(),
        ProviderRouteClaim::from_route(&context.route),
        true,
    )
    .expect("route attestation");
    attestation.validate().expect("attestation digest");

    let report = attestation.verify(&context).expect("route report");
    assert_eq!(report.status, RouteVerificationStatus::Verified);
    assert_eq!(report.reason, "ok");
    assert_eq!(report.route_digest, context.route_digest);
    assert_eq!(
        report.prompt_pack_digest,
        context.prompt_pack.content_digest
    );
    assert_eq!(
        report.mcp_route_digest.as_deref(),
        context
            .mcp_route
            .as_ref()
            .map(|route| route.route_digest.as_str())
    );
    assert_eq!(report.data_policy_digest, context.data_policy.policy_digest);
    report.validate().expect("report digest");

    let repeat = attestation.verify(&context).expect("route report again");
    assert_eq!(report, repeat, "verification must be deterministic");

    let value = serde_json::to_value(&attestation).expect("attestation JSON");
    let decoded: RouteAttestation = serde_json::from_value(value).expect("attestation decode");
    assert_eq!(decoded, attestation);
}
