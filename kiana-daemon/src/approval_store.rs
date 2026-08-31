use async_trait::async_trait;
use kiana_domain::{
    APPROVAL_CHALLENGE_SCHEMA, ApprovalChallenge, ApprovalId, ApprovalState, CapabilityRequest,
    PendingApproval, PermissionProfile, RequestContext, SessionId,
};
use kiana_ports::{ApprovalStorePort, PortError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

const DEFAULT_APPROVAL_TTL: Duration = Duration::from_secs(5 * 60);

pub(crate) struct MemoryApprovalStore {
    records: Mutex<HashMap<ApprovalId, ApprovalRecord>>,
    ttl: Duration,
    path: Option<PathBuf>,
}

impl MemoryApprovalStore {
    pub(crate) fn new() -> Self {
        Self::with_ttl(DEFAULT_APPROVAL_TTL)
    }

    fn with_ttl(ttl: Duration) -> Self {
        Self {
            records: Mutex::new(HashMap::new()),
            ttl,
            path: None,
        }
    }

    pub(crate) fn open(path: impl Into<PathBuf>) -> Result<Self, PortError> {
        let path = path.into();
        let records = if path.exists() {
            load_records(&path)?
        } else {
            HashMap::new()
        };
        Ok(Self {
            records: Mutex::new(records),
            ttl: DEFAULT_APPROVAL_TTL,
            path: Some(path),
        })
    }

    pub(crate) fn open_default() -> Result<Self, PortError> {
        Ok(Self::open(default_approval_path()?)?)
    }

    fn persist(
        &self,
        approval_id: ApprovalId,
        expected_state: Option<ApprovalState>,
        record: &ApprovalRecord,
    ) -> Result<(), PortError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        persist_record(path, approval_id, expected_state, record)
    }
}

#[derive(Clone, PartialEq)]
struct ApprovalRecord {
    pending: PendingApproval,
    binding: ApprovalBinding,
    expires_at: Instant,
    state: ApprovalState,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct ApprovalBinding {
    session_id: SessionId,
    actor_id: String,
    project_root: String,
    project_trusted: bool,
    permission_profile: PermissionProfile,
    role_id: String,
    department_id: String,
    path_allow: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct PersistedApprovalRecord {
    pending: PendingApproval,
    binding: ApprovalBinding,
    state: ApprovalState,
}

pub(crate) type JsonlApprovalStore = MemoryApprovalStore;

#[cfg(unix)]
struct ProcessApprovalLock(std::fs::File);

#[cfg(unix)]
impl ProcessApprovalLock {
    fn acquire(path: &Path) -> Result<Self, PortError> {
        let lock_path = path.with_extension("jsonl.lock");
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                PortError::Failed(format!("approval_store_lock_create_failed:{error}"))
            })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|error| {
                PortError::Failed(format!("approval_store_lock_open_failed:{error}"))
            })?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(PortError::Failed(format!(
                "approval_store_lock_acquire_failed:{}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(Self(file))
    }
}

#[cfg(unix)]
impl Drop for ProcessApprovalLock {
    fn drop(&mut self) {
        let _ = unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

#[cfg(not(unix))]
struct ProcessApprovalLock;

#[cfg(not(unix))]
impl ProcessApprovalLock {
    fn acquire(_path: &Path) -> Result<Self, PortError> {
        Ok(Self)
    }
}

#[async_trait]
impl ApprovalStorePort for MemoryApprovalStore {
    async fn stage(
        &self,
        context: &RequestContext,
        request: CapabilityRequest,
        reason: &str,
    ) -> Result<ApprovalChallenge, PortError> {
        if request.request_id != context.request_id {
            return Err(PortError::Failed(
                "approval_request_context_mismatch".to_owned(),
            ));
        }
        let actor_id = context
            .actor_id
            .as_deref()
            .map(str::trim)
            .filter(|actor| !actor.is_empty())
            .ok_or_else(|| PortError::Failed("approval_actor_required".to_owned()))?;
        let request_hash = capability_request_hash(context, &request)?;
        let approval_id = ApprovalId::new();
        let expires_at = Instant::now()
            .checked_add(self.ttl)
            .ok_or_else(|| PortError::Failed("approval_expiry_overflow".to_owned()))?;
        let expires_at_unix_ms = unix_time_ms()
            .checked_add(self.ttl.as_millis().min(u128::from(u64::MAX)) as u64)
            .ok_or_else(|| PortError::Failed("approval_expiry_overflow".to_owned()))?;
        let challenge = ApprovalChallenge {
            schema: APPROVAL_CHALLENGE_SCHEMA.to_owned(),
            approval_id,
            request_id: request.request_id,
            request_hash,
            expires_at_unix_ms,
            reason: reason.to_owned(),
            nonce: approval_id.to_string(),
            policy_version: "kiana.policy.v1".to_owned(),
        };
        let record = ApprovalRecord {
            pending: PendingApproval {
                challenge: challenge.clone(),
                request,
            },
            binding: ApprovalBinding {
                session_id: context.session_id.clone(),
                actor_id: actor_id.to_owned(),
                project_root: context.project_root.clone(),
                project_trusted: context.project_trusted,
                permission_profile: context.permission_profile,
                role_id: context.role_id.clone(),
                department_id: context.department_id.clone(),
                path_allow: context.path_allow.clone(),
            },
            expires_at,
            state: ApprovalState::Staged,
        };
        let mut records = self.records.lock().await;
        records.insert(approval_id, record.clone());
        if let Err(error) = self.persist(approval_id, None, &record) {
            records.remove(&approval_id);
            return Err(error);
        }
        Ok(challenge)
    }

    async fn activate(&self, approval_id: ApprovalId) -> Result<(), PortError> {
        let mut records = self.records.lock().await;
        let previous = records
            .get(&approval_id)
            .cloned()
            .ok_or_else(|| PortError::Failed("approval_not_found".to_owned()))?;
        if previous.state == ApprovalState::Consumed {
            return Err(PortError::Conflict("approval_already_consumed".to_owned()));
        }
        if previous.state != ApprovalState::Staged {
            return Err(PortError::Failed("approval_not_staged".to_owned()));
        }
        let mut next = previous.clone();
        if Instant::now() >= next.expires_at {
            next.state = ApprovalState::Expired;
            records.insert(approval_id, next.clone());
            if let Err(error) = self.persist(approval_id, Some(previous.state), &next) {
                records.insert(approval_id, previous);
                return Err(error);
            }
            return Err(PortError::Failed("approval_expired".to_owned()));
        }
        next.state = next
            .state
            .transition(ApprovalState::Active)
            .map_err(|error| PortError::Failed(error.to_string()))?;
        records.insert(approval_id, next.clone());
        if let Err(error) = self.persist(approval_id, Some(previous.state), &next) {
            records.insert(approval_id, previous);
            return Err(error);
        }
        Ok(())
    }

    async fn consume(
        &self,
        context: &RequestContext,
        approval_id: ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        self.consume_with_proof(context, approval_id, None, None)
            .await
    }

    async fn consume_with_proof(
        &self,
        context: &RequestContext,
        approval_id: ApprovalId,
        request_hash: Option<&str>,
        nonce: Option<&str>,
    ) -> Result<PendingApproval, PortError> {
        let mut records = self.records.lock().await;
        let previous = records
            .get(&approval_id)
            .cloned()
            .ok_or_else(|| PortError::Failed("approval_not_found".to_owned()))?;
        if previous.state == ApprovalState::Consumed {
            return Err(PortError::Conflict("approval_already_consumed".to_owned()));
        }
        if previous.state == ApprovalState::Expired {
            return Err(PortError::Failed("approval_expired".to_owned()));
        }
        if previous.state == ApprovalState::Cancelled {
            return Err(PortError::Conflict("approval_already_consumed".to_owned()));
        }
        if previous.state != ApprovalState::Active {
            return Err(PortError::Failed("approval_not_active".to_owned()));
        }
        if Instant::now() >= previous.expires_at {
            let mut next = previous.clone();
            next.state = ApprovalState::Expired;
            records.insert(approval_id, next.clone());
            if let Err(error) = self.persist(approval_id, Some(previous.state), &next) {
                records.insert(approval_id, previous);
                return Err(error);
            }
            return Err(PortError::Failed("approval_expired".to_owned()));
        }
        let actor_id = context
            .actor_id
            .as_deref()
            .map(str::trim)
            .filter(|actor| !actor.is_empty())
            .ok_or_else(|| PortError::Failed("approval_actor_required".to_owned()))?;
        if previous.binding.session_id != context.session_id
            || previous.binding.actor_id != actor_id
            || previous.binding.project_root != context.project_root
            || previous.binding.project_trusted != context.project_trusted
            || previous.binding.permission_profile != context.permission_profile
            || previous.binding.role_id != context.role_id
            || previous.binding.department_id != context.department_id
            || previous.binding.path_allow != context.path_allow
        {
            return Err(PortError::Failed("approval_context_mismatch".to_owned()));
        }
        if request_hash.is_some() != nonce.is_some() {
            return Err(PortError::Failed("approval_proof_incomplete".to_owned()));
        }
        if let Some(expected_hash) = request_hash {
            if expected_hash != previous.pending.challenge.request_hash {
                return Err(PortError::Failed(
                    "approval_request_hash_mismatch".to_owned(),
                ));
            }
        }
        if let Some(expected_nonce) = nonce {
            if expected_nonce != previous.pending.challenge.nonce {
                return Err(PortError::Failed("approval_nonce_mismatch".to_owned()));
            }
        }
        if capability_request_hash(context, &previous.pending.request)?
            != previous.pending.challenge.request_hash
        {
            let mut next = previous.clone();
            next.state = ApprovalState::Consumed;
            records.insert(approval_id, next.clone());
            if let Err(error) = self.persist(approval_id, Some(previous.state), &next) {
                records.insert(approval_id, previous);
                return Err(error);
            }
            return Err(PortError::Failed(
                "approval_request_integrity_mismatch".to_owned(),
            ));
        }
        let mut next = previous.clone();
        next.state = next
            .state
            .transition(ApprovalState::Approved)
            .map_err(|error| PortError::Failed(error.to_string()))?;
        next.state = next
            .state
            .transition(ApprovalState::Consumed)
            .map_err(|error| PortError::Failed(error.to_string()))?;
        let pending = next.pending.clone();
        records.insert(approval_id, next.clone());
        if let Err(error) = self.persist(approval_id, Some(previous.state), &next) {
            records.insert(approval_id, previous);
            return Err(error);
        }
        Ok(pending)
    }

    async fn invalidate(
        &self,
        context: &RequestContext,
        approval_id: ApprovalId,
        _reason: &str,
    ) -> Result<(), PortError> {
        let mut records = self.records.lock().await;
        let previous = records
            .get(&approval_id)
            .cloned()
            .ok_or_else(|| PortError::Failed("approval_not_found".to_owned()))?;
        if previous.state.is_terminal() {
            return Err(if previous.state == ApprovalState::Consumed {
                PortError::Conflict("approval_already_consumed".to_owned())
            } else {
                PortError::Failed("approval_not_active".to_owned())
            });
        }
        if previous.state != ApprovalState::Active {
            return Err(PortError::Failed("approval_not_active".to_owned()));
        }
        let actor_id = context
            .actor_id
            .as_deref()
            .map(str::trim)
            .filter(|actor| !actor.is_empty())
            .ok_or_else(|| PortError::Failed("approval_actor_required".to_owned()))?;
        if previous.binding.session_id != context.session_id
            || previous.binding.actor_id != actor_id
            || previous.binding.project_root != context.project_root
            || previous.binding.project_trusted != context.project_trusted
            || previous.binding.permission_profile != context.permission_profile
            || previous.binding.role_id != context.role_id
            || previous.binding.department_id != context.department_id
            || previous.binding.path_allow != context.path_allow
        {
            return Err(PortError::Failed("approval_context_mismatch".to_owned()));
        }
        let mut next = previous.clone();
        next.state = next
            .state
            .transition(ApprovalState::Cancelled)
            .map_err(|error| PortError::Failed(error.to_string()))?;
        records.insert(approval_id, next.clone());
        if let Err(error) = self.persist(approval_id, Some(previous.state), &next) {
            records.insert(approval_id, previous);
            return Err(error);
        }
        Ok(())
    }
}

fn capability_request_hash(
    context: &RequestContext,
    request: &CapabilityRequest,
) -> Result<String, PortError> {
    let canonical = serde_json::json!({
        "request": request,
        "binding": {
            "session_id": context.session_id,
            "actor_id": context.actor_id,
            "project_root": context.project_root,
            "project_trusted": context.project_trusted,
            "permission_profile": context.permission_profile,
            "role_id": context.role_id,
            "department_id": context.department_id,
            "work_packet_id": context.work_packet_id,
            "path_allow": context.path_allow,
        },
    });
    let encoded = canonical_json_bytes(&canonical)
        .map_err(|error| PortError::Failed(format!("approval_request_serialize:{error}")))?;
    let digest = Sha256::digest(encoded);
    Ok(format!("sha256:{digest:x}"))
}

fn canonical_json_bytes(value: &serde_json::Value) -> Result<Vec<u8>, serde_json::Error> {
    fn write_value(
        value: &serde_json::Value,
        output: &mut Vec<u8>,
    ) -> Result<(), serde_json::Error> {
        match value {
            serde_json::Value::Object(object) => {
                output.push(b'{');
                let mut entries: Vec<_> = object.iter().collect();
                entries.sort_by(|left, right| left.0.cmp(right.0));
                for (index, (key, value)) in entries.into_iter().enumerate() {
                    if index > 0 {
                        output.push(b',');
                    }
                    serde_json::to_writer(&mut *output, key)?;
                    output.push(b':');
                    write_value(value, output)?;
                }
                output.push(b'}');
            }
            serde_json::Value::Array(array) => {
                output.push(b'[');
                for (index, value) in array.iter().enumerate() {
                    if index > 0 {
                        output.push(b',');
                    }
                    write_value(value, output)?;
                }
                output.push(b']');
            }
            scalar => serde_json::to_writer(&mut *output, scalar)?,
        }
        Ok(())
    }

    let mut output = Vec::new();
    write_value(value, &mut output)?;
    Ok(output)
}

fn default_approval_path() -> Result<PathBuf, PortError> {
    let home = std::env::var("KIANA_HOME")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|value| format!("{value}/.kiana"))
        })
        .ok_or_else(|| PortError::Failed("approval_home_required".to_owned()))?;
    let home = PathBuf::from(home);
    if !home.is_absolute() {
        return Err(PortError::Failed(
            "approval_home_must_be_absolute".to_owned(),
        ));
    }
    Ok(home.join("approvals").join("records.jsonl"))
}

fn load_records(path: &Path) -> Result<HashMap<ApprovalId, ApprovalRecord>, PortError> {
    let _process_lock = ProcessApprovalLock::acquire(path)?;
    load_records_unlocked(path)
}

fn load_records_unlocked(path: &Path) -> Result<HashMap<ApprovalId, ApprovalRecord>, PortError> {
    let contents = fs::read_to_string(path)
        .map_err(|error| PortError::Failed(format!("approval_store_read_failed:{error}")))?;
    let mut records = HashMap::new();
    for (index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let persisted: PersistedApprovalRecord = serde_json::from_str(line).map_err(|error| {
            PortError::Failed(format!(
                "approval_store_corrupt:line={}:{}",
                index + 1,
                error
            ))
        })?;
        let expires_at = if persisted.state.is_terminal() {
            Instant::now()
        } else {
            let remaining = persisted
                .pending
                .challenge
                .expires_at_unix_ms
                .saturating_sub(unix_time_ms());
            Instant::now()
                .checked_add(Duration::from_millis(remaining))
                .ok_or_else(|| PortError::Failed("approval_expiry_overflow".to_owned()))?
        };
        let approval_id = persisted.pending.challenge.approval_id;
        if records
            .insert(
                approval_id,
                ApprovalRecord {
                    pending: persisted.pending,
                    binding: persisted.binding,
                    expires_at,
                    state: persisted.state,
                },
            )
            .is_some()
        {
            return Err(PortError::Failed("approval_store_duplicate_id".to_owned()));
        }
    }
    Ok(records)
}

fn persist_record(
    path: &Path,
    approval_id: ApprovalId,
    expected_state: Option<ApprovalState>,
    record: &ApprovalRecord,
) -> Result<(), PortError> {
    let _process_lock = ProcessApprovalLock::acquire(path)?;
    let mut merged = if path.exists() {
        load_records_unlocked(path)?
    } else {
        HashMap::new()
    };
    match (expected_state, merged.get(&approval_id)) {
        (None, Some(_)) => {
            return Err(PortError::Conflict(
                "approval_store_duplicate_id".to_owned(),
            ));
        }
        (Some(_), None) => {
            return Err(PortError::Conflict(
                "approval_store_concurrent_update".to_owned(),
            ));
        }
        (Some(expected), Some(remote)) => {
            if remote.pending != record.pending || remote.binding != record.binding {
                return Err(PortError::Conflict(
                    "approval_store_payload_conflict".to_owned(),
                ));
            }
            if remote.state != expected {
                return Err(PortError::Conflict(
                    "approval_store_concurrent_update".to_owned(),
                ));
            }
        }
        (None, None) => {}
    }
    merged.insert(approval_id, record.clone());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| PortError::Failed(format!("approval_store_create_failed:{error}")))?;
    }
    let mut ids: Vec<_> = merged.keys().copied().collect();
    ids.sort_by_key(|id| id.to_string());
    let mut encoded = String::new();
    for id in ids {
        let record = merged
            .get(&id)
            .ok_or_else(|| PortError::Failed("approval_store_record_missing".to_owned()))?;
        let persisted = PersistedApprovalRecord {
            pending: record.pending.clone(),
            binding: record.binding.clone(),
            state: record.state,
        };
        encoded.push_str(
            &serde_json::to_string(&persisted).map_err(|error| {
                PortError::Failed(format!("approval_store_encode_failed:{error}"))
            })?,
        );
        encoded.push('\n');
    }
    let temporary = path.with_file_name(format!(
        ".{}.tmp-{}-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("records"),
        std::process::id(),
        unix_time_ms()
    ));
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| PortError::Failed(format!("approval_store_write_failed:{error}")))?;
    let write_result = file
        .write_all(encoded.as_bytes())
        .and_then(|_| file.flush())
        .and_then(|_| file.sync_data());
    drop(file);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(PortError::Failed(format!(
            "approval_store_write_failed:{error}"
        )));
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(PortError::Failed(format!(
            "approval_store_write_failed:{error}"
        )));
    }
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        let directory = fs::File::open(parent).map_err(|error| {
            PortError::Failed(format!("approval_store_sync_open_failed:{error}"))
        })?;
        directory
            .sync_all()
            .map_err(|error| PortError::Failed(format!("approval_store_sync_failed:{error}")))?;
    }
    Ok(())
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_domain::{CapabilityKind, RequestId};
    use serde_json::json;

    #[test]
    fn canonical_json_sorts_nested_object_keys_but_preserves_array_order() {
        let first = json!({
            "outer": {
                "z": [{"b": 2, "a": 1}],
                "a": true,
            },
            "items": [3, 1, 2],
        });
        let second = json!({
            "items": [3, 1, 2],
            "outer": {
                "a": true,
                "z": [{"a": 1, "b": 2}],
            },
        });

        assert_eq!(
            canonical_json_bytes(&first).unwrap(),
            canonical_json_bytes(&second).unwrap()
        );
        assert_ne!(
            canonical_json_bytes(&first).unwrap(),
            canonical_json_bytes(&json!({
                "items": [1, 3, 2],
                "outer": {
                    "a": true,
                    "z": [{"a": 1, "b": 2}],
                },
            }))
            .unwrap()
        );
    }

    fn trusted_context(session: &str, actor: &str) -> RequestContext {
        let mut context = RequestContext::local(session, "/repo");
        context.actor_id = Some(actor.to_owned());
        context.project_trusted = true;
        context
    }

    fn local_write(context: &RequestContext) -> CapabilityRequest {
        CapabilityRequest::new(
            context.request_id,
            CapabilityKind::Filesystem,
            "context.index.cache.write",
            json!({ "path": ".kiana/context-index.json" }),
        )
        .with_risk(kiana_domain::RiskLevel::LocalWrite)
    }

    #[tokio::test]
    async fn approval_is_inactive_until_event_commit_then_consumed_once() {
        let store = MemoryApprovalStore::new();
        let context = trusted_context("session-1", "actor-1");
        let challenge = store
            .stage(
                &context,
                local_write(&context),
                "local_write_requires_approval",
            )
            .await
            .unwrap();
        assert_eq!(challenge.request_id, context.request_id);
        assert!(challenge.request_hash.starts_with("sha256:"));
        assert_eq!(challenge.request_hash.len(), "sha256:".len() + 64);
        assert_eq!(
            store
                .consume(&context, challenge.approval_id)
                .await
                .unwrap_err(),
            PortError::Failed("approval_not_active".to_owned())
        );

        store.activate(challenge.approval_id).await.unwrap();
        let pending = store
            .consume(&context, challenge.approval_id)
            .await
            .unwrap();
        assert_eq!(
            pending.request.arguments["path"],
            ".kiana/context-index.json"
        );
        assert_eq!(
            store
                .consume(&context, challenge.approval_id)
                .await
                .unwrap_err(),
            PortError::Conflict("approval_already_consumed".to_owned())
        );
    }

    #[tokio::test]
    async fn wrong_context_does_not_consume_the_rightful_approval() {
        let store = MemoryApprovalStore::new();
        let context = trusted_context("session-1", "actor-1");
        let challenge = store
            .stage(&context, local_write(&context), "approval_required")
            .await
            .unwrap();
        store.activate(challenge.approval_id).await.unwrap();

        let wrong_actor = trusted_context("session-1", "actor-2");
        assert_eq!(
            store
                .consume(&wrong_actor, challenge.approval_id)
                .await
                .unwrap_err(),
            PortError::Failed("approval_context_mismatch".to_owned())
        );
        let mut wrong_path_scope = trusted_context("session-1", "actor-1");
        wrong_path_scope.path_allow.push("other.txt".to_owned());
        assert_eq!(
            store
                .consume(&wrong_path_scope, challenge.approval_id)
                .await
                .unwrap_err(),
            PortError::Failed("approval_context_mismatch".to_owned())
        );
        assert!(store.consume(&context, challenge.approval_id).await.is_ok());
    }

    #[tokio::test]
    async fn expired_and_tampered_approvals_fail_closed() {
        let expiring = MemoryApprovalStore::with_ttl(Duration::from_millis(50));
        let context = trusted_context("session-1", "actor-1");
        let challenge = expiring
            .stage(&context, local_write(&context), "approval_required")
            .await
            .unwrap();
        expiring.activate(challenge.approval_id).await.unwrap();
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(
            expiring
                .consume(&context, challenge.approval_id)
                .await
                .unwrap_err(),
            PortError::Failed("approval_expired".to_owned())
        );

        let tampered = MemoryApprovalStore::new();
        let challenge = tampered
            .stage(&context, local_write(&context), "approval_required")
            .await
            .unwrap();
        tampered.activate(challenge.approval_id).await.unwrap();
        tampered
            .records
            .lock()
            .await
            .get_mut(&challenge.approval_id)
            .unwrap()
            .pending
            .request
            .request_id = RequestId::new();
        assert_eq!(
            tampered
                .consume(&context, challenge.approval_id)
                .await
                .unwrap_err(),
            PortError::Failed("approval_request_integrity_mismatch".to_owned())
        );
    }

    #[tokio::test]
    async fn partial_proof_is_rejected_without_consuming_approval() {
        let store = MemoryApprovalStore::new();
        let context = trusted_context("session-1", "actor-1");
        let challenge = store
            .stage(&context, local_write(&context), "approval_required")
            .await
            .unwrap();
        store.activate(challenge.approval_id).await.unwrap();
        assert_eq!(
            store
                .consume_with_proof(
                    &context,
                    challenge.approval_id,
                    Some(&challenge.request_hash),
                    None,
                )
                .await
                .unwrap_err(),
            PortError::Failed("approval_proof_incomplete".to_owned())
        );
        assert!(store.consume(&context, challenge.approval_id).await.is_ok());
    }

    #[tokio::test]
    async fn supplied_proof_must_match_without_consuming_approval() {
        let store = MemoryApprovalStore::new();
        let context = trusted_context("session-1", "actor-1");
        let challenge = store
            .stage(&context, local_write(&context), "approval_required")
            .await
            .unwrap();
        store.activate(challenge.approval_id).await.unwrap();
        assert_eq!(
            store
                .consume_with_proof(
                    &context,
                    challenge.approval_id,
                    Some("wrong-hash"),
                    Some(&challenge.nonce),
                )
                .await
                .unwrap_err(),
            PortError::Failed("approval_request_hash_mismatch".to_owned())
        );
        assert!(
            store
                .consume_with_proof(
                    &context,
                    challenge.approval_id,
                    Some(&challenge.request_hash),
                    Some(&challenge.nonce),
                )
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn disk_stores_merge_independent_approvals_without_losing_records() {
        let path = std::env::temp_dir().join(format!(
            "kiana-approval-merge-{}-{}.jsonl",
            std::process::id(),
            unix_time_ms()
        ));
        let first_context = trusted_context("session-merge-1", "actor-1");
        let second_context = trusted_context("session-merge-2", "actor-2");
        let first = MemoryApprovalStore::open(&path).unwrap();
        let second = MemoryApprovalStore::open(&path).unwrap();
        let first_challenge = first
            .stage(
                &first_context,
                local_write(&first_context),
                "approval_required",
            )
            .await
            .unwrap();
        first.activate(first_challenge.approval_id).await.unwrap();
        let second_challenge = second
            .stage(
                &second_context,
                local_write(&second_context),
                "approval_required",
            )
            .await
            .unwrap();
        second.activate(second_challenge.approval_id).await.unwrap();

        let reopened = MemoryApprovalStore::open(&path).unwrap();
        let persisted = fs::read_to_string(&path).unwrap();
        assert_eq!(persisted.lines().count(), 2);
        assert!(
            reopened
                .consume_with_proof(
                    &second_context,
                    second_challenge.approval_id,
                    Some(&second_challenge.request_hash),
                    Some(&second_challenge.nonce),
                )
                .await
                .is_ok()
        );
        let first_reopened = MemoryApprovalStore::open(&path).unwrap();
        assert!(
            first_reopened
                .consume(&first_context, first_challenge.approval_id)
                .await
                .is_ok()
        );
        let _ = fs::remove_file(&path);
    }

    #[tokio::test]
    async fn stale_disk_approval_cannot_overwrite_a_consumed_record() {
        let path = std::env::temp_dir().join(format!(
            "kiana-approval-stale-{}-{}.jsonl",
            std::process::id(),
            unix_time_ms()
        ));
        let context = trusted_context("session-stale", "actor-stale");
        let first = MemoryApprovalStore::open(&path).unwrap();
        let challenge = first
            .stage(&context, local_write(&context), "approval_required")
            .await
            .unwrap();
        first.activate(challenge.approval_id).await.unwrap();
        let stale = MemoryApprovalStore::open(&path).unwrap();
        first
            .consume_with_proof(
                &context,
                challenge.approval_id,
                Some(&challenge.request_hash),
                Some(&challenge.nonce),
            )
            .await
            .unwrap();
        assert_eq!(
            stale
                .consume_with_proof(
                    &context,
                    challenge.approval_id,
                    Some(&challenge.request_hash),
                    Some(&challenge.nonce),
                )
                .await
                .unwrap_err(),
            PortError::Conflict("approval_store_concurrent_update".to_owned())
        );
        let reopened = MemoryApprovalStore::open(&path).unwrap();
        assert_eq!(
            reopened
                .consume(&context, challenge.approval_id)
                .await
                .unwrap_err(),
            PortError::Conflict("approval_already_consumed".to_owned())
        );
        let _ = fs::remove_file(&path);
    }

    #[tokio::test]
    async fn disk_store_survives_reopen_and_consumes_once() {
        let path = std::env::temp_dir().join(format!(
            "kiana-approval-store-{}-{}.jsonl",
            std::process::id(),
            unix_time_ms()
        ));
        let context = trusted_context("session-disk", "/repo");
        let challenge = {
            let store = MemoryApprovalStore::open(&path).unwrap();
            let challenge = store
                .stage(&context, local_write(&context), "approval_required")
                .await
                .unwrap();
            store.activate(challenge.approval_id).await.unwrap();
            challenge
        };
        let reopened = MemoryApprovalStore::open(&path).unwrap();
        assert!(
            reopened
                .consume_with_proof(
                    &context,
                    challenge.approval_id,
                    Some(&challenge.request_hash),
                    Some(&challenge.nonce),
                )
                .await
                .is_ok()
        );
        assert_eq!(
            reopened
                .consume(&context, challenge.approval_id)
                .await
                .unwrap_err(),
            PortError::Conflict("approval_already_consumed".to_owned())
        );
        let persisted = fs::read_to_string(&path).unwrap();
        assert!(persisted.contains("consumed"), "{persisted}");
        let _ = fs::remove_file(&path);
    }
}
