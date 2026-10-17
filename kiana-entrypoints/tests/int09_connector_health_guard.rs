use kiana_domain::{
    json_digest, ConnectorHealthFact, ConnectorHealthStatus, CONNECTOR_FIXTURE_SOURCE,
};
use kiana_entrypoints::workbench_render::render_connector_health;
use serde_json::json;

fn health_entry(
    binding_id: &str,
    status: ConnectorHealthStatus,
    error_code: Option<&str>,
    stale: bool,
    source_cursor: u64,
) -> serde_json::Value {
    let evidence = matches!(status, ConnectorHealthStatus::Verified)
        .then(|| json_digest(&json!({"probe": binding_id})));
    let health = ConnectorHealthFact::new(
        "connector-demo",
        binding_id,
        status,
        "read_only",
        1_700_000_000_000,
        evidence,
        error_code.map(str::to_owned),
        1,
        None,
        CONNECTOR_FIXTURE_SOURCE,
        vec!["local_fixture_only".to_owned()],
    )
    .expect("valid health fact");
    json!({
        "health": health,
        "source_cursor": source_cursor,
        "projection_version": "connector-health.v1",
        "binding_revision": 1,
        "stale": stale,
        "proof_level": "source",
        "limitations": ["local_fixture_only", "external_provider_not_probed"],
    })
}

#[test]
fn workbench_health_renders_every_class_without_provider_material() {
    let statuses = [
        (ConnectorHealthStatus::Verified, None),
        (ConnectorHealthStatus::ConnectivityOnly, None),
        (
            ConnectorHealthStatus::CredentialInvalid,
            Some("invalid_token"),
        ),
        (
            ConnectorHealthStatus::ScopeInsufficient,
            Some("missing_scope"),
        ),
        (
            ConnectorHealthStatus::EndpointUnreachable,
            Some("connection_refused"),
        ),
        (
            ConnectorHealthStatus::ProviderError,
            Some("provider_specific"),
        ),
        (
            ConnectorHealthStatus::Unsupported,
            Some("transport_unsupported"),
        ),
    ];
    let entries = statuses
        .iter()
        .enumerate()
        .map(|(index, (status, error_code))| {
            health_entry(
                &format!("binding-{index}"),
                *status,
                *error_code,
                index == 2,
                index as u64 + 2,
            )
        })
        .collect::<Vec<_>>();
    let projection = json!({
        "schema": "kiana.connector-health-projection.v1",
        "source_cursor": 32,
        "projection_version": "connector-health.v1",
        "entries": entries,
        "limitations": [],
    });

    let rows = render_connector_health(&projection).expect("render");
    assert_eq!(rows.len(), statuses.len());
    let rendered = rows
        .iter()
        .map(|row| row.status.as_str())
        .collect::<Vec<_>>();
    for (status, _) in statuses {
        assert!(
            rendered.contains(&status.as_str()),
            "missing rendered class: {}",
            status.as_str()
        );
    }
    assert!(rows[2].stale);
    assert_eq!(
        rows[0].limitations,
        vec![
            "local_fixture_only".to_owned(),
            "external_provider_not_probed".to_owned()
        ]
    );
    let serialized = serde_json::to_string(&projection).unwrap();
    for forbidden in [
        "raw body",
        "credential_value",
        "secret_value",
        "Authorization",
    ] {
        assert!(!serialized.contains(forbidden));
        assert!(!format!("{rows:?}").contains(forbidden));
    }
}

#[test]
fn workbench_health_rejects_unknown_oversized_and_duplicate_projection_rows() {
    let entry = health_entry(
        "binding-demo",
        ConnectorHealthStatus::ConnectivityOnly,
        None,
        false,
        2,
    );
    let projection = json!({
        "schema": "kiana.connector-health-projection.v1",
        "source_cursor": 2,
        "projection_version": "connector-health.v1",
        "entries": [entry.clone()],
        "limitations": [],
    });
    assert!(render_connector_health(&projection).is_ok());

    let mut unknown = projection.clone();
    unknown["raw_provider_error"] = json!("401 invalid token");
    assert!(render_connector_health(&unknown).is_err());

    let mut future_row = projection.clone();
    future_row["entries"][0]["source_cursor"] = json!(3);
    assert!(render_connector_health(&future_row).is_err());

    let mut duplicate = projection;
    duplicate["entries"] = json!([entry.clone(), entry]);
    assert!(render_connector_health(&duplicate).is_err());
}

#[test]
fn workbench_health_is_a_bounded_display_of_the_typed_projection() {
    let render = include_str!("../src/workbench_render.rs");
    let chat = include_str!("../src/workbench_chat.rs");
    let client = include_str!("../../kiana-client/src/lib.rs");
    let protocol = include_str!("../../kiana-protocol/src/ui_contracts.rs");

    for marker in [
        "UiConnectorHealthProjection",
        "render_connector_health",
        "connector_health_ui_too_large",
        "entry.health.binding_id",
        "entry.health.connector_id",
        "entry.health.status",
        "stale",
        "limitations",
        "/connector-health",
        "connector.health",
        "is_connector_health",
        "connector_health_projection_missing",
        "UI_CONNECTOR_HEALTH_SCHEMA",
        "ConnectorHealthFact",
    ] {
        assert!(
            render.contains(marker)
                || chat.contains(marker)
                || client.contains(marker)
                || protocol.contains(marker),
            "INT-09 UI marker missing: {marker}"
        );
    }

    for forbidden in [
        "std::process::Command",
        "tokio::spawn",
        "reqwest::",
        "std::fs::",
        "raw_provider_error",
        "credential_value",
        "secret_value",
    ] {
        assert!(
            !render.contains(forbidden),
            "INT-09 UI renderer crossed a side-effect or secret boundary: {forbidden}"
        );
    }
}
