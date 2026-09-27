use kiana_domain::{
    ConnectorSurface, ConnectorSurfaceItem, ConnectorSurfaceQuery, ConnectorSurfaceQueryKind,
    ConnectorSurfaceResponse, Freshness, CONNECTOR_SURFACE_QUERY_SCHEMA,
    CONNECTOR_SURFACE_RESPONSE_SCHEMA,
};
use serde_json::json;

const D1: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const D2: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn query() -> ConnectorSurfaceQuery {
    ConnectorSurfaceQuery {
        schema: CONNECTOR_SURFACE_QUERY_SCHEMA.to_owned(),
        surface: ConnectorSurface::Cli,
        kind: ConnectorSurfaceQueryKind::Reconciliation,
        binding_id: "binding-1".to_owned(),
        scope_digest: D1.to_owned(),
        source_cursor: Some(10),
        after_item: 0,
        limit: 20,
        cursor: None,
        read_only: true,
    }
}

#[test]
fn one_query_shape_is_shared_by_all_four_surfaces() {
    for surface in [
        ConnectorSurface::Cli,
        ConnectorSurface::Web,
        ConnectorSurface::Workbench,
        ConnectorSurface::Mcp,
    ] {
        let mut query = query();
        query.surface = surface;
        query.validate().unwrap();
    }
}

#[test]
fn response_cursor_limitation_and_secret_fences_are_explicit() {
    let item = ConnectorSurfaceItem {
        item_id: "notification:one".to_owned(),
        dedup_key: "connector:one".to_owned(),
        title: "Connector needs review".to_owned(),
        summary: "Provider result is unknown".to_owned(),
        evidence_digest: D1.to_owned(),
        source_cursor: 10,
        unknown: true,
    };
    let mut response = ConnectorSurfaceResponse {
        schema: CONNECTOR_SURFACE_RESPONSE_SCHEMA.to_owned(),
        surface: ConnectorSurface::Workbench,
        kind: ConnectorSurfaceQueryKind::Reconciliation,
        binding_id: "binding-1".to_owned(),
        scope_digest: D1.to_owned(),
        source_cursor: 10,
        projection_digest: D2.to_owned(),
        freshness: Freshness::Current,
        after_item: 0,
        items: vec![item],
        limitations: vec!["manual evidence required".to_owned()],
        next_cursor: None,
        response_digest: String::new(),
    };
    response.response_digest = kiana_domain::json_digest(&json!({
        "schema": response.schema,
        "surface": response.surface,
        "kind": response.kind,
        "binding_id": response.binding_id,
        "scope_digest": response.scope_digest,
        "source_cursor": response.source_cursor,
        "projection_digest": response.projection_digest,
        "freshness": response.freshness,
        "after_item": response.after_item,
        "items": response.items,
        "limitations": response.limitations,
        "next_cursor": response.next_cursor,
    }));
    response.validate().unwrap();

    let mut secret = response.clone();
    secret.items[0].summary = "Authorization: Bearer abc".to_owned();
    secret.response_digest = kiana_domain::json_digest(&json!({
        "schema": secret.schema,
        "surface": secret.surface,
        "kind": secret.kind,
        "binding_id": secret.binding_id,
        "scope_digest": secret.scope_digest,
        "source_cursor": secret.source_cursor,
        "projection_digest": secret.projection_digest,
        "freshness": secret.freshness,
        "after_item": secret.after_item,
        "items": secret.items,
        "limitations": secret.limitations,
        "next_cursor": secret.next_cursor,
    }));
    assert_eq!(
        secret.validate().unwrap_err(),
        "connector_surface_item_invalid"
    );

    let mut forged = query();
    forged.read_only = false;
    assert_eq!(
        forged.validate().unwrap_err(),
        "connector_surface_query_invalid"
    );
    let mut value = serde_json::to_value(query()).unwrap();
    value["actor_id"] = json!("forged");
    assert!(serde_json::from_value::<ConnectorSurfaceQuery>(value).is_err());
}
