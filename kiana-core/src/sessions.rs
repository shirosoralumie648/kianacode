use super::redaction::redact_event_value;
use super::*;

impl ControlPlane {
    pub async fn send_runner(&self, command: RunnerCommand) -> Result<Vec<RunnerEvent>, CoreError> {
        Ok(self.runner.send(command).await?)
    }

    pub(crate) async fn resolve_run_id(
        &self,
        context: &RequestContext,
        run_id: Option<RunId>,
    ) -> Result<Result<RunId, &'static str>, CoreError> {
        let binding = match self.session_binding(context.session_id.as_str()) {
            Some(binding) => binding,
            None => {
                let Some(binding) = self
                    .rebuild_session_binding(context.session_id.as_str())
                    .await?
                else {
                    return Ok(Err("session_not_found"));
                };
                if !Self::same_session_principal(&binding, context) {
                    return Ok(Err("session_owner_mismatch"));
                }
                self.cache_rebuilt_session_if_absent(context.session_id.as_str(), binding)
            }
        };
        if !Self::same_session_principal(&binding, context) {
            return Ok(Err("session_owner_mismatch"));
        }
        if let Some(requested) = run_id {
            if requested != binding.run_id {
                return Ok(Err("run_owner_mismatch"));
            }
        }
        Ok(Ok(binding.run_id))
    }

    async fn rebuild_session_binding(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionBinding>, CoreError> {
        let Some(events) = self.read_all_events().await? else {
            return Ok(None);
        };
        let Some(data) = events.iter().rev().find_map(|event| {
            if event.kind != "run.authorized" {
                return None;
            }
            let data = redact_event_value(&event.data);
            (data.get("session_id").and_then(Value::as_str) == Some(session_id)).then_some(data)
        }) else {
            return Ok(None);
        };
        let Some(run_id) = data
            .get("run_id")
            .and_then(Value::as_str)
            .and_then(RunId::parse_str)
        else {
            return Ok(None);
        };
        let Some(project_root) = data
            .get("project_root")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            return Ok(None);
        };
        let Some(role_id) = data
            .get("role_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            return Ok(None);
        };
        let Some(department_id) = data
            .get("department_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            return Ok(None);
        };
        let actor_id = match data.get("actor_id") {
            Some(Value::String(actor_id)) => Some(actor_id.clone()),
            Some(Value::Null) | None => None,
            Some(_) => return Ok(None),
        };
        Ok(Some(SessionBinding {
            run_id,
            actor_id,
            project_root,
            role_id,
            department_id,
        }))
    }

    fn cache_rebuilt_session_if_absent(
        &self,
        session_id: &str,
        binding: SessionBinding,
    ) -> SessionBinding {
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(session_id.to_owned())
            .or_insert(binding)
            .clone()
    }

    pub(crate) fn session_binding(&self, session_id: &str) -> Option<SessionBinding> {
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(session_id)
            .cloned()
    }

    pub(crate) fn session_known(&self, session_id: &str) -> bool {
        self.session_run_id(session_id).is_some()
    }

    pub(crate) fn session_run_id(&self, session_id: &str) -> Option<RunId> {
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(session_id)
            .map(|binding| binding.run_id)
    }

    pub(crate) fn remember_session(&self, context: &RequestContext, run_id: RunId) {
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                context.session_id.as_str().to_owned(),
                SessionBinding {
                    run_id,
                    actor_id: context.actor_id.clone(),
                    project_root: context.project_root.clone(),
                    role_id: context.role_id.clone(),
                    department_id: context.department_id.clone(),
                },
            );
    }

    pub(crate) fn forget_session(&self, session_id: &str, run_id: RunId) {
        let mut sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        if sessions
            .get(session_id)
            .is_some_and(|binding| binding.run_id == run_id)
        {
            sessions.remove(session_id);
        }
    }

    pub(crate) fn same_session_principal(
        binding: &SessionBinding,
        context: &RequestContext,
    ) -> bool {
        binding.actor_id == context.actor_id
            && Self::canonical_project_root(&binding.project_root)
                == Self::canonical_project_root(&context.project_root)
            && binding.role_id == context.role_id
            && binding.department_id == context.department_id
    }

    pub(crate) fn canonical_project_root(root: &str) -> std::path::PathBuf {
        let path = Path::new(root);
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    }

    pub(crate) fn acquire_builder_path_locks(
        &self,
        project_root: &str,
        session_id: &str,
        path_allow: &[String],
    ) -> Result<(), &'static str> {
        let paths = builder_lock_paths(path_allow);
        let mut locks = self
            .path_locks
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for path in &paths {
            for (held, owner) in locks.iter() {
                if owner != session_id && path_locks_conflict(path, held) {
                    return Err("path_lock_conflict");
                }
            }
        }

        let lock_key = path_lock_session_key(project_root, session_id);
        let durable = match acquire_durable_path_locks(project_root, &paths) {
            Ok(durable) => durable,
            Err(reason) => return Err(reason),
        };
        for path in paths {
            locks.insert(path, session_id.to_owned());
        }
        self.durable_path_locks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(lock_key, durable);
        Ok(())
    }

    pub(crate) fn release_builder_path_locks(&self, project_root: &str, session_id: &str) {
        self.path_locks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|_, owner| owner != session_id);
        self.durable_path_locks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&path_lock_session_key(project_root, session_id));
    }

    pub(crate) fn watch_cancel(&self, run_id: RunId) -> watch::Receiver<bool> {
        let mut cancellations = self
            .cancellations
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(tx) = cancellations.get(&run_id) {
            return tx.subscribe();
        }
        let (tx, rx) = watch::channel(false);
        cancellations.insert(run_id, tx);
        rx
    }

    pub(crate) fn signal_cancel(&self, run_id: RunId) -> bool {
        let mut cancellations = self
            .cancellations
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(tx) = cancellations.get(&run_id) {
            let _ = tx.send(true);
            return true;
        }
        // Cancel arrived before drive_run subscribed: leave a pre-signaled
        // watch so the in-flight capability select! still aborts.
        let (tx, _rx) = watch::channel(true);
        cancellations.insert(run_id, tx);
        false
    }

    pub(crate) fn clear_cancel(&self, run_id: RunId) {
        self.cancellations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&run_id);
    }
}

fn path_lock_session_key(project_root: &str, session_id: &str) -> String {
    format!(
        "{}\0{}",
        ControlPlane::canonical_project_root(project_root).display(),
        session_id
    )
}

fn durable_path_lock_root() -> PathBuf {
    std::env::var_os("KIANA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".kiana")))
        .unwrap_or_else(|| PathBuf::from(".kiana"))
        .join("locks")
}

fn durable_path_lock_path(project_root: &str, path: &str) -> PathBuf {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    ControlPlane::canonical_project_root(project_root)
        .display()
        .to_string()
        .hash(&mut hasher);
    path.hash(&mut hasher);
    durable_path_lock_root().join(format!("{:016x}.lock", hasher.finish()))
}

fn acquire_durable_path_locks(
    project_root: &str,
    paths: &[String],
) -> Result<Vec<PathLockLease>, &'static str> {
    let root = durable_path_lock_root();
    fs::create_dir_all(&root).map_err(|_| "path_lock_unavailable")?;
    let mut leases = Vec::with_capacity(paths.len());
    for path in paths {
        let lock_path = durable_path_lock_path(project_root, path);
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|_| "path_lock_unavailable")?;
        if !try_lock_path_file(&file) {
            return Err("path_lock_conflict");
        }
        leases.push(PathLockLease { _file: file });
    }
    Ok(leases)
}

#[cfg(unix)]
fn try_lock_path_file(file: &File) -> bool {
    use std::os::fd::AsRawFd;
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
}

#[cfg(not(unix))]
fn try_lock_path_file(_file: &File) -> bool {
    true
}
