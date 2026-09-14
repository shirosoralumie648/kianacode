//! Private workspaces for script execution. Publication uses the patch commit boundary.
use kiana_domain::{AuthorizedCapabilityRequest, RequestId};
use kiana_ports::PortError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const FILE_LIMIT: usize = 100_000;
const FILE_BYTES: u64 = 16 * 1024 * 1024;
const TOTAL_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FileVersion {
    pub sha256: String,
    pub bytes: u64,
    pub executable: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PublishedFile {
    pub path: String,
    pub before: Option<FileVersion>,
    pub after: Option<Vec<u8>>,
    pub executable: bool,
}

/// Contains no independent authorization. The original request's publication scope is retained.
pub(crate) struct ExecutionWorkspace {
    source: PathBuf,
    storage: PathBuf,
    pub root: PathBuf,
    pub workdir: PathBuf,
    baseline: BTreeMap<String, FileVersion>,
    path_allow: Vec<String>,
    request_id: RequestId,
    retained: bool,
}

impl ExecutionWorkspace {
    pub(crate) fn retain_unknown(&mut self) {
        self.retained = true;
    }
    pub fn prepare(
        request: &AuthorizedCapabilityRequest,
        source: &Path,
        workdir: &Path,
    ) -> Result<Self, PortError> {
        let source = source.canonicalize().map_err(io_error)?;
        let relative = workdir
            .strip_prefix(&source)
            .map_err(|_| error("workdir_outside_project"))?;
        let path_allow = request.request.arguments["path_allow"]
            .as_array()
            .ok_or_else(|| error("write_scope_required"))?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .and_then(kiana_domain::normalize_role_path)
                    .ok_or_else(|| error("write_scope_invalid"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if path_allow.is_empty() {
            return Err(error("write_scope_empty"));
        }
        let storage = std::env::temp_dir().join(format!("kiana-environment-{}", RequestId::new()));
        create_private_directory(&storage)?;
        let root = storage.join("workspace");
        create_private_directory(&root)?;
        let mut workspace = Self {
            source: source.clone(),
            storage,
            root: root.clone(),
            workdir: root.join(relative),
            baseline: BTreeMap::new(),
            path_allow,
            request_id: request.request.request_id,
            retained: false,
        };
        let policy = crate::data_governance::read_policy(&source)?;
        let mut bytes = 0;
        walk(
            &source,
            &source,
            Some(&root),
            &mut workspace.baseline,
            &mut bytes,
            &policy.revoked_sources.iter().cloned().collect::<Vec<_>>(),
            0,
        )?;
        if !workspace.workdir.is_dir() {
            return Err(error("workdir_unavailable_in_snapshot"));
        }
        let manifest = json!({"schema":"kiana.execution-workspace.v1","request_id":workspace.request_id,
            "source_root":source,"owner":request.request.arguments["actor_id"],"session_id":request.request.arguments["session_id"],
            "baseline":workspace.baseline,"path_allow":workspace.path_allow,"mode":"isolated_staged"});
        write_private_json(&workspace.storage.join("manifest.json"), &manifest)?;
        Ok(workspace)
    }

    /// Keep the private snapshot available for operator reconciliation after an unconfirmed stop.
    /// The manifest contains no credentials and Drop will not delete the workspace.
    pub fn retain_for_reconciliation(
        mut self,
        reason: impl Into<String>,
    ) -> Result<Value, PortError> {
        self.retained = true;
        let reason = reason.into();
        if reason.trim().is_empty() || reason.len() > 256 {
            return Err(error("workspace_reconcile_reason_invalid"));
        }
        write_private_json(
            &self.storage.join("reconciliation.json"),
            &json!({
                "schema":"kiana.execution-workspace-reconciliation.v1",
                "request_id":self.request_id,
                "reason":reason,
                "source_root":self.source,
                "workspace_root":self.root,
                "workdir":self.workdir,
                "path_allow":self.path_allow,
                "retained":true,
            }),
        )?;
        Ok(
            json!({"mode":"isolated_staged","published":false,"retained":true,
            "reason":reason,"request_id":self.request_id,"workspace":self.storage}),
        )
    }

    pub fn finish(mut self, successful: bool) -> Result<Value, PortError> {
        let mut current = BTreeMap::new();
        let mut bytes = 0;
        walk(
            &self.root,
            &self.root,
            None,
            &mut current,
            &mut bytes,
            &[],
            0,
        )?;
        let keys = self
            .baseline
            .keys()
            .chain(current.keys())
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let mut changes = Vec::new();
        let mut denied = Vec::new();
        for path in keys {
            let before = self.baseline.get(&path);
            let after = current.get(&path);
            if before == after {
                continue;
            }
            if !kiana_domain::allow_list_covers(&self.path_allow, &path)
                || path
                    .split('/')
                    .any(crate::harness_sandbox::private_component)
                || path == ".git"
                || path.starts_with(".git/")
            {
                denied.push(path);
                continue;
            }
            let content = after
                .map(|_| read_regular(&self.root.join(&path)))
                .transpose()?;
            changes.push(PublishedFile {
                path,
                before: before.cloned(),
                after: content,
                executable: after.is_some_and(|file| file.executable),
            });
        }
        if !denied.is_empty() {
            self.retained = true;
            write_private_json(
                &self.storage.join("rejected.json"),
                &json!({"paths":denied,"published":false}),
            )?;
            return Err(error("changeset_outside_grant:not_published"));
        }
        if !successful {
            // No script changes are published after failure, timeout or cancellation.
            return Ok(
                json!({"mode":"isolated_staged","published":false,"discarded_changes":changes.len(),"host_effect":"none"}),
            );
        }
        if changes.len() > 4096 {
            return Err(error("publication_file_limit"));
        }
        let result =
            crate::apply_patch::publish_workspace_files(&self.source, &changes, self.request_id)?;
        Ok(
            json!({"mode":"isolated_staged","published":true,"host_effect":"confirmed","changes":result,
            "baseline_digest":kiana_domain::json_digest(&json!(self.baseline))}),
        )
    }
}

impl Drop for ExecutionWorkspace {
    fn drop(&mut self) {
        if !self.retained {
            let _ = fs::remove_dir_all(&self.storage);
        }
    }
}

pub(crate) fn file_version(bytes: &[u8], executable: bool) -> FileVersion {
    FileVersion {
        sha256: format!("{:x}", Sha256::digest(bytes)),
        bytes: bytes.len() as u64,
        executable,
    }
}
pub(crate) fn read_regular(path: &Path) -> Result<Vec<u8>, PortError> {
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > FILE_BYTES {
        return Err(error("file_type_or_size_denied"));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = options.open(path).map_err(io_error)?;
    let opened = file.metadata().map_err(io_error)?;
    #[cfg(unix)]
    if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() || opened.nlink() != 1 {
        return Err(error("file_identity_changed_or_hardlink"));
    }
    let mut bytes = Vec::new();
    file.take(FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > FILE_BYTES {
        return Err(error("file_size_limit"));
    }
    Ok(bytes)
}
fn walk(
    root: &Path,
    directory: &Path,
    destination: Option<&Path>,
    manifest: &mut BTreeMap<String, FileVersion>,
    total: &mut u64,
    revoked: &[String],
    depth: usize,
) -> Result<(), PortError> {
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::ffi::OsStrExt;
        fn visit(
            root: &Path,
            relative: &Path,
            handle: File,
            destination: Option<&Path>,
            manifest: &mut BTreeMap<String, FileVersion>,
            total: &mut u64,
            revoked: &[String],
            depth: usize,
        ) -> Result<(), PortError> {
            if depth > 64 {
                return Err(error("path_depth_limit"));
            }
            let anchored = PathBuf::from(format!("/proc/self/fd/{}", handle.as_raw_fd()));
            let mut entries = fs::read_dir(&anchored)
                .map_err(io_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(io_error)?;
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let name = entry.file_name();
                let text = name.to_str().ok_or_else(|| error("path_not_utf8"))?;
                let relative = relative.join(&name);
                let key = relative
                    .to_str()
                    .ok_or_else(|| error("path_not_utf8"))?
                    .to_owned();
                if crate::harness_sandbox::private_component(text)
                    || text == ".git"
                    || revoked
                        .iter()
                        .any(|source| key == *source || key.starts_with(&format!("{source}/")))
                {
                    continue;
                }
                let kind = entry.file_type().map_err(io_error)?;
                if kind.is_symlink() || (!kind.is_file() && !kind.is_dir()) {
                    return Err(error("special_file_requires_explicit_adapter"));
                }
                if kind.is_dir() {
                    let name =
                        std::ffi::CString::new(name.as_bytes()).map_err(|_| error("path_nul"))?;
                    let fd = unsafe {
                        libc::openat(
                            handle.as_raw_fd(),
                            name.as_ptr(),
                            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                        )
                    };
                    if fd < 0 {
                        return Err(error("directory_identity_changed"));
                    }
                    let child = unsafe { File::from_raw_fd(fd) };
                    if let Some(destination) = destination {
                        create_private_directory(&destination.join(&relative))?;
                    }
                    visit(
                        root,
                        &relative,
                        child,
                        destination,
                        manifest,
                        total,
                        revoked,
                        depth + 1,
                    )?;
                    continue;
                }
                if manifest.len() >= FILE_LIMIT {
                    return Err(error("file_count_limit"));
                }
                let bytes = read_regular(&anchored.join(&name))?;
                *total = total.saturating_add(bytes.len() as u64);
                if *total > TOTAL_BYTES {
                    return Err(error("workspace_byte_limit"));
                }
                let metadata = entry.metadata().map_err(io_error)?;
                let executable = metadata.permissions().mode() & 0o111 != 0;
                if let Some(destination) = destination {
                    let mut options = OpenOptions::new();
                    options
                        .write(true)
                        .create_new(true)
                        .mode(if executable { 0o700 } else { 0o600 })
                        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
                    let mut file = options
                        .open(destination.join(&relative))
                        .map_err(io_error)?;
                    file.write_all(&bytes).map_err(io_error)?;
                }
                manifest.insert(key, file_version(&bytes, executable));
            }
            Ok(())
        }
        let mut options = OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_DIRECTORY);
        let handle = options.open(directory).map_err(io_error)?;
        let relative = directory
            .strip_prefix(root)
            .map_err(|_| error("path_escape"))?;
        visit(
            root,
            relative,
            handle,
            destination,
            manifest,
            total,
            revoked,
            depth,
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (
            root,
            directory,
            destination,
            manifest,
            total,
            revoked,
            depth,
        );
        Err(error("snapshot_handle_backend_unsupported"))
    }
}
pub(crate) fn create_private_directory(path: &Path) -> Result<(), PortError> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).map_err(io_error)
}
pub(crate) fn write_private_json(path: &Path, value: &impl Serialize) -> Result<(), PortError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut file = options.open(path).map_err(io_error)?;
    serde_json::to_writer(&mut file, value).map_err(|_| error("journal_encode_failed"))?;
    file.write_all(b"\n").map_err(io_error)?;
    file.sync_all().map_err(io_error)?;
    if let Some(parent) = path.parent() {
        File::open(parent)
            .and_then(|file| file.sync_all())
            .map_err(io_error)?;
    }
    Ok(())
}
fn error(reason: &str) -> PortError {
    PortError::Failed(format!("execution_workspace:{reason}"))
}
fn io_error(error: std::io::Error) -> PortError {
    PortError::Failed(format!("execution_workspace_io:{error}"))
}
