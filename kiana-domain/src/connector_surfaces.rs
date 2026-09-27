//! INT-28 shared read-only connector surface DTOs.
//!
//! CLI, Web, Workbench and MCP consume the same query/response shape. Actor, approval, lease and
//! context authority are intentionally absent; mutations continue through ConnectorCommand and
//! DaemonHost. The response is a projection with evidence/limitations, not a connector fact.

use crate::{json_digest, redact_text, Freshness};
use serde::{Deserialize, Serialize};

pub const CONNECTOR_SURFACE_QUERY_SCHEMA: &str = "kiana.connector-surface-query.v1";
pub const CONNECTOR_SURFACE_CURSOR_SCHEMA: &str = "kiana.connector-surface-cursor.v1";
pub const CONNECTOR_SURFACE_RESPONSE_SCHEMA: &str = "kiana.connector-surface-response.v1";
pub const CONNECTOR_SURFACE_MAX_LIMIT: u16 = 100;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorSurface {
    Cli,
    Web,
    Workbench,
    Mcp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorSurfaceQueryKind {
    Health,
    Invocation,
    Reconciliation,
    Approval,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorSurfaceCursor {
    pub schema: String,
    pub surface: ConnectorSurface,
    pub kind: ConnectorSurfaceQueryKind,
    pub binding_id: String,
    pub scope_digest: String,
    pub source_cursor: u64,
    pub after_item: u64,
    pub cursor_digest: String,
}

impl ConnectorSurfaceCursor {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_SURFACE_CURSOR_SCHEMA
            || !valid_text(&self.binding_id)
            || !valid_digest(&self.scope_digest)
            || self.source_cursor == 0
            || !valid_digest(&self.cursor_digest)
            || self.cursor_digest != self.digest()
        {
            return Err("connector_surface_cursor_invalid".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "surface": self.surface,
            "kind": self.kind,
            "binding_id": self.binding_id,
            "scope_digest": self.scope_digest,
            "source_cursor": self.source_cursor,
            "after_item": self.after_item,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorSurfaceQuery {
    pub schema: String,
    pub surface: ConnectorSurface,
    pub kind: ConnectorSurfaceQueryKind,
    pub binding_id: String,
    pub scope_digest: String,
    #[serde(default)]
    pub source_cursor: Option<u64>,
    #[serde(default)]
    pub after_item: u64,
    pub limit: u16,
    #[serde(default)]
    pub cursor: Option<ConnectorSurfaceCursor>,
    pub read_only: bool,
}

impl ConnectorSurfaceQuery {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_SURFACE_QUERY_SCHEMA
            || !valid_text(&self.binding_id)
            || !valid_digest(&self.scope_digest)
            || self.source_cursor == Some(0)
            || self.limit == 0
            || self.limit > CONNECTOR_SURFACE_MAX_LIMIT
            || !self.read_only
            || !self
                .source_cursor
                .is_none_or(|cursor| self.after_item <= cursor)
            || self.source_cursor.is_none() && self.after_item != 0
        {
            return Err("connector_surface_query_invalid".to_owned());
        }
        if let Some(cursor) = &self.cursor {
            cursor.validate()?;
            if cursor.surface != self.surface
                || cursor.kind != self.kind
                || cursor.binding_id != self.binding_id
                || cursor.scope_digest != self.scope_digest
                || self.source_cursor != Some(cursor.source_cursor)
                || self.after_item != cursor.after_item
            {
                return Err("connector_surface_cursor_binding_invalid".to_owned());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorSurfaceItem {
    pub item_id: String,
    pub dedup_key: String,
    pub title: String,
    pub summary: String,
    pub evidence_digest: String,
    pub source_cursor: u64,
    pub unknown: bool,
}

impl ConnectorSurfaceItem {
    pub fn validate(&self) -> Result<(), String> {
        if !valid_text(&self.item_id)
            || !valid_text(&self.dedup_key)
            || !valid_text(&self.title)
            || !valid_text(&self.summary)
            || redact_text(&self.title) != self.title
            || redact_text(&self.summary) != self.summary
            || !valid_digest(&self.evidence_digest)
            || self.source_cursor == 0
        {
            return Err("connector_surface_item_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorSurfaceResponse {
    pub schema: String,
    pub surface: ConnectorSurface,
    pub kind: ConnectorSurfaceQueryKind,
    pub binding_id: String,
    pub scope_digest: String,
    pub source_cursor: u64,
    pub projection_digest: String,
    pub freshness: Freshness,
    pub after_item: u64,
    pub items: Vec<ConnectorSurfaceItem>,
    pub limitations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<ConnectorSurfaceCursor>,
    pub response_digest: String,
}

impl ConnectorSurfaceResponse {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_SURFACE_RESPONSE_SCHEMA
            || !valid_text(&self.binding_id)
            || !valid_digest(&self.scope_digest)
            || self.source_cursor == 0
            || !valid_digest(&self.projection_digest)
            || self.items.len() > usize::from(CONNECTOR_SURFACE_MAX_LIMIT)
            || self
                .limitations
                .iter()
                .any(|value| !valid_text(value) || redact_text(value) != *value)
            || !valid_digest(&self.response_digest)
            || self.response_digest != self.digest()
        {
            return Err("connector_surface_response_invalid".to_owned());
        }
        for item in &self.items {
            item.validate()?;
        }
        if let Some(cursor) = &self.next_cursor {
            cursor.validate()?;
            if cursor.surface != self.surface
                || cursor.kind != self.kind
                || cursor.binding_id != self.binding_id
                || cursor.scope_digest != self.scope_digest
                || cursor.source_cursor != self.source_cursor
                || cursor.after_item <= self.after_item
            {
                return Err("connector_surface_next_cursor_invalid".to_owned());
            }
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "surface": self.surface,
            "kind": self.kind,
            "binding_id": self.binding_id,
            "scope_digest": self.scope_digest,
            "source_cursor": self.source_cursor,
            "projection_digest": self.projection_digest,
            "freshness": self.freshness,
            "after_item": self.after_item,
            "items": self.items,
            "limitations": self.limitations,
            "next_cursor": self.next_cursor,
        }))
    }
}

pub fn validate_connector_surface_query(query: &ConnectorSurfaceQuery) -> Result<(), String> {
    query.validate()
}

pub fn validate_connector_surface_response(
    response: &ConnectorSurfaceResponse,
) -> Result<(), String> {
    response.validate()
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.contains(['\0', '\r', '\n'])
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
