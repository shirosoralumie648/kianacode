//! Governed source retention and conservative invalidation of derived local material.
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{
    json_digest, merge_memory_deletion_entries, AuthorizedCapabilityRequest, CapabilityKind,
    CapabilityResult, DataPolicy, MemoryDeletionEntry, MemoryDeletionIndex, MemoryRetrievalSurface,
    ProcessingGrant,
};
use kiana_ports::PortError;
use serde_json::{json, Value};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub(crate) fn register(broker: &mut CapabilityBroker) -> Result<(), PortError> {
    broker.register_static(
        CapabilityKind::Filesystem,
        "data.governance",
        std::sync::Arc::new(DataHandler),
    )
}
struct DataHandler;
#[async_trait::async_trait]
impl CapabilityHandler for DataHandler {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        let id = request.request.request_id;
        if request.request.arguments["operator_authorized"] != true
            || request.request.cell_id.is_some()
        {
            return Err(failed("governance_operator_required"));
        }
        let arguments = request.request.arguments;
        let result = tokio::task::spawn_blocking(move || apply(&arguments))
            .await
            .map_err(|_| failed("governance_join_failed"))??;
        Ok(CapabilityResult::success(id, result))
    }
}
pub(crate) fn policy_path(root: &Path) -> PathBuf {
    root.join(".kiana").join("data-policy.json")
}
pub(crate) fn deletion_index_path(root: &Path) -> PathBuf {
    root.join(".kiana").join("memory-deletion-index.json")
}
pub(crate) fn read_policy(root: &Path) -> Result<DataPolicy, PortError> {
    let managed = root.join(".kiana");
    if managed.exists()
        && fs::symlink_metadata(&managed)
            .map_err(io)?
            .file_type()
            .is_symlink()
    {
        return Err(failed("governance_path_symlink"));
    }
    let path = policy_path(root);
    if !path.exists() {
        return Ok(DataPolicy::default());
    }
    let mut file = open_safe(&path, false)?;
    let _lock = lock(&file)?;
    read_policy_file(&mut file)
}
fn read_policy_file(file: &mut File) -> Result<DataPolicy, PortError> {
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() > 1024 * 1024 {
        return Err(failed("governance_policy_too_large"));
    }
    if bytes.is_empty() {
        return Ok(DataPolicy::default());
    }
    if bytes.last() != Some(&b'\n') {
        return Err(failed("governance_policy_incomplete"));
    }
    let mut policy = DataPolicy::default();
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let mut next: DataPolicy =
            serde_json::from_slice(line).map_err(|_| failed("governance_policy_invalid"))?;
        if next.revision < policy.revision {
            return Err(failed("governance_revision_regressed"));
        }
        if next.policy_digest.is_empty() {
            next.refresh_digest();
        }
        next.validate()
            .map_err(|_| failed("governance_policy_integrity_failed"))?;
        policy = next;
    }
    Ok(policy)
}

/// Read the persisted deletion index. A missing file means "no deletion recorded yet" and is never
/// treated as a fence failure; a malformed, tampered or unreadable index is.
pub(crate) fn read_deletion_index(root: &Path) -> Result<Option<MemoryDeletionIndex>, PortError> {
    let managed = root.join(".kiana");
    if managed.exists()
        && fs::symlink_metadata(&managed)
            .map_err(io)?
            .file_type()
            .is_symlink()
    {
        return Err(failed("governance_path_symlink"));
    }
    let path = deletion_index_path(root);
    if !path.exists() {
        return Ok(None);
    }
    let mut file = open_safe(&path, false)?;
    let _lock = lock(&file)?;
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() > 1024 * 1024 {
        return Err(failed("governance_deletion_index_too_large"));
    }
    let index: MemoryDeletionIndex =
        serde_json::from_slice(&bytes).map_err(|_| failed("governance_deletion_index_invalid"))?;
    index
        .validate()
        .map_err(|_| failed("governance_deletion_index_integrity_failed"))?;
    Ok(Some(index))
}

/// The deletion index is rewritten as a whole: every entry keeps its own surface receipts.
fn write_deletion_index(file: &mut File, index: &MemoryDeletionIndex) -> Result<(), PortError> {
    let bytes =
        serde_json::to_vec(index).map_err(|_| failed("governance_deletion_index_invalid"))?;
    file.seek(SeekFrom::Start(0)).map_err(io)?;
    file.set_len(0).map_err(io)?;
    file.write_all(&bytes).map_err(io)?;
    file.write_all(b"\n").map_err(io)?;
    file.sync_all().map_err(io)
}

/// Only a surface whose derived store was actually rebuilt or removed may claim Invalidated; the
/// evidence for that claim is the tombstone digest the surface was fenced against.
fn fence_purged_entry(entry: &mut MemoryDeletionEntry, stamp: u64) -> Result<(), PortError> {
    let evidence = entry.tombstone_digest.clone();
    for surface in MemoryRetrievalSurface::ALL {
        entry
            .fence_surface(surface, stamp, evidence.clone())
            .map_err(failed)?;
    }
    Ok(())
}

fn deletion_index_summary(index: &MemoryDeletionIndex) -> Value {
    let pending = index.pending_surfaces();
    let receipt = if index.tombstones() == 0 {
        "clean"
    } else if pending.is_empty() {
        "fenced"
    } else {
        "partial"
    };
    json!({
        "schema": index.schema,
        "project_root": index.project_root,
        "policy_revision": index.policy_revision,
        "data_epoch": index.data_epoch,
        "index_digest": index.index_digest,
        "tombstones": index.tombstones(),
        "pending_surfaces": pending,
        "receipt": receipt,
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
fn apply(arguments: &Value) -> Result<Value, PortError> {
    let root = Path::new(
        arguments["project_root"]
            .as_str()
            .ok_or_else(|| failed("governance_project_required"))?,
    )
    .canonicalize()
    .map_err(io)?;
    let action = arguments["action"]
        .as_str()
        .ok_or_else(|| failed("governance_action_required"))?;
    if action == "list" {
        let mut policy = read_policy(&root)?;
        policy.refresh_digest();
        return Ok(
            json!({"schema":"kiana.data-policy.v1","policy":policy,"data_epoch":policy.data_epoch}),
        );
    }
    let managed = root.join(".kiana");
    if managed.exists()
        && fs::symlink_metadata(&managed)
            .map_err(io)?
            .file_type()
            .is_symlink()
    {
        return Err(failed("governance_path_symlink"));
    }
    fs::create_dir_all(&managed).map_err(io)?;
    let mut file = open_safe(&policy_path(&root), true)?;
    let _lock = lock(&file)?;
    let mut policy = read_policy_file(&mut file)?;
    if arguments["expected_revision"].as_u64() != Some(policy.revision) {
        return Err(PortError::Conflict(
            "governance_revision_conflict".to_owned(),
        ));
    }
    let mut affected = Vec::new();
    let mut erased = Vec::new();
    let mut deletion_index_report = Value::Null;
    if action == "register" {
        let mut grant: ProcessingGrant = serde_json::from_value(arguments["grant"].clone())
            .map_err(|_| failed("processing_grant_invalid"))?;
        let path = confined(&root, &grant.source_path)?;
        let bytes = fs::read(&path).map_err(io)?;
        let actual = bytes_digest(&bytes);
        if grant.content_hash != actual {
            return Err(failed("processing_source_changed"));
        }
        grant.created_by = arguments["actor_id"]
            .as_str()
            .ok_or_else(|| failed("governance_actor_required"))?
            .to_owned();
        policy.register(grant).map_err(failed)?;
    } else if matches!(action, "delete" | "revoke" | "expire") {
        let id = arguments["grant_id"]
            .as_str()
            .ok_or_else(|| failed("processing_grant_id_required"))?;
        if action == "expire" {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            if policy
                .grants
                .get(id)
                .and_then(|grant| grant.retention.expires_at_ms)
                .is_none_or(|expiry| expiry > now)
            {
                return Err(failed("processing_retention_not_expired"));
            }
        }
        affected = policy.revoke(id).map_err(failed)?;
        // Persist denial before touching derived files. Interrupted propagation remains denied.
        write_policy(&mut file, &policy)?;
        // The read-time fence is refreshed at the new epoch before any derived byte moves, so an
        // interrupted purge can never leave a search reading an index bound to the old epoch.
        let existing_entries = match read_deletion_index(&root)? {
            Some(index) => index.entries,
            None => Vec::new(),
        };
        let stamp = now_ms().max(1);
        let mut index_file = open_safe(&deletion_index_path(&root), true)?;
        let _index_lock = lock(&index_file)?;
        let denial_index = MemoryDeletionIndex::new(
            root.to_string_lossy().into_owned(),
            policy.revision,
            policy.data_epoch,
            stamp,
            merge_memory_deletion_entries(&existing_entries, &[]),
        )
        .map_err(failed)?;
        write_deletion_index(&mut index_file, &denial_index)?;
        if action == "delete" || action == "expire" {
            for id in &affected {
                let grant = &policy.grants[id];
                let path = root.join(&grant.source_path);
                if path.exists() {
                    let path = confined(&root, &grant.source_path)?;
                    if bytes_digest(&fs::read(&path).map_err(io)?) != grant.content_hash {
                        return Err(failed("result_unknown:governance_source_changed"));
                    }
                    fs::remove_file(path).map_err(io)?;
                    erased.push(grant.source_path.clone());
                }
            }
        }
        let mut purged = Vec::new();
        purge_memory(
            &managed.join("memory"),
            &policy.revoked_sources,
            &root,
            true,
            &mut purged,
        )?;
        if let Ok(home_root) = crate::harness_memory::kiana_home() {
            purge_memory(
                &home_root.join("memory"),
                &policy.revoked_sources,
                &root,
                false,
                &mut purged,
            )?;
        }
        // Derived caches are disposable; invalidate them as a unit, never the event ledger.
        for name in [
            "context-index.json",
            "context-artifacts.json",
            "context-artifact-store.json",
        ] {
            let path = managed.join(name);
            if path.exists() {
                remove_managed_tree(&path)?;
            }
        }
        for name in ["cache", "context", "index", "compaction"] {
            let path = managed.join(name);
            if path.exists() {
                remove_managed_tree(&path)?;
            }
        }
        // Second write: only now can lexical/vector/cache/receipt claim an evidence-bound receipt
        // for the rows that were actually removed above. A surviving copy stays denied either way.
        let mut entries = merge_memory_deletion_entries(&existing_entries, &purged);
        for entry in entries.iter_mut() {
            fence_purged_entry(entry, stamp)?;
        }
        let deletion_index = MemoryDeletionIndex::new(
            root.to_string_lossy().into_owned(),
            policy.revision,
            policy.data_epoch,
            stamp,
            entries,
        )
        .map_err(failed)?;
        write_deletion_index(&mut index_file, &deletion_index)?;
        deletion_index_report = deletion_index_summary(&deletion_index);
    } else {
        return Err(failed("governance_action_invalid"));
    }
    write_policy(&mut file, &policy)?;
    let propagation_state = if affected.is_empty() {
        "available"
    } else {
        "revoked"
    };
    Ok(
        json!({"schema":"kiana.data-governance-result.v1","project_root":root,"revision":policy.revision,"action":action,
        "affected_grants":affected,"erased_sources":erased,"policy":policy,
        "data_epoch":policy.data_epoch,
        "deletion_index":deletion_index_report,
        "propagation":{"receipt":propagation_state,"audit":if policy.grants.values().any(|grant| grant.retention.retain_audit_metadata) {"metadata_retained"} else {propagation_state},"artifact":propagation_state,"memory":propagation_state,"memory-jsonl":propagation_state,"memory-body":propagation_state,"index":propagation_state,"bm25-index":propagation_state,"dense-index":propagation_state,"repo-index":propagation_state,"cache":propagation_state,"prompt-cache":propagation_state,"context-plan":propagation_state,"summary":propagation_state,"checkpoint":propagation_state,"ui":propagation_state,"export":propagation_state},
        "historical_receipts":"preserved_invalid","reinjection":"blocked","cache_policy":"revoked_sources_excluded","runner_snapshots":"require_fresh_context",
        "audit_metadata_retained":true}),
    )
}
fn write_policy(file: &mut File, policy: &DataPolicy) -> Result<(), PortError> {
    let bytes = serde_json::to_vec(policy).map_err(|_| failed("governance_policy_invalid"))?;
    file.seek(SeekFrom::End(0)).map_err(io)?;
    file.write_all(&bytes).map_err(io)?;
    file.write_all(b"\n").map_err(io)?;
    file.sync_all().map_err(io)
}
fn bytes_digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("sha256:{:x}", Sha256::digest(bytes))
}
fn confined(root: &Path, path: &str) -> Result<PathBuf, PortError> {
    let path =
        kiana_domain::normalize_role_path(path).ok_or_else(|| failed("governance_path_invalid"))?;
    if path == "." || path.starts_with(".git/") || path == ".git" || path.starts_with(".kiana/") {
        return Err(failed("governance_protected_path"));
    }
    let mut cursor = root.to_path_buf();
    for part in Path::new(&path).components() {
        cursor.push(part.as_os_str());
        if fs::symlink_metadata(&cursor)
            .map_err(io)?
            .file_type()
            .is_symlink()
        {
            return Err(failed("governance_path_symlink"));
        }
    }
    let resolved = cursor.canonicalize().map_err(io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if fs::metadata(&resolved).map_err(io)?.nlink() != 1 {
            return Err(failed("governance_source_hardlink"));
        }
    }
    if !resolved.starts_with(root) || !resolved.is_file() {
        return Err(failed("governance_path_escape"));
    }
    Ok(resolved)
}
fn purge_memory(
    root: &Path,
    revoked: &std::collections::BTreeSet<String>,
    project: &Path,
    project_scoped: bool,
    purged: &mut Vec<MemoryDeletionEntry>,
) -> Result<(), PortError> {
    if !root.exists() {
        return Ok(());
    }
    if fs::symlink_metadata(root)
        .map_err(io)?
        .file_type()
        .is_symlink()
    {
        return Err(failed("governance_path_symlink"));
    }
    for item in fs::read_dir(root).map_err(io)? {
        let path = item.map_err(io)?.path();
        if path.is_dir() {
            purge_memory(&path, revoked, project, project_scoped, purged)?;
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let mut file = open_safe(&path, false)?;
        let _lock = lock(&file)?;
        let mut text = String::new();
        file.read_to_string(&mut text).map_err(io)?;
        let mut kept = Vec::new();
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let record: Value =
                serde_json::from_str(line).map_err(|_| failed("governance_memory_invalid"))?;
            let project_derived = project_scoped
                || record["project_root"]
                    .as_str()
                    .and_then(|root| Path::new(root).canonicalize().ok())
                    .is_some_and(|root| root == project);
            let source_revoked = revoked.iter().any(|source| {
                record["source"]
                    .as_str()
                    .is_some_and(|value| value.contains(source))
            });
            if project_derived || source_revoked {
                // The row is about to disappear; record the tombstone before rewriting the file.
                if let Some(entry) = purged_entry(&record) {
                    purged.push(entry);
                }
            } else {
                kept.push(line);
            }
        }
        file.seek(SeekFrom::Start(0)).map_err(io)?;
        file.set_len(0).map_err(io)?;
        for line in kept {
            writeln!(file, "{line}").map_err(io)?;
        }
        file.sync_all().map_err(io)?;
    }
    Ok(())
}

/// A purge records only rows it can name: id, collection and revision must be present, because an
/// entry that cannot identify its object could not fence a revived copy of it.
fn purged_entry(record: &Value) -> Option<MemoryDeletionEntry> {
    let record_id = record["id"].as_str()?.trim();
    let collection = record["collection"].as_str()?.trim();
    let revision = record["revision"]
        .as_u64()
        .filter(|revision| *revision > 0)?;
    if record_id.is_empty() || collection.is_empty() {
        return None;
    }
    MemoryDeletionEntry::new(
        record_id.to_owned(),
        collection.to_owned(),
        revision,
        json_digest(record),
        now_ms().max(1),
    )
    .ok()
}
fn remove_managed_tree(path: &Path) -> Result<(), PortError> {
    let metadata = fs::symlink_metadata(path).map_err(io)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        for item in fs::read_dir(path).map_err(io)? {
            remove_managed_tree(&item.map_err(io)?.path())?;
        }
        fs::remove_dir(path).map_err(io)
    } else {
        fs::remove_file(path).map_err(io)
    }
}
fn open_safe(path: &Path, create: bool) -> Result<File, PortError> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(create);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path).map_err(io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if file.metadata().map_err(io)?.nlink() != 1 {
            return Err(failed("governance_path_hardlink"));
        }
    }
    Ok(file)
}
struct FileLock(i32);
fn lock(file: &File) -> Result<FileLock, PortError> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let fd = file.as_raw_fd();
        if unsafe { libc::flock(fd, libc::LOCK_EX) } != 0 {
            return Err(failed("governance_lock_failed"));
        }
        Ok(FileLock(fd))
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        Err(failed("governance_lock_unsupported"))
    }
}
impl Drop for FileLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::flock(self.0, libc::LOCK_UN);
        }
    }
}
fn io(error: std::io::Error) -> PortError {
    failed(format!("governance_io:{error}"))
}
fn failed(reason: impl Into<String>) -> PortError {
    PortError::Failed(reason.into())
}
