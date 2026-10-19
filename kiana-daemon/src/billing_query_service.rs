//! BQ-23 query surfaces over the BQ-20 projection: budget summary, usage breakdown, cost export
//! and reconciliation inbox.
//!
//! This module does not append events, reserve or release budget, stage or consume approvals,
//! and it never calls a model. It reads the same `ControlPlane` EventLog the rest of the product
//! uses; the projection is a read model that can be discarded and rebuilt.
//!
//! Deny-first rules that live here:
//! - a store that cannot enumerate its log is refused (`billing_query_projection_unsupported`);
//! - an empty or truncated source never renders as "zero rows, success"
//!   (`billing_query_projection_source_cursor_missing`, `_retention_truncated`);
//! - a committed cursor that does not match the events actually read is refused
//!   (`billing_query_projection_cursor_lagging`) instead of returning a short history;
//! - the wire boundary digest and project id must equal the server-derived ones
//!   (`billing_query_boundary_mismatch`, `billing_query_project_mismatch`).
//!
//! Proof ceiling: local read behaviour over the process EventLog, not durable cross-process
//! projection, provider-invoice truth or live billing.

use crate::DaemonHost;
use kiana_core::ControlPlane;
use kiana_domain::{
    json_digest, BillingProjectionSnapshot, BillingQueryKind, BillingQueryRequest, CoreResponse,
    ExecutionStatus, ProjectIdentity, RequestContext, RequestId,
};
use kiana_protocol::ResponseEnvelope;
use kiana_query::{project_billing_ledger, query_billing_projection, QueryDataBoundary};
use serde_json::{json, Value};
use std::path::Path;

/// Versioned human command that carries a BQ-23 query through the existing command route.
/// It is not a model-visible tool: harness tool schemas are untouched.
pub const BILLING_QUERY_COMMAND: &str = "budget.query.v1";
/// Output schema of one query answer shared by CLI, Workbench and Web.
pub const BILLING_QUERY_SURFACE_SCHEMA: &str = "kiana.billing-query-surface.v1";
/// Default page limit for a command that does not ask for one.
pub const BILLING_QUERY_DEFAULT_LIMIT: u16 = 50;

const BILLING_QUERY_COMMAND_ARGUMENT_SCHEMA: &str = "kiana.budget-card-query.v1";

fn blocked(request_id: RequestId, reason: impl Into<String>) -> ResponseEnvelope {
    ResponseEnvelope::rejected(request_id, reason)
}

fn blocked_core(request_id: RequestId, reason: impl Into<String>) -> kiana_domain::CoreResponse {
    kiana_domain::CoreResponse::blocked(request_id, reason)
}

fn wire(core: kiana_domain::CoreResponse) -> ResponseEnvelope {
    ResponseEnvelope::from_core(core)
}

/// Read the committed EventLog and rebuild the billing projection for one query.
async fn rebuild_projection(
    core: &ControlPlane,
    data_epoch: u64,
) -> Result<(BillingProjectionSnapshot, String), String> {
    let events = match core.persisted_events().await {
        Ok(Some(events)) => events,
        Ok(None) => return Err("billing_query_projection_unsupported".to_owned()),
        Err(_) => return Err("billing_query_projection_unavailable".to_owned()),
    };
    let cursor = match core.cursor_snapshot().await {
        Ok(Some(cursor)) => cursor,
        Ok(None) => return Err("billing_query_projection_unsupported".to_owned()),
        Err(_) => return Err("billing_query_projection_unavailable".to_owned()),
    };
    // A truncated log cannot be rebuilt from the start, so the summary would silently omit facts.
    if cursor.first_retained_cursor > 1 {
        return Err("billing_query_projection_retention_truncated".to_owned());
    }
    if cursor.last_committed_cursor != events.len() as u64 {
        return Err("billing_query_projection_cursor_lagging".to_owned());
    }
    if events.is_empty() || cursor.last_committed_cursor == 0 {
        return Err("billing_query_projection_source_cursor_missing".to_owned());
    }
    let snapshot = project_billing_ledger(data_epoch, cursor.last_committed_cursor, &events)
        .map_err(|error| format!("billing_query_projection_invalid:{error}"))?;
    if snapshot.source.source_cursor == 0 {
        return Err("billing_query_projection_source_cursor_missing".to_owned());
    }
    Ok((snapshot, cursor.snapshot_digest))
}

/// Derive the server-owned data boundary for this project.
fn derive_boundary(
    canonical_root: &str,
    data_epoch: u64,
    export_requested: bool,
) -> Result<QueryDataBoundary, String> {
    let policy = crate::data_governance::read_policy(Path::new(canonical_root))
        .map_err(|_| "billing_query_boundary_unavailable".to_owned())?;
    let canonical = Path::new(canonical_root)
        .canonicalize()
        .unwrap_or_else(|_| Path::new(canonical_root).to_path_buf());
    let revoked = policy.revoked_sources.iter().any(|source| {
        source == canonical_root
            || Path::new(source)
                .canonicalize()
                .unwrap_or_else(|_| Path::new(source).to_path_buf())
                == canonical
    });
    let scope_digest = json_digest(&json!({
        "schema": "kiana.billing-query-scope.v1",
        "project_root": canonical_root,
    }));
    QueryDataBoundary::derive(
        canonical_root,
        scope_digest,
        policy.data_epoch.max(data_epoch),
        revoked,
        true,
        export_requested,
    )
    .map_err(|error| format!("billing_query_boundary_invalid:{error}"))
}

/// The shared answer shape: one response plus the runtime/business-separated receipt view.
fn surface_output(
    response: &kiana_domain::BillingQueryResponse,
    receipt_view: &kiana_core::BillingQueryReceiptView,
    cursor_snapshot_digest: &str,
) -> Value {
    json!({
        "schema": BILLING_QUERY_SURFACE_SCHEMA,
        "billing_query": response,
        "receipt_view": receipt_view,
        "cursor_snapshot_digest": cursor_snapshot_digest,
    })
}

async fn serve(
    core: &ControlPlane,
    request_id: RequestId,
    canonical_root: &str,
    data_epoch: u64,
    query: &BillingQueryRequest,
) -> kiana_domain::CoreResponse {
    let export_requested = query.kind == BillingQueryKind::CostExport;
    let boundary = match derive_boundary(canonical_root, data_epoch, export_requested) {
        Ok(boundary) => boundary,
        Err(reason) => return blocked_core(request_id, reason),
    };
    // The caller may only read the scope the server derived for this project.
    if query.data_boundary_digest != boundary.scope_digest {
        return blocked_core(request_id, "billing_query_boundary_mismatch");
    }
    let (snapshot, cursor_snapshot_digest) = match rebuild_projection(core, data_epoch).await {
        Ok(projection) => projection,
        Err(reason) => return blocked_core(request_id, reason),
    };
    // A projection with no source cursor is not an empty bill. The adapter refuses it too; this
    // early check keeps the reason visible even if the adapter changes.
    if snapshot.source.source_cursor == 0 {
        return blocked_core(request_id, "billing_query_projection_source_cursor_missing");
    }
    let response = match query_billing_projection(query, &snapshot, &boundary) {
        Ok(response) => response,
        Err(error) => return blocked_core(request_id, error.to_string()),
    };
    let receipt_view =
        kiana_core::BillingQueryReceiptView::runtime_only(ExecutionStatus::Completed);
    kiana_domain::CoreResponse::completed(
        request_id,
        surface_output(&response, &receipt_view, &cursor_snapshot_digest),
    )
}

/// Serve one protocol-level `RequestBody::BillingQuery`.
///
/// Runs after the daemon authenticated the actor, resolved the security context and built the
/// project identity. Project id and boundary digest are compared with server values; nothing from
/// the wire selects which project is read.
pub(crate) async fn serve_billing_query(
    host: &DaemonHost,
    context: &RequestContext,
    project_identity: &ProjectIdentity,
    data_epoch: u64,
    query: BillingQueryRequest,
) -> ResponseEnvelope {
    if query.project_id != project_identity.project_id {
        return blocked(context.request_id, "billing_query_project_mismatch");
    }
    if let Err(error) = kiana_core::BillingQueryAdmission::admit(
        &query,
        &kiana_core::BillingQueryEffectWitness::none(),
    ) {
        return blocked(context.request_id, error.to_string());
    }
    wire(
        serve(
            &host.core,
            context.request_id,
            project_identity.canonical_root.as_str(),
            data_epoch,
            &query,
        )
        .await,
    )
}

/// Serve the versioned `budget.query.v1` human command.
///
/// The arguments carry only kind, limit and paging; project id, boundary digest and read-only
/// marker are server values. A caller-supplied identity field is refused by name rather than
/// dropped silently.
pub(crate) async fn serve_billing_query_command(
    host: &DaemonHost,
    context: &RequestContext,
    project_identity: &ProjectIdentity,
    data_epoch: u64,
    arguments: &Value,
) -> ResponseEnvelope {
    const IDENTITY_KEYS: [&str; 5] = [
        "actor_id",
        "project_id",
        "project_root",
        "data_boundary_digest",
        "read_only",
    ];
    let Some(object) = arguments.as_object() else {
        return blocked(context.request_id, "billing_query_arguments_invalid");
    };
    if object
        .keys()
        .any(|key| IDENTITY_KEYS.contains(&key.as_str()))
    {
        return blocked(
            context.request_id,
            "billing_query_identity_override_rejected",
        );
    }
    if let Some(schema) = object.get("schema").and_then(Value::as_str) {
        if schema != BILLING_QUERY_COMMAND_ARGUMENT_SCHEMA {
            return blocked(context.request_id, "billing_query_argument_schema_invalid");
        }
    }
    let kind = match object.get("kind") {
        None | Some(Value::Null) => BillingQueryKind::BudgetSummary,
        Some(value) => match serde_json::from_value::<BillingQueryKind>(value.clone()) {
            Ok(kind) => kind,
            Err(_) => return blocked(context.request_id, "billing_query_kind_invalid"),
        },
    };
    let limit = match object.get("limit") {
        None | Some(Value::Null) => BILLING_QUERY_DEFAULT_LIMIT,
        Some(value) => match value.as_u64() {
            Some(limit) if limit > 0 && limit <= kiana_domain::BILLING_QUERY_MAX_LIMIT as u64 => {
                limit as u16
            }
            _ => return blocked(context.request_id, "billing_query_limit_invalid"),
        },
    };
    let after_cursor = match object.get("after_cursor") {
        None | Some(Value::Null) => 0,
        Some(value) => match value.as_u64() {
            Some(after) => after,
            None => return blocked(context.request_id, "billing_query_after_cursor_invalid"),
        },
    };
    let source_cursor = match object.get("source_cursor") {
        None | Some(Value::Null) => None,
        Some(value) => match value.as_u64() {
            Some(source) if source > 0 => Some(source),
            _ => return blocked(context.request_id, "billing_query_source_cursor_invalid"),
        },
    };
    let cursor = match object.get("cursor") {
        None | Some(Value::Null) => None,
        Some(value) => {
            match serde_json::from_value::<kiana_domain::BillingQueryCursor>(value.clone()) {
                Ok(cursor) => Some(cursor),
                Err(_) => return blocked(context.request_id, "billing_query_cursor_invalid"),
            }
        }
    };
    let export_requested = kind == BillingQueryKind::CostExport;
    let boundary = match derive_boundary(
        project_identity.canonical_root.as_str(),
        data_epoch,
        export_requested,
    ) {
        Ok(boundary) => boundary,
        Err(reason) => return blocked(context.request_id, reason),
    };
    let query = BillingQueryRequest {
        schema: kiana_domain::BILLING_QUERY_SCHEMA.to_owned(),
        kind,
        project_id: project_identity.project_id,
        data_boundary_digest: boundary.scope_digest.clone(),
        source_cursor,
        after_cursor,
        limit,
        cursor,
        read_only: true,
    };
    if let Err(error) = query.validate() {
        return blocked(
            context.request_id,
            format!("billing_query_request_invalid:{error}"),
        );
    }
    if let Err(error) = kiana_core::BillingQueryAdmission::admit(
        &query,
        &kiana_core::BillingQueryEffectWitness::none(),
    ) {
        return blocked(context.request_id, error.to_string());
    }
    let response = serve(
        &host.core,
        context.request_id,
        project_identity.canonical_root.as_str(),
        data_epoch,
        &query,
    )
    .await;
    if response.status != ExecutionStatus::Completed {
        return wire(response);
    }
    // Human command transport: `response_outcome` expects `command_result` with a text value. The
    // text is the same JSON surface a protocol caller receives, so every entrypoint renders one
    // shape and no surface recomputes the summary.
    let value = serde_json::to_string(&response.output)
        .unwrap_or_else(|_| "{\"schema\":\"kiana.billing-query-surface.v1\"}".to_owned());
    let completed = kiana_domain::CoreResponse::completed(
        context.request_id,
        json!({
            "command_result": {
                "output_type": "text",
                "value": value,
                "metadata": {
                    "schema": BILLING_QUERY_SURFACE_SCHEMA,
                    "command": BILLING_QUERY_COMMAND,
                },
            }
        }),
    );
    wire(completed)
}

/// Marker for the source guard: this service owns no fact or authority write path.
pub const BILLING_QUERY_SERVICE_READ_ONLY: bool = true;
