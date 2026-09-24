//! provider/model route, prompt-pack, data policy, credential and MCP fixture coverage.

use std::collections::BTreeSet;

use kiana_domain::{
    McpRouteAttestation, McpRouteTransport, ModelProtocol, ModelRoute, PromptPackAttestation,
    PromptPackTrust, ProviderRouteClaim, RouteAttestation, RouteAttestationContext,
    RouteDataPolicyBinding, RouteVerificationStatus,
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

fn mcp(transport: McpRouteTransport) -> McpRouteAttestation {
    McpRouteAttestation::new(
        "mcp-fixture",
        transport,
        D,
        "audience:mcp-fixture",
        "network:mcp-fixture",
        true,
    )
    .expect("mcp fixture")
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
fn exact_provider_model_prompt_policy_and_mcp_binding_verifies() {
    let attestation = attestation(
        PromptPackTrust::Signed,
        Some(mcp(McpRouteTransport::Stdio)),
        true,
    );
    let expected = attestation.context.clone();
    let report = attestation.verify(&expected).expect("route report");
    assert_eq!(report.status, RouteVerificationStatus::Verified);
    assert_eq!(report.reason, "ok");
    report.validate().expect("report digest");
}

#[test]
fn provider_claim_and_route_drift_are_blocked_before_dispatch() {
    let attestation = attestation(PromptPackTrust::Product, None, true);
    let expected = attestation.context.clone();

    let mut forged_route = route();
    forged_route.model_id = "other-model".to_owned();
    let forged_claim = ProviderRouteClaim::from_route(&forged_route);
    let forged = RouteAttestation::new(attestation.context.clone(), forged_claim, true)
        .expect("well-formed forged provider claim");
    let report = forged.verify(&expected).expect("blocked report");
    assert_eq!(report.status, RouteVerificationStatus::Blocked);
    assert_eq!(report.reason, "route_attestation_provider_claim_mismatch");

    let changed_context = context(PromptPackTrust::Product, None);
    let mut route_drift = changed_context.route.clone();
    route_drift.connection_id = "other-connection".to_owned();
    let changed_context = RouteAttestationContext::new(
        route_drift.clone(),
        changed_context.prompt_pack,
        changed_context.mcp_route,
        changed_context.data_policy,
        changed_context.credential_ref_digest,
        changed_context.account_id,
        changed_context.audience,
        changed_context.release_manifest_digest,
        changed_context.release_signature_attestation_digest,
    )
    .expect("well-formed route drift");
    let route_drift_attestation = RouteAttestation::new(
        changed_context.clone(),
        ProviderRouteClaim::from_route(&route_drift),
        true,
    )
    .expect("route drift attestation");
    let report = route_drift_attestation
        .verify(&expected)
        .expect("blocked route report");
    assert_eq!(report.reason, "route_attestation_route_mismatch");
}

#[test]
fn untrusted_unknown_and_unverified_claims_stay_blocked_or_unknown() {
    let untrusted = attestation(PromptPackTrust::Untrusted, None, true);
    let report = untrusted
        .verify(&untrusted.context)
        .expect("untrusted report");
    assert_eq!(report.status, RouteVerificationStatus::Blocked);
    assert_eq!(report.reason, "route_attestation_prompt_pack_untrusted");

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
}

#[test]
fn policy_credential_account_audience_and_mcp_transport_drift_is_denied() {
    let attestation = attestation(
        PromptPackTrust::Product,
        Some(mcp(McpRouteTransport::Stdio)),
        true,
    );

    let mut policy_drift = attestation.context.clone();
    policy_drift.data_policy = RouteDataPolicyBinding::new(
        C,
        5,
        8,
        "different-purpose",
        BTreeSet::from(["restricted".to_owned()]),
    )
    .expect("policy drift");
    assert_eq!(
        attestation.verify(&policy_drift).unwrap().reason,
        "route_attestation_data_policy_mismatch"
    );

    let mut credential_drift = attestation.context.clone();
    credential_drift.credential_ref_digest = D.to_owned();
    assert_eq!(
        attestation.verify(&credential_drift).unwrap().reason,
        "route_attestation_credential_mismatch"
    );

    let mut account_drift = attestation.context.clone();
    account_drift.account_id = "other-account".to_owned();
    assert_eq!(
        attestation.verify(&account_drift).unwrap().reason,
        "route_attestation_account_mismatch"
    );

    let mut audience_drift = attestation.context.clone();
    audience_drift.audience = "audience:other".to_owned();
    assert_eq!(
        attestation.verify(&audience_drift).unwrap().reason,
        "route_attestation_audience_mismatch"
    );

    let http_attestation = attestation(
        PromptPackTrust::Product,
        Some(mcp(McpRouteTransport::Http)),
        true,
    );
    assert_eq!(
        http_attestation.verify(&http_attestation.context).unwrap().reason,
        "route_attestation_mcp_transport_unsupported"
    );
}

#[test]
fn strict_serde_and_digest_fences_reject_forged_values() {
    let attestation = attestation(PromptPackTrust::Signed, None, true);
    let mut value = serde_json::to_value(&attestation).expect("attestation JSON");
    value["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<RouteAttestation>(value).is_err());

    let mut forged = attestation.clone();
    forged.context.credential_ref_digest = D.to_owned();
    assert_eq!(
        forged.validate().unwrap_err(),
        "route_attestation_digest_mismatch"
    );

    let mut claim = ProviderRouteClaim::from_route(&route());
    claim.model_id = "forged-model".to_owned();
    assert_eq!(claim.validate().unwrap_err(), "route_claim_digest_mismatch");

    let no_mcp = attestation(PromptPackTrust::Product, None, true);
    let mcp_attestation = attestation(
        PromptPackTrust::Product,
        Some(mcp(McpRouteTransport::Stdio)),
        true,
    );
    assert_eq!(
        mcp_attestation.verify(&no_mcp.context).unwrap().reason,
        "route_attestation_mcp_route_mismatch"
    );
}
