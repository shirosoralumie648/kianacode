use kiana_protocol::{
    OpsAuthoritySnapshot, OpsCommand, OpsCommandRequest, OpsEnvelope, OpsEnvelopeBody, OpsError,
    OpsErrorCode, OpsEvent, OpsEventKind, OpsQuery, OpsQueryRequest, OpsReplayDisposition,
    OpsScope, OpsUnknownEnvelope, RequestEnvelope, RequestMetadata,
};
use serde_json::json;

/// `Uuid::from_u128(value)` 的规范连字符形式。
///
/// `kiana-protocol` 不直接依赖 `uuid` crate，因此这里按 UUID 的
/// `8-4-4-4-12` 十六进制布局展开确定性的测试标识，再交给
/// 每个标识类型自带的 `parse_str` 构造。
fn uuid_from_u128(value: u128) -> String {
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        (value >> 96) as u32,
        ((value >> 80) & 0xffff) as u16,
        ((value >> 64) & 0xffff) as u16,
        ((value >> 48) & 0xffff) as u16,
        value & 0xffff_ffff_ffff
    )
}

fn project() -> kiana_protocol::ProjectId {
    kiana_protocol::ProjectId::parse_str(&uuid_from_u128(1)).expect("project id")
}

fn root() -> kiana_protocol::StorageRootId {
    kiana_protocol::StorageRootId::parse_str(&uuid_from_u128(2)).expect("storage root id")
}

fn authority(actor: &str, epoch: u64) -> OpsAuthoritySnapshot {
    OpsAuthoritySnapshot {
        actor_id: actor.to_owned(),
        authority_epoch: epoch,
        scope: OpsScope::new(Some(project()), Some(root())),
    }
}

fn command() -> OpsCommandRequest {
    OpsCommandRequest::new(
        OpsCommand::Status,
        kiana_protocol::OperationId::parse_str(&uuid_from_u128(3)).expect("operation id"),
        "op-key-1",
        authority("operator-a", 7),
        json!({"include_evidence": true}),
    )
    .unwrap()
}

#[test]
fn ops_command_query_event_error_unknown_envelopes_round_trip() {
    let command = command();
    let operation_id = command.operation_id;
    command.validate().unwrap();
    let query = OpsQueryRequest::new(
        OpsQuery::Status,
        kiana_protocol::RequestId::parse_str(&uuid_from_u128(4)).expect("query request id"),
        authority("operator-a", 7),
        None,
        json!({}),
    )
    .unwrap();
    let event = OpsEvent::new(
        "ops.operation.accepted",
        OpsEventKind::Accepted,
        operation_id,
        1,
        json!({"replayed": false}),
    )
    .unwrap();
    let error = OpsError::new(
        OpsErrorCode::ResultUnknown,
        "query the original operation",
        false,
        Some(operation_id),
    )
    .unwrap();
    let unknown = OpsUnknownEnvelope::new(
        "ops.future.command",
        json!({"opaque": true}),
        "unsupported by this client",
    )
    .unwrap();
    for body in [
        OpsEnvelopeBody::Command(command),
        OpsEnvelopeBody::Query(query),
        OpsEnvelopeBody::Event(event),
        OpsEnvelopeBody::Error(error),
        OpsEnvelopeBody::Unknown(unknown),
    ] {
        let envelope = OpsEnvelope::new(
            kiana_protocol::RequestId::parse_str(&uuid_from_u128(5)).expect("envelope request id"),
            body,
        )
        .unwrap();
        assert_eq!(
            serde_json::from_str::<OpsEnvelope>(&serde_json::to_string(&envelope).unwrap())
                .unwrap(),
            envelope
        );
    }
}

#[test]
fn forged_authority_unknown_command_scope_and_idempotency_conflict_fail_closed() {
    let request = command();
    let expected = authority("operator-a", 7);
    request.validate_authority(&expected).unwrap();

    let mut forged_actor = request.clone();
    forged_actor.authority.actor_id = "operator-b".to_owned();
    forged_actor.request_digest = forged_actor.digest();
    assert_eq!(
        forged_actor.validate_authority(&expected).unwrap_err(),
        "ops_actor_mismatch"
    );

    let mut forged_epoch = request.clone();
    forged_epoch.authority.authority_epoch = 8;
    forged_epoch.request_digest = forged_epoch.digest();
    assert_eq!(
        forged_epoch.validate_authority(&expected).unwrap_err(),
        "ops_authority_epoch_mismatch"
    );

    let mut unknown = request.clone();
    unknown.command = "ops.future".to_owned();
    unknown.request_digest = unknown.digest();
    assert_eq!(unknown.validate().unwrap_err(), "ops_unknown_command");

    let mut out_of_scope = request.clone();
    out_of_scope.authority.scope = OpsScope::new(Some(project()), None);
    out_of_scope.request_digest = out_of_scope.digest();
    assert_eq!(
        out_of_scope.validate_authority(&expected).unwrap_err(),
        "ops_scope_mismatch"
    );

    assert_eq!(
        request.replay_against(Some(&request)).unwrap(),
        OpsReplayDisposition::Replay
    );
    let mut changed = request.clone();
    changed.payload = json!({"include_evidence": false});
    changed.request_digest = changed.digest();
    assert_eq!(
        changed.replay_against(Some(&request)).unwrap_err(),
        "ops_idempotency_conflict"
    );

    let mut changed_key = request.clone();
    changed_key.idempotency_key = "op-key-2".to_owned();
    changed_key.request_digest = changed_key.digest();
    assert_eq!(
        changed_key.replay_against(Some(&request)).unwrap_err(),
        "ops_idempotency_conflict"
    );

    let envelope =
        RequestEnvelope::ops_command(RequestMetadata::local("s", "/tmp/project"), request).unwrap();
    assert_eq!(envelope.schema, kiana_protocol::PROTOCOL_SCHEMA);
}
