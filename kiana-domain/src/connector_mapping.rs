//! INT-25 versioned connector object mapping, ArtifactRef input and pagination contracts.
//!
//! External payload fields are data only. A mapping names bounded output fields and is bound to a
//! connector/binding/account/operation schema digest; it cannot supply capability, actor, path or
//! approval authority. Page cursors carry digests and server identity rather than a raw token, so
//! a cursor from another account or mapping cannot be replayed.

use crate::{json_digest, ArtifactRef};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const CONNECTOR_MAPPING_SCHEMA: &str = "kiana.connector-object-mapping.v1";
pub const CONNECTOR_INPUT_ARTIFACT_SCHEMA: &str = "kiana.connector-input-artifact.v1";
pub const CONNECTOR_PAGE_CURSOR_SCHEMA: &str = "kiana.connector-page-cursor.v1";
pub const CONNECTOR_MAPPED_PAGE_SCHEMA: &str = "kiana.connector-mapped-page.v1";
pub const CONNECTOR_MAPPING_MAX_FIELDS: usize = 128;
pub const CONNECTOR_MAPPING_MAX_VALUE_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorObjectMapping {
    pub schema: String,
    pub version: u64,
    pub connector_id: String,
    pub binding_id: String,
    pub account_id: String,
    pub operation: String,
    pub input_schema_digest: String,
    pub output_schema_digest: String,
    pub fields: BTreeMap<String, String>,
    pub provenance_digest: String,
    pub mapping_digest: String,
}

impl ConnectorObjectMapping {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_MAPPING_SCHEMA
            || self.version == 0
            || self.fields.is_empty()
            || self.fields.len() > CONNECTOR_MAPPING_MAX_FIELDS
            || !valid_digest(&self.input_schema_digest)
            || !valid_digest(&self.output_schema_digest)
            || !valid_digest(&self.provenance_digest)
            || !valid_digest(&self.mapping_digest)
            || self.mapping_digest != self.digest()
        {
            return Err("connector_mapping_header_invalid".to_owned());
        }
        for (value, field, max) in [
            (&self.connector_id, "connector_mapping_connector", 256),
            (&self.binding_id, "connector_mapping_binding", 256),
            (&self.account_id, "connector_mapping_account", 256),
            (&self.operation, "connector_mapping_operation", 256),
        ] {
            bounded(value, field, max)?;
        }
        let mut targets = BTreeSet::new();
        for (external, target) in &self.fields {
            bounded(external, "connector_mapping_external_field", 256)?;
            bounded(target, "connector_mapping_target_field", 256)?;
            if !targets.insert(target) || forbidden_target(target) {
                return Err("connector_mapping_target_not_data_field".to_owned());
            }
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "connector_id": self.connector_id,
            "binding_id": self.binding_id,
            "account_id": self.account_id,
            "operation": self.operation,
            "input_schema_digest": self.input_schema_digest,
            "output_schema_digest": self.output_schema_digest,
            "fields": self.fields,
            "provenance_digest": self.provenance_digest,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorInputArtifact {
    pub schema: String,
    pub artifact: ArtifactRef,
    pub mapping_digest: String,
    pub source_cursor: u64,
    pub input_digest: String,
}

impl ConnectorInputArtifact {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_INPUT_ARTIFACT_SCHEMA
            || self.source_cursor == 0
            || !valid_digest(&self.mapping_digest)
            || !valid_digest(&self.input_digest)
            || self.input_digest != self.digest()
        {
            return Err("connector_input_artifact_invalid".to_owned());
        }
        self.artifact.validate()?;
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "artifact": self.artifact,
            "mapping_digest": self.mapping_digest,
            "source_cursor": self.source_cursor,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorPageCursor {
    pub schema: String,
    pub connector_id: String,
    pub binding_id: String,
    pub account_id: String,
    pub operation: String,
    pub scope_digest: String,
    pub mapping_digest: String,
    pub source_cursor: u64,
    pub page_token_digest: String,
    pub authority_epoch: u64,
    pub after_index: u64,
    pub cursor_digest: String,
}

impl ConnectorPageCursor {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_PAGE_CURSOR_SCHEMA
            || self.source_cursor == 0
            || !valid_digest(&self.scope_digest)
            || !valid_digest(&self.mapping_digest)
            || !valid_digest(&self.page_token_digest)
            || self.authority_epoch == 0
            || !valid_digest(&self.cursor_digest)
            || self.cursor_digest != self.digest()
        {
            return Err("connector_page_cursor_invalid".to_owned());
        }
        for (value, field, max) in [
            (&self.connector_id, "connector_cursor_connector", 256),
            (&self.binding_id, "connector_cursor_binding", 256),
            (&self.account_id, "connector_cursor_account", 256),
            (&self.operation, "connector_cursor_operation", 256),
        ] {
            bounded(value, field, max)?;
        }
        Ok(())
    }

    pub fn matches_mapping(&self, mapping: &ConnectorObjectMapping) -> Result<(), String> {
        mapping.validate()?;
        self.validate()?;
        if self.connector_id != mapping.connector_id
            || self.binding_id != mapping.binding_id
            || self.account_id != mapping.account_id
            || self.operation != mapping.operation
            || self.mapping_digest != mapping.mapping_digest
        {
            return Err("connector_page_cursor_mapping_binding_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "connector_id": self.connector_id,
            "binding_id": self.binding_id,
            "account_id": self.account_id,
            "operation": self.operation,
            "scope_digest": self.scope_digest,
            "mapping_digest": self.mapping_digest,
            "source_cursor": self.source_cursor,
            "page_token_digest": self.page_token_digest,
            "authority_epoch": self.authority_epoch,
            "after_index": self.after_index,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorMappedObject {
    pub ordinal: u64,
    pub value: Value,
    pub value_digest: String,
    pub provenance_digest: String,
}

impl ConnectorMappedObject {
    pub fn validate(&self) -> Result<(), String> {
        let bytes = serde_json::to_vec(&self.value)
            .map_err(|_| "connector_mapped_object_value_invalid".to_owned())?;
        if self.value_digest != json_digest(&self.value)
            || !valid_digest(&self.value_digest)
            || !valid_digest(&self.provenance_digest)
            || bytes.len() > CONNECTOR_MAPPING_MAX_VALUE_BYTES
        {
            return Err("connector_mapped_object_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorMappedPage {
    pub schema: String,
    pub mapping_digest: String,
    pub input_artifact: ConnectorInputArtifact,
    pub cursor: ConnectorPageCursor,
    pub objects: Vec<ConnectorMappedObject>,
    pub output_provenance_digest: String,
    pub output_digest: String,
}

impl ConnectorMappedPage {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_MAPPED_PAGE_SCHEMA
            || !valid_digest(&self.mapping_digest)
            || !valid_digest(&self.output_provenance_digest)
            || !valid_digest(&self.output_digest)
            || self.output_digest != self.digest()
        {
            return Err("connector_mapped_page_header_invalid".to_owned());
        }
        self.input_artifact.validate()?;
        self.cursor.validate()?;
        if self.mapping_digest != self.cursor.mapping_digest
            || self.mapping_digest != self.input_artifact.mapping_digest
        {
            return Err("connector_mapped_page_mapping_mismatch".to_owned());
        }
        let mut previous = None;
        for object in &self.objects {
            object.validate()?;
            if previous.is_some_and(|value| object.ordinal <= value) {
                return Err("connector_mapped_page_order_invalid".to_owned());
            }
            previous = Some(object.ordinal);
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "mapping_digest": self.mapping_digest,
            "input_artifact": self.input_artifact,
            "cursor": self.cursor,
            "objects": self.objects,
            "output_provenance_digest": self.output_provenance_digest,
        }))
    }
}

pub fn validate_connector_mapping(mapping: &ConnectorObjectMapping) -> Result<(), String> {
    mapping.validate()
}

fn forbidden_target(value: &str) -> bool {
    [
        "capability",
        "actor",
        "role",
        "approval",
        "scope",
        "path",
        "command",
        "endpoint",
    ]
    .iter()
    .any(|forbidden| value == *forbidden || value.starts_with(&format!("{forbidden}.")))
}

fn bounded(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\r', '\n']) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
