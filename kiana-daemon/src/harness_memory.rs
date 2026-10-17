//! Six-layer memory broker for the owned harness.
//!
//! Collections are partitioned JSONL files. Search and write are tools; the
//! runner never touches the store. Chat is never auto-ingested.

use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{
    json_digest, memory_query_terms, AdapterCommitState, AdapterResultKind,
    AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult, EvidenceStatus, MemoryAdmission,
    MemoryBodyRef, MemoryClassification, MemoryCollection, MemoryJournalFact, MemoryMutation,
    MemoryMutationAuthority, MemoryMutationJournalStage, MemoryMutationLedger,
    MemoryMutationOperation, MemoryMutationOutcome, MemoryMutationReceipt, MemoryMutationTarget,
    MemoryOrigin, MemoryProjectionFence, MemoryRecord, MemoryScope as DomainMemoryScope,
    MemorySensitivity, MemoryState, Purpose, RoleSpec, RuntimeEvent, SourceKind, SourceRef,
    MEMORY_FACT_EVENT_KIND, MEMORY_LAYER_COMPANY, MEMORY_LAYER_DEPARTMENT,
    MEMORY_LAYER_INSTANCE_SCRATCH, MEMORY_LAYER_PROJECT, MEMORY_LAYER_ROLE, MEMORY_LAYER_USER,
    MEMORY_RECORD_SCHEMA, MEMORY_RECORD_SCHEMA_V2, MEMORY_REVIEW_SCHEMA, MEMORY_SEARCH_SCHEMA,
    MEMORY_STREAM, MEMORY_WRITE_SCHEMA,
};
use kiana_ports::{EventStorePort, PortError};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SEARCH_OPERATION: &str = "memory.search";
const WRITE_OPERATION: &str = "memory.write";
const KIANA_HOME_ENV: &str = "KIANA_HOME";
const DEFAULT_LIMIT: usize = 10;
const MAX_LIMIT: usize = 100;
const REVIEW_OPERATION: &str = "memory.review";

#[derive(Clone)]
struct MemoryScope {
    home: Option<PathBuf>,
    events: Option<std::sync::Arc<dyn EventStorePort>>,
}
impl MemoryScope {
    fn capture(events: Option<std::sync::Arc<dyn EventStorePort>>) -> Self {
        Self {
            home: std::env::var_os(KIANA_HOME_ENV).map(PathBuf::from),
            events,
        }
    }
    fn check(&self, arguments: &Value) -> Result<(), PortError> {
        if arguments["memory_snapshot"]
            != json!({"schema":"kiana.memory-storage.v1","home":self.home})
        {
            return Err(failed("memory_storage_scope_changed"));
        }
        Ok(())
    }
}

/// Server-owned facts for one memory request.
///
/// The collection ACL, session, project and epoch fences come from the authorized
/// `ExecutionScope` that Core built; the store root was captured once at registration.  Model
/// arguments may repeat those values, never replace them: a contradicting session, project or
/// storage snapshot is denied instead of being interpreted, an explicit collection must already be
/// inside the ACL, and a search without an explicit collection may only use role grants that the
/// ACL still covers.
struct ServerMemoryScope {
    acl: DomainMemoryScope,
    project_root: String,
    session_id: String,
    policy_epoch: u64,
    data_epoch: u64,
}

/// Compare a model-supplied project root with the server-owned project identity.
///
/// Core records both the display root and its canonical form, so either shape is accepted; the
/// comparison never admits a different directory.  When canonicalization is impossible (a missing
/// path or a platform without it) only the exact strings can match.
fn project_root_matches_execution_scope(
    project: &kiana_domain::ProjectIdentity,
    candidate: &str,
) -> bool {
    if project.root == candidate || project.canonical_root == candidate {
        return true;
    }
    let canonical = fs::canonicalize(candidate);
    let Ok(canonical) = canonical else {
        return false;
    };
    [&project.root, &project.canonical_root]
        .into_iter()
        .any(|allowed| fs::canonicalize(allowed).map(|path| path == canonical).unwrap_or(false))
}

fn server_memory_scope(
    request: &AuthorizedCapabilityRequest,
    arguments: &Value,
    allow_write: bool,
) -> Result<ServerMemoryScope, PortError> {
    let execution = request
        .request
        .execution_scope
        .as_ref()
        .ok_or_else(|| failed("memory_scope_required"))?;
    let purpose = Purpose {
        id: if allow_write {
            "memory.write".to_owned()
        } else {
            "memory.search".to_owned()
        },
        description: "server-derived memory operation purpose".to_owned(),
    };
    let scope =
        DomainMemoryScope::from_execution_scope(execution, purpose, allow_write).map_err(failed)?;
    // Session and project identity are fixed before the ACL decision: the arguments may repeat the
    // server values (Core stamps them) but can never move the store cursor or the resolved root.
    let session_id = execution.session_id.as_str().to_owned();
    if let Some(requested) = optional_string(arguments, "session_id") {
        if requested != session_id {
            return Err(failed("memory_session_scope_mismatch"));
        }
    }
    if let Some(requested) = optional_string(arguments, "project_root") {
        if !project_root_matches_execution_scope(&execution.project, &requested) {
            return Err(failed("memory_project_scope_mismatch"));
        }
    }
    let role = RoleSpec::lookup(&optional_string(arguments, "role_id").unwrap_or_default())
        .ok_or_else(|| failed("role_unknown"))?;
    if let Some(raw) = optional_string(arguments, "collection") {
        let requested = MemoryCollection::parse(&raw)
            .ok_or_else(|| failed("memory_scope_collection_unknown"))?;
        if !scope.allows_collection(&requested) {
            return Err(failed("memory_scope_collection_denied"));
        }
    } else if allow_write {
        return Err(failed("memory_scope_collection_required"));
    } else {
        // A default read set may only be used while every role grant is still inside the
        // server-derived ACL; a role named by the request cannot widen the effective collections.
        for grant in role.granted_collections() {
            if !scope.allows_collection(&grant) {
                return Err(failed("memory_scope_role_denied"));
            }
        }
    }
    Ok(ServerMemoryScope {
        acl: scope,
        project_root: execution.project.root.clone(),
        session_id,
        policy_epoch: execution.authority_epoch,
        data_epoch: execution.data_epoch,
    })
}

fn memory_stream_id(
    scope: &MemoryScope,
    project_root: &str,
    collection: &str,
    session_id: &str,
) -> String {
    let session_scope = MemoryCollection::parse(collection)
        .filter(|parsed| parsed.layer == MEMORY_LAYER_INSTANCE_SCRATCH)
        .map(|_| session_id)
        .unwrap_or_default();
    json_digest(&json!({
        "home": scope.home,
        "project_root": project_root,
        "collection": collection,
        "session_id": session_scope,
    }))
}

/// Confirm that the file projection has caught up with the committed memory stream.  An empty
/// EventStore stream plus a non-empty file is treated as unjournaled visibility, not a cache hit.
async fn ensure_memory_projection(scope: &MemoryScope, arguments: &Value) -> Result<(), PortError> {
    let Some(events) = scope.events.clone() else {
        return Ok(());
    };
    let role = RoleSpec::lookup(&optional_string(arguments, "role_id").unwrap_or_default())
        .ok_or_else(|| failed("role_unknown"))?;
    let collections = if optional_string(arguments, "collection").is_some() {
        vec![required_collection(arguments)?]
    } else {
        requested_collections(arguments, &role)?
    };
    let project_root = optional_string(arguments, "project_root").unwrap_or_default();
    let session_id = optional_string(arguments, "session_id").unwrap_or_default();
    for collection in collections {
        let stream_id = memory_stream_id(scope, &project_root, &collection.collection, &session_id);
        let committed = events.read_stream(MEMORY_STREAM, &stream_id).await?;
        let path = collection_path_scoped(
            &collection,
            &project_root,
            &session_id,
            scope.home.as_deref(),
        )?;
        let projected = tokio::task::spawn_blocking(move || read_records(&path))
            .await
            .map_err(|error| failed(format!("memory_projection_read_join_failed:{error}")))??;
        if committed.is_empty() {
            if !projected.is_empty() {
                return Err(failed("memory_projection_unjournaled"));
            }
            continue;
        }
        let projection = kiana_domain::project_memory_facts(&committed).map_err(failed)?;
        if !projection.matches_records(&projected) {
            return Err(failed("memory_projection_lag"));
        }
    }
    Ok(())
}

/// Commit a memory fact before its JSONL projection is appended.  This synchronous helper is
/// called from `spawn_blocking`; `Handle::block_on` therefore waits on the EventStore without
/// blocking the Tokio worker thread that owns the request.
fn journal_memory_fact(
    scope: &MemoryScope,
    request_id: Option<kiana_domain::RequestId>,
    project_root: &str,
    collection: &str,
    session_id: &str,
    operation: &str,
    mutation_key: &str,
    record: &MemoryRecord,
    mutation: Option<&MemoryMutation>,
    stage: Option<MemoryMutationJournalStage>,
) -> Result<(u64, bool), PortError> {
    let Some(events) = scope.events.clone() else {
        return Ok((0, false));
    };
    let request_id = request_id.ok_or_else(|| failed("memory_journal_request_required"))?;
    let stream_id = memory_stream_id(scope, project_root, collection, session_id);
    let event_key = format!("memory.fact:{stream_id}:{mutation_key}");
    let handle = tokio::runtime::Handle::try_current()
        .map_err(|_| failed("memory_journal_runtime_unavailable"))?;
    handle.block_on(async move {
        let history = events.read_stream(MEMORY_STREAM, &stream_id).await?;
        let projection = kiana_domain::project_memory_facts(&history).map_err(failed)?;
        let expected = projection.source_cursor;
        if let Some(previous) = history
            .iter()
            .find(|event| event.idempotency_key.as_deref() == Some(event_key.as_str()))
        {
            let fact: MemoryJournalFact = serde_json::from_value(previous.data.clone())
                .map_err(|_| failed("memory_fact_decode_failed"))?;
            if fact.operation != operation
                || json_digest(&serde_json::to_value(&fact.record).unwrap_or(Value::Null))
                    != json_digest(&serde_json::to_value(record).unwrap_or(Value::Null))
            {
                return Err(PortError::Conflict(
                    "memory_journal_idempotency_payload_mismatch".to_owned(),
                ));
            }
            if let Some(mutation) = mutation {
                let Some(journal) = fact.mutation.as_ref() else {
                    return Err(failed("memory_mutation_journal_missing"));
                };
                if journal.mutation_digest != mutation.digest() {
                    return Err(PortError::Conflict(
                        "memory_mutation_journal_digest_mismatch".to_owned(),
                    ));
                }
            }
            return Ok((previous.stream_version.unwrap_or(expected), true));
        }
        let version = expected
            .checked_add(1)
            .ok_or_else(|| failed("memory_journal_version_exhausted"))?;
        let mut fact = MemoryJournalFact::new(
            operation,
            mutation_key,
            record.clone(),
            MemoryBodyRef::new(&stream_id, &record.content_hash),
        );
        if let (Some(mutation), Some(stage)) = (mutation, stage) {
            fact = fact
                .with_mutation(mutation.clone(), stage)
                .map_err(failed)?;
        }
        fact.validate().map_err(failed)?;
        let event = RuntimeEvent::new(
            request_id,
            1,
            MEMORY_FACT_EVENT_KIND,
            serde_json::to_value(fact).map_err(|_| failed("memory_fact_encode_failed"))?,
        )
        .map_err(|error| failed(format!("memory_fact_event_invalid:{error}")))?
        .with_stream_metadata(MEMORY_STREAM, stream_id, version)
        .with_idempotency_key(event_key);
        let result = events
            .append_idempotent_expected(event, Some(expected))
            .await?;
        Ok((
            result.event.stream_version.unwrap_or(version),
            result.replayed,
        ))
    })
}
pub(crate) fn register(
    broker: &mut CapabilityBroker,
    events: std::sync::Arc<dyn EventStorePort>,
) -> Result<(), PortError> {
    let scope = MemoryScope::capture(Some(events));
    broker.register_static(
        CapabilityKind::Query,
        SEARCH_OPERATION,
        std::sync::Arc::new(MemorySearchHandler(scope.clone())),
    )?;
    broker.register_static(
        CapabilityKind::Filesystem,
        WRITE_OPERATION,
        std::sync::Arc::new(MemoryWriteHandler(scope.clone())),
    )?;
    broker.register_static(
        CapabilityKind::Filesystem,
        REVIEW_OPERATION,
        std::sync::Arc::new(MemoryReviewHandler(scope)),
    )
}

struct MemorySearchHandler(MemoryScope);

#[async_trait::async_trait]
impl CapabilityHandler for MemorySearchHandler {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        if request.request.operation != SEARCH_OPERATION {
            return Err(PortError::Failed("harness_operation_mismatch".to_owned()));
        }
        let request_id = request.request.request_id;
        let arguments = request.request.arguments.clone();
        self.0.check(&arguments)?;
        server_memory_scope(&request, &arguments, false)?;
        ensure_memory_projection(&self.0, &arguments).await?;
        let scope = self.0.clone();
        let output = tokio::task::spawn_blocking(move || search_records_scoped(&arguments, &scope))
            .await
            .map_err(|error| PortError::Failed(format!("memory_search_join_failed:{error}")))??;
        let result = CapabilityResult::success(request_id, output);
        kiana_domain::attach_adapter_result(
            result,
            AdapterResultKind::Memory,
            AdapterCommitState::Committed,
        )
        .map_err(|error| failed(format!("adapter_result_invalid:{error}")))
    }
}

struct MemoryWriteHandler(MemoryScope);

#[async_trait::async_trait]
impl CapabilityHandler for MemoryWriteHandler {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        if request.request.operation != WRITE_OPERATION {
            return Err(PortError::Failed("harness_operation_mismatch".to_owned()));
        }
        let request_id = request.request.request_id;
        let arguments = request.request.arguments.clone();
        self.0.check(&arguments)?;
        let request_scope = server_memory_scope(&request, &arguments, true)?;
        let scope = self.0.clone();
        let output = tokio::task::spawn_blocking(move || {
            write_record_with_mutation(&arguments, &scope, Some(request_id), Some(&request_scope))
        })
        .await
        .map_err(|error| PortError::Failed(format!("memory_write_join_failed:{error}")))??;
        let result = CapabilityResult::success(request_id, output);
        kiana_domain::attach_adapter_result(
            result,
            AdapterResultKind::Memory,
            AdapterCommitState::Committed,
        )
        .map_err(|error| failed(format!("adapter_result_invalid:{error}")))
    }
}

struct MemoryReviewHandler(MemoryScope);
#[async_trait::async_trait]
impl CapabilityHandler for MemoryReviewHandler {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        if request.request.operation != REVIEW_OPERATION || request.request.cell_id.is_some() {
            return Err(failed("memory_review_operator_required"));
        }
        let request_id = request.request.request_id;
        let arguments = request.request.arguments.clone();
        self.0.check(&arguments)?;
        let request_scope = server_memory_scope(&request, &arguments, true)?;
        ensure_memory_projection(&self.0, &arguments).await?;
        let scope = self.0.clone();
        let output = tokio::task::spawn_blocking(move || {
            review_records_with_mutation(&arguments, &scope, Some(request_id), Some(&request_scope))
        })
        .await
        .map_err(|error| failed(format!("memory_review_join_failed:{error}")))??;
        let result = CapabilityResult::success(request_id, output);
        kiana_domain::attach_adapter_result(
            result,
            AdapterResultKind::Memory,
            AdapterCommitState::Committed,
        )
        .map_err(|error| failed(format!("adapter_result_invalid:{error}")))
    }
}

#[cfg(test)]
fn search_records(arguments: &Value) -> Result<Value, PortError> {
    search_records_scoped(arguments, &MemoryScope::capture(None))
}
fn search_records_scoped(arguments: &Value, scope: &MemoryScope) -> Result<Value, PortError> {
    let query = required_string(arguments, "query", "memory_query_required")?;
    let terms = memory_query_terms(&query);
    let role = RoleSpec::lookup(&optional_string(arguments, "role_id").unwrap_or_default())
        .ok_or_else(|| failed("role_unknown"))?;
    let collections = requested_collections(arguments, &role)?;
    let session_id = optional_string(arguments, "session_id").unwrap_or_default();
    let project_root = optional_string(arguments, "project_root").unwrap_or_default();
    let data_policy = crate::data_governance::read_policy(Path::new(&project_root))?;
    let limit = arguments
        .get("limit")
        .and_then(Value::as_u64)
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_LIMIT as u64)
        .min(MAX_LIMIT as u64) as usize;
    let mut records = Vec::new();
    for collection in collections {
        let path = collection_path_scoped(
            &collection,
            &project_root,
            &session_id,
            scope.home.as_deref(),
        )?;
        for record in read_records(&path)? {
            if record.collection != collection.collection
                || (collection.layer == MEMORY_LAYER_INSTANCE_SCRATCH
                    && record.session_id != session_id)
            {
                continue;
            }
            let fence = MemoryProjectionFence::evaluate(
                &record,
                &data_policy,
                data_policy.data_epoch,
                &project_root,
                now_ms(),
            )
            .map_err(failed)?;
            if fence.searchable() {
                records.push(record);
            }
        }
    }
    let superseded = records
        .iter()
        .filter_map(|record| record.supersedes.as_ref())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    records.retain(|record| !superseded.contains(&record.id));
    records.sort_by(|a, b| (&a.collection, &a.id).cmp(&(&b.collection, &b.id)));
    let mut hits = crate::memory_retrieval::rank_records(&records, &query, limit)?;
    for hit in &mut hits {
        hit["query"] = json!(query);
        hit["retrieved_by"] = json!(role.role_id);
        hit["retrieval_session_id"] = json!(session_id);
    }
    Ok(
        json!({"schema":MEMORY_SEARCH_SCHEMA,"query":query,"terms":terms,
        "role_id":role.role_id,"department_id":role.department_id,"session_id":session_id,
        "data_epoch":data_policy.data_epoch,"hits":hits}),
    )
}

#[cfg(test)]
fn write_record(arguments: &Value) -> Result<Value, PortError> {
    write_record_scoped(arguments, &MemoryScope::capture(None), None)
}
fn write_record_scoped(
    arguments: &Value,
    scope: &MemoryScope,
    request_id: Option<kiana_domain::RequestId>,
) -> Result<Value, PortError> {
    write_record_with_mutation(arguments, scope, request_id, None)
}

fn write_record_with_mutation(
    arguments: &Value,
    scope: &MemoryScope,
    request_id: Option<kiana_domain::RequestId>,
    request_scope: Option<&ServerMemoryScope>,
) -> Result<Value, PortError> {
    let collection = required_collection(arguments)?;
    if collection.collection == "user-private" {
        return Err(failed("memory_user_private_requires_operator"));
    }
    let text = required_string(arguments, "text", "memory_text_required")?;
    let source = required_string(arguments, "source", "memory_source_required")?;
    if text.len() > 64 * 1024 || source.len() > 4096 {
        return Err(failed("memory_record_too_large"));
    }
    let role_id = required_string(arguments, "role_id", "role_unknown")?;
    let role = RoleSpec::lookup(&role_id).ok_or_else(|| failed("role_unknown"))?;
    if !role.allows_memory_write(&collection.collection) {
        return Err(failed("role_memory_write_denied"));
    }
    // The server scope owns session, project and epochs; arguments are only a fallback for the
    // scoped test entry points that have no authorized request behind them.
    let session_id = match request_scope {
        Some(server) => server.session_id.clone(),
        None => required_string(arguments, "session_id", "memory_session_required")?,
    };
    let project_root = match request_scope {
        Some(server) => server.project_root.clone(),
        None => optional_string(arguments, "project_root").unwrap_or_default(),
    };
    let mutation_scope = request_scope.map(|server| &server.acl);
    let (policy_epoch, data_epoch) = request_scope
        .map(|server| (server.policy_epoch, server.data_epoch))
        .unwrap_or((1, 1));
    let scratch = collection.layer == MEMORY_LAYER_INSTANCE_SCRATCH;
    let mutation_key = optional_string(arguments, "idempotency_key").unwrap_or_else(|| {
        format!(
            "memory.write:{}",
            request_id.unwrap_or_else(kiana_domain::RequestId::new)
        )
    });
    validate_memory_mutation_key(&mutation_key)?;
    let record = MemoryRecord {
        project_root: project_root.clone(),
        schema: MEMORY_RECORD_SCHEMA_V2.to_owned(),
        id: stable_memory_record_id(&mutation_key),
        layer: collection.layer.clone(),
        collection: collection.collection.clone(),
        content_hash: format!("{:x}", Sha256::digest(text.as_bytes())),
        kind: "fact".to_owned(),
        evidence: Vec::new(),
        supersedes: None,
        text,
        source,
        role_id: role.role_id,
        department_id: role.department_id,
        session_id: session_id.clone(),
        created_at_ms: now_ms(),
        origin: MemoryOrigin::Model,
        admission_state: if scratch {
            MemoryAdmission::Ephemeral
        } else {
            MemoryAdmission::Candidate
        },
        state: if scratch {
            MemoryState::Active
        } else {
            MemoryState::Draft
        },
        classification: MemoryClassification::for_collection(&collection),
        purpose: Some(Purpose {
            id: if scratch {
                "memory.scratch".to_owned()
            } else {
                "memory.candidate".to_owned()
            },
            description: "server-derived memory write purpose".to_owned(),
        }),
        sensitivity: MemorySensitivity::Internal,
        validity: Default::default(),
        retention: None,
        dependencies: Vec::new(),
        import_mode: kiana_domain::MemoryImportMode::Native,
        revision: 1,
        last_mutation_key: Some(mutation_key.clone()),
        reviewed_by: None,
        review_reason: None,
        reviewed_at_ms: None,
    };
    let path = collection_path_scoped(
        &collection,
        &project_root,
        &session_id,
        scope.home.as_deref(),
    )?;
    let mut file = open_record_file(&path, true, true)?;
    let existing = read_records_file(&file)?;
    let policy = crate::data_governance::read_policy(Path::new(&project_root))?;
    if policy
        .revoked_sources
        .iter()
        .any(|source| record.source.contains(source))
    {
        return Err(failed("data_source_revoked"));
    }
    let (record, replayed) =
        if let Some(previous) = existing.iter().find(|previous| previous.id == record.id) {
            if previous.content_hash != record.content_hash
                || previous.text != record.text
                || previous.source != record.source
                || previous.collection != record.collection
                || previous.session_id != record.session_id
                || previous.project_root != record.project_root
                || previous.role_id != record.role_id
                || previous.department_id != record.department_id
            {
                return Err(failed("memory_idempotency_conflict"));
            }
            (previous.clone(), true)
        } else {
            (record, false)
        };
    let mut journal_cursor = 0u64;
    let mut journal_replayed = false;
    let mutation_receipt = if let Some(mutation_scope) = mutation_scope {
        let payload = serde_json::to_value(&record).map_err(|_| failed("memory_record_invalid"))?;
        let mutation = MemoryMutation::new(
            format!("memory.write:{}", record.id),
            MemoryMutationOperation::Add,
            mutation_scope.principal.principal_id.clone(),
            mutation_scope.clone(),
            vec![MemoryMutationTarget::new(&record.id, &record.collection, 0)],
            vec![memory_source_ref(&record.id, &record.source, &record.text)?],
            policy_epoch,
            data_epoch,
            mutation_key.clone(),
            &payload,
        )
        .map_err(failed)?;
        if replayed {
            let (cursor, replayed_event) = journal_memory_fact(
                scope,
                request_id,
                &project_root,
                &record.collection,
                &record.session_id,
                "write",
                &mutation_key,
                &record,
                Some(&mutation),
                Some(if scratch {
                    MemoryMutationJournalStage::Ephemeral
                } else {
                    MemoryMutationJournalStage::Candidate
                }),
            )?;
            journal_cursor = cursor;
            journal_replayed = replayed_event;
            Some(replayed_memory_receipt(&mutation, record.revision)?)
        } else {
            let mut ledger = MemoryMutationLedger::new();
            for previous in &existing {
                ledger
                    .seed_record(&previous.id, &previous.collection, previous.revision)
                    .map_err(failed)?;
            }
            let MemoryMutationOutcome::Committed { receipt } =
                ledger.apply(mutation.clone()).map_err(failed)?
            else {
                return Err(failed("memory_mutation_unexpected_replay"));
            };
            let (cursor, replayed_event) = journal_memory_fact(
                scope,
                request_id,
                &project_root,
                &record.collection,
                &record.session_id,
                "write",
                &mutation_key,
                &record,
                Some(&mutation),
                Some(if scratch {
                    MemoryMutationJournalStage::Ephemeral
                } else {
                    MemoryMutationJournalStage::Candidate
                }),
            )?;
            journal_cursor = cursor;
            journal_replayed = replayed_event;
            append_record_file(&mut file, &record)?;
            Some(receipt)
        }
    } else {
        if !replayed {
            append_record_file(&mut file, &record)?;
        }
        None
    };
    Ok(
        json!({"schema":MEMORY_WRITE_SCHEMA,"id":record.id,"layer":record.layer,
        "collection":record.collection,"source":record.source,"path":path.display().to_string(),
        "promoted":false,"origin":record.origin,"admission_state":record.admission_state,
        "state":record.state,"revision":record.revision,"classification":record.classification,
        "effect_committed":true,"write_synced":true,"replayed":replayed,
        "journaled":scope.events.is_some(),"journal_replayed":journal_replayed,"projection_cursor":journal_cursor,
        "idempotency_key":mutation_key,"mutation_receipt":mutation_receipt,
        "stop_confirmed":true}),
    )
}

/// This operation is not in the model's five-tool registry. The ControlPlane command
/// boundary stamps the operator identity and requires approval of the exact revision.
#[cfg(test)]
fn review_records(arguments: &Value) -> Result<Value, PortError> {
    review_records_scoped(arguments, &MemoryScope::capture(None))
}
fn review_records_scoped(arguments: &Value, scope: &MemoryScope) -> Result<Value, PortError> {
    review_records_with_mutation(arguments, scope, None, None)
}

fn review_records_with_mutation(
    arguments: &Value,
    scope: &MemoryScope,
    request_id: Option<kiana_domain::RequestId>,
    request_scope: Option<&ServerMemoryScope>,
) -> Result<Value, PortError> {
    if arguments
        .get("operator_authorized")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Err(failed("memory_review_operator_required"));
    }
    let actor = required_string(arguments, "actor_id", "memory_review_operator_required")?;
    let action = required_string(arguments, "action", "memory_review_action_required")?;
    let collection = required_collection(arguments)?;
    if collection.layer == MEMORY_LAYER_INSTANCE_SCRATCH {
        return Err(failed("memory_scratch_not_promotable"));
    }
    let role = RoleSpec::lookup(&optional_string(arguments, "role_id").unwrap_or_default())
        .ok_or_else(|| failed("role_unknown"))?;
    if !role.allows_knowledge(&collection.collection) {
        return Err(failed("role_knowledge_denied"));
    }
    let project = match request_scope {
        Some(server) => server.project_root.clone(),
        None => optional_string(arguments, "project_root").unwrap_or_default(),
    };
    let session = match request_scope {
        Some(server) => server.session_id.clone(),
        None => optional_string(arguments, "session_id").unwrap_or_default(),
    };
    // The server scope owns the collection ACL and the epochs; arguments are only a fallback for
    // the scoped test entry points that have no authorized request behind them.
    let mutation_scope = request_scope.map(|server| &server.acl);
    let (policy_epoch, data_epoch) = request_scope
        .map(|server| (server.policy_epoch, server.data_epoch))
        .unwrap_or((1, 1));
    let path = collection_path_scoped(&collection, &project, &session, scope.home.as_deref())?;
    if action == "accept_proposal" {
        if scope.events.is_some() {
            return Err(failed("memory_proposal_event_journal_required"));
        }
        return accept_proposal(arguments, &collection, &path, &actor);
    }
    if action == "list" {
        let records = read_records(&path)?
            .into_iter()
            .filter(|r| {
                r.collection == collection.collection
                    && r.admission_state == MemoryAdmission::Candidate
            })
            .map(|r| r.hit())
            .collect::<Vec<_>>();
        return Ok(
            json!({"schema":MEMORY_REVIEW_SCHEMA,"action":action,"collection":collection.collection,"candidates":records}),
        );
    }
    if action != "promote" && action != "reject" {
        return Err(failed("memory_review_action_invalid"));
    }
    let id = required_string(arguments, "record_id", "memory_record_id_required")?;
    let reason = required_string(arguments, "reason", "memory_review_reason_required")?;
    let expected = arguments
        .get("expected_revision")
        .and_then(Value::as_u64)
        .filter(|r| *r > 0)
        .ok_or_else(|| failed("memory_review_revision_required"))?;
    let mutation_key = optional_string(arguments, "idempotency_key")
        .unwrap_or_else(|| format!("memory.review:{}:{}:{}", actor, id, expected));
    validate_memory_mutation_key(&mutation_key)?;
    let mut file = open_record_file(&path, false, true)?;
    let existing = read_records_file(&file)?;
    let mut record = existing
        .iter()
        .find(|r| r.id == id && r.collection == collection.collection)
        .cloned()
        .ok_or_else(|| failed("memory_record_not_found"))?;
    if record.revision != expected {
        if mutation_scope.is_some()
            && record.last_mutation_key.as_deref() == Some(mutation_key.as_str())
        {
            if record.review_reason.as_deref() != Some(reason.as_str())
                || ((action == "promote") != (record.admission_state == MemoryAdmission::Qualified))
            {
                return Err(failed("memory_idempotency_conflict"));
            }
            let operation = if action == "promote" {
                MemoryMutationOperation::Approve
            } else {
                MemoryMutationOperation::Revoke
            };
            let payload = json!({"action":action,"record_id":record.id,"reason":reason,
                "expected_revision":expected,"collection":collection.collection});
            let mutation_scope = mutation_scope.expect("checked above");
            let mutation = MemoryMutation::new(
                format!("memory.review:{}", record.id),
                operation,
                mutation_scope.principal.principal_id.clone(),
                mutation_scope.clone(),
                vec![MemoryMutationTarget::new(
                    &record.id,
                    &record.collection,
                    expected,
                )],
                vec![memory_source_ref(&record.id, &record.source, &record.text)?],
                policy_epoch,
                data_epoch,
                mutation_key.clone(),
                &payload,
            )
            .map_err(failed)?
            .with_authority(MemoryMutationAuthority::Human)
            .map_err(failed)?;
            let receipt = replayed_memory_receipt(&mutation, record.revision)?;
            return Ok(
                json!({"schema":MEMORY_REVIEW_SCHEMA,"action":action,"record":record.hit(),
                "revision":record.revision,"idempotency_key":mutation_key,
                "mutation_receipt":receipt,"replayed":true}),
            );
        }
        return Err(PortError::Conflict("memory_revision_conflict".to_owned()));
    }
    if record.admission_state != MemoryAdmission::Candidate || record.state != MemoryState::Draft {
        return Err(PortError::Conflict("memory_candidate_required".to_owned()));
    }
    if record.import_mode == kiana_domain::MemoryImportMode::LegacyImport {
        return Err(failed("memory_legacy_import_requires_native_successor"));
    }
    if action == "promote" && record.evidence.is_empty() {
        return Err(failed("memory_review_evidence_required"));
    }
    let operation = if action == "promote" {
        MemoryMutationOperation::Approve
    } else {
        MemoryMutationOperation::Revoke
    };
    let payload = json!({"action":action,"record_id":record.id,"reason":reason,
        "expected_revision":expected,"collection":collection.collection});
    let journal_mutation = if let Some(mutation_scope) = mutation_scope {
        Some(
            MemoryMutation::new(
                format!("memory.review:{}", record.id),
                operation,
                mutation_scope.principal.principal_id.clone(),
                mutation_scope.clone(),
                vec![MemoryMutationTarget::new(
                    &record.id,
                    &record.collection,
                    expected,
                )],
                vec![memory_source_ref(&record.id, &record.source, &record.text)?],
                policy_epoch,
                data_epoch,
                mutation_key.clone(),
                &payload,
            )
            .map_err(failed)?
            .with_authority(MemoryMutationAuthority::Human)
            .map_err(failed)?,
        )
    } else {
        None
    };
    let mutation_receipt = if let Some(mutation) = journal_mutation.clone() {
        let mut ledger = MemoryMutationLedger::new();
        for previous in &existing {
            ledger
                .seed_record(&previous.id, &previous.collection, previous.revision)
                .map_err(failed)?;
        }
        let MemoryMutationOutcome::Committed { receipt } =
            ledger.apply(mutation).map_err(failed)?
        else {
            return Err(failed("memory_mutation_unexpected_replay"));
        };
        Some(receipt)
    } else {
        None
    };
    record.schema = MEMORY_RECORD_SCHEMA_V2.to_owned();
    record.revision = record
        .revision
        .checked_add(1)
        .ok_or_else(|| failed("memory_revision_exhausted"))?;
    record.admission_state = if action == "promote" {
        MemoryAdmission::Qualified
    } else {
        MemoryAdmission::Rejected
    };
    record.state = if action == "promote" {
        MemoryState::Active
    } else {
        MemoryState::Rejected
    };
    record.classification = MemoryClassification::for_collection(&collection);
    record.last_mutation_key = Some(mutation_key.clone());
    record.reviewed_by = Some(actor);
    record.review_reason = Some(reason);
    record.reviewed_at_ms = Some(now_ms());
    record.validate_lifecycle().map_err(failed)?;
    let (journal_cursor, journal_replayed) = if mutation_scope.is_some() {
        journal_memory_fact(
            scope,
            request_id,
            &project,
            &record.collection,
            &record.session_id,
            "review",
            &mutation_key,
            &record,
            journal_mutation.as_ref(),
            Some(if action == "promote" {
                MemoryMutationJournalStage::Approve
            } else {
                MemoryMutationJournalStage::Tombstone
            }),
        )?
    } else {
        (0, false)
    };
    append_record_file(&mut file, &record)?;
    Ok(
        json!({"schema":MEMORY_REVIEW_SCHEMA,"action":action,"record":record.hit(),"revision":record.revision,
            "idempotency_key":mutation_key,"mutation_receipt":mutation_receipt,
            "journaled":scope.events.is_some(),"journal_replayed":journal_replayed,"projection_cursor":journal_cursor}),
    )
}

/// Materialization is reachable only after a human approved a server-resolved proposal.
fn accept_proposal(
    arguments: &Value,
    collection: &MemoryCollection,
    path: &Path,
    actor: &str,
) -> Result<Value, PortError> {
    if arguments
        .get("proposal_authorized")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Err(failed("memory_proposal_authority_required"));
    }
    let proposal: kiana_domain::MemoryProposal = serde_json::from_value(
        arguments
            .get("proposal")
            .cloned()
            .ok_or_else(|| failed("memory_proposal_required"))?,
    )
    .map_err(|_| failed("memory_proposal_invalid"))?;
    proposal.validate().map_err(failed)?;
    let project = required_string(arguments, "project_root", "memory_project_required")?;
    if fs::canonicalize(&proposal.project_root)
        .map_err(|_| failed("memory_proposal_project_mismatch"))?
        != fs::canonicalize(&project).map_err(|_| failed("memory_proposal_project_mismatch"))?
    {
        return Err(failed("memory_proposal_project_mismatch"));
    }
    let role = RoleSpec::lookup(&proposal.role_id)
        .ok_or_else(|| failed("memory_proposal_role_invalid"))?;
    if role.department_id != proposal.department_id {
        return Err(failed("memory_proposal_role_invalid"));
    }
    if proposal
        .facts
        .iter()
        .any(|fact| fact.collection != collection.collection)
    {
        return Err(failed("memory_proposal_collection_mismatch"));
    }
    let reason = required_string(arguments, "reason", "memory_review_reason_required")?;
    let mut file = open_record_file(path, true, true)?;
    let mut records = read_records_file(&file)?;
    let mut additions = Vec::new();
    let mut results = Vec::new();
    for (index, fact) in proposal.facts.iter().enumerate() {
        let id = format!("{}:{index}", proposal.id);
        let content_hash = format!("{:x}", Sha256::digest(fact.text.as_bytes()));
        if let Some(existing) = records.iter().find(|record| record.id == id) {
            if existing.content_hash != content_hash {
                return Err(PortError::Conflict(
                    "memory_proposal_replay_conflict".to_owned(),
                ));
            }
            results.push(existing.hit());
            continue;
        }
        let target = if fact.operation != kiana_domain::MemorySuggestion::Add {
            let target_id = fact
                .target_record_id
                .as_ref()
                .ok_or_else(|| failed("memory_proposal_target_required"))?;
            let reference = fact
                .similar_records
                .iter()
                .find(|record| record.get("id").and_then(Value::as_str) == Some(target_id.as_str()))
                .ok_or_else(|| failed("memory_proposal_target_evidence_required"))?;
            let expected = reference
                .get("revision")
                .and_then(Value::as_u64)
                .ok_or_else(|| failed("memory_review_revision_required"))?;
            let target = records
                .iter()
                .find(|record| {
                    record.id == *target_id && record.collection == collection.collection
                })
                .ok_or_else(|| failed("memory_record_not_found"))?;
            if target.revision != expected || !target.searchable() {
                return Err(PortError::Conflict("memory_revision_conflict".to_owned()));
            }
            Some(target.clone())
        } else {
            None
        };
        let record = if fact.operation == kiana_domain::MemorySuggestion::Delete {
            let mut target = target.ok_or_else(|| failed("memory_record_not_found"))?;
            target.revision = target
                .revision
                .checked_add(1)
                .ok_or_else(|| failed("memory_revision_exhausted"))?;
            target.admission_state = MemoryAdmission::Rejected;
            target.state = MemoryState::Rejected;
            target.reviewed_by = Some(actor.to_owned());
            target.review_reason = Some(reason.clone());
            target.reviewed_at_ms = Some(now_ms());
            target
        } else {
            MemoryRecord {
                project_root: arguments["project_root"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                schema: MEMORY_RECORD_SCHEMA_V2.to_owned(),
                id,
                layer: collection.layer.clone(),
                collection: collection.collection.clone(),
                text: fact.text.clone(),
                source: format!("event:{}", fact.evidence[0].event_id),
                role_id: proposal.role_id.clone(),
                department_id: proposal.department_id.clone(),
                session_id: proposal.session_id.to_string(),
                created_at_ms: now_ms(),
                kind: fact.kind.clone(),
                evidence: fact.evidence.clone(),
                content_hash,
                supersedes: target.map(|record| record.id),
                origin: proposal.origin,
                admission_state: MemoryAdmission::Qualified,
                state: MemoryState::Active,
                classification: MemoryClassification::for_collection(collection),
                purpose: Some(Purpose {
                    id: "memory.review".to_owned(),
                    description: "reviewed memory fact".to_owned(),
                }),
                sensitivity: MemorySensitivity::Internal,
                validity: Default::default(),
                retention: None,
                dependencies: Vec::new(),
                import_mode: kiana_domain::MemoryImportMode::Native,
                revision: 1,
                last_mutation_key: None,
                reviewed_by: Some(actor.to_owned()),
                review_reason: Some(reason.clone()),
                reviewed_at_ms: Some(now_ms()),
            }
        };
        record.validate_lifecycle().map_err(failed)?;
        results.push(record.hit());
        records.retain(|existing| existing.id != record.id);
        records.push(record.clone());
        additions.push(record);
    }
    let mut bytes = Vec::new();
    for record in additions {
        serde_json::to_writer(&mut bytes, &record).map_err(|_| failed("memory_record_invalid"))?;
        bytes.push(b'\n');
    }
    if !bytes.is_empty() {
        file.write_all(&bytes)
            .and_then(|_| file.sync_data())
            .map_err(|_| failed("result_unknown:memory_proposal_persistence_failed"))?;
    }
    Ok(
        json!({"schema":MEMORY_REVIEW_SCHEMA,"action":"accept_proposal","proposal_id":proposal.id,"collection":collection.collection,"records":results}),
    )
}

fn requested_collections(
    arguments: &Value,
    role: &RoleSpec,
) -> Result<Vec<MemoryCollection>, PortError> {
    match optional_string(arguments, "collection") {
        Some(raw) => {
            let collection = MemoryCollection::parse(&raw)
                .ok_or_else(|| PortError::Failed("memory_collection_unknown".to_owned()))?;
            if !role.allows_knowledge(&collection.collection) {
                return Err(PortError::Failed("role_knowledge_denied".to_owned()));
            }
            Ok(vec![collection])
        }
        None => {
            let mut collections = role.granted_collections();
            collections.dedup();
            Ok(collections)
        }
    }
}

fn required_collection(arguments: &Value) -> Result<MemoryCollection, PortError> {
    let raw = required_string(arguments, "collection", "memory_collection_required")?;
    MemoryCollection::parse(&raw)
        .ok_or_else(|| PortError::Failed("memory_collection_unknown".to_owned()))
}

#[cfg(test)]
fn collection_path(
    collection: &MemoryCollection,
    project_root: &str,
    session_id: &str,
) -> Result<PathBuf, PortError> {
    collection_path_scoped(
        collection,
        project_root,
        session_id,
        MemoryScope::capture(None).home.as_deref(),
    )
}
fn collection_path_scoped(
    collection: &MemoryCollection,
    project_root: &str,
    session_id: &str,
    storage_home: Option<&Path>,
) -> Result<PathBuf, PortError> {
    if collection.home_scoped() {
        let home = storage_home
            .filter(|path| path.is_absolute())
            .ok_or_else(|| failed("memory_home_required"))?;
        let name = match collection.collection.as_str() {
            MEMORY_LAYER_COMPANY => "company.jsonl",
            MEMORY_LAYER_USER => "user.jsonl",
            "user:prefs" => "user-prefs.jsonl",
            "user-private" => "user-private.jsonl",
            other => {
                return Err(PortError::Failed(format!(
                    "memory_collection_unknown:{other}"
                )));
            }
        };
        return Ok(home.join("memory").join(name));
    }
    let root = confined_project_root(project_root)?;
    let base = root.join(".kiana").join("memory");
    let path = match collection.layer.as_str() {
        MEMORY_LAYER_DEPARTMENT => {
            let name = if collection.collection == "planning:unreleased-debate" {
                "planning-unreleased.jsonl".to_owned()
            } else {
                let id = collection
                    .collection
                    .strip_prefix("department:")
                    .unwrap_or(&collection.collection);
                format!("{id}.jsonl")
            };
            base.join("department").join(name)
        }
        MEMORY_LAYER_ROLE => {
            let id = collection
                .collection
                .strip_prefix("role:")
                .unwrap_or(&collection.collection);
            base.join("role").join(format!("{id}.jsonl"))
        }
        MEMORY_LAYER_PROJECT => {
            let name = match collection.collection.as_str() {
                MEMORY_LAYER_PROJECT => "project.jsonl",
                "project:code" => "code.jsonl",
                "project:docs" => "docs.jsonl",
                "project:events" => "events.jsonl",
                other => {
                    return Err(PortError::Failed(format!(
                        "memory_collection_unknown:{other}"
                    )));
                }
            };
            base.join("project").join(name)
        }
        MEMORY_LAYER_INSTANCE_SCRATCH => {
            let session = session_id.trim();
            if session.is_empty() {
                return Err(PortError::Failed("memory_session_required".to_owned()));
            }
            if session.contains('/') || session.contains('\\') || session.contains("..") {
                return Err(PortError::Failed("memory_session_invalid".to_owned()));
            }
            base.join("instance").join(format!("{session}.jsonl"))
        }
        other => {
            return Err(PortError::Failed(format!(
                "memory_collection_unknown:{other}"
            )));
        }
    };
    Ok(path)
}

pub(crate) fn kiana_home() -> Result<PathBuf, PortError> {
    let raw = std::env::var(KIANA_HOME_ENV)
        .map_err(|_| PortError::Failed("memory_home_required".to_owned()))?;
    let path = PathBuf::from(raw.trim());
    if !path.is_absolute() {
        return Err(PortError::Failed("memory_home_required".to_owned()));
    }
    Ok(path)
}

fn confined_project_root(project_root: &str) -> Result<PathBuf, PortError> {
    let root = PathBuf::from(project_root.trim());
    if project_root.trim().is_empty() || !root.is_absolute() {
        return Err(PortError::Failed("memory_project_required".to_owned()));
    }
    Ok(root)
}

fn stable_memory_record_id(idempotency_key: &str) -> String {
    format!("mem-{:x}", Sha256::digest(idempotency_key.as_bytes()))
}

fn validate_memory_mutation_key(key: &str) -> Result<(), PortError> {
    if key.trim().is_empty() || key.len() > 256 || key.contains('\0') {
        return Err(failed("memory_mutation_idempotency_key_invalid"));
    }
    Ok(())
}

fn memory_source_ref(record_id: &str, source: &str, text: &str) -> Result<SourceRef, PortError> {
    SourceRef::new(
        format!("memory-mutation:{record_id}"),
        SourceKind::Memory,
        if source.trim().is_empty() {
            format!("memory:{record_id}")
        } else {
            source.to_owned()
        },
        "mutation-input:v1",
        json_digest(&json!({"source":source,"text":text})),
        None,
        if source.trim().is_empty() {
            EvidenceStatus::Unverifiable
        } else {
            EvidenceStatus::Attributed
        },
    )
    .map_err(failed)
}

fn replayed_memory_receipt(
    mutation: &MemoryMutation,
    revision: u64,
) -> Result<MemoryMutationReceipt, PortError> {
    let receipt = MemoryMutationReceipt {
        schema: kiana_domain::MEMORY_MUTATION_RECEIPT_SCHEMA.to_owned(),
        mutation_id: mutation.mutation_id.clone(),
        operation: mutation.operation,
        actor: mutation.actor.clone(),
        scope_digest: mutation.scope.scope_digest.clone(),
        idempotency_key: mutation.idempotency_key.clone(),
        mutation_digest: mutation.digest(),
        committed_revision: revision,
        target_revisions: mutation
            .expected_revisions
            .iter()
            .map(|target| kiana_domain::MemoryMutationTargetRevision {
                record_id: target.record_id.clone(),
                collection: target.collection.clone(),
                revision,
            })
            .collect(),
        policy_epoch: mutation.policy_epoch,
        data_epoch: mutation.data_epoch,
    };
    receipt.validate().map_err(failed)?;
    Ok(receipt)
}

fn failed(reason: impl Into<String>) -> PortError {
    PortError::Failed(reason.into())
}

fn validate_record_parent(path: &Path, create: bool) -> Result<(), PortError> {
    let parent = path.parent().ok_or_else(|| failed("memory_path_invalid"))?;
    let mut current = PathBuf::new();
    for component in parent.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                return Err(failed("memory_path_alias_denied"))
            }
            Ok(_) => (),
            Err(error) if error.kind() == ErrorKind::NotFound && create => {
                fs::create_dir(&current)
                    .map_err(|error| failed(format!("memory_write_failed:{error}")))?;
            }
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(failed(format!("memory_read_failed:{error}"))),
        }
    }
    Ok(())
}
fn open_record_file(path: &Path, create: bool, write: bool) -> Result<File, PortError> {
    validate_record_parent(path, create)?;
    let mut options = OpenOptions::new();
    options.read(true).append(write).create(create);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let file = options.open(path).map_err(|error| {
        failed(format!(
            "memory_{}_failed:{error}",
            if write { "write" } else { "read" }
        ))
    })?;
    let meta = file
        .metadata()
        .map_err(|error| failed(format!("memory_metadata_failed:{error}")))?;
    if !meta.is_file() {
        return Err(failed("memory_file_required"));
    }
    #[cfg(unix)]
    {
        if meta.nlink() != 1 {
            return Err(failed("memory_path_hardlink"));
        }
        let operation = if write { libc::LOCK_EX } else { libc::LOCK_SH };
        if unsafe { libc::flock(file.as_raw_fd(), operation) } != 0 {
            return Err(failed("memory_lock_unavailable"));
        }
    }
    #[cfg(not(unix))]
    if write {
        return Err(failed("memory_lock_unavailable"));
    }
    Ok(file)
}
fn read_records(path: &Path) -> Result<Vec<MemoryRecord>, PortError> {
    validate_record_parent(path, false)?;
    if !path_is_present(path)? {
        return Ok(Vec::new());
    }
    read_records_file(&open_record_file(path, false, false)?)
}
fn read_records_file(file: &File) -> Result<Vec<MemoryRecord>, PortError> {
    if file
        .metadata()
        .map_err(|_| failed("memory_metadata_failed"))?
        .len()
        > 64 * 1024 * 1024
    {
        return Err(failed("memory_store_limit"));
    }
    let mut records: BTreeMap<String, MemoryRecord> = BTreeMap::new();
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    loop {
        line.clear();
        if Read::by_ref(&mut reader)
            .take(256 * 1024 + 1)
            .read_line(&mut line)
            .map_err(|e| failed(format!("memory_read_failed:{e}")))?
            == 0
        {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        if !line.ends_with('\n') {
            return Err(failed("memory_record_incomplete"));
        }
        if line.len() > 256 * 1024 {
            return Err(failed("memory_record_limit"));
        }
        let raw = kiana_domain::parse_bounded_json(line.as_bytes())
            .map_err(|_| failed("memory_record_invalid"))?;
        let mut record: MemoryRecord =
            serde_json::from_value(raw.clone()).map_err(|_| failed("memory_record_invalid"))?;
        if record.schema != MEMORY_RECORD_SCHEMA && record.schema != MEMORY_RECORD_SCHEMA_V2 {
            return Err(failed("memory_record_schema_unsupported"));
        }
        // v1 predates the lifecycle/provenance contract. Import it explicitly as an
        // unverifiable draft; a legacy row can never become searchable without a new review.
        if record.schema == MEMORY_RECORD_SCHEMA {
            record = MemoryRecord::legacy_import(raw.clone()).map_err(failed)?;
        }
        record.validate_lifecycle().map_err(failed)?;
        if record.revision == 0 {
            return Err(failed("memory_record_revision_invalid"));
        }
        if let Some(previous) = records.get(&record.id) {
            if record.revision != previous.revision.saturating_add(1)
                || record.text != previous.text
                || record.source != previous.source
                || record.collection != previous.collection
                || record.origin != previous.origin
                || record.created_at_ms != previous.created_at_ms
            {
                return Err(failed("memory_revision_invalid"));
            }
        }
        records.insert(record.id.clone(), record);
    }
    Ok(records.into_values().collect())
}
#[cfg(test)]
fn append_record(path: &Path, record: &MemoryRecord) -> Result<(), PortError> {
    let mut file = open_record_file(path, true, true)?;
    // Check the tail before appending: never hide a torn or malformed earlier decision.
    read_records_file(&file)?;
    append_record_file(&mut file, record)
}
fn append_record_file(file: &mut File, record: &MemoryRecord) -> Result<(), PortError> {
    let mut encoded =
        serde_json::to_vec(record).map_err(|e| failed(format!("memory_write_failed:{e}")))?;
    encoded.push(b'\n');
    file.write_all(&encoded)
        .and_then(|_| file.sync_data())
        .map_err(|e| failed(format!("result_unknown:memory_write_unconfirmed:{e}")))
}

fn path_is_present(path: &Path) -> Result<bool, PortError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(PortError::Failed(format!("memory_read_failed:{error}"))),
    }
}

fn required_string(arguments: &Value, key: &str, error: &str) -> Result<String, PortError> {
    optional_string(arguments, key).ok_or_else(|| PortError::Failed(error.to_owned()))
}

fn optional_string(arguments: &Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_domain::MEMORY_LAYERS;

    #[cfg(unix)]
    fn temp_memory_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "kiana-memory-{label}-{}-{}",
            std::process::id(),
            now_ms()
        ))
    }

    #[test]
    fn six_layers_are_catalogued() {
        assert_eq!(
            MEMORY_LAYERS,
            [
                MEMORY_LAYER_COMPANY,
                MEMORY_LAYER_DEPARTMENT,
                MEMORY_LAYER_ROLE,
                MEMORY_LAYER_PROJECT,
                MEMORY_LAYER_USER,
                MEMORY_LAYER_INSTANCE_SCRATCH,
            ]
        );
    }

    #[test]
    fn user_and_project_paths_do_not_mix() {
        let project = PathBuf::from("/tmp/kiana-memory-project");
        let user = MemoryCollection::parse("user-private").unwrap();
        let project_col = MemoryCollection::parse("project").unwrap();
        std::env::set_var(KIANA_HOME_ENV, "/tmp/kiana-memory-home");
        let user_path = collection_path(&user, project.to_str().unwrap(), "session-1").unwrap();
        let project_path =
            collection_path(&project_col, project.to_str().unwrap(), "session-1").unwrap();
        assert!(user_path.starts_with("/tmp/kiana-memory-home/memory"));
        assert!(project_path.starts_with(project.join(".kiana").join("memory")));
        assert_ne!(user_path, project_path);
        std::env::remove_var(KIANA_HOME_ENV);
    }

    #[cfg(unix)]
    #[test]
    fn legacy_memory_is_unverifiable_until_reviewed() {
        let root = temp_memory_path("legacy-import");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("memory.jsonl");
        let row = json!({
            "schema": MEMORY_RECORD_SCHEMA,
            "id": "legacy-daemon",
            "layer": MEMORY_LAYER_PROJECT,
            "collection": MEMORY_LAYER_PROJECT,
            "text": "legacy row",
            "source": "legacy-string",
            "role_id": "builder",
            "department_id": "executing",
            "session_id": "session-legacy",
            "created_at_ms": 1
        });
        fs::write(&path, format!("{}\n", row)).unwrap();
        let records = read_records(&path).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].origin, MemoryOrigin::Unknown);
        assert_eq!(records[0].admission_state, MemoryAdmission::Candidate);
        assert_eq!(records[0].state, MemoryState::Draft);
        assert!(!records[0].searchable());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn memory_read_rejects_zero_revision_before_projection() {
        let root = temp_memory_path("zero-revision");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("memory.jsonl");
        let row = json!({
            "schema": MEMORY_RECORD_SCHEMA_V2,
            "id": "zero-revision",
            "layer": MEMORY_LAYER_PROJECT,
            "collection": MEMORY_LAYER_PROJECT,
            "text": "unpersisted revision",
            "source": "fixture",
            "role_id": "builder",
            "department_id": "executing",
            "session_id": "session-zero-revision",
            "created_at_ms": 1,
            "kind": "fact",
            "revision": 0
        });
        fs::write(&path, format!("{row}\n")).unwrap();

        let error = read_records(&path).unwrap_err();
        assert!(error.to_string().contains("memory_record_revision_invalid"));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn memory_review_without_evidence_keeps_candidate_unmodified_and_unjournaled() {
        let collection = MemoryCollection::parse("department:planning").unwrap();
        let model_candidate = MemoryRecord {
            schema: MEMORY_RECORD_SCHEMA_V2.to_owned(),
            id: "model-candidate".to_owned(),
            layer: collection.layer.clone(),
            collection: collection.collection.clone(),
            text: "model supplied candidate".to_owned(),
            source: "model-write".to_owned(),
            kind: "fact".to_owned(),
            origin: MemoryOrigin::Model,
            admission_state: MemoryAdmission::Candidate,
            state: MemoryState::Draft,
            purpose: Some(Purpose {
                id: "memory.candidate".to_owned(),
                description: "review evidence fixture".to_owned(),
            }),
            sensitivity: MemorySensitivity::Internal,
            revision: 1,
            ..MemoryRecord::default()
        };
        let legacy_import = json!({
            "schema": MEMORY_RECORD_SCHEMA,
            "id": "legacy-candidate",
            "layer": collection.layer,
            "collection": collection.collection,
            "text": "imported legacy candidate",
            "source": "legacy-source",
            "role_id": "pm",
            "department_id": "planning",
            "session_id": "session-review",
            "created_at_ms": 1
        });
        let cases = [
            (
                "model-review-evidence",
                serde_json::to_value(model_candidate).unwrap(),
                "model-candidate",
                "memory_review_evidence_required",
            ),
            (
                "legacy-review-evidence",
                legacy_import,
                "legacy-candidate",
                "memory_legacy_import_requires_native_successor",
            ),
        ];

        for (label, row, record_id, expected_error) in cases {
            let root = temp_memory_path(label);
            let project_root = root.to_string_lossy().into_owned();
            let path =
                collection_path_scoped(&collection, &project_root, "session-review", None).unwrap();
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, format!("{}\n", row)).unwrap();
            let before = fs::read_to_string(&path).unwrap();
            let events: std::sync::Arc<dyn EventStorePort> =
                std::sync::Arc::new(kiana_eventlog::MemoryEventLog::new());
            let scope = MemoryScope {
                home: None,
                events: Some(events.clone()),
            };
            let arguments = json!({
                "action": "promote",
                "collection": collection.collection,
                "project_root": project_root,
                "session_id": "session-review",
                "role_id": "pm",
                "actor_id": "operator",
                "operator_authorized": true,
                "record_id": record_id,
                "reason": "review without source evidence",
                "expected_revision": 1
            });
            let result = tokio::task::spawn_blocking(move || {
                review_records_with_mutation(
                    &arguments,
                    &scope,
                    Some(kiana_domain::RequestId::new()),
                    None,
                )
            })
            .await
            .unwrap();

            assert!(matches!(
                result,
                Err(PortError::Failed(ref code)) if code == expected_error
            ));
            assert_eq!(fs::read_to_string(&path).unwrap(), before);
            assert!(events.read_all().await.unwrap().is_empty());
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn memory_read_rejects_existing_symlink_without_reading_target() {
        use std::os::unix::fs::symlink;

        let root = temp_memory_path("read-symlink");
        fs::create_dir_all(&root).unwrap();
        let outside = root.join("outside.jsonl");
        let linked = root.join("memory.jsonl");
        let original = "outside-memory\n";
        fs::write(&outside, original).unwrap();
        symlink(&outside, &linked).unwrap();

        let error = read_records(&linked).unwrap_err();
        assert!(error.to_string().contains("memory_read_failed"));
        assert_eq!(fs::read_to_string(&outside).unwrap(), original);

        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn memory_write_rejects_existing_symlink_without_mutating_target() {
        use std::os::unix::fs::symlink;

        let root = temp_memory_path("write-symlink");
        fs::create_dir_all(&root).unwrap();
        let outside = root.join("outside.jsonl");
        let linked = root.join("memory.jsonl");
        let original = "outside-memory\n";
        fs::write(&outside, original).unwrap();
        symlink(&outside, &linked).unwrap();
        let record = MemoryRecord {
            schema: MEMORY_RECORD_SCHEMA.to_owned(),
            id: "mem-test".to_owned(),
            layer: MEMORY_LAYER_PROJECT.to_owned(),
            collection: MEMORY_LAYER_PROJECT.to_owned(),
            text: "should not write".to_owned(),
            source: "test".to_owned(),
            role_id: "builder".to_owned(),
            department_id: "executing".to_owned(),
            session_id: "session-1".to_owned(),
            created_at_ms: 1,
            ..MemoryRecord::default()
        };

        let error = append_record(&linked, &record).unwrap_err();
        assert!(error.to_string().contains("memory_write_failed"));
        assert_eq!(fs::read_to_string(&outside).unwrap(), original);

        fs::remove_dir_all(root).unwrap();
    }

    /// Server-owned execution scope used by the CAP-19 memory fixtures.  Core stamps the same
    /// project/session/role into the arguments, so a contradicting argument is a denial and the
    /// ACL the adapter derives here is the only one a write or query may use.
    #[cfg(unix)]
    fn cap19_execution_scope(
        request: &kiana_domain::CapabilityRequest,
        project_root: &str,
        session: &str,
        memory_scopes: &[&str],
    ) -> kiana_domain::ExecutionScope {
        let permission_scope = kiana_domain::ScopeSet::new(
            kiana_domain::ScopeDimension::Restricted(vec![
                "memory.search".to_owned(),
                "memory.write".to_owned(),
            ]),
            kiana_domain::ScopeDimension::NotApplicable,
            kiana_domain::ScopeDimension::NotApplicable,
            kiana_domain::ScopeDimension::NotApplicable,
            kiana_domain::ScopeLimit::NotApplicable,
            kiana_domain::ScopeLimit::NotApplicable,
        )
        .unwrap();
        let mut scope = kiana_domain::ExecutionScope {
            schema: kiana_domain::EXECUTION_SCOPE_SCHEMA.to_owned(),
            version: kiana_domain::EXECUTION_SCOPE_SCHEMA_VERSION,
            principal: kiana_domain::AuthenticatedPrincipalRef::local(),
            project: kiana_domain::ProjectIdentity::new(
                project_root.to_owned(),
                project_root.to_owned(),
                None,
                None,
                "cap19-memory-scope",
            )
            .unwrap(),
            session_id: kiana_domain::SessionId::new(session),
            run_id: None,
            turn_id: None,
            cell_id: None,
            grant_refs: Vec::new(),
            budget_lease_id: None,
            work_packet_id: None,
            environment_id: "kiana-local".to_owned(),
            workspace_revision: None,
            permission_scope: permission_scope.clone(),
            read_roots: vec![project_root.to_owned()],
            write_roots: vec![project_root.to_owned()],
            read_denies: Vec::new(),
            write_denies: Vec::new(),
            memory_scopes: memory_scopes.iter().map(|value| (*value).to_owned()).collect(),
            server_scopes: Vec::new(),
            network_policy: Vec::new(),
            authority_epoch: 1,
            trust_revision: kiana_domain::json_digest(&json!({"trusted": true})),
            data_epoch: 1,
            cancellation_epoch: 1,
            deadline_unix_ms: u64::MAX,
            fencing_token: 1,
            catalog_digest: kiana_domain::capability_action_catalog_digest(),
            action_digest: String::new(),
            permission_scope_digest: permission_scope.digest(),
            scope_digest: String::new(),
        };
        let mut without_scope = request.clone();
        without_scope.execution_scope = None;
        scope.action_digest = kiana_domain::capability_action_digest(&without_scope);
        scope.scope_digest = scope.digest();
        scope.validate().unwrap();
        scope
    }

    /// Core-stamped memory arguments: the store snapshot is captured at registration, so a fixture
    /// `MemoryScope { home: None }` only matches a null snapshot.
    #[cfg(unix)]
    fn cap19_memory_arguments(project_root: &str, session: &str, role_id: &str) -> Value {
        json!({
            "session_id": session,
            "project_root": project_root,
            "role_id": role_id,
            "memory_snapshot": {"schema": "kiana.memory-storage.v1", "home": null},
        })
    }

    #[cfg(unix)]
    fn cap19_authorized_memory_request(
        project_root: &str,
        operation: &str,
        session: &str,
        memory_scopes: &[&str],
        arguments: Value,
    ) -> AuthorizedCapabilityRequest {
        let capability = if operation == SEARCH_OPERATION {
            CapabilityKind::Query
        } else {
            CapabilityKind::Filesystem
        };
        let mut request = kiana_domain::CapabilityRequest::new(
            kiana_domain::RequestId::new(),
            capability,
            operation,
            arguments,
        );
        request.execution_scope = Some(cap19_execution_scope(
            &request,
            project_root,
            session,
            memory_scopes,
        ));
        AuthorizedCapabilityRequest::new("policy:cap19-memory-scope", request).unwrap()
    }

    #[cfg(unix)]
    async fn cap19_write(
        scope: MemoryScope,
        project_root: &str,
        session: &str,
        role_id: &str,
        collection: &str,
        text: &str,
        key: &str,
    ) -> Result<Value, PortError> {
        let mut arguments = cap19_memory_arguments(project_root, session, role_id);
        arguments["collection"] = json!(collection);
        arguments["text"] = json!(text);
        arguments["source"] = json!("cap19-success-fixture");
        arguments["idempotency_key"] = json!(key);
        let request = cap19_authorized_memory_request(
            project_root,
            WRITE_OPERATION,
            session,
            &[collection],
            arguments,
        );
        let result = MemoryWriteHandler(scope).execute(request).await?;
        Ok(result.output)
    }

    #[cfg(unix)]
    async fn cap19_search(
        scope: MemoryScope,
        project_root: &str,
        session: &str,
        role_id: &str,
        collection: &str,
        query: &str,
    ) -> Result<Value, PortError> {
        let mut arguments = cap19_memory_arguments(project_root, session, role_id);
        arguments["collection"] = json!(collection);
        arguments["query"] = json!(query);
        let request = cap19_authorized_memory_request(
            project_root,
            SEARCH_OPERATION,
            session,
            &[collection],
            arguments,
        );
        let result = MemorySearchHandler(scope).execute(request).await?;
        Ok(result.output)
    }

    /// Event store that holds the first `read_stream` open until the test releases it, so the
    /// cancel fixture can prove the writer is still inside the commit boundary.
    #[cfg(unix)]
    struct CommitGateEventLog {
        inner: kiana_eventlog::MemoryEventLog,
        entered: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    }

    #[cfg(unix)]
    #[async_trait::async_trait]
    impl EventStorePort for CommitGateEventLog {
        async fn append(&self, event: RuntimeEvent) -> Result<(), PortError> {
            self.inner.append(event).await
        }

        async fn append_idempotent_expected(
            &self,
            event: RuntimeEvent,
            expected: Option<u64>,
        ) -> Result<kiana_ports::EventAppendResult, PortError> {
            self.inner.append_idempotent_expected(event, expected).await
        }

        async fn read_request(
            &self,
            request_id: &kiana_domain::RequestId,
        ) -> Result<Vec<RuntimeEvent>, PortError> {
            self.inner.read_request(request_id).await
        }

        async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
            self.inner.read_all().await
        }

        async fn read_stream(
            &self,
            aggregate_type: &str,
            aggregate_id: &str,
        ) -> Result<Vec<RuntimeEvent>, PortError> {
            let entered = self.entered.lock().unwrap().take();
            if let Some(entered) = entered {
                let _ = entered.send(());
                let release = self.release.lock().unwrap();
                let _ = release.recv();
            }
            self.inner.read_stream(aggregate_type, aggregate_id).await
        }
    }

    /// CAP-19 deny-first: session, project, storage snapshot, explicit collection and default role
    /// grants all come from the server-owned execution scope; model arguments cannot replace them.
    #[cfg(unix)]
    #[tokio::test]
    async fn memory_scope_cannot_be_replaced_by_model_arguments() {
        let root = temp_memory_path("cap19-scope-deny");
        fs::create_dir_all(&root).unwrap();
        let project_root = fs::canonicalize(&root).unwrap().to_string_lossy().into_owned();
        let events: std::sync::Arc<dyn EventStorePort> =
            std::sync::Arc::new(kiana_eventlog::MemoryEventLog::new());
        let scope = MemoryScope {
            home: None,
            events: Some(events.clone()),
        };
        let session = "session-cap19-deny";
        let foreign_root = format!("{project_root}-foreign");

        let mut forged_session =
            cap19_memory_arguments(&project_root, "session-cap19-forged", "builder");
        forged_session["collection"] = json!("scratch");
        forged_session["text"] = json!("forged session must not write");
        forged_session["source"] = json!("cap19-fixture");

        let mut forged_project = cap19_memory_arguments(&foreign_root, session, "builder");
        forged_project["collection"] = json!("scratch");
        forged_project["text"] = json!("forged project must not write");
        forged_project["source"] = json!("cap19-fixture");

        let mut foreign_collection = cap19_memory_arguments(&project_root, session, "builder");
        foreign_collection["collection"] = json!("company");
        foreign_collection["query"] = json!("cap19 company memory");

        let mut forged_snapshot = cap19_memory_arguments(&project_root, session, "builder");
        forged_snapshot["collection"] = json!("scratch");
        forged_snapshot["text"] = json!("forged root must not write");
        forged_snapshot["source"] = json!("cap19-fixture");
        forged_snapshot["memory_snapshot"] = json!({
            "schema": "kiana.memory-storage.v1",
            "home": "/tmp/cap19-forged-home"
        });

        let uncovered_grants = cap19_memory_arguments(&project_root, session, "builder");

        let cases = [
            (
                "forged-session",
                WRITE_OPERATION,
                vec!["scratch"],
                forged_session,
                "memory_session_scope_mismatch",
            ),
            (
                "forged-project",
                WRITE_OPERATION,
                vec!["scratch"],
                forged_project,
                "memory_project_scope_mismatch",
            ),
            (
                "collection-outside-acl",
                SEARCH_OPERATION,
                vec!["scratch"],
                foreign_collection,
                "memory_scope_collection_denied",
            ),
            (
                "forged-storage-snapshot",
                WRITE_OPERATION,
                vec!["scratch"],
                forged_snapshot,
                "memory_storage_scope_changed",
            ),
            (
                "role-grants-outside-acl",
                SEARCH_OPERATION,
                vec!["company", "project"],
                uncovered_grants,
                "memory_scope_role_denied",
            ),
        ];
        for (label, operation, memory_scopes, arguments, expected) in cases {
            let request = cap19_authorized_memory_request(
                &project_root,
                operation,
                session,
                &memory_scopes,
                arguments,
            );
            let result = if operation == WRITE_OPERATION {
                MemoryWriteHandler(scope.clone()).execute(request).await
            } else {
                MemorySearchHandler(scope.clone()).execute(request).await
            };
            match result {
                Err(PortError::Failed(code)) => assert_eq!(code, expected, "{label}"),
                other => panic!("{label}: expected {expected}, got {other:?}"),
            }
        }
        assert!(events.read_all().await.unwrap().is_empty());
        assert!(
            !root.join(".kiana").exists(),
            "denied memory requests must not create store paths"
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// CAP-19 deny-first: a store file that resolves through a symlink is refused without touching
    /// the target, and a scope session that tries to escape the instance namespace is refused
    /// before any path is opened.
    #[cfg(unix)]
    #[tokio::test]
    async fn memory_store_symlink_or_namespace_escape_is_rejected() {
        use std::os::unix::fs::symlink;

        let root = temp_memory_path("cap19-symlink");
        fs::create_dir_all(&root).unwrap();
        let project_root = fs::canonicalize(&root).unwrap().to_string_lossy().into_owned();
        let events: std::sync::Arc<dyn EventStorePort> =
            std::sync::Arc::new(kiana_eventlog::MemoryEventLog::new());
        let scope = MemoryScope {
            home: None,
            events: Some(events.clone()),
        };
        let session = "session-cap19-symlink";
        let collection = MemoryCollection::parse("scratch").unwrap();
        let path = collection_path_scoped(&collection, &project_root, session, None).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let outside = root.join("outside.jsonl");
        fs::write(&outside, "outside-memory\n").unwrap();
        symlink(&outside, &path).unwrap();

        let mut arguments = cap19_memory_arguments(&project_root, session, "builder");
        arguments["collection"] = json!("scratch");
        arguments["text"] = json!("must not follow the store link");
        arguments["source"] = json!("cap19-fixture");
        let request = cap19_authorized_memory_request(
            &project_root,
            WRITE_OPERATION,
            session,
            &["scratch"],
            arguments,
        );
        let result = MemoryWriteHandler(scope.clone()).execute(request).await;
        assert!(matches!(
            result,
            Err(PortError::Failed(ref code)) if code.starts_with("memory_write_failed")
        ));
        assert_eq!(fs::read_to_string(&outside).unwrap(), "outside-memory\n");

        let escape_session = "../cap19-escape";
        let mut arguments = cap19_memory_arguments(&project_root, escape_session, "builder");
        arguments["collection"] = json!("scratch");
        arguments["text"] = json!("must not escape the instance namespace");
        arguments["source"] = json!("cap19-fixture");
        let request = cap19_authorized_memory_request(
            &project_root,
            WRITE_OPERATION,
            escape_session,
            &["scratch"],
            arguments,
        );
        let result = MemoryWriteHandler(scope.clone()).execute(request).await;
        assert!(matches!(
            result,
            Err(PortError::Failed(ref code)) if code == "memory_session_invalid"
        ));
        assert!(!root
            .join(".kiana")
            .join("memory")
            .join("cap19-escape.jsonl")
            .exists());
        assert!(events.read_all().await.unwrap().is_empty());

        fs::remove_dir_all(root).unwrap();
    }

    /// CAP-19 deny-first: a pre-cancelled write never claims execution, and a write that is already
    /// inside the commit boundary is only reported as completed-before-cancel after the writer
    /// stops, with the projection on disk.
    #[cfg(unix)]
    #[tokio::test]
    async fn memory_cancel_does_not_report_success_before_writer_stops() {
        let root = temp_memory_path("cap19-cancel");
        fs::create_dir_all(&root).unwrap();
        let project_root = fs::canonicalize(&root).unwrap().to_string_lossy().into_owned();
        let session = "session-cap19-cancel";
        let collection = MemoryCollection::parse("scratch").unwrap();
        let path = collection_path_scoped(&collection, &project_root, session, None).unwrap();

        // Cancelled before start: no execution and no store side effect.
        let plain_events: std::sync::Arc<dyn EventStorePort> =
            std::sync::Arc::new(kiana_eventlog::MemoryEventLog::new());
        let plain_scope = MemoryScope {
            home: None,
            events: Some(plain_events),
        };
        let (_cancel_before, cancelled_before) = tokio::sync::watch::channel(true);
        let mut arguments = cap19_memory_arguments(&project_root, session, "builder");
        arguments["collection"] = json!("scratch");
        arguments["text"] = json!("cancelled before start");
        arguments["source"] = json!("cap19-fixture");
        let request = cap19_authorized_memory_request(
            &project_root,
            WRITE_OPERATION,
            session,
            &["scratch"],
            arguments,
        );
        let result = MemoryWriteHandler(plain_scope)
            .execute_cancellable(request, cancelled_before)
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.output["not_executed"], json!(true));
        assert_eq!(result.output["cancelled"], json!(true));
        assert_eq!(result.output["stop_confirmed"], json!(true));
        assert!(!path.exists());

        // Cancelled while the journal commit is in flight: the handler keeps waiting for the
        // blocked writer and only reports completion after it stops.
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let gate = std::sync::Arc::new(CommitGateEventLog {
            inner: kiana_eventlog::MemoryEventLog::new(),
            entered: std::sync::Mutex::new(Some(entered_tx)),
            release: std::sync::Mutex::new(release_rx),
        });
        let gated_events: std::sync::Arc<dyn EventStorePort> = gate.clone();
        let gated_scope = MemoryScope {
            home: None,
            events: Some(gated_events),
        };
        let (cancel_tx, cancelled) = tokio::sync::watch::channel(false);
        let mut arguments = cap19_memory_arguments(&project_root, session, "builder");
        arguments["collection"] = json!("scratch");
        arguments["text"] = json!("cancelled after the writer entered the commit path");
        arguments["source"] = json!("cap19-fixture");
        arguments["idempotency_key"] = json!("cap19-cancel-write");
        let request = cap19_authorized_memory_request(
            &project_root,
            WRITE_OPERATION,
            session,
            &["scratch"],
            arguments,
        );
        let mut task = tokio::spawn(async move {
            MemoryWriteHandler(gated_scope)
                .execute_cancellable(request, cancelled)
                .await
        });
        entered_rx.await.unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(200), &mut task)
                .await
                .is_err(),
            "handler must keep waiting for the blocked writer"
        );
        cancel_tx.send(true).unwrap();
        release_tx.send(()).unwrap();
        let result = task.await.unwrap().unwrap();
        assert!(result.success);
        assert_eq!(result.output["completed_before_cancel"], json!(true));
        assert_eq!(result.output["cancelled"], json!(true));
        assert_eq!(result.output["stop_confirmed"], json!(true));
        assert!(result.output.get("not_executed").is_none());
        assert_eq!(read_records(&path).unwrap().len(), 1);
        assert_eq!(gate.inner.read_all().await.unwrap().len(), 1);

        fs::remove_dir_all(root).unwrap();
    }

    /// CAP-19 success/regression: legal scratch and candidate writes stay inside the server scope
    /// with journal-before-projection commit, a duplicate request replays against the committed
    /// story, and the projection fence is checked before a query reads the store.
    #[cfg(unix)]
    #[tokio::test]
    async fn legal_memory_write_and_search_commit_inside_the_server_scope() {
        let root = temp_memory_path("cap19-success");
        fs::create_dir_all(&root).unwrap();
        let project_root = fs::canonicalize(&root).unwrap().to_string_lossy().into_owned();
        let session = "session-cap19-success";
        let events: std::sync::Arc<dyn EventStorePort> =
            std::sync::Arc::new(kiana_eventlog::MemoryEventLog::new());
        let scope = MemoryScope {
            home: None,
            events: Some(events.clone()),
        };
        let scratch = MemoryCollection::parse("scratch").unwrap();
        let scratch_path = collection_path_scoped(&scratch, &project_root, session, None).unwrap();
        let planning = MemoryCollection::parse("department:planning").unwrap();
        let planning_path =
            collection_path_scoped(&planning, &project_root, session, None).unwrap();

        let first = cap19_write(
            scope.clone(),
            &project_root,
            session,
            "builder",
            "scratch",
            "cap19 rail anchor keeps the session pinned",
            "cap19-success-scratch",
        )
        .await
        .unwrap();
        assert_eq!(first["replayed"], json!(false));
        assert_eq!(first["journaled"], json!(true));
        assert_eq!(first["effect_committed"], json!(true));
        assert_eq!(first["stop_confirmed"], json!(true));

        let duplicate = cap19_write(
            scope.clone(),
            &project_root,
            session,
            "builder",
            "scratch",
            "cap19 rail anchor keeps the session pinned",
            "cap19-success-scratch",
        )
        .await
        .unwrap();
        assert_eq!(duplicate["replayed"], json!(true));
        assert_eq!(read_records(&scratch_path).unwrap().len(), 1);

        let candidate = cap19_write(
            scope.clone(),
            &project_root,
            session,
            "pm",
            "department:planning",
            "cap19 planning candidate waits for review",
            "cap19-success-candidate",
        )
        .await
        .unwrap();
        assert_eq!(candidate["replayed"], json!(false));
        assert_eq!(candidate["admission_state"], json!("candidate"));
        assert_eq!(candidate["state"], json!("draft"));
        let candidates = read_records(&planning_path).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].admission_state, MemoryAdmission::Candidate);
        assert_eq!(candidates[0].state, MemoryState::Draft);

        let search = cap19_search(
            scope.clone(),
            &project_root,
            session,
            "builder",
            "scratch",
            "cap19 rail anchor",
        )
        .await
        .unwrap();
        let hits = search["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0]["id"],
            json!(stable_memory_record_id("cap19-success-scratch"))
        );
        assert_eq!(hits[0]["collection"], json!("scratch"));
        assert_eq!(hits[0]["retrieval_session_id"], json!(session));
        assert_eq!(search["data_epoch"], json!(1));
        assert_eq!(events.read_all().await.unwrap().len(), 2);

        fs::remove_dir_all(root).unwrap();
    }

    /// CAP-19 success/regression: after the project data policy revokes a source, new writes that
    /// reference it stop, and nothing reaches the file or the journal.
    #[cfg(unix)]
    #[tokio::test]
    async fn revoked_source_stops_new_writes_inside_the_server_scope() {
        let root = temp_memory_path("cap19-revoked");
        fs::create_dir_all(&root).unwrap();
        let project_root = fs::canonicalize(&root).unwrap().to_string_lossy().into_owned();
        let session = "session-cap19-revoked";
        fs::create_dir_all(root.join(".kiana")).unwrap();
        fs::write(
            root.join(".kiana").join("data-policy.json"),
            "{\"revision\":1,\"data_epoch\":2,\"grants\":{},\"revoked_sources\":[\"cap19-revoked\"]}\n",
        )
        .unwrap();
        let events: std::sync::Arc<dyn EventStorePort> =
            std::sync::Arc::new(kiana_eventlog::MemoryEventLog::new());
        let scope = MemoryScope {
            home: None,
            events: Some(events.clone()),
        };
        let scratch = MemoryCollection::parse("scratch").unwrap();
        let path = collection_path_scoped(&scratch, &project_root, session, None).unwrap();

        let mut arguments = cap19_memory_arguments(&project_root, session, "builder");
        arguments["collection"] = json!("scratch");
        arguments["text"] = json!("revoked source must not be written");
        arguments["source"] = json!("cap19-revoked-source");
        let request = cap19_authorized_memory_request(
            &project_root,
            WRITE_OPERATION,
            session,
            &["scratch"],
            arguments,
        );
        let result = MemoryWriteHandler(scope).execute(request).await;
        assert!(matches!(
            result,
            Err(PortError::Failed(ref code)) if code == "data_source_revoked"
        ));
        let persisted = if path.exists() {
            fs::read_to_string(&path).unwrap()
        } else {
            String::new()
        };
        assert!(persisted.is_empty(), "revoked source must not be persisted");
        assert!(events.read_all().await.unwrap().is_empty());

        fs::remove_dir_all(root).unwrap();
    }
}
