//! Governed source retention and conservative invalidation of derived local material.
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{
    AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult, DataPolicy, ProcessingGrant,
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
        let next: DataPolicy =
            serde_json::from_slice(line).map_err(|_| failed("governance_policy_invalid"))?;
        if next.revision < policy.revision {
            return Err(failed("governance_revision_regressed"));
        }
        policy = next;
    }
    Ok(policy)
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
        return Ok(json!({"schema":"kiana.data-policy.v1","policy":read_policy(&root)?}));
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
        purge_memory(
            &managed.join("memory"),
            &policy.revoked_sources,
            &root,
            true,
        )?;
        if let Ok(home_root) = crate::harness_memory::kiana_home() {
            purge_memory(
                &home_root.join("memory"),
                &policy.revoked_sources,
                &root,
                false,
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
    } else {
        return Err(failed("governance_action_invalid"));
    }
    write_policy(&mut file, &policy)?;
    Ok(
        json!({"schema":"kiana.data-governance-result.v1","project_root":root,"revision":policy.revision,"action":action,
        "affected_grants":affected,"erased_sources":erased,"policy":policy,
        "cache_policy":"revoked_sources_excluded","runner_snapshots":"require_fresh_context",
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
            purge_memory(&path, revoked, project, project_scoped)?;
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
            if !project_derived && !source_revoked {
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
