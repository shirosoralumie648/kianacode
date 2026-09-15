//! Controlled, redacted audit export projection.
//!
//! Export materialization is a ControlPlane operation. It consumes the scoped OA-16/OA-17 query
//! output, binds content/query/source hashes in a manifest, and reports delivery as Unknown unless
//! a separate server-owned delivery adapter supplies confirmation. It never exposes raw EventLog
//! records or treats a client-supplied recipient/ack as authority.

use kiana_domain::{
    json_digest, AuditDeliveryReceipt, AuditDeliveryState, AuditExportFormat, AuditExportManifest,
    AuditRecord, CoreResponse, EventId, PermissionProfile, RequestContext,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

use super::redaction::*;
use super::{AuditQueryInput, ControlPlane, CoreError};

const MAX_EXPORT_CONTENT_BYTES: usize = 512 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditExportInput {
    pub query: AuditQueryInput,
    pub format: AuditExportFormat,
    pub purpose: String,
    pub recipient: String,
    pub retention_class: String,
    pub deliver: bool,
}

impl AuditExportInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        self.query
            .validate()
            .map_err(|_| "audit_export_query_invalid")?;
        for (value, field, max) in [
            (&self.purpose, "audit_export_purpose", 256),
            (&self.recipient, "audit_export_recipient", 256),
            (&self.retention_class, "audit_export_retention_class", 64),
        ] {
            if value.trim().is_empty()
                || value.len() > max
                || value.contains('\n')
                || value.contains('\r')
            {
                return Err(field);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AuditExportError {
    #[error("audit_export_invalid:{0}")]
    Invalid(&'static str),
    #[error("audit_export_query_failed:{0}")]
    QueryFailed(String),
    #[error("audit_export_records_invalid")]
    RecordsInvalid,
    #[error("audit_export_content_too_large")]
    ContentTooLarge,
    #[error("audit_export_redaction_failed:{0}")]
    RedactionFailed(String),
    #[error("audit_export_manifest_invalid:{0}")]
    ManifestInvalid(String),
    #[error("audit_export_delivery_invalid:{0}")]
    DeliveryInvalid(String),
}

fn enum_text<T: serde::Serialize>(value: &T) -> Result<String, AuditExportError> {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or(AuditExportError::RecordsInvalid)
}

fn redacted_record(record: &AuditRecord) -> Result<Value, AuditExportError> {
    let raw = serde_json::to_value(record).map_err(|_| AuditExportError::RecordsInvalid)?;
    Ok(redact_event_value(&raw))
}

fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

fn render_content(
    records: &[AuditRecord],
    format: AuditExportFormat,
) -> Result<String, AuditExportError> {
    let mut redacted = Vec::with_capacity(records.len());
    for record in records {
        record
            .validate()
            .map_err(|_| AuditExportError::RecordsInvalid)?;
        redacted.push(redacted_record(record)?);
    }
    let content = match format {
        AuditExportFormat::Jsonl => {
            redacted
                .iter()
                .map(|record| {
                    serde_json::to_string(record).map_err(|_| AuditExportError::RecordsInvalid)
                })
                .collect::<Result<Vec<_>, _>>()?
                .join("\n")
                + if redacted.is_empty() { "" } else { "\n" }
        }
        AuditExportFormat::Json => {
            serde_json::to_string(&redacted).map_err(|_| AuditExportError::RecordsInvalid)?
        }
        AuditExportFormat::Csv => {
            let mut output = String::from(
                "audit_id,action_kind,decision,target_kind,target_ref,source_cursor,reason_code,record_digest\n",
            );
            for record in records {
                output.push_str(
                    &[
                        csv_escape(&record.audit_id),
                        csv_escape(&enum_text(&record.action_kind)?),
                        csv_escape(&enum_text(&record.decision)?),
                        csv_escape(&record.target_kind),
                        csv_escape(&record.target_ref),
                        record.source_cursor.to_string(),
                        csv_escape(record.reason_code.as_deref().unwrap_or_default()),
                        csv_escape(&record.record_digest),
                    ]
                    .join(","),
                );
                output.push('\n');
            }
            output
        }
    };
    if content.len() > MAX_EXPORT_CONTENT_BYTES {
        return Err(AuditExportError::ContentTooLarge);
    }
    if ["Bearer ", "sk-", "password", "raw-secret", "authorization:"]
        .iter()
        .any(|sentinel| {
            content
                .to_ascii_lowercase()
                .contains(&sentinel.to_ascii_lowercase())
        })
    {
        return Err(AuditExportError::RedactionFailed(
            "audit_export_secret_sentinel".to_owned(),
        ));
    }
    Ok(content)
}

fn query_digest(query: &AuditQueryInput) -> String {
    json_digest(&json!({
        "source_cursor": query.source_cursor,
        "after_cursor": query.after_cursor,
        "limit": query.limit,
        "action_kind": query.action_kind,
        "decision": query.decision,
        "target_kind": query.target_kind,
        "cursor": query.cursor,
    }))
}

fn source_ids(records: &[AuditRecord], fallback: &[EventId]) -> Vec<EventId> {
    let mut ids = BTreeSet::new();
    for record in records {
        for event_id in &record.source_event_ids {
            ids.insert(event_id.to_string());
        }
    }
    if ids.is_empty() {
        return fallback.iter().copied().take(256).collect();
    }
    fallback
        .iter()
        .filter(|event_id| ids.contains(&event_id.to_string()))
        .copied()
        .take(256)
        .collect()
}

impl ControlPlane {
    /// Materialize a redacted audit export from the same scoped query path used by UI clients.
    pub async fn export_audit(
        &self,
        context: &RequestContext,
        input: AuditExportInput,
    ) -> Result<CoreResponse, CoreError> {
        if let Err(reason) = input.validate() {
            return Ok(CoreResponse::blocked(context.request_id, reason));
        }
        if context.permission_profile == PermissionProfile::Safe {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "audit_export_requires_explicit_permission",
            ));
        }
        if context
            .actor_id
            .as_deref()
            .is_none_or(|actor| actor.trim().is_empty())
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "audit_export_unauthenticated",
            ));
        }
        let query_response = self
            .query_audit(context, input.query.clone())
            .await
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()))?;
        if query_response.status != kiana_domain::ExecutionStatus::Completed {
            return Ok(query_response);
        }
        let records: Vec<AuditRecord> = serde_json::from_value(
            query_response
                .output
                .get("records")
                .cloned()
                .unwrap_or(Value::Null),
        )
        .map_err(|_| AuditExportError::RecordsInvalid)
        .map_err(|error| kiana_ports::PortError::Failed(error.to_string()))?;
        let content = render_content(&records, input.format)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()))?;
        let source_cursor = query_response.output["source_cursor"]
            .as_u64()
            .ok_or_else(|| {
                kiana_ports::PortError::Failed("audit_export_cursor_missing".to_owned())
            })?;
        let projection_version = query_response.output["projection_version"]
            .as_u64()
            .ok_or_else(|| {
                kiana_ports::PortError::Failed("audit_export_projection_version_missing".to_owned())
            })?;
        let projection = self
            .audit_projection()
            .await
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()))?;
        let artifact_digest = json_digest(&json!(&content));
        let manifest = AuditExportManifest::new(
            format!("export:{}", context.request_id),
            query_digest(&input.query),
            source_cursor,
            projection_version,
            u32::try_from(records.len()).unwrap_or(u32::MAX),
            input.format,
            input.purpose,
            input.recipient,
            input.retention_class,
            artifact_digest,
            source_ids(&records, &projection.source_event_ids),
        )
        .map_err(|error| kiana_ports::PortError::Failed(error.to_string()))?;
        let delivery = if input.deliver {
            Some(
                AuditDeliveryReceipt::new(
                    format!("delivery:{}", context.request_id),
                    manifest.export_id.clone(),
                    manifest.manifest_digest.clone(),
                    manifest.artifact_digest.clone(),
                    manifest.recipient.clone(),
                    AuditDeliveryState::Unknown,
                    None,
                    manifest.source_cursor,
                )
                .map_err(|error| kiana_ports::PortError::Failed(error.to_string()))?,
            )
        } else {
            None
        };
        let limitations = if delivery.is_some() {
            vec!["delivery_not_confirmed".to_owned()]
        } else {
            Vec::new()
        };
        Ok(CoreResponse::completed(
            context.request_id,
            json!({
                "schema": "kiana.audit-export.v1",
                "manifest": manifest,
                "content": content,
                "delivery": delivery,
                "limitations": limitations,
            }),
        ))
    }
}
