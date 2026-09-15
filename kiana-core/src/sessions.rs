use super::redaction::redact_event_value;
use super::*;

impl ControlPlane {
    /// A session selects its role once at the authenticated daemon boundary.
    /// Subsequent requests cannot change the assignment by changing wire metadata.
    pub async fn bind_session_assignment(&self, context: &RequestContext) -> Result<(), CoreError> {
        let role = RoleSpec::lookup(&context.role_id)
            .ok_or_else(|| PortError::Failed("role_unknown".to_owned()))?;
        let mut principal = kiana_domain::AuthenticatedPrincipalRef::local();
        principal.principal_id = context.actor_id.clone().unwrap_or_default();
        principal.principal_digest = principal.digest();
        principal.validate().map_err(PortError::Failed)?;
        let project = kiana_domain::ProjectIdentity::new(
            context.project_root.clone(),
            Self::canonical_project_root(&context.project_root)
                .to_string_lossy()
                .into_owned(),
            None,
            None,
            kiana_domain::json_digest(&json!({"trusted":context.project_trusted})),
        )
        .map_err(PortError::Failed)?;
        let typed = kiana_domain::SessionAssignment::new(
            principal.clone(),
            project.clone(),
            context.session_id.as_str(),
            role.role_id.clone(),
            role.department_id.clone(),
            1,
            1,
        )
        .map_err(PortError::Failed)?;
        let assignment = json!({"schema":"kiana.session-assignment.v1","session_id":context.session_id,
            "actor_id":context.actor_id,"project_root":Self::canonical_project_root(&context.project_root),
            "role_id":role.role_id,"department_id":role.department_id,"prompt_hash":role.prompt_hash,
            "model_profile":role.model_profile,"principal":principal,"project_identity":project,
            "assignment":typed});
        let key = kiana_domain::json_digest(
            &json!({"session_id":context.session_id,"project_root":Self::canonical_project_root(&context.project_root)}),
        );
        for _ in 0..4 {
            let prior = self.events.read_stream("session_assignment", &key).await?;
            if let Some(event) = prior.first() {
                if event.data != assignment {
                    return Err(
                        PortError::Conflict("session_assignment_mismatch".to_owned()).into(),
                    );
                }
                return Ok(());
            }
            let event =
                RuntimeEvent::new(RequestId::new(), 1, "session.assigned", assignment.clone())?
                    .with_stream_metadata("session_assignment", &key, 1);
            match self.events.append_expected(event, Some(0)).await {
                Ok(()) => return Ok(()),
                Err(PortError::Conflict(_)) => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(PortError::Conflict("session_assignment_contention".to_owned()).into())
    }
    pub(crate) async fn await_capability_stop(&self, run_id: RunId) -> bool {
        let receiver = self
            .capability_stops
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&run_id)
            .map(watch::Sender::subscribe);
        let Some(mut receiver) = receiver else {
            return true;
        };
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if let Some(confirmed) = *receiver.borrow() {
                    return confirmed;
                }
                if receiver.changed().await.is_err() {
                    return false;
                }
            }
        })
        .await
        .unwrap_or(false)
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
        if let Some(assignment) = data.get("assignment") {
            let typed: kiana_domain::SessionAssignment = serde_json::from_value(assignment.clone())
                .map_err(|_| PortError::Failed("session_assignment_invalid".to_owned()))?;
            typed.validate().map_err(PortError::Failed)?;
            if typed.session_id.as_str() != session_id
                || typed.principal.principal_id
                    != data
                        .get("actor_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                || typed.role_id
                    != data
                        .get("role_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                || typed.department_id
                    != data
                        .get("department_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
            {
                return Ok(None);
            }
        }
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

pub(crate) fn acquire_durable_path_locks(
    project_root: &str,
    paths: &[String],
) -> Result<Vec<PathLockLease>, &'static str> {
    let root = durable_path_lock_root();
    fs::create_dir_all(&root).map_err(|_| "path_lock_unavailable")?;
    // Shared ancestor locks and an exclusive leaf lock detect file/directory overlap
    // across processes while allowing unrelated subtrees to proceed concurrently.
    let mut modes = std::collections::BTreeMap::<String, bool>::new();
    for path in paths {
        let normalized = kiana_domain::normalize_role_path(path).ok_or("path_lock_invalid")?;
        modes.entry(".".to_owned()).or_insert(false);
        let mut ancestor = Path::new(&normalized).parent();
        while let Some(parent) = ancestor {
            let text = parent.to_string_lossy();
            if !text.is_empty() {
                modes.entry(text.into_owned()).or_insert(false);
            }
            ancestor = parent.parent();
        }
        modes.insert(normalized, true);
    }
    let mut leases = Vec::with_capacity(modes.len());
    for (path, exclusive) in modes {
        let lock_path = durable_path_lock_path(project_root, &path);
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        options.custom_flags(libc::O_NOFOLLOW).mode(0o600);
        let file = options
            .open(lock_path)
            .map_err(|_| "path_lock_unavailable")?;
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let mode = if exclusive {
                libc::LOCK_EX
            } else {
                libc::LOCK_SH
            };
            if unsafe { libc::flock(file.as_raw_fd(), mode | libc::LOCK_NB) } != 0 {
                return Err("path_lock_conflict");
            }
        }
        #[cfg(not(unix))]
        {
            let _ = exclusive;
            return Err("path_lock_platform_unsupported");
        }
        leases.push(PathLockLease { _file: file });
    }
    Ok(leases)
}

#[cfg(all(unix, test))]
fn try_lock_path_file(file: &File) -> bool {
    use std::os::fd::AsRawFd;
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
}

#[cfg(all(not(unix), test))]
fn try_lock_path_file(_file: &File) -> bool {
    true
}
