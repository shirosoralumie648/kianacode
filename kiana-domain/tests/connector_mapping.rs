use kiana_domain::{
    ArtifactId, ArtifactProvenance, ArtifactVersion, ConnectorInputArtifact, ConnectorMappedObject,
    ConnectorMappedPage, ConnectorObjectMapping, ConnectorPageCursor, ARTIFACT_REF_SCHEMA,
    CONNECTOR_INPUT_ARTIFACT_SCHEMA, CONNECTOR_MAPPED_PAGE_SCHEMA, CONNECTOR_MAPPING_SCHEMA,
    CONNECTOR_PAGE_CURSOR_SCHEMA,
};
use serde_json::json;
use std::collections::BTreeMap;

const D1: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const D2: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SCOPE: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn mapping() -> ConnectorObjectMapping {
    let mut mapping = ConnectorObjectMapping {
        schema: CONNECTOR_MAPPING_SCHEMA.to_owned(),
        version: 1,
        connector_id: "connector-1".to_owned(),
        binding_id: "binding-1".to_owned(),
        account_id: "account-1".to_owned(),
        operation: "list".to_owned(),
        input_schema_digest: D1.to_owned(),
        output_schema_digest: D2.to_owned(),
        fields: BTreeMap::from([
            ("external_id".to_owned(), "id".to_owned()),
            ("external_name".to_owned(), "name".to_owned()),
        ]),
        provenance_digest: D1.to_owned(),
        mapping_digest: String::new(),
    };
    mapping.mapping_digest = kiana_domain::json_digest(&json!({
        "schema": mapping.schema,
        "version": mapping.version,
        "connector_id": mapping.connector_id,
        "binding_id": mapping.binding_id,
        "account_id": mapping.account_id,
        "operation": mapping.operation,
        "input_schema_digest": mapping.input_schema_digest,
        "output_schema_digest": mapping.output_schema_digest,
        "fields": mapping.fields,
        "provenance_digest": mapping.provenance_digest,
    }));
    mapping
}

fn artifact() -> kiana_domain::ArtifactRef {
    ArtifactVersion::new(
        ArtifactId::new(),
        1,
        "application/json",
        br#"{"input":true}"#,
        SCOPE,
        ArtifactProvenance {
            producer_kind: "connector".to_owned(),
            producer_id: "connector-1".to_owned(),
            source_event_id: None,
            source_run_id: None,
            recorded_by: "server".to_owned(),
        },
        1,
    )
    .unwrap()
    .as_ref()
}

fn cursor(mapping: &ConnectorObjectMapping) -> ConnectorPageCursor {
    let mut cursor = ConnectorPageCursor {
        schema: CONNECTOR_PAGE_CURSOR_SCHEMA.to_owned(),
        connector_id: mapping.connector_id.clone(),
        binding_id: mapping.binding_id.clone(),
        account_id: mapping.account_id.clone(),
        operation: mapping.operation.clone(),
        scope_digest: SCOPE.to_owned(),
        mapping_digest: mapping.mapping_digest.clone(),
        source_cursor: 10,
        page_token_digest: D2.to_owned(),
        authority_epoch: 2,
        after_index: 0,
        cursor_digest: String::new(),
    };
    cursor.cursor_digest = kiana_domain::json_digest(&json!({
        "schema": cursor.schema,
        "connector_id": cursor.connector_id,
        "binding_id": cursor.binding_id,
        "account_id": cursor.account_id,
        "operation": cursor.operation,
        "scope_digest": cursor.scope_digest,
        "mapping_digest": cursor.mapping_digest,
        "source_cursor": cursor.source_cursor,
        "page_token_digest": cursor.page_token_digest,
        "authority_epoch": cursor.authority_epoch,
        "after_index": cursor.after_index,
    }));
    cursor
}

#[test]
fn mapping_artifact_cursor_and_page_are_bound_and_ordered() {
    let mapping = mapping();
    mapping.validate().unwrap();
    let artifact = artifact();
    let mut input = ConnectorInputArtifact {
        schema: CONNECTOR_INPUT_ARTIFACT_SCHEMA.to_owned(),
        artifact,
        mapping_digest: mapping.mapping_digest.clone(),
        source_cursor: 10,
        input_digest: String::new(),
    };
    input.input_digest = kiana_domain::json_digest(&json!({
        "schema": input.schema,
        "artifact": input.artifact,
        "mapping_digest": input.mapping_digest,
        "source_cursor": input.source_cursor,
    }));
    input.validate().unwrap();
    let cursor = cursor(&mapping);
    cursor.matches_mapping(&mapping).unwrap();
    let object = ConnectorMappedObject {
        ordinal: 1,
        value: json!({"id": "one", "name": "One"}),
        value_digest: D1.to_owned(),
        provenance_digest: D2.to_owned(),
    };
    let mut page = ConnectorMappedPage {
        schema: CONNECTOR_MAPPED_PAGE_SCHEMA.to_owned(),
        mapping_digest: mapping.mapping_digest.clone(),
        input_artifact: input,
        cursor,
        objects: vec![object],
        output_provenance_digest: D1.to_owned(),
        output_digest: String::new(),
    };
    page.objects[0].value_digest = kiana_domain::json_digest(&page.objects[0].value);
    page.output_digest = kiana_domain::json_digest(&json!({
        "schema": page.schema,
        "mapping_digest": page.mapping_digest,
        "input_artifact": page.input_artifact,
        "cursor": page.cursor,
        "objects": page.objects,
        "output_provenance_digest": page.output_provenance_digest,
    }));
    page.validate().unwrap();
}

#[test]
fn authority_targets_cursor_binding_order_and_unknown_fields_fail_closed() {
    let mut mapping = mapping();
    mapping
        .fields
        .insert("external_role".to_owned(), "role".to_owned());
    mapping.mapping_digest = kiana_domain::json_digest(&json!({
        "schema": mapping.schema,
        "version": mapping.version,
        "connector_id": mapping.connector_id,
        "binding_id": mapping.binding_id,
        "account_id": mapping.account_id,
        "operation": mapping.operation,
        "input_schema_digest": mapping.input_schema_digest,
        "output_schema_digest": mapping.output_schema_digest,
        "fields": mapping.fields,
        "provenance_digest": mapping.provenance_digest,
    }));
    assert_eq!(
        mapping.validate().unwrap_err(),
        "connector_mapping_target_not_data_field"
    );

    let mut value = serde_json::to_value(mapping()).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ConnectorObjectMapping>(value).is_err());

    let mut page = ConnectorMappedPage {
        schema: CONNECTOR_MAPPED_PAGE_SCHEMA.to_owned(),
        mapping_digest: D1.to_owned(),
        input_artifact: ConnectorInputArtifact {
            schema: CONNECTOR_INPUT_ARTIFACT_SCHEMA.to_owned(),
            artifact: artifact(),
            mapping_digest: D1.to_owned(),
            source_cursor: 1,
            input_digest: D1.to_owned(),
        },
        cursor: cursor(&mapping()),
        objects: vec![],
        output_provenance_digest: D1.to_owned(),
        output_digest: D1.to_owned(),
    };
    page.objects.push(ConnectorMappedObject {
        ordinal: 2,
        value: json!({"id": 2}),
        value_digest: D1.to_owned(),
        provenance_digest: D1.to_owned(),
    });
    page.objects.push(ConnectorMappedObject {
        ordinal: 1,
        value: json!({"id": 1}),
        value_digest: D1.to_owned(),
        provenance_digest: D1.to_owned(),
    });
    assert_eq!(
        page.validate().unwrap_err(),
        "connector_mapped_page_header_invalid"
    );
}
