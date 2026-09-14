//! Per-invocation stdio MCP transport. All child ownership stays with the invocation.
use kiana_ports::PortError;
use kiana_services::mcp::McpServerConfig;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::watch;
use tokio::time::Instant;

const FRAME_BYTES: usize = 512 * 1024;
const TOTAL_BYTES: usize = 4 * 1024 * 1024;
const FRAME_COUNT: usize = 2048;
const CALL_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) struct ConfinedMcpClient {
    child: Child,
    process_group: Option<u32>,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr: Option<tokio::task::JoinHandle<(Vec<u8>, bool)>>,
    cancellation: watch::Receiver<bool>,
    deadline: Instant,
    next_id: u64,
    read_bytes: usize,
    frames: usize,
    catalog_changed: bool,
    pub call_started: bool,
    pub server_info: Value,
}

impl ConfinedMcpClient {
    pub async fn connect(
        config: &McpServerConfig,
        scope: &Value,
        cancellation: watch::Receiver<bool>,
    ) -> Result<Self, PortError> {
        if scope["project_trusted"] != true {
            return Err(failed("mcp_project_untrusted"));
        }
        if *cancellation.borrow() {
            return Err(failed("cancelled:mcp_not_started"));
        }
        let root = Path::new(
            scope["project_root"]
                .as_str()
                .ok_or_else(|| failed("mcp_project_required"))?,
        );
        let sandbox = scope["sandbox"].as_str().unwrap_or("read-only");
        let paths = scope["path_allow"]
            .as_array()
            .ok_or_else(|| failed("mcp_path_scope_required"))?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| failed("mcp_path_scope_invalid"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut plan = crate::harness_sandbox::bwrap_plan_scoped(root, root, sandbox, &paths)?;
        if let Some(logical) = scope["logical_project_root"].as_str() {
            plan.remap_workspace(root, Path::new(logical));
        }
        let program = config
            .command
            .as_deref()
            .ok_or_else(|| failed("mcp_command_required"))?;
        // The process uses sealed bytes, not a path reopened after the approval hash check.
        let pin = scope["mcp_snapshot"]["executable"].clone();
        let argument_files = scope["mcp_snapshot"]["argument_files"]
            .as_array()
            .cloned()
            .ok_or_else(|| failed("mcp_snapshot_required"))?;
        let program_owned = program.to_owned();
        let mounts = tokio::task::spawn_blocking(move || {
            let executable = sealed_executable(&program_owned, &pin)?;
            let mut mounts = vec![(executable, "/tmp/kiana-mcp-executable".to_owned())];
            for pin in argument_files {
                let path = pin["path"]
                    .as_str()
                    .ok_or_else(|| failed("mcp_config_file_invalid"))?;
                mounts.push((sealed_executable(path, &pin)?, path.to_owned()));
            }
            Ok::<_, PortError>(mounts)
        })
        .await
        .map_err(|_| failed("mcp_executable_prepare_failed"))??;
        let mut extra = Vec::new();
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            for (file, path) in &mounts {
                extra.extend([
                    std::ffi::OsString::from("--ro-bind-fd"),
                    file.as_raw_fd().to_string().into(),
                    path.into(),
                ]);
            }
        }
        for (key, value) in config.env.as_ref().into_iter().flat_map(|env| env.iter()) {
            if key != "LANG" && key != "LC_ALL" && key != "LC_CTYPE" && key != "TZ" {
                return Err(failed("mcp_config_env_not_supported"));
            }
            extra.extend(["--setenv".into(), key.into(), value.into()]);
        }
        let boundary = plan
            .args
            .iter()
            .rposition(|argument| argument == "--")
            .ok_or_else(|| failed("mcp_sandbox_plan_invalid"))?;
        plan.args.splice(boundary..boundary, extra);
        let mut command = Command::new(&plan.program);
        plan.configure_command(&mut command);
        command
            .arg("/tmp/kiana-mcp-executable")
            .args(config.args.as_deref().unwrap_or_default())
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            command.process_group(0);
            unsafe {
                command.pre_exec(move || {
                    for (file, _) in &mounts {
                        if libc::fcntl(file.as_raw_fd(), libc::F_SETFD, 0) < 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                    }
                    Ok(())
                });
            }
        }
        let mut child = command.spawn().map_err(|_| failed("mcp_spawn_failed"))?;
        let process_group = child.id();
        let (Some(stdin), Some(stdout), Some(mut stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            let stopped =
                crate::harness_capabilities::terminate_process_group(&mut child, process_group)
                    .await;
            return Err(failed(if stopped {
                "mcp_pipe_unavailable"
            } else {
                "result_unknown:mcp_stop_unconfirmed"
            }));
        };
        let stderr = tokio::spawn(async move {
            let mut bytes = Vec::new();
            let mut truncated = false;
            let mut buf = [0u8; 4096];
            loop {
                match stderr.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        let keep = count.min(16_384usize.saturating_sub(bytes.len()));
                        bytes.extend_from_slice(&buf[..keep]);
                        truncated |= keep < count;
                    }
                }
            }
            (bytes, truncated)
        });
        let timeout = scope["timeout_ms"]
            .as_u64()
            .unwrap_or(60_000)
            .clamp(1, 60_000);
        let mut client = Self {
            child,
            process_group,
            stdin,
            stdout: BufReader::new(stdout),
            stderr: Some(stderr),
            cancellation,
            deadline: Instant::now() + Duration::from_millis(timeout),
            next_id: 1,
            read_bytes: 0,
            frames: 0,
            catalog_changed: false,
            call_started: false,
            server_info: Value::Null,
        };
        let initialized = async {
            let result = client.request("initialize",json!({"protocolVersion":"2025-06-18","capabilities":{},
                "clientInfo":{"name":"kiana","version":env!("CARGO_PKG_VERSION")}})).await?;
            let version = result["protocolVersion"].as_str().ok_or_else(||failed("mcp_protocol_version_required"))?;
            if !matches!(version,"2024-11-05"|"2025-03-26"|"2025-06-18") { return Err(failed("mcp_protocol_version_unsupported")); }
            if !result["serverInfo"].is_object() || !result["capabilities"].is_object() { return Err(failed("mcp_initialize_invalid")); }
            client.server_info = json!({"protocol_version":version,"implementation":result["serverInfo"],"capabilities":result["capabilities"]});
            client.write(json!({"jsonrpc":"2.0","method":"notifications/initialized"})).await
        }.await;
        if let Err(error) = initialized {
            client.stop().await?;
            return Err(error);
        }
        Ok(client)
    }

    async fn write(&mut self, value: Value) -> Result<(), PortError> {
        kiana_domain::validate_json_limits(&value).map_err(|_| failed("mcp_request_limit"))?;
        let mut bytes = serde_json::to_vec(&value).map_err(|_| failed("mcp_request_invalid"))?;
        bytes.push(b'\n');
        let deadline = self.deadline.min(Instant::now() + CALL_TIMEOUT);
        let cancellation = &mut self.cancellation;
        let stdin = &mut self.stdin;
        tokio::select! { biased;
            _ = kiana_ports::wait_for_cancellation(cancellation) => Err(failed("cancelled:mcp_write_stopped")),
            result = tokio::time::timeout_at(deadline, async { stdin.write_all(&bytes).await?; stdin.flush().await }) =>
                result.map_err(|_|failed("mcp_write_timeout"))?.map_err(|_|failed("mcp_write_failed")),
        }
    }

    async fn read_frame(&mut self) -> Result<Value, PortError> {
        let mut bytes = Vec::new();
        loop {
            let buf = self
                .stdout
                .fill_buf()
                .await
                .map_err(|_| failed("mcp_read_failed"))?;
            if buf.is_empty() {
                return Err(failed("mcp_stream_closed"));
            }
            let count = buf
                .iter()
                .position(|byte| *byte == b'\n')
                .map(|index| index + 1)
                .unwrap_or(buf.len());
            let ends = buf[count - 1] == b'\n';
            self.read_bytes = self.read_bytes.saturating_add(count);
            if bytes.len() + count > FRAME_BYTES || self.read_bytes > TOTAL_BYTES {
                return Err(failed("mcp_response_byte_limit"));
            }
            bytes.extend_from_slice(&buf[..count]);
            self.stdout.consume(count);
            if ends {
                break;
            }
        }
        self.frames += 1;
        if self.frames > FRAME_COUNT {
            return Err(failed("mcp_response_frame_limit"));
        }
        kiana_domain::parse_bounded_json(&bytes).map_err(|_| failed("mcp_response_invalid"))
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, PortError> {
        if *self.cancellation.borrow() {
            return Err(failed("cancelled:mcp_not_sent"));
        }
        if self.catalog_changed {
            return Err(failed("mcp_catalog_changed"));
        }
        let id = self.next_id;
        self.next_id += 1;
        // A partial write may already have reached the server; callers classify it as unknown.
        if method == "tools/call" {
            self.call_started = true;
        }
        let deadline = self.deadline.min(Instant::now() + CALL_TIMEOUT);
        let mut cancellation = self.cancellation.clone();
        tokio::select! { biased;
            _ = kiana_ports::wait_for_cancellation(&mut cancellation) => Err(failed("cancelled:mcp_request_stopped")),
            result = tokio::time::timeout_at(deadline, async {
                self.write(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})).await?;
                for _ in 0..128 {
                    let message = self.read_frame().await?;
                    if !message.is_object() || message["jsonrpc"] != "2.0" { return Err(failed("mcp_response_version_invalid")); }
                    if let Some(method) = message.get("method") {
                        let method = method.as_str().filter(|m| !m.is_empty()).ok_or_else(||failed("mcp_server_message_invalid"))?;
                        if message.get("result").is_some() || message.get("error").is_some() { return Err(failed("mcp_server_message_invalid")); }
                        if let Some(request_id) = message.get("id") {
                            if !request_id.is_string() && request_id.as_i64().is_none() { return Err(failed("mcp_server_request_id_invalid")); }
                            self.write(json!({"jsonrpc":"2.0","id":request_id,"error":{"code":-32601,"message":"Server initiated requests are disabled"}})).await?;
                        } else if method == "notifications/tools/list_changed" {
                            self.catalog_changed = true;
                            return Err(failed("mcp_catalog_changed"));
                        } else if !method.starts_with("notifications/") { return Err(failed("mcp_notification_invalid")); }
                        continue;
                    }
                    if message["id"] != id { return Err(failed("mcp_response_id_mismatch")); }
                    if message.get("result").is_some() == message.get("error").is_some() { return Err(failed("mcp_response_envelope_invalid")); }
                    if let Some(error) = message.get("error") {
                        if error["code"].as_i64().is_none() || !error["message"].is_string() { return Err(failed("mcp_error_invalid")); }
                        return Err(failed("mcp_provider_error"));
                    }
                    return Ok(message["result"].clone());
                }
                Err(failed("mcp_notification_budget_exceeded"))
            }) => result.map_err(|_|failed("mcp_response_timeout"))?,
        }
    }

    pub async fn list_tools(&mut self) -> Result<Vec<Value>, PortError> {
        let mut tools = Vec::new();
        let mut names = HashSet::new();
        let mut cursors = HashSet::new();
        let mut cursor = None;
        for _ in 0..32 {
            let params = cursor
                .as_ref()
                .map(|cursor| json!({"cursor":cursor}))
                .unwrap_or_else(|| json!({}));
            let result = self.request("tools/list", params).await?;
            let page = result["tools"]
                .as_array()
                .ok_or_else(|| failed("mcp_tools_invalid"))?;
            if tools.len() + page.len() > 1024 {
                return Err(failed("mcp_tool_catalog_limit"));
            }
            for tool in page {
                let name = tool["name"]
                    .as_str()
                    .filter(|name| !name.is_empty() && name.len() <= 256)
                    .ok_or_else(|| failed("mcp_tool_name_invalid"))?;
                if !names.insert(name.to_owned()) {
                    return Err(failed("mcp_tool_ambiguous"));
                }
                tools.push(tool.clone());
            }
            if result.get("nextCursor").is_none_or(Value::is_null) {
                tools.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
                kiana_domain::validate_json_limits(&json!(tools))
                    .map_err(|_| failed("mcp_tool_catalog_limit"))?;
                return Ok(tools);
            }
            let next = result["nextCursor"]
                .as_str()
                .filter(|cursor| !cursor.is_empty() && cursor.len() <= 4096)
                .ok_or_else(|| failed("mcp_cursor_invalid"))?;
            if !cursors.insert(next.to_owned()) {
                return Err(failed("mcp_cursor_cycle"));
            }
            cursor = Some(next.to_owned());
        }
        Err(failed("mcp_tool_catalog_page_limit"))
    }

    pub async fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, PortError> {
        self.request("tools/call", json!({"name":name,"arguments":arguments}))
            .await
    }

    pub async fn stop(&mut self) -> Result<Value, PortError> {
        if !crate::harness_capabilities::terminate_process_group(
            &mut self.child,
            self.process_group,
        )
        .await
        {
            return Err(failed("result_unknown:mcp_stop_unconfirmed"));
        }
        self.process_group = None;
        let (bytes, truncated) = if let Some(mut task) = self.stderr.take() {
            match tokio::time::timeout(Duration::from_secs(1), &mut task).await {
                Ok(Ok(value)) => value,
                _ => {
                    task.abort();
                    let _ = task.await;
                    (Vec::new(), true)
                }
            }
        } else {
            (Vec::new(), false)
        };
        // Diagnostics are evidence, never untrusted instructions or raw secret-bearing stderr.
        Ok(
            json!({"stop_confirmed":true,"stderr_bytes_retained":bytes.len(),"stderr_truncated":truncated,
            "stderr_digest":kiana_domain::journal_sha256(&bytes)}),
        )
    }
}
impl Drop for ConfinedMcpClient {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(group) = self.process_group {
            unsafe {
                libc::kill(-(group as i32), libc::SIGKILL);
            }
        }
        if let Some(stderr) = self.stderr.take() {
            stderr.abort();
        }
    }
}

#[cfg(unix)]
fn sealed_executable(program: &str, pin: &Value) -> Result<Arc<std::fs::File>, PortError> {
    use std::io::{Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = std::fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = options
        .open(program)
        .map_err(|_| failed("mcp_executable_unavailable"))?;
    let meta = file
        .metadata()
        .map_err(|_| failed("mcp_executable_unavailable"))?;
    if !meta.is_file() || meta.len() > 128 * 1024 * 1024 {
        return Err(failed("mcp_executable_invalid"));
    }
    let mut bytes = Vec::new();
    file.take(128 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failed("mcp_executable_unavailable"))?;
    if pin["sha256"] != kiana_domain::journal_sha256(&bytes)
        || pin["bytes"] != bytes.len() as u64
        || pin["path"] != program
    {
        return Err(failed("mcp_executable_changed"));
    }
    let name = std::ffi::CString::new("kiana-mcp-executable").expect("static name");
    let fd =
        unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) };
    if fd < 0 {
        return Err(failed("mcp_executable_seal_unavailable"));
    }
    let mut sealed = unsafe { std::fs::File::from_raw_fd(fd) };
    sealed
        .write_all(&bytes)
        .map_err(|_| failed("mcp_executable_seal_failed"))?;
    if unsafe { libc::fchmod(fd, 0o500) } != 0
        || unsafe {
            libc::fcntl(
                sealed.as_raw_fd(),
                libc::F_ADD_SEALS,
                libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL,
            )
        } < 0
    {
        return Err(failed("mcp_executable_seal_failed"));
    }
    Ok(Arc::new(sealed))
}
#[cfg(not(unix))]
fn sealed_executable(_: &str, _: &Value) -> Result<Arc<std::fs::File>, PortError> {
    Err(failed("mcp_containment_unavailable"))
}
fn failed(message: impl Into<String>) -> PortError {
    PortError::Failed(message.into())
}
