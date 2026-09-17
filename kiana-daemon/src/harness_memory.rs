//! Six-layer memory broker for the owned harness.
//!
//! Collections are partitioned JSONL files. Search and write are tools; the
//! runner never touches the store. Chat is never auto-ingested.

use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{
    json_digest, memory_query_terms, AdapterCommitState, AdapterResultKind,
    AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult, EvidenceStatus, MemoryAdmission,
    MemoryClassification, MemoryCollection, MemoryMutation, MemoryMutationLedger,
    MemoryMutationOperation, MemoryMutationOutcome, MemoryMutationReceipt, MemoryMutationTarget,
    MemoryOrigin, MemoryRecord, MemoryScope as DomainMemoryScope, MemorySensitivity, MemoryState,
    Purpose, RoleSpec, SourceKind, SourceRef, MEMORY_LAYER_COMPANY, MEMORY_LAYER_DEPARTMENT,
    MEMORY_LAYER_INSTANCE_SCRATCH, MEMORY_LAYER_PROJECT, MEMORY_LAYER_ROLE, MEMORY_LAYER_USER,
    MEMORY_RECORD_SCHEMA, MEMORY_RECORD_SCHEMA_V2, MEMORY_REVIEW_SCHEMA, MEMORY_SEARCH_SCHEMA,
    MEMORY_WRITE_SCHEMA,
};
use kiana_ports::PortError;
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
}
impl MemoryScope {
    fn capture() -> Self {
        Self {
            home: std::env::var_os(KIANA_HOME_ENV).map(PathBuf::from),
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

fn server_memory_scope(
    request: &AuthorizedCapabilityRequest,
    arguments: &Value,
    allow_write: bool,
) -> Result<DomainMemoryScope, PortError> {
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
    if let Some(raw) = optional_string(arguments, "collection") {
        let requested = MemoryCollection::parse(&raw)
            .ok_or_else(|| failed("memory_scope_collection_unknown"))?;
        if !scope.allows_collection(&requested) {
            return Err(failed("memory_scope_collection_denied"));
        }
    } else if allow_write {
        return Err(failed("memory_scope_collection_required"));
    }
    Ok(scope)
}
pub(crate) fn register(broker: &mut CapabilityBroker) -> Result<(), PortError> {
    let scope = MemoryScope::capture();
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
        let mutation_scope = server_memory_scope(&request, &arguments, true)?;
        let execution_scope = request
            .request
            .execution_scope
            .as_ref()
            .ok_or_else(|| failed("memory_scope_required"))?;
        let policy_epoch = execution_scope.authority_epoch;
        let data_epoch = execution_scope.data_epoch;
        let scope = self.0.clone();
        let output = tokio::task::spawn_blocking(move || {
            write_record_with_mutation(
                &arguments,
                &scope,
                Some(request_id),
                Some(&mutation_scope),
                policy_epoch,
                data_epoch,
            )
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
        let mutation_scope = server_memory_scope(&request, &arguments, true)?;
        let execution_scope = request
            .request
            .execution_scope
            .as_ref()
            .ok_or_else(|| failed("memory_scope_required"))?;
        let policy_epoch = execution_scope.authority_epoch;
        let data_epoch = execution_scope.data_epoch;
        let scope = self.0.clone();
        let output = tokio::task::spawn_blocking(move || {
            review_records_with_mutation(
                &arguments,
                &scope,
                Some(&mutation_scope),
                policy_epoch,
                data_epoch,
            )
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
    search_records_scoped(arguments, &MemoryScope::capture())
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
        records.extend(read_records(&path)?.into_iter().filter(|record| {
            record.collection == collection.collection
                && !data_policy
                    .revoked_sources
                    .iter()
                    .any(|source| record.source.contains(source))
                && record.searchable()
                && (collection.layer != MEMORY_LAYER_INSTANCE_SCRATCH
                    || record.session_id == session_id)
        }));
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
        "role_id":role.role_id,"department_id":role.department_id,"session_id":session_id,"hits":hits}),
    )
}

#[cfg(test)]
fn write_record(arguments: &Value) -> Result<Value, PortError> {
    write_record_scoped(arguments, &MemoryScope::capture(), None)
}
fn write_record_scoped(
    arguments: &Value,
    scope: &MemoryScope,
    request_id: Option<kiana_domain::RequestId>,
) -> Result<Value, PortError> {
    write_record_with_mutation(arguments, scope, request_id, None, 1, 1)
}

fn write_record_with_mutation(
    arguments: &Value,
    scope: &MemoryScope,
    request_id: Option<kiana_domain::RequestId>,
    mutation_scope: Option<&DomainMemoryScope>,
    policy_epoch: u64,
    data_epoch: u64,
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
    let session_id = required_string(arguments, "session_id", "memory_session_required")?;
    let project_root = optional_string(arguments, "project_root").unwrap_or_default();
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
            Some(replayed_memory_receipt(&mutation, record.revision)?)
        } else {
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
        "idempotency_key":mutation_key,"mutation_receipt":mutation_receipt,
        "stop_confirmed":true}),
    )
}

/// This operation is not in the model's five-tool registry. The ControlPlane command
/// boundary stamps the operator identity and requires approval of the exact revision.
#[cfg(test)]
fn review_records(arguments: &Value) -> Result<Value, PortError> {
    review_records_scoped(arguments, &MemoryScope::capture())
}
fn review_records_scoped(arguments: &Value, scope: &MemoryScope) -> Result<Value, PortError> {
    review_records_with_mutation(arguments, scope, None, 1, 1)
}

fn review_records_with_mutation(
    arguments: &Value,
    scope: &MemoryScope,
    mutation_scope: Option<&DomainMemoryScope>,
    policy_epoch: u64,
    data_epoch: u64,
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
    let project = optional_string(arguments, "project_root").unwrap_or_default();
    let session = optional_string(arguments, "session_id").unwrap_or_default();
    let path = collection_path_scoped(&collection, &project, &session, scope.home.as_deref())?;
    if action == "accept_proposal" {
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
    let operation = if action == "promote" {
        MemoryMutationOperation::Approve
    } else {
        MemoryMutationOperation::Revoke
    };
    let payload = json!({"action":action,"record_id":record.id,"reason":reason,
        "expected_revision":expected,"collection":collection.collection});
    let mutation_receipt = if let Some(mutation_scope) = mutation_scope {
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
        .map_err(failed)?;
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
    append_record_file(&mut file, &record)?;
    Ok(
        json!({"schema":MEMORY_REVIEW_SCHEMA,"action":action,"record":record.hit(),"revision":record.revision,
            "idempotency_key":mutation_key,"mutation_receipt":mutation_receipt}),
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
        MemoryScope::capture().home.as_deref(),
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
}
