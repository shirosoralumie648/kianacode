use kiana_domain::{ConnectorSurface, ConnectorSurfaceQuery, ConnectorSurfaceQueryKind};
use kiana_protocol::{ConnectorSurfaceCursor, CONNECTOR_SURFACE_QUERY_SCHEMA};

#[test]
fn connector_surface_query_wire_round_trips_without_authority_fields() {
    let query = ConnectorSurfaceQuery {
        schema: CONNECTOR_SURFACE_QUERY_SCHEMA.to_owned(),
        surface: ConnectorSurface::Mcp,
        kind: ConnectorSurfaceQueryKind::Health,
        binding_id: "binding-1".to_owned(),
        scope_digest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .to_owned(),
        source_cursor: Some(4),
        after_item: 0,
        limit: 20,
        cursor: None,
        read_only: true,
    };
    query.validate().unwrap();
    let encoded = serde_json::to_value(&query).unwrap();
    assert_eq!(encoded["surface"], "mcp");
    assert!(serde_json::from_value::<ConnectorSurfaceQuery>(encoded).is_ok());

    let cursor = ConnectorSurfaceCursor {
        schema: kiana_domain::CONNECTOR_SURFACE_CURSOR_SCHEMA.to_owned(),
        surface: ConnectorSurface::Mcp,
        kind: ConnectorSurfaceQueryKind::Health,
        binding_id: "binding-1".to_owned(),
        scope_digest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .to_owned(),
        source_cursor: 4,
        after_item: 1,
        cursor_digest: String::new(),
    };
    let _ = cursor;
}
