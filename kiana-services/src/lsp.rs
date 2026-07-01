use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::{timeout, Duration};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspServerConfig {
    pub command: String,
    pub args: Vec<String>,
    pub root_path: String,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default = "default_initialization_options")]
    pub initialization_options: Value,
    #[serde(default)]
    pub settings: Option<Value>,
    #[serde(default)]
    pub startup_timeout_ms: Option<u64>,
    #[serde(default)]
    pub shutdown_timeout_ms: Option<u64>,
    #[serde(default)]
    pub restart_on_crash: bool,
    #[serde(default)]
    pub max_restarts: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub severity: DiagnosticSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticFile {
    pub file: String,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

pub struct LspClient {
    config: LspServerConfig,
    diagnostics: HashMap<String, Vec<Diagnostic>>,
    diagnostic_updates: Vec<DiagnosticFile>,
    transport: Option<StdioLspTransport>,
    initialize_result: Option<Value>,
    initialized: bool,
    restart_attempts: u64,
}

impl LspClient {
    pub fn new(config: LspServerConfig) -> Self {
        Self {
            config,
            diagnostics: HashMap::new(),
            diagnostic_updates: Vec::new(),
            transport: None,
            initialize_result: None,
            initialized: false,
            restart_attempts: 0,
        }
    }

    pub async fn start(&mut self) -> crate::errors::ServiceResult<()> {
        if self.initialized {
            return Ok(());
        }
        if self.config.command.trim().is_empty() {
            return Err(crate::errors::ServiceError::Connection(
                "LSP server command cannot be empty".to_string(),
            ));
        }
        if self.config.root_path.trim().is_empty() {
            return Err(crate::errors::ServiceError::Connection(
                "LSP server root_path cannot be empty".to_string(),
            ));
        }

        let mut transport = StdioLspTransport::spawn(
            &self.config.command,
            self.config.args.clone(),
            &self.config.root_path,
            &self.config.env,
        )
        .await?;
        let initialize_result = transport
            .request(
                "initialize",
                initialize_params(&self.config)?,
                self.config.startup_timeout(),
                &mut self.diagnostics,
                &mut self.diagnostic_updates,
            )
            .await?;
        transport.notify("initialized", json!({})).await?;
        if let Some(settings) = self
            .config
            .settings
            .as_ref()
            .filter(|value| !value.is_null())
        {
            transport
                .notify(
                    "workspace/didChangeConfiguration",
                    json!({ "settings": settings }),
                )
                .await?;
        }

        self.initialize_result = Some(initialize_result);
        self.initialized = true;
        self.transport = Some(transport);
        Ok(())
    }

    pub async fn shutdown(&mut self) -> crate::errors::ServiceResult<()> {
        let Some(mut transport) = self.transport.take() else {
            self.initialized = false;
            return Ok(());
        };

        let mut shutdown_error = None;
        if self.initialized {
            if let Err(error) = transport
                .request(
                    "shutdown",
                    Value::Null,
                    self.config.shutdown_timeout(),
                    &mut self.diagnostics,
                    &mut self.diagnostic_updates,
                )
                .await
            {
                shutdown_error = Some(error);
            }
            let _ = transport.notify("exit", Value::Null).await;
        }

        self.initialized = false;
        self.initialize_result = None;
        drop(transport);

        match shutdown_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    pub fn initialize_result(&self) -> Option<&Value> {
        self.initialize_result.as_ref()
    }

    pub async fn send_request(
        &mut self,
        method: &str,
        params: Value,
    ) -> crate::errors::ServiceResult<Value> {
        let retry_params = params.clone();
        match self.send_request_once(method, params).await {
            Ok(result) => Ok(result),
            Err(error) => {
                if self.try_restart_after_transport_error(&error).await? {
                    self.send_request_once(method, retry_params).await
                } else {
                    Err(error)
                }
            }
        }
    }

    async fn send_request_once(
        &mut self,
        method: &str,
        params: Value,
    ) -> crate::errors::ServiceResult<Value> {
        if !self.initialized {
            return Err(crate::errors::ServiceError::Connection(
                "LSP server not initialized".to_string(),
            ));
        }
        let Some(transport) = self.transport.as_mut() else {
            return Err(crate::errors::ServiceError::Connection(
                "LSP transport is not available".to_string(),
            ));
        };
        transport
            .request(
                method,
                params,
                Duration::from_secs(10),
                &mut self.diagnostics,
                &mut self.diagnostic_updates,
            )
            .await
    }

    pub async fn send_notification(
        &mut self,
        method: &str,
        params: Value,
    ) -> crate::errors::ServiceResult<()> {
        let retry_params = params.clone();
        match self.send_notification_once(method, params).await {
            Ok(()) => Ok(()),
            Err(error) => {
                if self.try_restart_after_transport_error(&error).await? {
                    self.send_notification_once(method, retry_params).await
                } else {
                    Err(error)
                }
            }
        }
    }

    async fn send_notification_once(
        &mut self,
        method: &str,
        params: Value,
    ) -> crate::errors::ServiceResult<()> {
        if !self.initialized {
            return Err(crate::errors::ServiceError::Connection(
                "LSP server not initialized".to_string(),
            ));
        }
        let Some(transport) = self.transport.as_mut() else {
            return Err(crate::errors::ServiceError::Connection(
                "LSP transport is not available".to_string(),
            ));
        };
        transport.notify(method, params).await
    }

    async fn try_restart_after_transport_error(
        &mut self,
        error: &crate::errors::ServiceError,
    ) -> crate::errors::ServiceResult<bool> {
        if !self.config.restart_on_crash || !is_restartable_lsp_transport_error(error) {
            return Ok(false);
        }

        let max_restarts = self.config.max_restarts();
        if self.restart_attempts >= max_restarts {
            self.transport.take();
            self.initialized = false;
            self.initialize_result = None;
            return Err(crate::errors::ServiceError::Connection(format!(
                "LSP server exceeded max restart attempts ({}) after transport error: {}",
                max_restarts, error
            )));
        }

        self.restart_attempts += 1;
        self.transport.take();
        self.initialized = false;
        self.initialize_result = None;
        self.start().await.map_err(|restart_error| {
            crate::errors::ServiceError::Connection(format!(
                "failed to restart LSP server after transport error (attempt {}/{}): {}; restart error: {}",
                self.restart_attempts, max_restarts, error, restart_error
            ))
        })?;
        Ok(true)
    }

    pub async fn drain_notifications(
        &mut self,
        max_wait: Duration,
    ) -> crate::errors::ServiceResult<()> {
        if !self.initialized {
            return Ok(());
        }
        let Some(transport) = self.transport.as_mut() else {
            return Ok(());
        };
        loop {
            let message = match timeout(max_wait, transport.read_message()).await {
                Ok(message) => message?,
                Err(_) => return Ok(()),
            };
            transport
                .handle_peer_message(message, &mut self.diagnostics, &mut self.diagnostic_updates)
                .await?;
        }
    }

    pub fn get_diagnostics(&self, file: &str) -> Vec<&Diagnostic> {
        self.diagnostics
            .get(file)
            .map(|d| d.iter().collect())
            .unwrap_or_default()
    }

    pub fn take_diagnostic_updates(&mut self) -> Vec<DiagnosticFile> {
        std::mem::take(&mut self.diagnostic_updates)
    }
}

impl LspServerConfig {
    fn startup_timeout(&self) -> Duration {
        Duration::from_millis(self.startup_timeout_ms.unwrap_or(10_000).max(1))
    }

    fn shutdown_timeout(&self) -> Duration {
        Duration::from_millis(self.shutdown_timeout_ms.unwrap_or(10_000).max(1))
    }

    fn max_restarts(&self) -> u64 {
        self.max_restarts.unwrap_or(3)
    }
}

fn is_restartable_lsp_transport_error(error: &crate::errors::ServiceError) -> bool {
    match error {
        crate::errors::ServiceError::Io(_) | crate::errors::ServiceError::Serialization(_) => true,
        crate::errors::ServiceError::Connection(message) => {
            message.contains("closed stdout")
                || message.contains("transport is not available")
                || message.contains("not initialized")
                || message.contains("missing Content-Length header")
        }
        _ => false,
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        self.transport.take();
        self.initialized = false;
    }
}

struct StdioLspTransport {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl StdioLspTransport {
    async fn spawn(
        command: &str,
        args: Vec<String>,
        root_path: &str,
        env: &HashMap<String, String>,
    ) -> crate::errors::ServiceResult<Self> {
        let mut command_builder = Command::new(command);
        command_builder
            .args(args)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let root = Path::new(root_path);
        if root.is_dir() {
            command_builder.current_dir(root);
        }

        let mut child = command_builder.spawn().map_err(|error| {
            crate::errors::ServiceError::Connection(format!(
                "failed to spawn LSP stdio server: {}",
                error
            ))
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            crate::errors::ServiceError::Connection(
                "LSP stdio server stdin was not available".to_string(),
            )
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            crate::errors::ServiceError::Connection(
                "LSP stdio server stdout was not available".to_string(),
            )
        })?;

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        })
    }

    async fn request(
        &mut self,
        method: &str,
        params: Value,
        request_timeout: Duration,
        diagnostics: &mut HashMap<String, Vec<Diagnostic>>,
        diagnostic_updates: &mut Vec<DiagnosticFile>,
    ) -> crate::errors::ServiceResult<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }))
        .await?;

        timeout(
            request_timeout,
            self.read_response(id, diagnostics, diagnostic_updates),
        )
        .await
        .map_err(|_| {
            crate::errors::ServiceError::Connection(format!("LSP request '{}' timed out", method))
        })?
    }

    async fn notify(&mut self, method: &str, params: Value) -> crate::errors::ServiceResult<()> {
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        }))
        .await
    }

    async fn write_message(&mut self, message: &Value) -> crate::errors::ServiceResult<()> {
        let body = serde_json::to_vec(message)?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.stdin.write_all(header.as_bytes()).await?;
        self.stdin.write_all(&body).await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn read_response(
        &mut self,
        id: u64,
        diagnostics: &mut HashMap<String, Vec<Diagnostic>>,
        diagnostic_updates: &mut Vec<DiagnosticFile>,
    ) -> crate::errors::ServiceResult<Value> {
        loop {
            let message = self.read_message().await?;
            if message.get("method").and_then(Value::as_str).is_some() {
                self.handle_peer_message(message, diagnostics, diagnostic_updates)
                    .await?;
                continue;
            }
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(crate::errors::ServiceError::Connection(format!(
                    "LSP request failed: {}",
                    error
                )));
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    async fn handle_peer_message(
        &mut self,
        message: Value,
        diagnostics: &mut HashMap<String, Vec<Diagnostic>>,
        diagnostic_updates: &mut Vec<DiagnosticFile>,
    ) -> crate::errors::ServiceResult<()> {
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return Ok(());
        };
        let request_id = message.get("id").cloned();
        if request_id.is_none() {
            if let Some(update) =
                handle_lsp_notification(method, message.get("params"), diagnostics)
            {
                diagnostic_updates.push(update);
            }
            return Ok(());
        }

        match handle_lsp_request(method, message.get("params")) {
            Ok(result) => {
                self.write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": request_id.unwrap(),
                    "result": result
                }))
                .await
            }
            Err((code, message_text)) => {
                self.write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": request_id.unwrap(),
                    "error": {
                        "code": code,
                        "message": message_text
                    }
                }))
                .await
            }
        }
    }

    async fn read_message(&mut self) -> crate::errors::ServiceResult<Value> {
        let mut content_length = None;
        let mut line = String::new();

        loop {
            line.clear();
            let bytes = self.stdout.read_line(&mut line).await?;
            if bytes == 0 {
                return Err(crate::errors::ServiceError::Connection(
                    "LSP stdio server closed stdout".to_string(),
                ));
            }
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                break;
            }
            if let Some((name, value)) = trimmed.split_once(':') {
                if name.eq_ignore_ascii_case("Content-Length") {
                    content_length = value
                        .trim()
                        .parse::<usize>()
                        .ok()
                        .filter(|length| *length > 0);
                }
            }
        }

        let content_length = content_length.ok_or_else(|| {
            crate::errors::ServiceError::Connection(
                "LSP message missing Content-Length header".to_string(),
            )
        })?;
        let mut body = vec![0; content_length];
        self.stdout.read_exact(&mut body).await?;
        Ok(serde_json::from_slice(&body)?)
    }
}

impl Drop for StdioLspTransport {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

fn default_initialization_options() -> Value {
    json!({})
}

fn initialize_params(config: &LspServerConfig) -> crate::errors::ServiceResult<Value> {
    let root_path = workspace_root_path(&config.root_path);
    let root_uri = root_uri(&root_path)?;
    let root_name = root_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("workspace");

    Ok(json!({
        "processId": std::process::id(),
        "rootPath": root_path.to_string_lossy(),
        "rootUri": root_uri,
        "workspaceFolders": [{
            "uri": root_uri,
            "name": root_name
        }],
        "initializationOptions": config.initialization_options.clone(),
        "capabilities": {
            "workspace": {
                "configuration": false,
                "workspaceFolders": false,
                "symbol": {
                    "dynamicRegistration": false
                }
            },
            "textDocument": {
                "synchronization": {
                    "dynamicRegistration": false,
                    "willSave": false,
                    "willSaveWaitUntil": false,
                    "didSave": true
                },
                "publishDiagnostics": {
                    "relatedInformation": true,
                    "versionSupport": false
                },
                "hover": {
                    "dynamicRegistration": false,
                    "contentFormat": ["markdown", "plaintext"]
                },
                "definition": {
                    "dynamicRegistration": false,
                    "linkSupport": true
                },
                "implementation": {
                    "dynamicRegistration": false,
                    "linkSupport": true
                },
                "references": {
                    "dynamicRegistration": false
                },
                "documentSymbol": {
                    "dynamicRegistration": false,
                    "hierarchicalDocumentSymbolSupport": true
                },
                "callHierarchy": {
                    "dynamicRegistration": false
                }
            },
            "general": {
                "positionEncodings": ["utf-16"]
            }
        }
    }))
}

fn workspace_root_path(root_path: &str) -> PathBuf {
    let root = PathBuf::from(root_path);
    if root.is_absolute() {
        root
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(&root))
            .unwrap_or(root)
    }
}

fn root_uri(root_path: &Path) -> crate::errors::ServiceResult<String> {
    Url::from_directory_path(root_path)
        .map(|url| url.to_string())
        .map_err(|_| {
            crate::errors::ServiceError::Connection(format!(
                "failed to build file URI for LSP root {}",
                root_path.display()
            ))
        })
}

fn handle_lsp_notification(
    method: &str,
    params: Option<&Value>,
    diagnostics: &mut HashMap<String, Vec<Diagnostic>>,
) -> Option<DiagnosticFile> {
    if method != "textDocument/publishDiagnostics" {
        return None;
    }
    let Some(params) = params else {
        return None;
    };
    let Some(uri) = params.get("uri").and_then(Value::as_str) else {
        return None;
    };
    let file = file_key_from_uri(uri);
    let parsed = params
        .get("diagnostics")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| parse_diagnostic(&file, item))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    diagnostics.insert(file.clone(), parsed.clone());
    Some(DiagnosticFile {
        file,
        diagnostics: parsed,
    })
}

fn handle_lsp_request(method: &str, params: Option<&Value>) -> Result<Value, (i64, String)> {
    match method {
        "workspace/configuration" => Ok(workspace_configuration_response(params)),
        "workspace/workspaceFolders" => Ok(json!([])),
        "client/registerCapability" | "client/unregisterCapability" => Ok(Value::Null),
        _ => Err((-32601, format!("LSP request method not found: {}", method))),
    }
}

fn workspace_configuration_response(params: Option<&Value>) -> Value {
    let count = params
        .and_then(|params| params.get("items"))
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    Value::Array((0..count).map(|_| Value::Null).collect())
}

fn parse_diagnostic(file: &str, item: &Value) -> Option<Diagnostic> {
    let start = item.get("range")?.get("start")?;
    let line = start.get("line").and_then(Value::as_u64).unwrap_or(0) + 1;
    let column = start.get("character").and_then(Value::as_u64).unwrap_or(0) + 1;
    let severity = match item.get("severity").and_then(Value::as_u64).unwrap_or(3) {
        1 => DiagnosticSeverity::Error,
        2 => DiagnosticSeverity::Warning,
        4 => DiagnosticSeverity::Hint,
        _ => DiagnosticSeverity::Info,
    };
    let message = item
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    Some(Diagnostic {
        file: file.to_string(),
        line: line.min(u32::MAX as u64) as u32,
        column: column.min(u32::MAX as u64) as u32,
        severity,
        message,
    })
}

fn file_key_from_uri(uri: &str) -> String {
    Url::parse(uri)
        .ok()
        .and_then(|url| url.to_file_path().ok())
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| uri.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::{Command as StdCommand, Stdio as StdStdio};
    use uuid::Uuid;

    fn config(command: &str, root_path: &str) -> LspServerConfig {
        LspServerConfig {
            command: command.to_string(),
            args: Vec::new(),
            root_path: root_path.to_string(),
            env: HashMap::new(),
            initialization_options: json!({}),
            settings: None,
            startup_timeout_ms: None,
            shutdown_timeout_ms: None,
            restart_on_crash: false,
            max_restarts: None,
        }
    }

    #[tokio::test]
    async fn start_rejects_empty_command() {
        let mut client = LspClient::new(config("", "."));
        let error = client.start().await.unwrap_err().to_string();
        assert!(error.contains("command cannot be empty"));
    }

    #[tokio::test]
    async fn start_rejects_empty_root_path() {
        let mut client = LspClient::new(config("rust-analyzer", ""));
        let error = client.start().await.unwrap_err().to_string();
        assert!(error.contains("root_path cannot be empty"));
    }

    #[tokio::test]
    async fn start_initializes_stdio_lsp_and_collects_diagnostics() {
        let root = std::env::temp_dir().join(format!("kiana-lsp-service-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let source_path = root.join("lib.rs");
        fs::write(&source_path, "fn main() {}\n").unwrap();
        let script = write_mock_lsp_server();
        let file_uri = Url::from_file_path(&source_path).unwrap().to_string();

        let mut client = LspClient::new(LspServerConfig {
            command: mock_lsp_python_command(),
            args: vec![script.to_string_lossy().to_string(), file_uri.clone()],
            root_path: root.to_string_lossy().to_string(),
            env: HashMap::new(),
            initialization_options: json!({}),
            settings: None,
            startup_timeout_ms: None,
            shutdown_timeout_ms: None,
            restart_on_crash: false,
            max_restarts: None,
        });

        client.start().await.unwrap();
        assert!(client.is_initialized());
        assert_eq!(
            client.initialize_result().unwrap()["capabilities"]["textDocumentSync"],
            json!(1)
        );
        assert_eq!(
            client.initialize_result().unwrap()["serverInfo"]["name"],
            json!("mock-lsp")
        );

        let diagnostics = client.get_diagnostics(&source_path.to_string_lossy());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].line, 1);
        assert_eq!(diagnostics[0].column, 4);
        assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostics[0].message, "mock warning");
        let updates = client.take_diagnostic_updates();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].file, source_path.to_string_lossy());
        assert_eq!(updates[0].diagnostics[0].message, "mock warning");
        assert!(client.take_diagnostic_updates().is_empty());

        client
            .send_notification(
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": file_uri,
                        "languageId": "rust",
                        "version": 1,
                        "text": "fn main() {}\n"
                    }
                }),
            )
            .await
            .unwrap();
        let symbols = client
            .send_request(
                "textDocument/documentSymbol",
                json!({ "textDocument": { "uri": file_uri } }),
            )
            .await
            .unwrap();
        assert_eq!(symbols[0]["name"], json!("main"));
        assert_eq!(symbols[0]["kind"], json!(12));

        client.shutdown().await.unwrap();
        assert!(!client.is_initialized());

        let _ = fs::remove_file(script);
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn start_passes_initialization_options_settings_and_startup_timeout() {
        let root = std::env::temp_dir().join(format!("kiana-lsp-config-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let source_path = root.join("lib.rs");
        fs::write(&source_path, "fn main() {}\n").unwrap();
        let script = write_mock_lsp_server();
        let log_path = root.join("lsp.log");
        let file_uri = Url::from_file_path(&source_path).unwrap().to_string();
        let mut env = HashMap::new();
        env.insert(
            "KIANA_LSP_SERVICE_LOG".to_string(),
            log_path.to_string_lossy().to_string(),
        );

        let mut client = LspClient::new(LspServerConfig {
            command: mock_lsp_python_command(),
            args: vec![script.to_string_lossy().to_string(), file_uri],
            root_path: root.to_string_lossy().to_string(),
            env,
            initialization_options: json!({ "probe": "service" }),
            settings: Some(json!({ "mock": { "strict": true } })),
            startup_timeout_ms: Some(1_000),
            shutdown_timeout_ms: Some(1_000),
            restart_on_crash: false,
            max_restarts: None,
        });

        client.start().await.unwrap();
        client.shutdown().await.unwrap();

        let events = fs::read_to_string(&log_path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert!(events.iter().any(|event| {
            event["kind"] == json!("initialize")
                && event["payload"]["initializationOptions"]["probe"] == json!("service")
        }));
        assert!(events.iter().any(|event| {
            event["kind"] == json!("didChangeConfiguration")
                && event["payload"]["settings"]["mock"]["strict"] == json!(true)
        }));

        let _ = fs::remove_file(script);
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn start_honors_startup_timeout() {
        let root = std::env::temp_dir().join(format!("kiana-lsp-timeout-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let source_path = root.join("lib.rs");
        fs::write(&source_path, "fn main() {}\n").unwrap();
        let script = write_mock_lsp_server();
        let file_uri = Url::from_file_path(&source_path).unwrap().to_string();
        let mut env = HashMap::new();
        env.insert("KIANA_LSP_SERVICE_DELAY_MS".to_string(), "200".to_string());

        let mut client = LspClient::new(LspServerConfig {
            command: mock_lsp_python_command(),
            args: vec![script.to_string_lossy().to_string(), file_uri],
            root_path: root.to_string_lossy().to_string(),
            env,
            initialization_options: json!({}),
            settings: None,
            startup_timeout_ms: Some(20),
            shutdown_timeout_ms: None,
            restart_on_crash: false,
            max_restarts: None,
        });

        let error = client.start().await.unwrap_err().to_string();
        assert!(error.contains("LSP request 'initialize' timed out"));

        let _ = fs::remove_file(script);
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn request_restarts_after_stdio_server_crash_when_enabled() {
        let root = std::env::temp_dir().join(format!("kiana-lsp-restart-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let source_path = root.join("lib.rs");
        fs::write(&source_path, "fn main() {}\n").unwrap();
        let script = write_mock_lsp_server();
        let file_uri = Url::from_file_path(&source_path).unwrap().to_string();
        let crash_marker = root.join("crashed-once");
        let log_path = root.join("lsp.log");
        let mut env = HashMap::new();
        env.insert(
            "KIANA_LSP_SERVICE_CRASH_ON_SYMBOL_ONCE".to_string(),
            crash_marker.to_string_lossy().to_string(),
        );
        env.insert(
            "KIANA_LSP_SERVICE_LOG".to_string(),
            log_path.to_string_lossy().to_string(),
        );

        let mut client = LspClient::new(LspServerConfig {
            command: mock_lsp_python_command(),
            args: vec![script.to_string_lossy().to_string(), file_uri.clone()],
            root_path: root.to_string_lossy().to_string(),
            env,
            initialization_options: json!({}),
            settings: None,
            startup_timeout_ms: Some(1_000),
            shutdown_timeout_ms: Some(1_000),
            restart_on_crash: true,
            max_restarts: Some(1),
        });

        client.start().await.unwrap();
        let symbols = client
            .send_request(
                "textDocument/documentSymbol",
                json!({ "textDocument": { "uri": file_uri } }),
            )
            .await
            .unwrap();
        assert_eq!(symbols[0]["name"], json!("main"));
        let initialize_count = fs::read_to_string(&log_path)
            .unwrap()
            .lines()
            .filter(|line| line.contains(r#""kind": "initialize""#))
            .count();
        assert_eq!(initialize_count, 2);

        client.shutdown().await.unwrap();
        let _ = fs::remove_file(script);
        let _ = fs::remove_dir_all(root);
    }

    fn write_mock_lsp_server() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "kiana-mock-lsp-{}-{}.py",
            std::process::id(),
            Uuid::new_v4()
        ));
        fs::write(
            &path,
            r#"
import json
import os
import sys
import time

diag_uri = sys.argv[1]
log_path = os.environ.get("KIANA_LSP_SERVICE_LOG")
delay_ms = int(os.environ.get("KIANA_LSP_SERVICE_DELAY_MS", "0"))
crash_on_symbol_once = os.environ.get("KIANA_LSP_SERVICE_CRASH_ON_SYMBOL_ONCE")

def log_event(kind, payload):
    if not log_path:
        return
    with open(log_path, "a", encoding="utf-8") as fh:
        fh.write(json.dumps({"kind": kind, "payload": payload}) + "\n")

def read_message():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        name, value = line.decode("ascii").split(":", 1)
        headers[name.lower()] = value.strip()
    length = int(headers["content-length"])
    return json.loads(sys.stdin.buffer.read(length).decode("utf-8"))

def write_message(message):
    body = json.dumps(message).encode("utf-8")
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode("ascii"))
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()

while True:
    msg = read_message()
    if msg is None:
        break
    method = msg.get("method")
    if method == "initialize":
        log_event("initialize", msg.get("params"))
        if delay_ms:
            time.sleep(delay_ms / 1000.0)
        write_message({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": diag_uri,
                "diagnostics": [{
                    "range": {
                        "start": {"line": 0, "character": 3},
                        "end": {"line": 0, "character": 7}
                    },
                    "severity": 2,
                    "message": "mock warning"
                }]
            }
        })
        write_message({
            "jsonrpc": "2.0",
            "id": "cfg-1",
            "method": "workspace/configuration",
            "params": {
                "items": [
                    {"section": "rust"},
                    {"section": "typescript"}
                ]
            }
        })
        config_response = read_message()
        config_ok = config_response.get("id") == "cfg-1" and config_response.get("result") == [None, None]
        write_message({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {
                "capabilities": {
                    "textDocumentSync": 1,
                    "definitionProvider": True,
                    "referencesProvider": True,
                    "documentSymbolProvider": True
                },
                "serverInfo": {"name": "mock-lsp" if config_ok else "bad-config-response", "version": "1.0.0"}
            }
        })
    elif method == "workspace/didChangeConfiguration":
        log_event("didChangeConfiguration", msg.get("params"))
    elif method == "shutdown":
        write_message({"jsonrpc": "2.0", "id": msg["id"], "result": None})
    elif method == "textDocument/documentSymbol":
        if crash_on_symbol_once and not os.path.exists(crash_on_symbol_once):
            with open(crash_on_symbol_once, "w", encoding="utf-8") as fh:
                fh.write("crashed\n")
            sys.exit(2)
        write_message({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": [{
                "name": "main",
                "kind": 12,
                "range": {
                    "start": {"line": 0, "character": 0},
                    "end": {"line": 0, "character": 12}
                },
                "selectionRange": {
                    "start": {"line": 0, "character": 3},
                    "end": {"line": 0, "character": 7}
                }
            }]
        })
    elif method == "exit":
        break
"#,
        )
        .unwrap();
        path
    }

    fn mock_lsp_python_command() -> String {
        ["python3", "python", "py"]
            .into_iter()
            .find(|candidate| {
                StdCommand::new(candidate)
                    .arg("--version")
                    .stdout(StdStdio::null())
                    .stderr(StdStdio::null())
                    .status()
                    .map(|status| status.success())
                    .unwrap_or(false)
            })
            .unwrap_or("python3")
            .to_string()
    }
}
