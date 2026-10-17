use kiana_domain::{
    ModelProtocol, ModelRoute, ProtectedReplayMaterial, ProtectedReplayScope, ProtectedReplayStore,
    RequestId,
};

fn route() -> ModelRoute {
    ModelRoute {
        provider_id: "fixture".to_owned(),
        protocol: ModelProtocol::OpenAiResponses,
        connection_id: "connection-a".to_owned(),
        model_id: "model-a".to_owned(),
        profile: "default".to_owned(),
        configuration_revision: "config.v1".to_owned(),
        streaming: true,
    }
}

fn material(route: &ModelRoute) -> ProtectedReplayMaterial {
    ProtectedReplayMaterial::new(
        ProtectedReplayScope {
            connection_id: route.connection_id.clone(),
            protocol: route.protocol,
            model_id: route.model_id.clone(),
            route_digest: route.digest(),
            effort: Some("medium".to_owned()),
            prompt_digest: "sha256:prompt".to_owned(),
            tool_catalog_digest: "sha256:tools".to_owned(),
            data_revision: "data.v1".to_owned(),
            source_call_id: RequestId::new(),
            expires_at_unix_ms: 20_000,
        },
        b"private reasoning signature",
    )
    .expect("valid protected material")
}

#[test]
fn signed_reasoning_survives_tool_continuation_byte_for_byte_in_process() {
    let route = route();
    let mut store = ProtectedReplayStore::default();
    let material = material(&route);
    let reference = store.insert("artifact-1", material).expect("insert");
    let restored = store.get(&reference, &route, 1_000).expect("read");
    assert_eq!(restored.bytes(), b"private reasoning signature");
}

#[test]
fn replay_scope_and_deletion_fail_closed() {
    let route = route();
    let mut store = ProtectedReplayStore::default();
    let reference = store
        .insert("artifact-1", material(&route))
        .expect("insert");
    let mut foreign = route.clone();
    foreign.connection_id = "connection-b".to_owned();
    assert_eq!(
        store.get(&reference, &foreign, 1_000).unwrap_err().code,
        "replay_material_cannot_cross_connection_or_model_scope"
    );
    assert!(store.delete("artifact-1"));
    assert_eq!(
        store.get(&reference, &route, 1_000).unwrap_err().code,
        "missing_reasoning_material_blocks_resume"
    );
}

#[test]
fn expired_replay_material_is_not_resumable() {
    let route = route();
    let mut store = ProtectedReplayStore::default();
    let reference = store
        .insert("artifact-1", material(&route))
        .expect("insert");
    assert_eq!(
        store.get(&reference, &route, 20_000).unwrap_err().code,
        "protected_replay_expired"
    );
}

#[test]
fn public_replay_summary_is_digest_only_and_cannot_resume() {
    let route = route();
    let material = material(&route);
    let summary = material.public_summary("artifact-1").expect("summary");
    assert_eq!(summary.schema, kiana_domain::PUBLIC_REPLAY_SUMMARY_SCHEMA);
    assert_eq!(summary.redaction, kiana_domain::PUBLIC_REPLAY_REDACTION);
    assert_eq!(summary.material_sha256, material.digest());
    assert!(summary.material_bytes > 0);
    let json = serde_json::to_string(&summary).expect("summary is public metadata");
    assert!(!json.contains("private reasoning"));
    assert_eq!(
        summary.resume().unwrap_err().code,
        "public_replay_summary_is_not_resume_material"
    );

    // The private material itself is not serializable and never prints its bytes.
    let debug = format!("{material:?}");
    assert!(!debug.contains("private reasoning"));
    assert!(debug.contains("byte_len"));
}

#[test]
fn material_is_bound_to_the_declared_reasoning_kind() {
    let route = route();
    let scope = ProtectedReplayScope {
        connection_id: route.connection_id.clone(),
        protocol: route.protocol,
        model_id: route.model_id.clone(),
        route_digest: route.digest(),
        effort: Some("medium".to_owned()),
        prompt_digest: "sha256:prompt".to_owned(),
        tool_catalog_digest: "sha256:tools".to_owned(),
        data_revision: "data.v1".to_owned(),
        source_call_id: RequestId::new(),
        expires_at_unix_ms: 20_000,
    };
    // A Gemini thought payload cannot be bound to a Responses scope.
    assert_eq!(
        ProtectedReplayMaterial::new_with_kind(
            scope.clone(),
            Some(kiana_domain::ReasoningReplayKind::GeminiThoughtSignature),
            b"private".to_vec(),
        )
        .unwrap_err()
        .code,
        "protected_replay_kind_protocol_mismatch"
    );

    let material = ProtectedReplayMaterial::new_with_kind(
        scope,
        Some(kiana_domain::ReasoningReplayKind::OpenAiResponsesReasoningItem),
        b"private".to_vec(),
    )
    .expect("kind-bound material");
    let declared = material.declared_kind();
    assert_eq!(
        declared,
        Some(kiana_domain::ReasoningReplayKind::OpenAiResponsesReasoningItem)
    );
    let mut store = ProtectedReplayStore::default();
    let reference = store.insert("artifact-1", material).expect("insert");
    assert_eq!(reference.kind, declared);
    assert!(store.get(&reference, &route, 1_000).is_ok());

    // A reference that widens the TTL beyond the stored scope is refused.
    let mut widened = reference.clone();
    widened.expires_at_unix_ms = 20_001;
    assert_eq!(
        store.get(&widened, &route, 1_000).unwrap_err().code,
        "protected_replay_ttl_widened"
    );
}

#[test]
fn model_switch_revokes_route_bound_material() {
    let route = route();
    let mut store = ProtectedReplayStore::default();
    let reference = store
        .insert("artifact-1", material(&route))
        .expect("insert");

    let mut switched = route.clone();
    switched.model_id = "model-b".to_owned();
    assert_eq!(store.invalidate_route_change(&switched), 1);
    assert_eq!(store.len(), 0);
    assert_eq!(store.revocation_reason("artifact-1"), Some("model_switch"));
    assert_eq!(
        store.get(&reference, &route, 1_000).unwrap_err().code,
        "replay_material_revoked"
    );

    // An operator revocation records its own reason and is idempotent enough to report it.
    let mut store = ProtectedReplayStore::default();
    let reference = store
        .insert("artifact-2", material(&route))
        .expect("insert");
    store
        .revoke("artifact-2", "data_governance_revocation")
        .expect("revoke");
    assert_eq!(
        store.revocation_reason("artifact-2"),
        Some("data_governance_revocation")
    );
    assert_eq!(
        store.get(&reference, &route, 1_000).unwrap_err().code,
        "replay_material_revoked"
    );
    assert_eq!(
        store
            .revoke("artifact-3", "operator_revocation")
            .unwrap_err()
            .code,
        "protected_replay_artifact_unknown"
    );
}

#[test]
fn compaction_boundary_keeps_continuations_out_of_the_summary() {
    let route = route();
    let mut assistant = kiana_domain::ModelMessage::assistant("tool turn");
    assistant.continuation = Some(kiana_domain::ProviderContinuation {
        schema: kiana_domain::PROVIDER_CONTINUATION_SCHEMA.to_owned(),
        provider_id: route.provider_id.clone(),
        protocol: route.protocol,
        route_digest: route.digest(),
        item_ref: "artifact-1".to_owned(),
    });
    let messages = vec![
        kiana_domain::ModelMessage::user("question"),
        assistant,
        kiana_domain::ModelMessage::tool("call-1", "{\"ok\":true}"),
    ];
    let boundary = kiana_domain::ProtectedHistoryBoundary::split(&messages);
    assert_eq!(boundary.compactable.len(), 2);
    assert_eq!(boundary.protected.len(), 1);
    assert_eq!(boundary.compaction_input().len(), 2);
    assert_eq!(
        boundary.resume_after_compaction().unwrap_err().code,
        "protected_continuation_not_recoverable_from_compacted_view"
    );
    assert_eq!(boundary.notices.len(), 1);
    assert!(!boundary.notices[0].contains("artifact-1"));

    // A history without any protected continuation is fully compactable.
    let plain = vec![kiana_domain::ModelMessage::user("question")];
    let boundary = kiana_domain::ProtectedHistoryBoundary::split(&plain);
    assert_eq!(boundary.compactable.len(), 1);
    assert!(boundary.resume_after_compaction().is_ok());
}
