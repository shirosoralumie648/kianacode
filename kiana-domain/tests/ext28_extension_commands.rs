//! EXT-28 command contract fixtures.
//!
//! Deny paths come first: a read-only command may not carry lifecycle binding fields, and a
//! mutation is refused unless it carries the operator, reason, idempotency key, expected
//! registry version and — for install/enable — the typed lifecycle binding the server issued
//! through a read-only inspect. Only then is the encoded argument object compared with the
//! shared `extension.manage` route, and the committed receipt decoded back into the versioned
//! DTO. Local tests are not run in this session (CI-only); the manual lane is
//! `.github/workflows/ext28-extension-commands.yml`.

use kiana_domain::{
    ExtensionCommand, ExtensionCommandError, ExtensionCommandErrorCode, ExtensionCommandReceipt,
    ExtensionCommandRequest, ExtensionCommandResponse, ExtensionConfigEntry, ExtensionConfigScope,
    ExtensionConfigSnapshot, EXTENSION_COMMAND_RECEIPT_SCHEMA, EXTENSION_COMMAND_SCHEMA,
};

const TRUST_REVISION: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const DEPENDENCY_DIGEST: &str =
    "sha256:2222222222222222222222222222222222222222222222222222222222222222";

fn operator_fields(request: &mut ExtensionCommandRequest, version: u64) {
    request.expected_registry_version = Some(version);
    request.actor_id = Some("operator".to_owned());
    request.reason = Some("EXT-28 command fixture".to_owned());
    request.idempotency_key = Some(format!("ext28-{}", request.command.action()));
}

fn config_snapshot(extension_id: &str, registry_version: u64) -> ExtensionConfigSnapshot {
    ExtensionConfigSnapshot::new(
        extension_id,
        registry_version,
        vec![ExtensionConfigEntry {
            key: "greeting".to_owned(),
            scope: ExtensionConfigScope::Run,
            source_ref: "run:fixture".to_owned(),
            value_digest: format!("sha256:{}", "3".repeat(64)),
            secret_handle: None,
        }],
    )
    .expect("fixture config snapshot")
}

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

    // Read-only commands may not smuggle any lifecycle binding either: inspect is a pure read
    // of the server-issued binding, not a way to carry one into the registry.
    for request in [
        {
            let mut request = ExtensionCommandRequest::new(ExtensionCommand::List);
            request.trust_revision = Some(TRUST_REVISION.to_owned());
            request
        },
        {
            let mut request = ExtensionCommandRequest::new(ExtensionCommand::List);
            request.dependency_snapshot_digest = Some(DEPENDENCY_DIGEST.to_owned());
            request
        },
        {
            let mut request = ExtensionCommandRequest::new(ExtensionCommand::Inspect);
            request.extension_id = Some("example.skill".to_owned());
            request.approval_id = Some("approval.ext28".to_owned());
            request
        },
        {
            let mut request = ExtensionCommandRequest::new(ExtensionCommand::Inspect);
            request.extension_id = Some("example.skill".to_owned());
            request.config_snapshot_digest = Some(TRUST_REVISION.to_owned());
            request
        },
        {
            let mut request = ExtensionCommandRequest::new(ExtensionCommand::Inspect);
            request.extension_id = Some("example.skill".to_owned());
            request.config_snapshot = Some(config_snapshot("example.skill", 1));
            request
        },
    ] {
        assert_eq!(
            request.validate().unwrap_err(),
            "extension_command_read_only_mutation_fields"
        );
    }
}

#[test]
fn mutation_requires_operator_reason_idempotency_and_expected_version() {
    let mut request = ExtensionCommandRequest::new(ExtensionCommand::Enable);
    request.extension_id = Some("example.skill".to_owned());
    assert_eq!(
        request.validate().unwrap_err(),
        "extension_command_mutation_fields_required"
    );

    operator_fields(&mut request, 4);
    assert_eq!(
        request.validate().unwrap_err(),
        "extension_command_lifecycle_binding_required"
    );
    request.trust_revision = Some(TRUST_REVISION.to_owned());
    request.dependency_snapshot_digest = Some(DEPENDENCY_DIGEST.to_owned());
    assert_eq!(
        request.validate().unwrap_err(),
        "extension_command_approval_required"
    );
    request.approval_id = Some("approval.ext28".to_owned());
    assert_eq!(
        request.validate().unwrap_err(),
        "extension_command_config_snapshot_digest_required"
    );
    request.config_snapshot_digest = Some(TRUST_REVISION.to_owned());
    assert_eq!(
        request.validate().unwrap_err(),
        "extension_command_config_snapshot_required"
    );

    let snapshot = config_snapshot("example.skill", 4);
    request.config_snapshot_digest = Some(snapshot.snapshot_digest.clone());
    request.config_snapshot = Some(snapshot);
    assert!(request.validate().is_ok());
    assert_eq!(request.to_arguments().unwrap()["action"], "enable");

    // A mutation addressed at one extension cannot consume another extension's snapshot, and a
    // snapshot minted for another registry generation cannot be replayed onto this one.
    let mut drifted = request.clone();
    drifted.config_snapshot = Some(config_snapshot("other.skill", 4));
    drifted.config_snapshot_digest = drifted
        .config_snapshot
        .as_ref()
        .map(|snapshot| snapshot.snapshot_digest.clone());
    assert_eq!(
        drifted.validate().unwrap_err(),
        "extension_command_config_snapshot_extension_mismatch"
    );
    let mut stale = request.clone();
    let stale_snapshot = config_snapshot("example.skill", 3);
    stale.config_snapshot_digest = Some(stale_snapshot.snapshot_digest.clone());
    stale.config_snapshot = Some(stale_snapshot);
    assert_eq!(
        stale.validate().unwrap_err(),
        "extension_command_config_snapshot_registry_version_mismatch"
    );
}

#[test]
fn package_commands_bind_project_relative_path_and_hash() {
    let mut request = ExtensionCommandRequest::new(ExtensionCommand::Install);
    request.extension_id = Some("example.skill".to_owned());
    operator_fields(&mut request, 1);
    assert_eq!(
        request.validate().unwrap_err(),
        "extension_command_package_required"
    );
    request.package_path = Some("packages/example.json".to_owned());
    request.package_sha256 = Some("a".repeat(64));
    assert_eq!(
        request.validate().unwrap_err(),
        "extension_command_lifecycle_binding_required"
    );
    request.package_path = Some("../escape.json".to_owned());
    assert_eq!(
        request.validate().unwrap_err(),
        "extension_command_package_path_invalid"
    );
    request.package_path = Some("packages/example.json".to_owned());
    request.package_sha256 = Some("A".repeat(64));
    assert_eq!(
        request.validate().unwrap_err(),
        "extension_command_package_hash_invalid"
    );
    request.trust_revision = Some(TRUST_REVISION.to_owned());
    request.dependency_snapshot_digest = Some(DEPENDENCY_DIGEST.to_owned());
    assert!(request.validate().is_ok());

    // Rollback carries the same package binding but no install-time lifecycle snapshot.
    let mut rollback = ExtensionCommandRequest::new(ExtensionCommand::Rollback);
    rollback.extension_id = Some("example.skill".to_owned());
    operator_fields(&mut rollback, 2);
    rollback.package_path = Some("packages/example.json".to_owned());
    rollback.package_sha256 = Some("b".repeat(64));
    assert!(rollback.validate().is_ok());
    assert_eq!(rollback.to_arguments().unwrap()["action"], "rollback");
}

#[test]
fn binding_fields_are_bounded_and_encoded_for_the_shared_route() {
    let snapshot = config_snapshot("example.skill", 2);
    let mut request = ExtensionCommandRequest::new(ExtensionCommand::Enable);
    request.extension_id = Some("example.skill".to_owned());
    operator_fields(&mut request, 2);
    request.trust_revision = Some(TRUST_REVISION.to_owned());
    request.dependency_snapshot_digest = Some(DEPENDENCY_DIGEST.to_owned());
    request.approval_id = Some("approval.ext28".to_owned());
    request.config_snapshot_digest = Some(snapshot.snapshot_digest.clone());
    request.config_snapshot = Some(snapshot.clone());
    assert!(request.validate().is_ok());

    // The encoded arguments stay inside the shared route's key whitelist and carry exactly the
    // server-issued binding, so the ControlPlane can re-derive the mutation digest.
    let arguments = request.to_arguments().expect("enable arguments");
    for key in [
        "action",
        "schema",
        "version",
        "extension_id",
        "expected_registry_version",
        "idempotency_key",
        "reason",
        "trust_revision",
        "dependency_snapshot_digest",
        "approval_id",
        "config_snapshot_digest",
        "config_snapshot",
    ] {
        assert!(arguments.get(key).is_some(), "missing encoded key {key}");
    }
    assert_eq!(arguments["trust_revision"], TRUST_REVISION);
    assert_eq!(
        arguments["config_snapshot"]["snapshot_digest"],
        snapshot.snapshot_digest
    );

    // Shape failures are rejected before any byte reaches the route: a digest that is not the
    // server `sha256:<hex>` spelling, a malformed approval id and an unbound snapshot body.
    let mut bad_trust = request.clone();
    bad_trust.trust_revision = Some("trust:stale-revision".to_owned());
    assert_eq!(
        bad_trust.validate().unwrap_err(),
        "extension_command_trust_revision_invalid"
    );
    let mut bad_dependency = request.clone();
    bad_dependency.dependency_snapshot_digest = Some("sha256:not-a-digest".to_owned());
    assert_eq!(
        bad_dependency.validate().unwrap_err(),
        "extension_command_dependency_snapshot_invalid"
    );
    let mut bad_config_digest = request.clone();
    bad_config_digest.config_snapshot_digest = Some("3".repeat(64));
    assert_eq!(
        bad_config_digest.validate().unwrap_err(),
        "extension_command_config_snapshot_digest_invalid"
    );
    let mut bad_approval = request.clone();
    bad_approval.approval_id = Some("approval id".to_owned());
    assert_eq!(
        bad_approval.validate().unwrap_err(),
        "extension_command_approval_id_invalid"
    );

    // Digest drift between the carried digest and the snapshot body is a local refusal.
    let mut mismatched = request.clone();
    mismatched.config_snapshot_digest = Some(DEPENDENCY_DIGEST.to_owned());
    assert_eq!(
        mismatched.validate().unwrap_err(),
        "extension_command_config_snapshot_digest_mismatch"
    );

    // `install` needs the base mutation fields too: expected version 0 is not a valid precondition.
    let mut zero = ExtensionCommandRequest::new(ExtensionCommand::Install);
    zero.extension_id = Some("example.skill".to_owned());
    operator_fields(&mut zero, 0);
    zero.package_path = Some("packages/example.json".to_owned());
    zero.package_sha256 = Some("c".repeat(64));
    zero.trust_revision = Some(TRUST_REVISION.to_owned());
    zero.dependency_snapshot_digest = Some(DEPENDENCY_DIGEST.to_owned());
    assert_eq!(
        zero.validate().unwrap_err(),
        "extension_command_expected_registry_version_invalid"
    );
}

#[test]
fn committed_receipt_is_decoded_and_rejects_unbound_data() {
    let receipt = ExtensionCommandReceipt {
        schema: EXTENSION_COMMAND_RECEIPT_SCHEMA.to_owned(),
        command: ExtensionCommand::Enable,
        extension_id: "example.skill".to_owned(),
        actor_id: "operator".to_owned(),
        reason: "EXT-28 command fixture".to_owned(),
        idempotency_key: "ext28-enable".to_owned(),
        expected_registry_version: 4,
        registry_version: 5,
        mutation_digest: Some(TRUST_REVISION.to_owned()),
        approval_id: Some("approval.ext28".to_owned()),
        event_id: "event-28".to_owned(),
        replayed: false,
    };
    receipt.validate().expect("fixture receipt");
    let output = serde_json::json!({
        "schema": EXTENSION_COMMAND_SCHEMA,
        "command": "enable",
        "registry_version": 5,
        "command_receipt": receipt,
    });
    let decoded =
        ExtensionCommandReceipt::from_response_output(&output).expect("decoded committed receipt");
    // The daemon emits the same shape, so the decoded projection must round-trip unchanged.
    let emitted: ExtensionCommandReceipt =
        serde_json::from_value(output["command_receipt"].clone()).expect("daemon-shaped receipt");
    assert_eq!(decoded, emitted);
    assert_eq!(decoded.event_id, "event-28");
    assert!(!decoded.replayed);

    // Missing or re-bound receipts are refused instead of being re-derived locally.
    assert_eq!(
        ExtensionCommandReceipt::from_response_output(&serde_json::json!({"schema": "x"}))
            .unwrap_err(),
        "extension_command_receipt_missing"
    );
    let mut unknown = output.clone();
    unknown["command_receipt"]["unbound"] = serde_json::json!(true);
    assert_eq!(
        ExtensionCommandReceipt::from_response_output(&unknown).unwrap_err(),
        "extension_command_receipt_invalid"
    );
    let mut regressed = output.clone();
    regressed["command_receipt"]["registry_version"] = serde_json::json!(4);
    assert_eq!(
        ExtensionCommandReceipt::from_response_output(&regressed).unwrap_err(),
        "extension_command_receipt_invalid"
    );
    let mut not_a_digest = output.clone();
    not_a_digest["command_receipt"]["mutation_digest"] = serde_json::json!("mutation");
    assert_eq!(
        ExtensionCommandReceipt::from_response_output(&not_a_digest).unwrap_err(),
        "extension_command_receipt_invalid"
    );
    let mut unsupported = output;
    unsupported["command_receipt"]["command"] = serde_json::json!("upgrade");
    assert_eq!(
        ExtensionCommandReceipt::from_response_output(&unsupported).unwrap_err(),
        "extension_command_receipt_invalid"
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

    let request = ExtensionCommandRequest::new(ExtensionCommand::List);
    let mut encoded = serde_json::to_value(&request).unwrap();
    encoded["trust_revision"] = serde_json::json!(TRUST_REVISION);
    let decoded: ExtensionCommandRequest = serde_json::from_value(encoded).unwrap();
    assert_eq!(
        decoded.validate().unwrap_err(),
        "extension_command_read_only_mutation_fields"
    );
}
