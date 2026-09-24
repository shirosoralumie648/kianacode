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
