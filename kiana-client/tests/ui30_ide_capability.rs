use kiana_client::{
    IdeCapabilityAdapter, IdeCapabilityError, IdeCapabilityPermit, IdeCapabilityRequest, IdeIntent,
    IdeOperation, IDE_CAPABILITY_SCHEMA, IDE_PERMIT_SCHEMA,
};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn request(operation: IdeOperation) -> IdeCapabilityRequest {
    IdeCapabilityRequest {
        schema: IDE_CAPABILITY_SCHEMA.to_owned(),
        operation,
        workspace_digest: digest('a'),
        relative_path: "src/main.rs".to_owned(),
        path_digest: digest('b'),
        expected_revision: 1,
        idempotency_key: "ide-30-action".to_owned(),
        artifact_id: None,
    }
}

fn permit(
    operation: IdeOperation,
    effect_allowed: bool,
    expires_at_unix_ms: u64,
) -> IdeCapabilityPermit {
    IdeCapabilityPermit {
        schema: IDE_PERMIT_SCHEMA.to_owned(),
        permit_id: "permit-30".to_owned(),
        operation,
        workspace_digest: digest('a'),
        path_digest: digest('b'),
        expected_revision: 1,
        owner_digest: digest('c'),
        permit_digest: digest('d'),
        expires_at_unix_ms,
        effect_allowed,
    }
}

#[test]
fn fixture_declares_ide_deny_first_contract() {
    let value: Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ui30-ide-capability.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(value["schema"], "kiana.ide-capability-fixture.v1");
    assert!(value["denied"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str().unwrap().contains("spawn")));
}

#[test]
fn capability_matrix_and_read_queries_do_not_grant_direct_effect() {
    let adapter = IdeCapabilityAdapter::new(digest('a'), digest('c')).unwrap();
    let matrix = adapter.capability_matrix().unwrap();
    assert_eq!(matrix.len(), 2);
    assert!(matrix
        .iter()
        .all(|row| !row.direct_effect && row.delegated_to_kiana));
    for operation in [
        IdeOperation::OpenFile,
        IdeOperation::ReadFile,
        IdeOperation::PatchPreview,
    ] {
        assert!(matches!(
            adapter.query(request(operation)).unwrap(),
            IdeIntent::Query(_)
        ));
    }
    assert!(matches!(
        adapter.query(IdeCapabilityRequest {
            relative_path: "../secret".to_owned(),
            ..request(IdeOperation::ReadFile)
        }),
        Err(IdeCapabilityError::PathInvalid)
    ));
    assert!(matches!(
        adapter.query(IdeCapabilityRequest {
            relative_path: "src/./main.rs".to_owned(),
            ..request(IdeOperation::ReadFile)
        }),
        Err(IdeCapabilityError::PathInvalid)
    ));
}

#[test]
fn apply_and_terminal_cancel_require_exact_live_permit() {
    let adapter = IdeCapabilityAdapter::new(digest('a'), digest('c')).unwrap();
    let apply = adapter
        .apply(
            request(IdeOperation::ApplyPatch),
            permit(IdeOperation::ApplyPatch, true, 101),
            100,
        )
        .unwrap();
    match apply {
        IdeIntent::Action(action) => action.validate().unwrap(),
        IdeIntent::Query(_) => panic!("apply must be a server action"),
    }
    assert!(matches!(
        adapter.apply(
            request(IdeOperation::ApplyPatch),
            permit(IdeOperation::ApplyPatch, false, 101),
            100
        ),
        Err(IdeCapabilityError::DirectEffectForbidden)
    ));
    assert!(matches!(
        adapter.apply(
            request(IdeOperation::ApplyPatch),
            permit(IdeOperation::ApplyPatch, true, 100),
            100
        ),
        Err(IdeCapabilityError::PermitExpired)
    ));
    assert!(matches!(
        adapter.apply(
            request(IdeOperation::ApplyPatch),
            permit(IdeOperation::ReadFile, true, 101),
            100
        ),
        Err(IdeCapabilityError::PermitInvalid("binding"))
    ));
    let cancel = adapter
        .cancel_terminal(
            request(IdeOperation::CancelTerminal),
            permit(IdeOperation::CancelTerminal, true, 101),
            100,
        )
        .unwrap();
    assert!(matches!(cancel, IdeIntent::Action(_)));
}

#[test]
fn terminal_output_is_bounded_and_never_spawns() {
    let adapter = IdeCapabilityAdapter::new(digest('a'), digest('c')).unwrap();
    assert!(matches!(
        adapter.terminal_output(request(IdeOperation::TerminalOutput), 128),
        Ok(IdeIntent::Query(_))
    ));
    assert_eq!(
        adapter.terminal_output(request(IdeOperation::TerminalOutput), 256 * 1024 + 1),
        Err(IdeCapabilityError::OutputBoundExceeded)
    );
}

#[test]
fn source_boundary_has_no_host_execution_primitives() {
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ide_capability.rs"))
            .unwrap();
    assert!(!source.contains("std::fs"));
    assert!(!source.contains("Command::"));
    assert!(!source.contains(".spawn("));
    assert!(source.contains("UiActionV1"));
    assert!(source.contains("permit"));
}
