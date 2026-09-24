use kiana_domain::{
    ExtensionCommand, ExtensionCommandError, ExtensionCommandErrorCode, ExtensionCommandRequest,
    ExtensionCommandResponse, EXTENSION_COMMAND_SCHEMA,
};

#[test]
fn read_only_commands_have_no_mutation_authority() {
    let mut request = ExtensionCommandRequest::new(ExtensionCommand::List);
    assert!(request.validate().is_ok());
    let arguments = request.to_arguments().expect("list arguments");
    assert_eq!(arguments["action"], "list");
    assert_eq!(arguments["schema"], EXTENSION_COMMAND_SCHEMA);

    request.extension_id = Some("example.skill".to_owned());
    request.actor_id = Some("operator".to_owned());
    assert_eq!(
        request.validate().unwrap_err(),
        "extension_command_read_only_mutation_fields"
    );
}

#[test]
fn mutation_requires_operator_reason_idempotency_and_expected_version() {
    let mut request = ExtensionCommandRequest::new(ExtensionCommand::Enable);
    request.extension_id = Some("example.skill".to_owned());
    assert_eq!(
        request.validate().unwrap_err(),
        "extension_command_mutation_fields_required"
    );

    request.expected_registry_version = Some(4);
    request.actor_id = Some("operator".to_owned());
    request.reason = Some("enable after review".to_owned());
    request.idempotency_key = Some("enable-example-4".to_owned());
    assert!(request.validate().is_ok());
    assert_eq!(request.to_arguments().unwrap()["action"], "enable");
}

#[test]
fn package_commands_bind_project_relative_path_and_hash() {
    let mut request = ExtensionCommandRequest::new(ExtensionCommand::Install);
    request.extension_id = Some("example.skill".to_owned());
    request.expected_registry_version = Some(1);
    request.actor_id = Some("operator".to_owned());
    request.reason = Some("install reviewed package".to_owned());
    request.idempotency_key = Some("install-example-1".to_owned());
    assert_eq!(
        request.validate().unwrap_err(),
        "extension_command_package_required"
    );
    request.package_path = Some("packages/example.json".to_owned());
    request.package_sha256 = Some("a".repeat(64));
    assert!(request.validate().is_ok());
    request.package_path = Some("../escape.json".to_owned());
    assert_eq!(
        request.validate().unwrap_err(),
        "extension_command_package_path_invalid"
    );
}

#[test]
fn structured_error_and_response_reject_unknown_or_unbound_data() {
    let error = ExtensionCommandError::new(
        ExtensionCommandErrorCode::ApprovalRequired,
        "mutation approval required",
        true,
    );
    assert!(error.validate().is_ok());
    let response = ExtensionCommandResponse {
        schema: EXTENSION_COMMAND_SCHEMA.to_owned(),
        command: ExtensionCommand::Enable,
        registry_version: 4,
        snapshot_digest: None,
        receipt: None,
        error: Some(error),
    };
    assert!(response.validate().is_ok());
    let mut encoded = serde_json::to_value(response).unwrap();
    encoded["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ExtensionCommandResponse>(encoded).is_err());
}
