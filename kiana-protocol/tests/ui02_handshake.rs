use kiana_protocol::*;
use serde_json::{json, Value};

fn capability(id: &str, enabled: bool, scope: &str) -> UiCapability {
    UiCapability {
        schema: UI_CAPABILITY_SCHEMA.to_owned(),
        capability_id: id.to_owned(),
        enabled,
        actions: vec![format!("{id}.read"), format!("{id}.write")],
        reason: None,
        scope_digest: kiana_domain::json_digest(&json!({"scope": scope})),
    }
}

#[test]
fn handshake_capabilities_are_the_principal_surface_intersection() {
    let principal = vec![
        capability("run", true, "run"),
        capability("admin", true, "admin"),
    ];
    let surface = vec![
        capability("run", true, "run"),
        capability("admin", true, "other"),
    ];
    let request = vec![
        UiCapabilityRequest {
            capability_id: "run".to_owned(),
            feature_version: "1".to_owned(),
        },
        UiCapabilityRequest {
            capability_id: "admin".to_owned(),
            feature_version: "1".to_owned(),
        },
    ];
    let result = intersect_ui_capabilities(&principal, &surface, &request).unwrap();
    assert!(result[0].enabled);
    assert!(!result[1].enabled);
    assert_eq!(
        result[1].reason.as_deref(),
        Some("ui_scope_intersection_empty")
    );
}

#[test]
fn handshake_and_health_are_versioned_and_unknown_fields_fail_closed() {
    let request = UiHandshakeRequest {
        schema: UI_HANDSHAKE_REQUEST_SCHEMA.to_owned(),
        client_version: "1.0".to_owned(),
        surface: UiSurface::Cli,
        requested_capabilities: Vec::new(),
        known_instance_id: None,
        known_authority_epoch: None,
    };
    request.validate().unwrap();
    let mut forged = serde_json::to_value(&request).unwrap();
    forged["unknown"] = json!(true);
    assert!(serde_json::from_value::<UiHandshakeRequest>(forged).is_err());

    let health = UiHealth {
        schema: UI_HEALTH_SCHEMA.to_owned(),
        instance_id: "instance-1".to_owned(),
        authority_epoch: 1,
        status: "ready".to_owned(),
        capabilities: vec!["run".to_owned()],
        limitations: Vec::new(),
    };
    health.validate().unwrap();
}

#[test]
fn stable_error_mapping_keeps_policy_denial_and_unknown_distinct() {
    let unknown = ResponseEnvelope {
        schema: PROTOCOL_SCHEMA.to_owned(),
        request_id: RequestId::new(),
        status: ExecutionStatus::ResultUnknown,
        output: Value::Null,
        error: Some("result_unknown:transport_lost".to_owned()),
    };
    let mapped = stable_error_from_response(&unknown).unwrap();
    assert_eq!(mapped.code, UiErrorCode::Unknown);
    assert_eq!(mapped.retry, UiRetryDisposition::QueryOriginal);

    let denied = ResponseEnvelope {
        schema: PROTOCOL_SCHEMA.to_owned(),
        request_id: RequestId::new(),
        status: ExecutionStatus::Blocked,
        output: Value::Null,
        error: Some("permission_denied".to_owned()),
    };
    let mapped = stable_error_from_response(&denied).unwrap();
    assert_eq!(mapped.code, UiErrorCode::PermissionDenied);
    assert_eq!(mapped.retry, UiRetryDisposition::DoNotRetry);
}
