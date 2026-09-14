//! Trusted executable hooks use the daemon's read-only process boundary.
use async_trait::async_trait;
use kiana_domain::{CapabilityRequest, RequestContext, RequestId, RuntimeEvent};
use kiana_ports::{EventStorePort, PortError, PreToolHookDecision, PreToolHookPort};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;

pub(crate) struct QueryPreToolHooks {
    mcp: Arc<crate::harness_mcp::McpRegistry>,
    events: Arc<dyn EventStorePort>,
    environment: Option<String>,
    user_file: Option<PathBuf>,
    timeout: Duration,
    memory_home: Option<PathBuf>,
}
struct HookSnapshot {
    commands: Vec<String>,
    digest: String,
}
impl QueryPreToolHooks {
    pub fn new(
        mcp: Arc<crate::harness_mcp::McpRegistry>,
        events: Arc<dyn EventStorePort>,
    ) -> Result<Self, PortError> {
        let environment = std::env::var("KIANA_PRE_TOOL_USE_HOOKS")
            .ok()
            .or_else(|| std::env::var("KIANA_HOOKS").ok());
        if environment
            .as_ref()
            .is_some_and(|raw| raw.len() > 64 * 1024)
        {
            return Err(failed("hook_config_limit"));
        }
        let user_file = std::env::var_os("KIANA_HOOKS_FILE")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("KIANA_HOME")
                    .map(PathBuf::from)
                    .or_else(|| {
                        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".kiana"))
                    })
                    .map(|home| home.join("hooks.json"))
            });
        if user_file.as_ref().is_some_and(|path| !path.is_absolute()) {
            return Err(failed("hook_config_absolute_required"));
        }
        let timeout = std::env::var("KIANA_HOOK_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(10_000u64)
            .clamp(1, 10_000);
        Ok(Self {
            mcp,
            events,
            environment,
            user_file,
            timeout: Duration::from_millis(timeout),
            memory_home: std::env::var_os("KIANA_HOME").map(PathBuf::from),
        })
    }

    fn snapshot(&self, context: &RequestContext) -> Result<HookSnapshot, PortError> {
        let mut sources = Vec::new();
        let mut commands = Vec::new();
        if let Some(raw) = &self.environment {
            commands.extend(parse_commands(raw)?);
            sources.push(json!({"scope":"host_environment","digest":kiana_domain::journal_sha256(raw.as_bytes())}));
        } else {
            if let Some(path) = &self.user_file {
                if let Some(raw) = read_config(path)? {
                    commands.extend(parse_file(&raw)?);
                    sources.push(json!({"scope":"host_file","path":path,"digest":kiana_domain::journal_sha256(raw.as_bytes())}));
                }
            }
            // Project configuration is not even opened until the server's trust check succeeds.
            if context.project_trusted {
                let path = Path::new(&context.project_root).join(".kiana/hooks.json");
                if let Some(raw) = read_config(&path)? {
                    commands.extend(parse_file(&raw)?);
                    sources.push(json!({"scope":"trusted_project","path":path,"digest":kiana_domain::journal_sha256(raw.as_bytes())}));
                }
            }
        }
        if commands.len() > 8
            || commands
                .iter()
                .any(|command| command.len() > 8192 || command.contains('\0'))
        {
            return Err(failed("hook_command_limit"));
        }
        let digest = kiana_domain::json_digest(
            &json!({"schema":"kiana.hook-snapshot.v1","sources":sources,"commands":commands,
            "timeout_ms":self.timeout.as_millis(),"sandbox":"read-only","phase":"PreToolUse"}),
        );
        Ok(HookSnapshot { commands, digest })
    }

    async fn decide_owned(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
        cancellation: watch::Receiver<bool>,
    ) -> Result<PreToolHookDecision, PortError> {
        let snapshot = self.snapshot(context)?;
        if request.arguments["hook_snapshot"]["digest"] != snapshot.digest {
            return Ok(PreToolHookDecision::Block(
                "hook_configuration_changed".to_owned(),
            ));
        }
        if snapshot.commands.is_empty() {
            return Ok(PreToolHookDecision::Allow);
        }
        if !context.project_trusted {
            return Ok(PreToolHookDecision::Block("project_untrusted".to_owned()));
        }
        let input = json!({"hook_event_name":"PreToolUse","tool_name":hook_tool_name(&request.operation),"tool_input":request.arguments,
            "tool_use_id":request.arguments["call_id"],"session_id":context.session_id,"cwd":context.project_root,
            "permission_mode":permission_mode_label(context.permission_profile)});
        let payload = serde_json::to_string(&input).map_err(|_| failed("hook_input_invalid"))?;
        if payload.len() > 64 * 1024 {
            return Ok(PreToolHookDecision::Block("hook_input_limit".to_owned()));
        }
        let root = Path::new(&context.project_root);
        let mut scope = request.arguments.clone();
        scope["sandbox"] = json!("read-only");
        scope["project_root"] = json!(context.project_root);
        scope["path_allow"] = json!(context.path_allow);
        let overall = Instant::now();
        let mut ask = None;
        for (index, command) in snapshot.commands.iter().enumerate() {
            if *cancellation.borrow() {
                return Ok(PreToolHookDecision::Block(
                    "cancelled:hook_not_started".to_owned(),
                ));
            }
            let remaining = Duration::from_secs(30).saturating_sub(overall.elapsed());
            if remaining.is_zero() {
                return Ok(PreToolHookDecision::Block(
                    "hook_deadline_exceeded".to_owned(),
                ));
            }
            let started = Instant::now();
            // Payload and command are positional parameters, never interpolated into shell code.
            let argv = vec![
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                "printf '%s\\n' \"$1\" | /bin/sh -c \"$2\"".to_owned(),
                "kiana-hook".to_owned(),
                payload.clone(),
                command.clone(),
            ];
            let output = crate::harness_capabilities::run_confined_cancellable(
                argv,
                root,
                root,
                "read-only",
                self.timeout.min(remaining),
                Some(cancellation.clone()),
                Some(&scope),
            )
            .await;
            let decision = match &output {
                Ok(output) => parse_decision(output),
                Err(error) => PreToolHookDecision::Block(format!("hook_execution_failed:{error}")),
            };
            let code = match &decision {
                PreToolHookDecision::Allow => "allow",
                PreToolHookDecision::Ask { .. } => "ask",
                PreToolHookDecision::Block(_) => "block",
            };
            let event=RuntimeEvent::new(RequestId::new(),1,"hook.decision",json!({"parent_request_id":request.request_id,
                "session_id":context.session_id,"project_root":context.project_root,"actor_id":context.actor_id,
                "phase":"PreToolUse","hook_snapshot":snapshot.digest,"command_digest":kiana_domain::journal_sha256(command.as_bytes()),
                "index":index,"decision":code,"duration_ms":started.elapsed().as_millis(),"sandbox":"read-only",
                "output_digest":output.as_ref().ok().map(kiana_domain::json_digest),"raw_output_retained":false,
                "stop_confirmed":output.as_ref().ok().and_then(|value|value.get("stop_confirmed"))})).map_err(|_|failed("hook_event_invalid"))?;
            self.events.append(event).await?;
            match decision {
                PreToolHookDecision::Block(reason) => {
                    return Ok(PreToolHookDecision::Block(reason))
                }
                PreToolHookDecision::Ask { reason } => ask = Some(reason),
                PreToolHookDecision::Allow => (),
            }
        }
        if *cancellation.borrow() {
            return Ok(PreToolHookDecision::Block(
                "cancelled:hook_stopped".to_owned(),
            ));
        }
        Ok(ask
            .map(|reason| PreToolHookDecision::Ask { reason })
            .unwrap_or(PreToolHookDecision::Allow))
    }
}

#[async_trait]
impl PreToolHookPort for QueryPreToolHooks {
    async fn prepare_action(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
        cancellation: watch::Receiver<bool>,
    ) -> Result<CapabilityRequest, PortError> {
        if *cancellation.borrow() {
            return Err(failed("cancelled:before_prepare"));
        }
        let mut prepared = self.mcp.prepare(context, request).await?;
        if matches!(
            request.operation.as_str(),
            "memory.search" | "memory.write" | "memory.review"
        ) {
            prepared.arguments["memory_snapshot"] =
                json!({"schema":"kiana.memory-storage.v1","home":self.memory_home});
        }
        let snapshot = self.snapshot(context)?;
        prepared.arguments["hook_snapshot"] = json!({"schema":"kiana.hook-snapshot.v1","digest":snapshot.digest,
            "phase":"PreToolUse","count":snapshot.commands.len(),"sandbox":"read-only"});
        if *cancellation.borrow() {
            return Err(failed("cancelled:prepare_stopped"));
        }
        Ok(prepared)
    }
    async fn decide(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
    ) -> Result<PreToolHookDecision, PortError> {
        let (_sender, cancellation) = watch::channel(false);
        self.decide_owned(context, request, cancellation).await
    }
    async fn decide_cancellable(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
        cancellation: watch::Receiver<bool>,
    ) -> Result<PreToolHookDecision, PortError> {
        self.decide_owned(context, request, cancellation).await
    }
}

fn parse_decision(output: &Value) -> PreToolHookDecision {
    let blocked = |reason: &str| PreToolHookDecision::Block(reason.to_owned());
    if output["cancelled"] == true {
        return blocked("cancelled:hook_stopped");
    }
    if output["timed_out"] == true {
        return blocked("hook_timed_out");
    }
    if output["exit_code"] != 0 {
        return blocked("hook_nonzero_exit");
    }
    let raw = output["stdout"].as_str().unwrap_or_default().trim();
    if raw.is_empty() {
        return PreToolHookDecision::Allow;
    }
    if raw.len() > 64 * 1024 {
        return blocked("hook_output_limit");
    }
    let Ok(value) = kiana_domain::parse_bounded_json(raw.as_bytes()) else {
        return blocked("hook_output_invalid");
    };
    if !value.is_object() {
        return blocked("hook_output_invalid");
    }
    let specific = value.get("hookSpecificOutput").unwrap_or(&value);
    for object in [&value, specific] {
        if [
            "updatedInput",
            "updateInput",
            "updated_input",
            "update_input",
        ]
        .iter()
        .any(|key| object.get(key).is_some_and(|value| !value.is_null()))
        {
            return blocked("hook_update_input_unsupported");
        }
    }
    if value["continue"] == false
        || value["continue_processing"] == false
        || value["decision"] == "block"
        || value["block"] == true
    {
        return blocked("hook_blocked");
    }
    for (object, key) in [
        (specific, "permissionDecision"),
        (&value, "permission_decision"),
        (&value, "decision"),
    ] {
        if object.get(key).is_some_and(|value| !value.is_string()) {
            return blocked("hook_decision_invalid");
        }
    }
    for key in ["continue", "continue_processing", "block"] {
        if value.get(key).is_some_and(|value| !value.is_boolean()) {
            return blocked("hook_decision_invalid");
        }
    }
    let decision = specific
        .get("permissionDecision")
        .or_else(|| value.get("permission_decision"))
        .or_else(|| value.get("decision"))
        .and_then(Value::as_str);
    match decision {
        Some("deny" | "block") => blocked("hook_blocked"),
        Some("ask") => PreToolHookDecision::Ask {
            reason: "hook_explicit_approval_required".to_owned(),
        },
        Some("allow" | "approve") | None => PreToolHookDecision::Allow,
        _ => blocked("hook_decision_invalid"),
    }
}
fn parse_commands(raw: &str) -> Result<Vec<String>, PortError> {
    if raw.trim().starts_with(['[', '{', '"']) {
        let value = kiana_domain::parse_bounded_json(raw.as_bytes())
            .map_err(|_| failed("hook_config_invalid"))?;
        let config = kiana_types::hooks::parse_hook_config(value)
            .map_err(|_| failed("hook_config_invalid"))?;
        return Ok(kiana_types::hooks::hook_commands_for_event(
            &config,
            "PreToolUse",
        ));
    }
    kiana_types::hooks::parse_hook_commands(raw).map_err(|_| failed("hook_config_invalid"))
}
fn parse_file(raw: &str) -> Result<Vec<String>, PortError> {
    let value = kiana_domain::parse_bounded_json(raw.as_bytes())
        .map_err(|_| failed("hook_config_invalid"))?;
    let config =
        kiana_types::hooks::parse_hook_config(value).map_err(|_| failed("hook_config_invalid"))?;
    // Other phases must not silently keep the old uncontained executor alive.
    if config.iter().any(|(phase, commands)| {
        !commands.is_empty()
            && !matches!(
                phase.as_str(),
                "PreToolUse"
                    | "pre_tool_use"
                    | "hooks"
                    | "fallback"
                    | "all"
                    | "KIANA_HOOKS"
                    | "KIANA_PRE_TOOL_USE_HOOKS"
            )
    }) {
        return Err(failed("hook_phase_unsupported"));
    }
    Ok(kiana_types::hooks::hook_commands_for_event(
        &config,
        "PreToolUse",
    ))
}
fn read_config(path: &Path) -> Result<Option<String>, PortError> {
    use std::io::Read;
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(failed("hook_config_symlink_denied"))
            }
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(failed("hook_config_unavailable")),
        }
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let file = options
        .open(path)
        .map_err(|_| failed("hook_config_unavailable"))?;
    let meta = file
        .metadata()
        .map_err(|_| failed("hook_config_unavailable"))?;
    if !meta.is_file() || meta.len() > 64 * 1024 {
        return Err(failed("hook_config_limit"));
    }
    let mut raw = String::new();
    file.take(64 * 1024 + 1)
        .read_to_string(&mut raw)
        .map_err(|_| failed("hook_config_invalid"))?;
    if raw.len() > 64 * 1024 {
        return Err(failed("hook_config_limit"));
    }
    Ok(Some(raw))
}
fn failed(reason: impl Into<String>) -> PortError {
    PortError::Failed(reason.into())
}
fn hook_tool_name(operation: &str) -> String {
    kiana_domain::model_tool_name(operation)
        .unwrap_or(operation)
        .to_owned()
}
fn permission_mode_label(profile: kiana_domain::PermissionProfile) -> &'static str {
    match profile {
        kiana_domain::PermissionProfile::Safe => "safe",
        kiana_domain::PermissionProfile::Balanced => "balanced",
        kiana_domain::PermissionProfile::Autonomous => "autonomous",
    }
}
