use kiana_domain::{
    json_digest, ModelCallPermit, ModelCallSpec, ModelProtocol, ModelPurpose, ModelRequest,
    ModelResponseFormat, ModelRoute, PreparedModelCall, RequestId, RunId, TokenBudget,
    MODEL_CALL_SCHEMA,
};

fn prepared() -> PreparedModelCall {
    let mut prepared = PreparedModelCall {
        schema: MODEL_CALL_SCHEMA.to_owned(),
        spec: ModelCallSpec {
            call_id: RequestId::new(),
            attempt_id: RequestId::new(),
            model_attempt_id: None,
            step_id: None,
            step: 1,
            purpose: ModelPurpose::Task,
            assignment: None,
            response_format: ModelResponseFormat::Text,
            replay: Vec::new(),
            deadline_unix_ms: 9_999_999_999,
        },
        route: ModelRoute {
            provider_id: "fake-provider".to_owned(),
            protocol: ModelProtocol::OpenAiChat,
            connection_id: "default".to_owned(),
            model_id: "fake-model".to_owned(),
            profile: "default".to_owned(),
            configuration_revision: "sha256:config".to_owned(),
            streaming: false,
        },
        request: ModelRequest {
            messages: vec![kiana_domain::ModelMessage::user("CI08 request")],
            tools: Vec::new(),
            sandbox: "read-only".to_owned(),
        },
        wire_body: serde_json::json!({"model":"fake-model","messages":[]}),
        request_hash: String::new(),
        budget: TokenBudget::new(32, 0, 0, 16, 4_096),
        tool_catalog_hash: kiana_domain::tool_catalog_hash(&[]),
        provider_account: Some(json_digest(&serde_json::json!({
            "provider": "fake-provider",
            "connection": "default"
        }))),
        credential_revision: Some(json_digest(&serde_json::json!("CI08_SECRET_SENTINEL"))),
    };
    prepared.seal();
    prepared.validate().expect("prepared call");
    prepared
}

fn permit(prepared: &PreparedModelCall) -> ModelCallPermit {
    ModelCallPermit {
        schema: "kiana.model-call-permit.v1".to_owned(),
        permit_id: RequestId::new(),
        run_id: RunId::new(),
        attempt_id: prepared.spec.attempt_id,
        request_hash: prepared.request_hash.clone(),
        expires_at_unix_ms: 9_999_999_999,
        route_digest: Some(prepared.route.digest()),
        configuration_revision: Some(prepared.route.configuration_revision.clone()),
        authority_revision: None,
        credential_revision: prepared.credential_revision.clone(),
        provider_account: prepared.provider_account.clone(),
    }
}

#[test]
fn route_admission_accepts_exact_binding_but_rejects_revision_drift() {
    let prepared = prepared();
    let permit = permit(&prepared);
    permit
        .validate_for_prepared(&prepared, 1_000)
        .expect("exact route binding");

    let mut route_drift = permit.clone();
    route_drift.route_digest = Some(json_digest(&serde_json::json!("different-route")));
    assert_eq!(
        route_drift
            .validate_for_prepared(&prepared, 1_000)
            .unwrap_err()
            .code,
        "model_route_admission_drift"
    );

    let mut configuration_drift = permit.clone();
    configuration_drift.configuration_revision = Some("sha256:other-config".to_owned());
    assert_eq!(
        configuration_drift
            .validate_for_prepared(&prepared, 1_000)
            .unwrap_err()
            .code,
        "model_route_admission_drift"
    );

    let mut credential_drift = permit.clone();
    credential_drift.credential_revision = Some(json_digest(&serde_json::json!("rotated")));
    assert_eq!(
        credential_drift
            .validate_for_prepared(&prepared, 1_000)
            .unwrap_err()
            .code,
        "model_route_admission_drift"
    );

    let mut account_drift = permit.clone();
    account_drift.provider_account = Some(json_digest(&serde_json::json!("other-account")));
    assert_eq!(
        account_drift
            .validate_for_prepared(&prepared, 1_000)
            .unwrap_err()
            .code,
        "model_route_admission_drift"
    );

    let mut authority_drift = permit;
    authority_drift.authority_revision = Some("sha256:authority".to_owned());
    assert_eq!(
        authority_drift
            .validate_for_prepared(&prepared, 1_000)
            .unwrap_err()
            .code,
        "model_authority_revision_drift"
    );
}

#[test]
fn route_admission_rejects_expired_or_missing_binding_without_effect() {
    let prepared = prepared();
    let mut expired = permit(&prepared);
    expired.expires_at_unix_ms = 1_000;
    assert_eq!(
        expired
            .validate_for_prepared(&prepared, 1_000)
            .unwrap_err()
            .code,
        "model_permit_scope_or_expiry_mismatch"
    );

    let mut missing = permit(&prepared);
    missing.provider_account = None;
    assert_eq!(
        missing
            .validate_for_prepared(&prepared, 1_000)
            .unwrap_err()
            .code,
        "model_route_admission_drift"
    );

    let encoded = serde_json::to_string(&permit(&prepared)).expect("permit json");
    assert!(!encoded.contains("CI08_SECRET_SENTINEL"));
}
