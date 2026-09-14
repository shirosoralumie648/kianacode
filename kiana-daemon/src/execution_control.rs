//! Operator continuations for a supervised process; every request still consumes a core permit.
use async_trait::async_trait;
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{
    AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult, RequestId, RuntimeEvent,
};
use kiana_ports::{EventStorePort, PortError};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot, watch};

struct Input {
    text: String,
    reply: oneshot::Sender<Result<usize, String>>,
}
struct ProcessEntry {
    identity: Value,
    cancelled: watch::Sender<bool>,
    input: mpsc::Sender<Input>,
    state: Mutex<Value>,
    output: Mutex<(String, String)>,
    #[cfg(unix)]
    terminal: Option<std::fs::File>,
}
pub(crate) struct ExecutionControl {
    events: Arc<dyn EventStorePort>,
    processes: Mutex<HashMap<String, Arc<ProcessEntry>>>,
    capacity: Arc<tokio::sync::Semaphore>,
}

impl ExecutionControl {
    pub fn register(
        broker: &mut CapabilityBroker,
        events: Arc<dyn EventStorePort>,
    ) -> Result<(), PortError> {
        let control = Arc::new(Self {
            events,
            processes: Mutex::new(HashMap::new()),
            capacity: Arc::new(tokio::sync::Semaphore::new(8)),
        });
        for (kind, name) in [
            (CapabilityKind::Filesystem, "workspace.transaction"),
            (CapabilityKind::Filesystem, "local.package"),
            (CapabilityKind::Query, "execution.output.read"),
            (CapabilityKind::Query, "environment.inspect"),
            (CapabilityKind::Query, "tool.search"),
            (CapabilityKind::Process, "process.start"),
            (CapabilityKind::Query, "process.poll"),
            (CapabilityKind::Process, "process.stdin"),
            (CapabilityKind::Process, "process.resize"),
            (CapabilityKind::Process, "process.stop"),
        ] {
            broker.register_static(kind, name, Arc::new(ExecutionHandler(control.clone())))?;
        }
        Ok(())
    }
    async fn fact(&self, id: &str, kind: &str, data: Value) -> Result<(), PortError> {
        for _ in 0..4 {
            let previous = self.events.read_stream("process", id).await?;
            let version = previous
                .iter()
                .filter_map(|event| event.stream_version)
                .max()
                .unwrap_or(0);
            let request_id = RequestId::new();
            let event = RuntimeEvent::new(request_id, 1, kind, kiana_domain::redact_value(&data))
                .map_err(|_| error("process_event_invalid"))?
                .with_stream_metadata("process", id, version + 1);
            match self.events.append_expected(event, Some(version)).await {
                Ok(()) => return Ok(()),
                Err(PortError::Conflict(_)) => continue,
                Err(error) => return Err(error),
            }
        }
        Err(error("result_unknown:process_event_unconfirmed"))
    }
    async fn authority(&self, root: &str) -> Result<Value, PortError> {
        let key = kiana_domain::json_digest(
            &json!({"project_root":Path::new(root).canonicalize().map_err(|_|error("project_root_unavailable"))?}),
        );
        let records = self.events.read_stream("authority", &key).await?;
        let data_revision = crate::data_governance::read_policy(Path::new(root))?.revision;
        records.last().map(|event|json!({"version":event.stream_version,"digest":event.data["revision_digest"],"data_revision":data_revision}))
            .ok_or_else(||error("process_authority_unavailable"))
    }
    async fn start(
        self: Arc<Self>,
        request: AuthorizedCapabilityRequest,
    ) -> Result<Value, PortError> {
        let arguments = &request.request.arguments;
        if arguments["operator_authorized"] != true {
            return Err(error("operator_required"));
        }
        let capacity = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| error("process_capacity_exceeded"))?;
        let root = PathBuf::from(required(arguments, "project_root")?)
            .canonicalize()
            .map_err(|_| error("project_root_unavailable"))?;
        let sandbox = arguments["sandbox"]
            .as_str()
            .unwrap_or("read-only")
            .to_owned();
        if !matches!(sandbox.as_str(), "read-only" | "workspace-write") {
            return Err(error("sandbox_unsupported"));
        }
        let workdir =
            crate::harness_capabilities::confined_workdir(&root, arguments.get("workdir"))?;
        let argv = crate::harness_capabilities::command_argv(arguments.get("command"))?;
        let timeout = arguments["timeout_ms"].as_u64().unwrap_or(300_000);
        if timeout == 0 || timeout > 3_600_000 {
            return Err(error("process_deadline_invalid"));
        }
        let paths: Vec<String> = arguments["path_allow"]
            .as_array()
            .ok_or_else(|| error("process_scope_required"))?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| error("process_scope_invalid"))
            })
            .collect::<Result<_, _>>()?;
        let leases = if sandbox == "workspace-write" {
            kiana_core::acquire_workspace_resources(&root.to_string_lossy(), &paths)?
        } else {
            Vec::new()
        };
        let workspace = if sandbox == "workspace-write" {
            let owned = request.clone();
            let root = root.clone();
            let workdir = workdir.clone();
            Some(
                tokio::task::spawn_blocking(move || {
                    crate::execution_workspace::ExecutionWorkspace::prepare(&owned, &root, &workdir)
                })
                .await
                .map_err(|_| error("process_workspace_join_failed"))??,
            )
        } else {
            None
        };
        let mut scope = arguments.clone();
        let (execution_root, execution_workdir) = if let Some(workspace) = &workspace {
            scope["path_allow"] = json!(["."]);
            scope["logical_project_root"] = json!(root);
            (&workspace.root, &workspace.workdir)
        } else {
            (&root, &workdir)
        };
        let mut command = crate::harness_capabilities::sandboxed_command_scoped(
            &argv,
            execution_root,
            execution_workdir,
            &sandbox,
            Some(&scope),
        )?;
        #[cfg(unix)]
        command.process_group(0);
        command.kill_on_drop(true);
        let tty = arguments["pty"] == true;
        #[cfg(unix)]
        let terminal = if tty {
            Some(open_terminal(&mut command)?)
        } else {
            None
        };
        #[cfg(not(unix))]
        if tty {
            return Err(error("pty_backend_unsupported"));
        }
        if !tty {
            command
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
        }
        let id = RequestId::new().to_string();
        let authority = self.authority(&root.to_string_lossy()).await?;
        let identity = json!({"process_id":id,"actor_id":arguments["actor_id"],"session_id":arguments["session_id"],
            "role_id":arguments["role_id"],"department_id":arguments["department_id"],"project_root":root,"authority":authority,
            "path_allow":paths,"sandbox":sandbox,"request_id":request.request.request_id,"execution_id":request.authorization_id,
            "timeout_ms":timeout,"pty":tty});
        {
            let map = self
                .processes
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if map
                .values()
                .filter(|entry| {
                    entry
                        .state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())["state"]
                        == "running"
                })
                .count()
                >= 8
            {
                return Err(error("process_capacity_exceeded"));
            }
        }
        self.fact(&id, "process.prepared", identity.clone()).await?;
        let mut child = command
            .spawn()
            .map_err(|cause| error(&format!("process_spawn_failed:{cause}")))?;
        let pid = child.id();
        let (cancelled, mut cancellation) = watch::channel(false);
        let (input, input_rx) = mpsc::channel(8);
        let entry = Arc::new(ProcessEntry {
            identity: identity.clone(),
            cancelled,
            input,
            state: Mutex::new(json!({"state":"running","stop_confirmed":false})),
            output: Mutex::new((String::new(), String::new())),
            #[cfg(unix)]
            terminal,
        });
        let (stdout, stderr, writer) = if tty {
            #[cfg(unix)]
            {
                let terminal = entry
                    .terminal
                    .as_ref()
                    .ok_or_else(|| error("pty_unavailable"))?;
                let reader = tokio::fs::File::from_std(
                    terminal
                        .try_clone()
                        .map_err(|_| error("pty_clone_failed"))?,
                );
                let writer = tokio::fs::File::from_std(
                    terminal
                        .try_clone()
                        .map_err(|_| error("pty_clone_failed"))?,
                );
                (
                    tokio::spawn(capture(reader, entry.clone(), false)),
                    None,
                    tokio::spawn(write_input(writer, input_rx)),
                )
            }
            #[cfg(not(unix))]
            {
                return Err(error("pty_backend_unsupported"));
            }
        } else {
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| error("process_stdout_unavailable"))?;
            let stderr = child
                .stderr
                .take()
                .ok_or_else(|| error("process_stderr_unavailable"))?;
            let stdin = child
                .stdin
                .take()
                .ok_or_else(|| error("process_stdin_unavailable"))?;
            (
                tokio::spawn(capture(stdout, entry.clone(), false)),
                Some(tokio::spawn(capture(stderr, entry.clone(), true))),
                tokio::spawn(write_input(stdin, input_rx)),
            )
        };
        if let Err(cause) = self.fact(&id, "process.started", identity.clone()).await {
            entry.cancelled.send_replace(true);
            let _ = crate::harness_capabilities::terminate_process_group(&mut child, pid).await;
            writer.abort();
            stdout.abort();
            if let Some(stderr) = stderr {
                stderr.abort();
            }
            return Err(error(&format!(
                "result_unknown:process_start_record_failed:{cause}"
            )));
        }
        self.processes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(id.clone(), entry.clone());
        let control = self.clone();
        let process_id = id.clone();
        let root_text = root.to_string_lossy().into_owned();
        tokio::spawn(async move {
            let _leases = leases;
            let _capacity = capacity;
            let deadline = tokio::time::sleep(Duration::from_millis(timeout));
            tokio::pin!(deadline);
            let reason;
            let status;
            loop {
                tokio::select! {
                    biased;
                    _=kiana_ports::wait_for_cancellation(&mut cancellation)=>{reason="cancelled";status=None;break;},
                    _=&mut deadline=>{reason="timed_out";status=None;break;},
                    result=child.wait()=>{reason="exited";status=result.ok();break;},
                    _=tokio::time::sleep(Duration::from_millis(100))=>{
                        if control.authority(&root_text).await.ok().as_ref()!=Some(&authority) {reason="authority_revoked";status=None;break;}
                    },
                }
            }
            let stopped =
                crate::harness_capabilities::terminate_process_group(&mut child, pid).await;
            writer.abort();
            let stdout_result = tokio::time::timeout(Duration::from_secs(2), stdout).await;
            let stderr_result = match stderr {
                Some(stderr) => Some(tokio::time::timeout(Duration::from_secs(2), stderr).await),
                None => None,
            };
            let captured = matches!(stdout_result, Ok(Ok(Ok(()))))
                && stderr_result
                    .as_ref()
                    .is_none_or(|result| matches!(result, Ok(Ok(Ok(())))));
            let successful = stopped
                && captured
                && reason == "exited"
                && status.is_some_and(|status| status.success())
                && control.authority(&root_text).await.ok().as_ref() == Some(&authority);
            let publication = match workspace {
                Some(workspace) => {
                    tokio::task::spawn_blocking(move || workspace.finish(successful))
                        .await
                        .map_err(|_| error("result_unknown:process_publication_join_failed"))
                        .and_then(|result| result)
                }
                None => Ok(json!({"host_effect":"none","published":false})),
            };
            let unknown = !stopped || publication.is_err();
            let outcome = json!({"state":if unknown {"result_unknown"}else if successful {"completed"}else if reason=="cancelled"||reason=="authority_revoked" {"cancelled"}else {"failed"},
                "reason":reason,"exit_code":status.and_then(|status|status.code()),"stop_confirmed":stopped,"capture_complete":captured,
                "workspace":publication.as_ref().ok(),"error":publication.as_ref().err().map(ToString::to_string)});
            let fact = control
                .fact(
                    &process_id,
                    "process.finished",
                    json!({"identity":identity,"outcome":outcome}),
                )
                .await;
            *entry
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = if fact.is_ok() {
                outcome
            } else {
                json!({"state":"result_unknown","stop_confirmed":stopped,"error":"process_terminal_record_failed"})
            };
            if !stopped {
                std::mem::forget(_leases);
                std::mem::forget(_capacity);
            }
        });
        Ok(
            json!({"process_id":id,"state":"running","started":true,"stop_confirmed":false,"timeout_ms":timeout,"pty":tty,"query_extends_lease":false}),
        )
    }
    async fn continuation(
        &self,
        request: &AuthorizedCapabilityRequest,
    ) -> Result<Value, PortError> {
        let arguments = &request.request.arguments;
        let id = required(arguments, "process_id")?;
        let entry = self
            .processes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(id)
            .cloned();
        let Some(entry) = entry else {
            let records = self.events.read_stream("process", id).await?;
            let identity = records
                .iter()
                .find(|event| event.kind == "process.prepared")
                .map(|event| &event.data)
                .ok_or_else(|| error("process_not_found"))?;
            check_owner(identity, arguments)?;
            if request.request.operation != "process.poll" {
                return Err(error(
                    "result_unknown:process_handle_unavailable_after_restart",
                ));
            }
            return Ok(records.iter().rev().find(|event|event.kind=="process.finished").map(|event|event.data["outcome"].clone())
                .unwrap_or_else(||json!({"state":"result_unknown","stop_confirmed":false,"automatic_retry":false})));
        };
        check_owner(&entry.identity, arguments)?;
        let operation = request.request.operation.as_str();
        if operation == "process.poll" {
            if self.authority(required(arguments, "project_root")?).await?
                != entry.identity["authority"]
            {
                return Ok(
                    json!({"process_id":id,"outcome":entry.state.lock().unwrap_or_else(|error|error.into_inner()).clone(),"output_unavailable":"authority_or_data_revision_changed"}),
                );
            }
            let output = entry
                .output
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            return Ok(
                json!({"process_id":id,"outcome":entry.state.lock().unwrap_or_else(|error|error.into_inner()).clone(),"stdout":output.0,"stderr":output.1,"preview_limit_bytes":65536}),
            );
        }
        if operation == "process.stop" {
            entry.cancelled.send_replace(true);
            for _ in 0..100 {
                let state = entry
                    .state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .clone();
                if state["state"] != "running" {
                    return Ok(state);
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            return Err(error("result_unknown:process_stop_unconfirmed"));
        }
        if self.authority(required(arguments, "project_root")?).await?
            != entry.identity["authority"]
        {
            return Err(error("process_authority_revoked"));
        }
        if entry
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())["state"]
            != "running"
        {
            return Err(error("process_not_running"));
        }
        if operation == "process.resize" {
            #[cfg(unix)]
            {
                use std::os::fd::AsRawFd;
                let rows = arguments["rows"]
                    .as_u64()
                    .filter(|value| *value > 0 && *value <= 1000)
                    .ok_or_else(|| error("pty_rows_invalid"))?;
                let cols = arguments["cols"]
                    .as_u64()
                    .filter(|value| *value > 0 && *value <= 1000)
                    .ok_or_else(|| error("pty_cols_invalid"))?;
                let terminal = entry
                    .terminal
                    .as_ref()
                    .ok_or_else(|| error("process_has_no_pty"))?;
                let size = libc::winsize {
                    ws_row: rows as u16,
                    ws_col: cols as u16,
                    ws_xpixel: 0,
                    ws_ypixel: 0,
                };
                if unsafe { libc::ioctl(terminal.as_raw_fd(), libc::TIOCSWINSZ, &size) } < 0 {
                    return Err(error("pty_resize_failed"));
                }
                return Ok(json!({"resized":true,"process_id":id}));
            }
            #[cfg(not(unix))]
            {
                return Err(error("pty_backend_unsupported"));
            }
        }
        let text = arguments["data"]
            .as_str()
            .ok_or_else(|| error("process_input_required"))?;
        if text.len() > 16 * 1024 {
            return Err(error("process_input_limit"));
        }
        let (reply, response) = oneshot::channel();
        entry
            .input
            .try_send(Input {
                text: text.to_owned(),
                reply,
            })
            .map_err(|_| error("process_input_backpressure"))?;
        let bytes = tokio::time::timeout(Duration::from_secs(5), response)
            .await
            .map_err(|_| {
                entry.cancelled.send_replace(true);
                error("result_unknown:process_input_timeout")
            })?
            .map_err(|_| error("result_unknown:process_input_unconfirmed"))?
            .map_err(|reason| error(&reason))?;
        Ok(json!({"process_id":id,"written_bytes":bytes}))
    }
}

struct ExecutionHandler(Arc<ExecutionControl>);
#[async_trait]
impl CapabilityHandler for ExecutionHandler {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        self.0.handle(self.0.clone(), request).await
    }
}
impl ExecutionControl {
    async fn handle(
        &self,
        owner: Arc<Self>,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        if request.request.operation == "process.start" {
            let request_id = request.request.request_id;
            let output = owner.start(request).await?;
            return Ok(CapabilityResult::success(request_id, output));
        }
        self.handle_read_or_continue(request).await
    }
    async fn handle_read_or_continue(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        let arguments = &request.request.arguments;
        if arguments["operator_authorized"] != true {
            return Err(error("operator_required"));
        }
        let output = match request.request.operation.as_str() {
            "workspace.transaction" => {
                let root = PathBuf::from(required(arguments, "project_root")?);
                let arguments = arguments.clone();
                tokio::task::spawn_blocking(move || {
                    crate::apply_patch::workspace_transaction(&root, &arguments)
                })
                .await
                .map_err(|_| error("result_unknown:workspace_transaction_join_failed"))??
            }
            "local.package" => {
                let arguments = arguments.clone();
                tokio::task::spawn_blocking(move || local_package(&arguments))
                    .await
                    .map_err(|_| error("result_unknown:local_package_join_failed"))??
            }
            "execution.output.read" => read_output(arguments)?,
            "environment.inspect" => {
                json!({"schema":"kiana.environment-support.v1","host":std::env::consts::OS,
                "backend":crate::harness_sandbox::SANDBOX_BACKEND,"workspace_mode":"isolated_staged",
                "network":"deny_all","file_identity":"descriptor_pinned","limits":{"output_bytes":1048576,"workspace_bytes":536870912,"processes":8},
                "behavior_verified":false})
            }
            "tool.search" => {
                let query = arguments["query"]
                    .as_str()
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if query.len() > 512 {
                    return Err(error("tool_query_limit"));
                }
                let role = kiana_domain::RoleSpec::lookup(required(arguments, "role_id")?)
                    .ok_or_else(|| error("role_unknown"))?;
                let tools = kiana_domain::tool_schemas()
                    .into_iter()
                    .filter(|tool| {
                        let name = tool["name"].as_str().unwrap_or_default();
                        role.tools.iter().any(|allowed| allowed == name)
                            && (query.is_empty()
                                || name.contains(&query)
                                || tool["description"]
                                    .as_str()
                                    .unwrap_or_default()
                                    .to_ascii_lowercase()
                                    .contains(&query))
                    })
                    .collect::<Vec<_>>();
                json!({"tools":tools,"catalog_digest":kiana_domain::capability_action_catalog_digest(),"does_not_grant_execution":true})
            }
            _ => self.continuation(&request).await?,
        };
        Ok(CapabilityResult::success(
            request.request.request_id,
            output,
        ))
    }
}

#[cfg(unix)]
fn open_terminal(command: &mut tokio::process::Command) -> Result<std::fs::File, PortError> {
    use std::os::fd::FromRawFd;
    let (mut master, mut slave) = (-1, -1);
    if unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    } != 0
    {
        return Err(error("pty_unavailable"));
    }
    let master = unsafe { std::fs::File::from_raw_fd(master) };
    let slave = unsafe { std::fs::File::from_raw_fd(slave) };
    command
        .stdin(slave.try_clone().map_err(|_| error("pty_clone_failed"))?)
        .stdout(slave.try_clone().map_err(|_| error("pty_clone_failed"))?)
        .stderr(slave);
    Ok(master)
}
async fn write_input<W: tokio::io::AsyncWrite + Unpin>(
    mut writer: W,
    mut input: mpsc::Receiver<Input>,
) {
    while let Some(input) = input.recv().await {
        let result = writer
            .write_all(input.text.as_bytes())
            .await
            .map(|()| input.text.len())
            .map_err(|error| format!("result_unknown:process_input:{error}"));
        let failed = result.is_err();
        let _ = input.reply.send(result);
        if failed {
            break;
        }
    }
}
async fn capture<R: tokio::io::AsyncRead + Unpin>(
    mut reader: R,
    entry: Arc<ProcessEntry>,
    stderr: bool,
) -> Result<(), PortError> {
    let mut redactor = kiana_domain::StreamingRedactor::new();
    let mut buffer = [0u8; 8192];
    let mut total = 0usize;
    loop {
        let count = reader
            .read(&mut buffer)
            .await
            .map_err(|_| error("process_output_read_failed"))?;
        if count == 0 {
            break;
        }
        total = total.saturating_add(count);
        if total > 32 * 1024 * 1024 {
            entry.cancelled.send_replace(true);
            return Err(error("process_output_quota_exceeded"));
        }
        let text = redactor.push(&String::from_utf8_lossy(&buffer[..count]));
        let text = crate::harness_capabilities::safe_output_text(&text);
        let mut output = entry
            .output
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let target = if stderr { &mut output.1 } else { &mut output.0 };
        target.push_str(&text);
        if target.len() > 65536 {
            let mut offset = target.len() - 65536;
            while !target.is_char_boundary(offset) {
                offset += 1;
            }
            target.drain(..offset);
        }
    }
    let tail = redactor.finish();
    let mut output = entry
        .output
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if stderr {
        output.1.push_str(&tail);
    } else {
        output.0.push_str(&tail);
    }
    Ok(())
}

fn output_directory() -> Result<PathBuf, PortError> {
    let root = std::env::var_os("KIANA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|value| PathBuf::from(value).join(".kiana")))
        .ok_or_else(|| error("output_store_unavailable"))?;
    if !root.is_absolute() {
        return Err(error("output_store_not_absolute"));
    }
    Ok(root.join("execution-output"))
}
pub(crate) fn store_output(
    request: &AuthorizedCapabilityRequest,
    output: &mut Value,
) -> Result<(), PortError> {
    let mut captured = serde_json::Map::new();
    for name in ["stdout", "stderr"] {
        if let Some(text) = output[name].as_str() {
            let safe = crate::harness_capabilities::safe_output_text(text);
            captured.insert(name.to_owned(), json!(safe));
            let mut end = safe.len().min(8192);
            while !safe.is_char_boundary(end) {
                end -= 1;
            }
            output[name] = json!(&safe[..end]);
        }
    }
    if captured.is_empty() {
        return Ok(());
    }
    let id = request.request.request_id.to_string();
    let root = required(&request.request.arguments, "project_root")?;
    let current = crate::data_governance::read_policy(Path::new(root))?.revision;
    let epoch = request.request.arguments["source_data_revision"]
        .as_u64()
        .unwrap_or(current);
    let record = json!({"schema":"kiana.execution-output.v1","identity":identity(&request.request.arguments),"data_epoch":epoch,"output":captured});
    let bytes = serde_json::to_vec(&record).map_err(|_| error("output_encode_failed"))?;
    crate::local_packages::LocalDir::open(&output_directory()?, true)?
        .publish(&format!("{id}.json"), &bytes)?;
    output["output_ref"] = json!({"output_id":id,"schema":"kiana.execution-output.v1","sha256":crate::local_packages::sha256(&bytes),"preview_bytes":8192});
    Ok(())
}
fn read_output(arguments: &Value) -> Result<Value, PortError> {
    let id = serde_json::from_value::<RequestId>(arguments["output_id"].clone())
        .map_err(|_| error("output_id_invalid"))?;
    let bytes = crate::local_packages::LocalDir::open(&output_directory()?, false)?
        .read(&format!("{id}.json"), 8 * 1024 * 1024)?;
    let record: Value =
        serde_json::from_slice(&bytes).map_err(|_| error("output_record_invalid"))?;
    check_owner(&record["identity"], arguments)?;
    if record["data_epoch"]
        != json!(
            crate::data_governance::read_policy(Path::new(required(arguments, "project_root")?))?
                .revision
        )
    {
        return Err(error("output_data_revoked"));
    }
    let stream = arguments["stream"].as_str().unwrap_or("stdout");
    if !matches!(stream, "stdout" | "stderr") {
        return Err(error("output_stream_invalid"));
    }
    let text = record["output"][stream].as_str().unwrap_or_default();
    let start = arguments["offset"].as_u64().unwrap_or(0) as usize;
    if start > text.len() || !text.is_char_boundary(start) {
        return Err(error("output_offset_invalid"));
    }
    let mut end = start
        .saturating_add(arguments["limit"].as_u64().unwrap_or(8192).min(65536) as usize)
        .min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Ok(
        json!({"output_id":id,"stream":stream,"text":&text[start..end],"next_offset":end,"complete":end==text.len()}),
    )
}
fn local_package(arguments: &Value) -> Result<Value, PortError> {
    let root = PathBuf::from(required(arguments, "project_root")?)
        .canonicalize()
        .map_err(|_| error("package_root_unavailable"))?;
    let package_id = serde_json::from_value::<RequestId>(arguments["package_id"].clone())
        .map_err(|_| error("package_id_invalid"))?;
    let destination = kiana_domain::normalize_role_path(required(arguments, "destination")?)
        .ok_or_else(|| error("package_destination_invalid"))?;
    if destination == "."
        || destination
            .split('/')
            .any(crate::harness_sandbox::private_component)
        || destination.starts_with(".git/")
    {
        return Err(error("package_destination_denied"));
    }
    let paths = arguments["path_allow"]
        .as_array()
        .ok_or_else(|| error("package_write_scope_required"))?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if !kiana_domain::allow_list_covers(&paths, &destination) {
        return Err(error("package_path_denied"));
    }
    if !arguments["manifest"].is_object() {
        return Err(error("package_manifest_required"));
    }
    let sources = arguments["sources"]
        .as_array()
        .ok_or_else(|| error("package_sources_required"))?;
    if sources.len() > 128 {
        return Err(error("package_source_limit"));
    }
    let directory = crate::local_packages::LocalDir::open(&root, false)?;
    let policy = crate::data_governance::read_policy(&root)?;
    let mut captured = Vec::new();
    let mut total = 0usize;
    for source in sources {
        let path = kiana_domain::normalize_role_path(required(source, "path")?)
            .ok_or_else(|| error("package_source_invalid"))?;
        if path
            .split('/')
            .any(crate::harness_sandbox::private_component)
            || path == ".git"
            || path.starts_with(".git/")
            || policy
                .revoked_sources
                .iter()
                .any(|source| path == *source || path.starts_with(&format!("{source}/")))
        {
            return Err(error("package_source_denied"));
        }
        let bytes = directory.read(&path, 2 * 1024 * 1024)?;
        total = total.saturating_add(bytes.len());
        if total > 2 * 1024 * 1024 {
            return Err(error("package_size_limit"));
        }
        let sha256 = crate::local_packages::sha256(&bytes);
        if source["sha256"] != sha256 {
            return Err(error("package_source_changed"));
        }
        let text = String::from_utf8(bytes).map_err(|_| error("package_source_utf8_required"))?;
        captured.push(json!({"path":path,"sha256":sha256,"content_utf8":text}));
    }
    let package = json!({"schema":"kiana.local-package.v1","package_id":package_id,"manifest":arguments["manifest"],"sources":captured});
    let bytes = serde_json::to_vec_pretty(&package).map_err(|_| error("package_encode_failed"))?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(error("package_size_limit"));
    }
    let digest = crate::local_packages::sha256(&bytes);
    let target = root.join(&destination);
    if target.exists() {
        if directory.read(&destination, 2 * 1024 * 1024)? != bytes {
            return Err(error("package_destination_conflict"));
        }
        return Ok(
            json!({"package_id":package_id,"path":destination,"sha256":digest,"bytes":bytes.len(),"reused":true}),
        );
    }
    let change = crate::execution_workspace::PublishedFile {
        path: destination.clone(),
        before: None,
        after: Some(bytes.clone()),
        executable: false,
    };
    let result = crate::apply_patch::publish_workspace_files(&root, &[change], package_id)?;
    Ok(
        json!({"package_id":package_id,"path":destination,"sha256":digest,"bytes":bytes.len(),"receipt":result}),
    )
}
fn identity(arguments: &Value) -> Value {
    json!({"actor_id":arguments["actor_id"],"session_id":arguments["session_id"],"role_id":arguments["role_id"],"department_id":arguments["department_id"],"project_root":arguments["project_root"]})
}
fn check_owner(stored: &Value, arguments: &Value) -> Result<(), PortError> {
    for name in [
        "actor_id",
        "session_id",
        "role_id",
        "department_id",
        "project_root",
    ] {
        if stored[name].is_null() || stored[name] != arguments[name] {
            return Err(error("execution_owner_mismatch"));
        }
    }
    Ok(())
}
fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str, PortError> {
    value[key]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error(&format!("execution_argument_required:{key}")))
}
fn error(reason: &str) -> PortError {
    PortError::Failed(reason.to_owned())
}
