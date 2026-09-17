use kiana_domain::{CredentialDisplayStatus, SecretRef};
use kiana_protocol::{
    ProviderCredentialProbeRequest, ProviderCredentialProbeResponse, ProviderUsePolicyEffect,
    ProviderUsePolicyView,
};

fn revision() -> String {
    format!("sha256:{}", "a".repeat(64))
}

fn secret_ref() -> SecretRef {
    SecretRef::new(
        "env",
        "OPENAI_API_KEY",
        "provider.use",
        "provider:openai",
        1,
    )
    .unwrap()
}

#[test]
fn probe_request_round_trips_without_raw_credential_material() {
    let request = ProviderCredentialProbeRequest::new(
        "openai",
        Some(secret_ref()),
        vec!["files.read".to_owned(), "model.invoke".to_owned()],
        Some(revision()),
    )
    .unwrap();
    assert_eq!(request.requested_scopes, ["files.read", "model.invoke"]);
    let encoded = serde_json::to_value(&request).unwrap();
    let text = encoded.to_string();
    assert!(!text.contains("super-secret"));
    assert_eq!(
        serde_json::from_value::<ProviderCredentialProbeRequest>(encoded).unwrap(),
        request
    );

    let mut unknown = serde_json::to_value(request).unwrap();
    unknown["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ProviderCredentialProbeRequest>(unknown).is_err());
}

#[test]
fn probe_response_is_digest_bound_and_scope_status_is_read_only() {
    let response = ProviderCredentialProbeResponse::new(
        "openai",
        CredentialDisplayStatus::ScopeInsufficient,
        Some(&secret_ref()),
        Some(1),
        Some(1_900_000_000_000),
        vec!["model.invoke".to_owned()],
        revision(),
        "missing_scope",
        1_800_000_000_000,
    )
    .unwrap();
    response.validate().unwrap();
    let encoded = serde_json::to_value(&response).unwrap();
    assert!(!encoded.to_string().contains("super-secret"));
    assert_eq!(
        serde_json::from_value::<ProviderCredentialProbeResponse>(encoded).unwrap(),
        response.clone()
    );

    let mut tampered = response.clone();
    tampered.reason = "changed".to_owned();
    assert!(tampered.validate().is_err());

    let mut unknown = serde_json::to_value(response).unwrap();
    unknown["token"] = serde_json::json!("raw-token");
    assert!(serde_json::from_value::<ProviderCredentialProbeResponse>(unknown).is_err());
}

#[test]
fn policy_view_round_trips_with_presence_only_fields() {
    let view = ProviderUsePolicyView::new(
        "openai",
        "provider.use",
        ProviderUsePolicyEffect::Deny,
        CredentialDisplayStatus::ReauthRequired,
        "provider_reauth_required",
        revision(),
        Some("reauth".to_owned()),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_value::<ProviderUsePolicyView>(serde_json::to_value(&view).unwrap())
            .unwrap(),
        view
    );
}
