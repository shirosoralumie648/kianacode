use crate::tool::*;
use async_trait::async_trait;
use kiana_services::lsp::{
    Diagnostic, DiagnosticFile, DiagnosticSeverity, LspClient, LspServerConfig,
};
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex as StdMutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use url::Url;
use walkdir::WalkDir;

#[derive(Debug, Deserialize)]
struct LspInput {
    #[serde(alias = "filePath")]
    path: String,
    #[serde(default, alias = "operation")]
    action: LspAction,
    #[serde(default)]
    server: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    symbol: Option<String>,
    #[serde(default)]
    line: Option<usize>,
    #[serde(default, alias = "character")]
    column: Option<usize>,
    #[serde(default)]
    max_results: Option<usize>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum LspAction {
    #[default]
    Summary,
    Diagnostics,
    PendingDiagnostics,
    #[serde(alias = "documentSymbol")]
    Symbols,
    #[serde(alias = "goToDefinition")]
    Definition,
    #[serde(alias = "findReferences")]
    References,
    Outline,
    Servers,
    Hover,
    #[serde(alias = "workspaceSymbol")]
    WorkspaceSymbol,
    #[serde(alias = "goToImplementation")]
    Implementation,
    #[serde(alias = "prepareCallHierarchy")]
    PrepareCallHierarchy,
    #[serde(alias = "incomingCalls")]
    IncomingCalls,
    #[serde(alias = "outgoingCalls")]
    OutgoingCalls,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SymbolEntry {
    line: usize,
    column: usize,
    kind: String,
    name: String,
    text: String,
    indent: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct ParsedLspConfig {
    command: String,
    args: Vec<String>,
    root_path: PathBuf,
    env: HashMap<String, String>,
    initialization_options: Value,
    settings: Option<Value>,
    startup_timeout_ms: Option<u64>,
    shutdown_timeout_ms: Option<u64>,
    restart_on_crash: bool,
    max_restarts: Option<u64>,
}

pub struct LspTool;

impl LspTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for LspTool {
    fn name(&self) -> &str {
        "LSP"
    }

    fn description(&self) -> &str {
        "Inspect local source files for diagnostics, symbols, definitions, references, hover, implementations, call hierarchy, outlines, and configured plugin LSP servers"
    }

    fn search_hint(&self) -> Option<&str> {
        Some("find symbols, definitions, references, hover info, implementations, call hierarchy, plugin LSP servers, and diagnostics in source files")
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "filePath": { "type": "string" },
                "action": {
                    "type": "string",
                    "enum": [
                        "summary", "diagnostics", "pending_diagnostics", "symbols", "definition", "references",
                        "outline", "servers", "hover", "workspace_symbol", "implementation",
                        "prepare_call_hierarchy", "incoming_calls", "outgoing_calls",
                        "documentSymbol", "goToDefinition", "findReferences", "workspaceSymbol",
                        "goToImplementation", "prepareCallHierarchy", "incomingCalls", "outgoingCalls"
                    ],
                    "description": "Operation to perform. summary preserves the legacy diagnostics and symbols response."
                },
                "operation": { "type": "string" },
                "query": { "type": "string" },
                "server": {
                    "type": "string",
                    "description": "Optional scoped plugin LSP server name, e.g. plugin:my-plugin:rust or rust."
                },
                "symbol": {
                    "type": "string",
                    "description": "Exact symbol for definition or references. Falls back to query."
                },
                "line": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "1-based line for LSP position requests."
                },
                "column": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "1-based column for LSP position requests."
                },
                "character": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Reference-compatible alias for column."
                },
                "max_results": { "type": "integer", "minimum": 1 }
            },
            "required": ["path"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "diagnostics": { "type": "array" },
                "symbols": { "type": "array" },
                "definitions": { "type": "array" },
                "references": { "type": "array" },
                "implementations": { "type": "array" },
                "hover": { "type": ["object", "null"] },
                "call_hierarchy": { "type": "array" },
                "incoming_calls": { "type": "array" },
                "outgoing_calls": { "type": "array" },
                "outline": { "type": "array" },
                "servers": { "type": "object" },
                "batches": { "type": "array" },
                "count": { "type": "integer" },
                "server": { "type": "string" },
                "lsp": { "type": "boolean" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, context: &ToolContext) -> ValidationResult {
        let input: LspInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(e) => return ValidationResult::err(format!("Invalid input: {}", e), 1),
        };
        if input.path.trim().is_empty() {
            return ValidationResult::err("path cannot be empty".to_string(), 2);
        }
        if let Err(error) = context.resolve_access_path(&input.path) {
            return ValidationResult::err(error, 9);
        }
        if input.line == Some(0) || input.column == Some(0) {
            return ValidationResult::err("line and column must be 1-based".to_string(), 4);
        }
        if (input.line.is_some() || input.column.is_some()) && input_position(&input).is_none() {
            return ValidationResult::err(
                "line and column/character must be provided together".to_string(),
                5,
            );
        }
        if matches!(input.action, LspAction::Definition | LspAction::References)
            && target_symbol(&input).is_none()
            && input_position(&input).is_none()
        {
            return ValidationResult::err(
                "definition and references require symbol/query or line/column".to_string(),
                3,
            );
        }
        if matches!(
            input.action,
            LspAction::Hover
                | LspAction::Implementation
                | LspAction::PrepareCallHierarchy
                | LspAction::IncomingCalls
                | LspAction::OutgoingCalls
        ) && input_position(&input).is_none()
        {
            return ValidationResult::err(
                format!("{:?} requires line and column/character", input.action),
                6,
            );
        }
        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: LspInput = serde_json::from_value(input.clone())?;
        if input.path.trim().is_empty() {
            return Err(ToolError::ValidationError(
                "path cannot be empty".to_string(),
            ));
        }

        let path = context
            .resolve_access_path(&input.path)
            .map_err(ToolError::PermissionDenied)?;
        let roots = context
            .access_roots()
            .unwrap_or_else(|| vec![PathBuf::from(&context.cwd)]);
        let max_results = input.max_results.unwrap_or(50).clamp(1, 500);
        let output = match input.action {
            LspAction::Servers => {
                let servers = load_plugin_lsp_servers();
                let count = servers.len();
                json!({
                    "action": "servers",
                    "path": path.to_string_lossy(),
                    "count": count,
                    "servers": servers
                })
            }
            LspAction::Summary => {
                let contents = read_required_source_file(&path, input.action)?;
                if let Some(output) =
                    lsp_summary_output(&path, &contents, &input, context, max_results).await?
                {
                    output
                } else {
                    let diagnostics = collect_diagnostics(&contents);
                    let symbols = collect_symbols(&contents, input.query.as_deref(), max_results)
                        .into_iter()
                        .map(|symbol| symbol_to_value(&path, symbol))
                        .collect::<Vec<_>>();

                    json!({
                        "action": "summary",
                        "path": path.to_string_lossy(),
                        "diagnostics": diagnostics,
                        "symbols": symbols
                    })
                }
            }
            LspAction::Diagnostics => {
                let contents = read_required_source_file(&path, input.action)?;
                if let Some(output) =
                    lsp_diagnostics_output(&path, &contents, &input, context).await?
                {
                    output
                } else {
                    json!({
                        "action": "diagnostics",
                        "path": path.to_string_lossy(),
                        "diagnostics": collect_diagnostics(&contents)
                    })
                }
            }
            LspAction::PendingDiagnostics => consume_pending_lsp_diagnostics(),
            LspAction::Hover => {
                let contents = read_required_source_file(&path, input.action)?;
                let position = input_position(&input).ok_or_else(|| {
                    ToolError::ValidationError(
                        "hover requires line and column/character".to_string(),
                    )
                })?;
                if let Some(output) =
                    lsp_hover_output(&path, &contents, &input, context, position).await?
                {
                    output
                } else {
                    json!({
                        "action": "hover",
                        "path": path.to_string_lossy(),
                        "hover": Value::Null
                    })
                }
            }
            LspAction::Symbols => {
                let contents = read_required_source_file(&path, input.action)?;
                if let Some(output) = lsp_document_symbols_output(
                    &path,
                    &contents,
                    &input,
                    context,
                    max_results,
                    false,
                )
                .await?
                {
                    output
                } else {
                    let symbols = collect_symbols(&contents, input.query.as_deref(), max_results)
                        .into_iter()
                        .map(|symbol| symbol_to_value(&path, symbol))
                        .collect::<Vec<_>>();
                    json!({
                        "action": "symbols",
                        "path": path.to_string_lossy(),
                        "symbols": symbols
                    })
                }
            }
            LspAction::WorkspaceSymbol => {
                let contents = read_required_source_file(&path, input.action)?;
                if let Some(output) =
                    lsp_workspace_symbol_output(&path, &contents, &input, context, max_results)
                        .await?
                {
                    output
                } else {
                    let symbols = collect_workspace_symbols(
                        &path,
                        &roots,
                        input.query.as_deref(),
                        max_results,
                    )?;
                    json!({
                        "action": "workspace_symbol",
                        "path": path.to_string_lossy(),
                        "symbols": symbols
                    })
                }
            }
            LspAction::Outline => {
                let contents = read_required_source_file(&path, input.action)?;
                if let Some(output) = lsp_document_symbols_output(
                    &path,
                    &contents,
                    &input,
                    context,
                    max_results,
                    true,
                )
                .await?
                {
                    output
                } else {
                    let outline = collect_symbols(&contents, input.query.as_deref(), max_results)
                        .into_iter()
                        .map(|symbol| outline_to_value(&path, symbol))
                        .collect::<Vec<_>>();
                    json!({
                        "action": "outline",
                        "path": path.to_string_lossy(),
                        "outline": outline
                    })
                }
            }
            LspAction::Implementation => {
                let contents = read_required_source_file(&path, input.action)?;
                let position = input_position(&input).ok_or_else(|| {
                    ToolError::ValidationError(
                        "implementation requires line and column/character".to_string(),
                    )
                })?;
                if let Some(output) = lsp_locations_output(
                    &path,
                    &contents,
                    &input,
                    context,
                    position,
                    max_results,
                    LspLocationRequest::Implementation,
                )
                .await?
                {
                    output
                } else {
                    json!({
                        "action": "implementation",
                        "path": path.to_string_lossy(),
                        "implementations": []
                    })
                }
            }
            LspAction::PrepareCallHierarchy => {
                let contents = read_required_source_file(&path, input.action)?;
                let position = input_position(&input).ok_or_else(|| {
                    ToolError::ValidationError(
                        "prepare_call_hierarchy requires line and column/character".to_string(),
                    )
                })?;
                if let Some(output) = lsp_call_hierarchy_output(
                    &path,
                    &contents,
                    &input,
                    context,
                    position,
                    max_results,
                    LspCallHierarchyRequest::Prepare,
                )
                .await?
                {
                    output
                } else {
                    json!({
                        "action": "prepare_call_hierarchy",
                        "path": path.to_string_lossy(),
                        "call_hierarchy": []
                    })
                }
            }
            LspAction::IncomingCalls => {
                let contents = read_required_source_file(&path, input.action)?;
                let position = input_position(&input).ok_or_else(|| {
                    ToolError::ValidationError(
                        "incoming_calls requires line and column/character".to_string(),
                    )
                })?;
                if let Some(output) = lsp_call_hierarchy_output(
                    &path,
                    &contents,
                    &input,
                    context,
                    position,
                    max_results,
                    LspCallHierarchyRequest::Incoming,
                )
                .await?
                {
                    output
                } else {
                    json!({
                        "action": "incoming_calls",
                        "path": path.to_string_lossy(),
                        "incoming_calls": []
                    })
                }
            }
            LspAction::OutgoingCalls => {
                let contents = read_required_source_file(&path, input.action)?;
                let position = input_position(&input).ok_or_else(|| {
                    ToolError::ValidationError(
                        "outgoing_calls requires line and column/character".to_string(),
                    )
                })?;
                if let Some(output) = lsp_call_hierarchy_output(
                    &path,
                    &contents,
                    &input,
                    context,
                    position,
                    max_results,
                    LspCallHierarchyRequest::Outgoing,
                )
                .await?
                {
                    output
                } else {
                    json!({
                        "action": "outgoing_calls",
                        "path": path.to_string_lossy(),
                        "outgoing_calls": []
                    })
                }
            }
            LspAction::Definition => {
                if let Some(position) = input_position(&input) {
                    let contents = read_required_source_file(&path, input.action)?;
                    if let Some(output) = lsp_locations_output(
                        &path,
                        &contents,
                        &input,
                        context,
                        position,
                        max_results,
                        LspLocationRequest::Definition,
                    )
                    .await?
                    {
                        output
                    } else {
                        json!({
                            "action": "definition",
                            "path": path.to_string_lossy(),
                            "definitions": []
                        })
                    }
                } else {
                    let symbol = target_symbol(&input).ok_or_else(|| {
                        ToolError::ValidationError(
                            "definition requires symbol/query or line/column".to_string(),
                        )
                    })?;
                    let definitions = collect_definitions(&path, &roots, &symbol, max_results)?;
                    json!({
                        "action": "definition",
                        "path": path.to_string_lossy(),
                        "symbol": symbol,
                        "definitions": definitions
                    })
                }
            }
            LspAction::References => {
                if let Some(position) = input_position(&input) {
                    let contents = read_required_source_file(&path, input.action)?;
                    if let Some(output) = lsp_locations_output(
                        &path,
                        &contents,
                        &input,
                        context,
                        position,
                        max_results,
                        LspLocationRequest::References,
                    )
                    .await?
                    {
                        output
                    } else {
                        json!({
                            "action": "references",
                            "path": path.to_string_lossy(),
                            "references": []
                        })
                    }
                } else {
                    let symbol = target_symbol(&input).ok_or_else(|| {
                        ToolError::ValidationError(
                            "references requires symbol/query or line/column".to_string(),
                        )
                    })?;
                    let references = collect_references(&path, &roots, &symbol, max_results)?;
                    json!({
                        "action": "references",
                        "path": path.to_string_lossy(),
                        "symbol": symbol,
                        "references": references
                    })
                }
            }
        };

        Ok(ToolOutput {
            data: output,
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": format_lsp_result_for_model(&output.data)
        })
    }
}

fn format_lsp_result_for_model(data: &Value) -> String {
    let action = data
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("summary");
    let path = data
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("unknown path");
    let mut lines = vec![format!("LSP {action} result for {path}.")];

    if let Some(server) = data.get("server").and_then(Value::as_str) {
        lines.push(format!("Server: {server}"));
    }
    if data.get("lsp").and_then(Value::as_bool) == Some(true) {
        lines.push("Source: language server".to_string());
    }
    if let Some(symbol) = data.get("symbol").and_then(Value::as_str) {
        lines.push(format!("Symbol: {symbol}"));
    }

    if let Some(servers) = data.get("servers").and_then(Value::as_object) {
        if servers.is_empty() {
            lines.push("No LSP servers configured.".to_string());
        } else {
            lines.push(format!("{} LSP server(s) configured:", servers.len()));
            for (name, server) in servers {
                let language = server
                    .get("language_id")
                    .or_else(|| server.get("languageId"))
                    .and_then(Value::as_str)
                    .unwrap_or("unknown language");
                lines.push(format!("- {name}: {language}"));
            }
        }
    }

    for (label, key) in [
        ("Diagnostics", "diagnostics"),
        ("Symbols", "symbols"),
        ("Definitions", "definitions"),
        ("References", "references"),
        ("Implementations", "implementations"),
        ("Call hierarchy", "call_hierarchy"),
        ("Incoming calls", "incoming_calls"),
        ("Outgoing calls", "outgoing_calls"),
        ("Outline", "outline"),
        ("Batches", "batches"),
    ] {
        push_lsp_items(&mut lines, label, data, key);
    }

    if let Some(hover) = data.get("hover") {
        if hover.is_null() {
            lines.push("Hover: no result".to_string());
        } else if let Some(text) = hover
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
        {
            lines.push(format!("Hover:\n{}", indent_lsp_lines(text.trim(), "  ")));
        }
    }

    lines.join("\n")
}

fn push_lsp_items(lines: &mut Vec<String>, label: &str, data: &Value, key: &str) {
    let Some(items) = data.get(key).and_then(Value::as_array) else {
        return;
    };
    lines.push(format!("{label}: {}", items.len()));
    for item in items.iter().take(5) {
        lines.push(format!("- {}", lsp_item_summary(item)));
    }
    if items.len() > 5 {
        lines.push(format!("- ... {} more", items.len() - 5));
    }
}

fn lsp_item_summary(item: &Value) -> String {
    if let Some(from) = item.get("from") {
        return format!("from {}", lsp_item_summary(from));
    }
    if let Some(to) = item.get("to") {
        return format!("to {}", lsp_item_summary(to));
    }

    let name = item
        .get("name")
        .and_then(Value::as_str)
        .or_else(|| item.get("message").and_then(Value::as_str))
        .or_else(|| item.get("text").and_then(Value::as_str))
        .unwrap_or("item");
    let kind = item
        .get("kind")
        .and_then(Value::as_str)
        .filter(|kind| !kind.trim().is_empty());
    let severity = item
        .get("severity")
        .and_then(Value::as_str)
        .filter(|severity| !severity.trim().is_empty());
    let file = item
        .get("file")
        .and_then(Value::as_str)
        .filter(|file| !file.trim().is_empty())
        .unwrap_or("unknown file");
    let line = item
        .get("line")
        .and_then(Value::as_i64)
        .or_else(|| {
            item.get("line")
                .and_then(Value::as_u64)
                .map(|line| line as i64)
        })
        .unwrap_or(0);
    let column = item
        .get("column")
        .and_then(Value::as_i64)
        .or_else(|| {
            item.get("column")
                .and_then(Value::as_u64)
                .map(|column| column as i64)
        })
        .unwrap_or(0);

    let mut prefix = String::new();
    if let Some(severity) = severity {
        prefix.push_str(&format!("[{severity}] "));
    }
    if let Some(kind) = kind {
        prefix.push_str(&format!("{kind} "));
    }
    format!("{prefix}{name} at {file}:{line}:{column}")
}

fn indent_lsp_lines(value: &str, indent: &str) -> String {
    value
        .lines()
        .map(|line| format!("{indent}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Clone)]
struct ResolvedPluginLspServer {
    name: String,
    language_id: String,
    config: LspServerConfig,
}

struct ActiveLspSession {
    key: String,
    server_name: String,
    uri: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LspLocationRequest {
    Definition,
    References,
    Implementation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LspCallHierarchyRequest {
    Prepare,
    Incoming,
    Outgoing,
}

struct ManagedLspClient {
    client: LspClient,
    opened_files: HashMap<String, String>,
}

static LSP_CLIENTS: OnceLock<Mutex<HashMap<String, ManagedLspClient>>> = OnceLock::new();
static LSP_DIAGNOSTICS: OnceLock<StdMutex<LspDiagnosticRegistry>> = OnceLock::new();

const MAX_DIAGNOSTICS_PER_FILE: usize = 10;
const MAX_TOTAL_DIAGNOSTICS: usize = 30;
const MAX_DELIVERED_FILES: usize = 500;

fn lsp_clients() -> &'static Mutex<HashMap<String, ManagedLspClient>> {
    LSP_CLIENTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lsp_diagnostic_registry() -> &'static StdMutex<LspDiagnosticRegistry> {
    LSP_DIAGNOSTICS.get_or_init(|| StdMutex::new(LspDiagnosticRegistry::default()))
}

#[derive(Default)]
struct LspDiagnosticRegistry {
    pending: Vec<PendingLspDiagnostic>,
    delivered: HashMap<String, HashSet<String>>,
    delivered_order: VecDeque<String>,
}

struct PendingLspDiagnostic {
    server_name: String,
    files: Vec<DiagnosticFile>,
    timestamp_ms: u128,
    attachment_sent: bool,
}

struct LspDiagnosticBatch {
    server_name: String,
    files: Vec<DiagnosticFile>,
    original_count: usize,
    delivered_count: usize,
    truncated_count: usize,
}

#[derive(Default)]
struct LspDiagnosticRegistryStatus {
    pending_notifications: usize,
    pending_files: usize,
    pending_diagnostics: usize,
    delivered_files: usize,
    oldest_pending_age_ms: u128,
}

impl LspDiagnosticRegistry {
    fn register(&mut self, server_name: &str, files: Vec<DiagnosticFile>) {
        if files.is_empty() {
            return;
        }
        self.pending.push(PendingLspDiagnostic {
            server_name: server_name.to_string(),
            files,
            timestamp_ms: now_ms(),
            attachment_sent: false,
        });
    }

    fn check_pending(&mut self) -> Vec<LspDiagnosticBatch> {
        let mut all_files = Vec::new();
        let mut server_names = HashSet::new();
        for pending in self.pending.iter_mut() {
            if pending.attachment_sent {
                continue;
            }
            pending.attachment_sent = true;
            server_names.insert(pending.server_name.clone());
            all_files.extend(pending.files.clone());
        }
        self.pending.retain(|pending| !pending.attachment_sent);

        if all_files.is_empty() {
            return Vec::new();
        }

        let original_count = all_files
            .iter()
            .map(|file| file.diagnostics.len())
            .sum::<usize>();
        let (files, duplicate_count) = self.deduplicate_files(all_files);
        let (files, truncated_count) = limit_diagnostic_volume(files);
        let delivered_count = files
            .iter()
            .map(|file| file.diagnostics.len())
            .sum::<usize>();
        if delivered_count == 0 {
            return Vec::new();
        }

        self.track_delivered(&files);
        let mut server_names = server_names.into_iter().collect::<Vec<_>>();
        server_names.sort();
        vec![LspDiagnosticBatch {
            server_name: server_names.join(", "),
            files,
            original_count,
            delivered_count,
            truncated_count: duplicate_count + truncated_count,
        }]
    }

    fn clear_pending(&mut self) {
        self.pending.clear();
    }

    fn reset(&mut self) {
        self.pending.clear();
        self.delivered.clear();
        self.delivered_order.clear();
    }

    fn clear_delivered_for_file(&mut self, file: &str) {
        self.delivered.remove(file);
        self.delivered_order.retain(|entry| entry != file);
    }

    fn status(&self) -> LspDiagnosticRegistryStatus {
        let now = now_ms();
        LspDiagnosticRegistryStatus {
            pending_notifications: self.pending.len(),
            pending_files: self.pending.iter().map(|pending| pending.files.len()).sum(),
            pending_diagnostics: self
                .pending
                .iter()
                .flat_map(|pending| pending.files.iter())
                .map(|file| file.diagnostics.len())
                .sum(),
            delivered_files: self.delivered.len(),
            oldest_pending_age_ms: self
                .pending
                .iter()
                .map(|pending| now.saturating_sub(pending.timestamp_ms))
                .max()
                .unwrap_or_default(),
        }
    }

    fn deduplicate_files(&self, all_files: Vec<DiagnosticFile>) -> (Vec<DiagnosticFile>, usize) {
        let mut seen_by_file: HashMap<String, HashSet<String>> = HashMap::new();
        let mut deduped: Vec<DiagnosticFile> = Vec::new();
        let mut duplicate_count = 0;

        for file in all_files {
            let previously_delivered = self.delivered.get(&file.file);
            let seen = seen_by_file.entry(file.file.clone()).or_default();
            let file_index = match deduped.iter().position(|entry| entry.file == file.file) {
                Some(index) => index,
                None => {
                    deduped.push(DiagnosticFile {
                        file: file.file.clone(),
                        diagnostics: Vec::new(),
                    });
                    deduped.len() - 1
                }
            };

            for diagnostic in file.diagnostics {
                let key = diagnostic_key(&diagnostic);
                if seen.contains(&key)
                    || previously_delivered.is_some_and(|delivered| delivered.contains(&key))
                {
                    duplicate_count += 1;
                    continue;
                }
                seen.insert(key);
                deduped[file_index].diagnostics.push(diagnostic);
            }
        }

        deduped.retain(|file| !file.diagnostics.is_empty());
        (deduped, duplicate_count)
    }

    fn track_delivered(&mut self, files: &[DiagnosticFile]) {
        for file in files {
            if !self.delivered.contains_key(&file.file) {
                self.delivered_order.push_back(file.file.clone());
            }
            while self.delivered_order.len() > MAX_DELIVERED_FILES {
                if let Some(evicted) = self.delivered_order.pop_front() {
                    self.delivered.remove(&evicted);
                }
            }
            let delivered = self.delivered.entry(file.file.clone()).or_default();
            for diagnostic in &file.diagnostics {
                delivered.insert(diagnostic_key(diagnostic));
            }
        }
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn diagnostic_key(diagnostic: &Diagnostic) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        diagnostic.file,
        diagnostic.line,
        diagnostic.column,
        diagnostic_severity_name(diagnostic.severity),
        diagnostic.message
    )
}

fn limit_diagnostic_volume(mut files: Vec<DiagnosticFile>) -> (Vec<DiagnosticFile>, usize) {
    let mut truncated_count = 0;
    let mut total = 0;
    for file in &mut files {
        file.diagnostics
            .sort_by_key(|diagnostic| severity_sort_value(diagnostic.severity));
        if file.diagnostics.len() > MAX_DIAGNOSTICS_PER_FILE {
            truncated_count += file.diagnostics.len() - MAX_DIAGNOSTICS_PER_FILE;
            file.diagnostics.truncate(MAX_DIAGNOSTICS_PER_FILE);
        }
        if total >= MAX_TOTAL_DIAGNOSTICS {
            truncated_count += file.diagnostics.len();
            file.diagnostics.clear();
            continue;
        }
        let remaining = MAX_TOTAL_DIAGNOSTICS - total;
        if file.diagnostics.len() > remaining {
            truncated_count += file.diagnostics.len() - remaining;
            file.diagnostics.truncate(remaining);
        }
        total += file.diagnostics.len();
    }
    files.retain(|file| !file.diagnostics.is_empty());
    (files, truncated_count)
}

fn severity_sort_value(severity: DiagnosticSeverity) -> u8 {
    match severity {
        DiagnosticSeverity::Error => 1,
        DiagnosticSeverity::Warning => 2,
        DiagnosticSeverity::Info => 3,
        DiagnosticSeverity::Hint => 4,
    }
}

fn register_client_diagnostic_updates(server_name: &str, client: &mut LspClient) {
    let updates = client.take_diagnostic_updates();
    if updates.is_empty() {
        return;
    }
    let mut registry = lsp_diagnostic_registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    registry.register(server_name, updates);
}

fn clear_pending_lsp_diagnostics() {
    let mut registry = lsp_diagnostic_registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    registry.clear_pending();
}

pub fn reset_all_lsp_diagnostic_state() {
    let mut registry = lsp_diagnostic_registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    registry.reset();
}

fn clear_delivered_lsp_diagnostics_for_file(file: &str) {
    let mut registry = lsp_diagnostic_registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    registry.clear_delivered_for_file(file);
}

pub fn consume_pending_lsp_diagnostics() -> Value {
    let mut registry = lsp_diagnostic_registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let batches = registry.check_pending();
    let count = batches
        .iter()
        .map(|batch| batch.delivered_count)
        .sum::<usize>();
    json!({
        "action": "pending_diagnostics",
        "lsp": true,
        "count": count,
        "batches": batches
            .iter()
            .map(lsp_diagnostic_batch_to_value)
            .collect::<Vec<_>>()
    })
}

pub async fn lsp_runtime_status() -> Value {
    let clients = lsp_clients().lock().await;
    let active_clients = clients.len();
    let open_files = clients
        .values()
        .map(|managed| managed.opened_files.len())
        .sum::<usize>();
    drop(clients);

    let registry = lsp_diagnostic_registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let diagnostic_status = registry.status();
    json!({
        "active_clients": active_clients,
        "open_files": open_files,
        "pending_notifications": diagnostic_status.pending_notifications,
        "pending_files": diagnostic_status.pending_files,
        "pending_diagnostics": diagnostic_status.pending_diagnostics,
        "delivered_files": diagnostic_status.delivered_files,
        "oldest_pending_age_ms": diagnostic_status.oldest_pending_age_ms
    })
}

fn lsp_diagnostic_batch_to_value(batch: &LspDiagnosticBatch) -> Value {
    json!({
        "server": batch.server_name,
        "files": batch.files.iter().map(diagnostic_file_to_value).collect::<Vec<_>>(),
        "original_count": batch.original_count,
        "delivered_count": batch.delivered_count,
        "truncated_count": batch.truncated_count
    })
}

fn diagnostic_file_to_value(file: &DiagnosticFile) -> Value {
    json!({
        "file": file.file,
        "diagnostics": file.diagnostics.iter().map(diagnostic_to_value).collect::<Vec<_>>()
    })
}

fn diagnostic_to_value(diagnostic: &Diagnostic) -> Value {
    json!({
        "file": &diagnostic.file,
        "line": diagnostic.line,
        "column": diagnostic.column,
        "severity": diagnostic_severity_name(diagnostic.severity),
        "message": &diagnostic.message
    })
}

pub async fn shutdown_lsp_clients() {
    let mut clients = lsp_clients().lock().await;
    for managed in clients.values_mut() {
        let _ = managed.client.shutdown().await;
    }
    clients.clear();
    clear_pending_lsp_diagnostics();
}

pub async fn sync_lsp_file_saved(
    context: &ToolContext,
    path: &Path,
    contents: &str,
) -> ToolResult<bool> {
    clear_delivered_lsp_diagnostics_for_file(&path.to_string_lossy());
    let input = LspInput {
        path: path.to_string_lossy().to_string(),
        action: LspAction::Diagnostics,
        server: None,
        query: None,
        symbol: None,
        line: None,
        column: None,
        max_results: None,
    };
    let Some(session) = start_lsp_session(path, contents, &input, context).await? else {
        return Ok(false);
    };
    {
        let mut clients = lsp_clients().lock().await;
        let Some(managed) = clients.get_mut(&session.key) else {
            return Ok(false);
        };
        managed
            .client
            .send_notification(
                "textDocument/didSave",
                json!({ "textDocument": { "uri": session.uri.clone() } }),
            )
            .await
            .map_err(|error| ToolError::Other(error.to_string()))?;
        let _ = managed
            .client
            .drain_notifications(Duration::from_millis(100))
            .await;
        register_client_diagnostic_updates(&session.server_name, &mut managed.client);
    }
    Ok(true)
}

async fn lsp_summary_output(
    path: &Path,
    contents: &str,
    input: &LspInput,
    context: &ToolContext,
    max_results: usize,
) -> ToolResult<Option<Value>> {
    let Some(session) = start_lsp_session(path, contents, input, context).await? else {
        return Ok(None);
    };
    let (symbols_result, diagnostics) = {
        let mut clients = lsp_clients().lock().await;
        let managed = clients.get_mut(&session.key).ok_or_else(|| {
            ToolError::Other(format!(
                "LSP session '{}' was not available",
                session.server_name
            ))
        })?;
        let symbols_result = managed
            .client
            .send_request(
                "textDocument/documentSymbol",
                json!({ "textDocument": { "uri": session.uri.clone() } }),
            )
            .await
            .map_err(|error| ToolError::Other(error.to_string()))?;
        let _ = managed
            .client
            .drain_notifications(Duration::from_millis(100))
            .await;
        register_client_diagnostic_updates(&session.server_name, &mut managed.client);
        let diagnostics = diagnostics_to_values(
            managed
                .client
                .get_diagnostics(&path.to_string_lossy())
                .into_iter()
                .collect(),
        );
        (symbols_result, diagnostics)
    };

    let symbols = lsp_symbols_to_values(
        path,
        &symbols_result,
        max_results,
        input.query.as_deref(),
        false,
    );
    Ok(Some(json!({
        "action": "summary",
        "path": path.to_string_lossy(),
        "server": session.server_name,
        "lsp": true,
        "diagnostics": diagnostics,
        "symbols": symbols
    })))
}

async fn lsp_diagnostics_output(
    path: &Path,
    contents: &str,
    input: &LspInput,
    context: &ToolContext,
) -> ToolResult<Option<Value>> {
    let Some(session) = start_lsp_session(path, contents, input, context).await? else {
        return Ok(None);
    };
    let diagnostics = {
        let mut clients = lsp_clients().lock().await;
        let managed = clients.get_mut(&session.key).ok_or_else(|| {
            ToolError::Other(format!(
                "LSP session '{}' was not available",
                session.server_name
            ))
        })?;
        let _ = managed
            .client
            .drain_notifications(Duration::from_millis(200))
            .await;
        register_client_diagnostic_updates(&session.server_name, &mut managed.client);
        diagnostics_to_values(
            managed
                .client
                .get_diagnostics(&path.to_string_lossy())
                .into_iter()
                .collect(),
        )
    };

    Ok(Some(json!({
        "action": "diagnostics",
        "path": path.to_string_lossy(),
        "server": session.server_name,
        "lsp": true,
        "diagnostics": diagnostics
    })))
}

async fn lsp_document_symbols_output(
    path: &Path,
    contents: &str,
    input: &LspInput,
    context: &ToolContext,
    max_results: usize,
    outline: bool,
) -> ToolResult<Option<Value>> {
    let Some(session) = start_lsp_session(path, contents, input, context).await? else {
        return Ok(None);
    };
    let result = {
        let mut clients = lsp_clients().lock().await;
        let managed = clients.get_mut(&session.key).ok_or_else(|| {
            ToolError::Other(format!(
                "LSP session '{}' was not available",
                session.server_name
            ))
        })?;
        let result = managed
            .client
            .send_request(
                "textDocument/documentSymbol",
                json!({ "textDocument": { "uri": session.uri.clone() } }),
            )
            .await
            .map_err(|error| ToolError::Other(error.to_string()))?;
        register_client_diagnostic_updates(&session.server_name, &mut managed.client);
        result
    };

    let items = lsp_symbols_to_values(path, &result, max_results, input.query.as_deref(), outline);
    if outline {
        Ok(Some(json!({
            "action": "outline",
            "path": path.to_string_lossy(),
            "server": session.server_name,
            "lsp": true,
            "outline": items
        })))
    } else {
        Ok(Some(json!({
            "action": "symbols",
            "path": path.to_string_lossy(),
            "server": session.server_name,
            "lsp": true,
            "symbols": items
        })))
    }
}

async fn lsp_hover_output(
    path: &Path,
    contents: &str,
    input: &LspInput,
    context: &ToolContext,
    position: (usize, usize),
) -> ToolResult<Option<Value>> {
    let Some(session) = start_lsp_session(path, contents, input, context).await? else {
        return Ok(None);
    };
    let result = send_lsp_session_request(
        &session,
        "textDocument/hover",
        json!({
            "textDocument": { "uri": session.uri.clone() },
            "position": {
                "line": position.0.saturating_sub(1),
                "character": position.1.saturating_sub(1)
            }
        }),
    )
    .await?;

    Ok(Some(json!({
        "action": "hover",
        "path": path.to_string_lossy(),
        "server": session.server_name,
        "lsp": true,
        "hover": hover_to_value(&result)
    })))
}

async fn lsp_workspace_symbol_output(
    path: &Path,
    contents: &str,
    input: &LspInput,
    context: &ToolContext,
    max_results: usize,
) -> ToolResult<Option<Value>> {
    let Some(session) = start_lsp_session(path, contents, input, context).await? else {
        return Ok(None);
    };
    let result = send_lsp_session_request(
        &session,
        "workspace/symbol",
        json!({ "query": input.query.as_deref().unwrap_or("") }),
    )
    .await?;
    let symbols = lsp_symbols_to_values(path, &result, max_results, input.query.as_deref(), false);

    Ok(Some(json!({
        "action": "workspace_symbol",
        "path": path.to_string_lossy(),
        "server": session.server_name,
        "lsp": true,
        "symbols": symbols
    })))
}

async fn lsp_locations_output(
    path: &Path,
    contents: &str,
    input: &LspInput,
    context: &ToolContext,
    position: (usize, usize),
    max_results: usize,
    request: LspLocationRequest,
) -> ToolResult<Option<Value>> {
    let Some(session) = start_lsp_session(path, contents, input, context).await? else {
        return Ok(None);
    };
    let method = match request {
        LspLocationRequest::Definition => "textDocument/definition",
        LspLocationRequest::References => "textDocument/references",
        LspLocationRequest::Implementation => "textDocument/implementation",
    };
    let mut params = json!({
        "textDocument": { "uri": session.uri.clone() },
        "position": {
            "line": position.0.saturating_sub(1),
            "character": position.1.saturating_sub(1)
        }
    });
    if request == LspLocationRequest::References {
        params["context"] = json!({ "includeDeclaration": true });
    }

    let result = send_lsp_session_request(&session, method, params).await?;

    let locations = lsp_locations_to_values(&result, max_results);
    let (action, key) = match request {
        LspLocationRequest::Definition => ("definition", "definitions"),
        LspLocationRequest::References => ("references", "references"),
        LspLocationRequest::Implementation => ("implementation", "implementations"),
    };
    let mut output = json!({
        "action": action,
        "path": path.to_string_lossy(),
        "server": session.server_name,
        "lsp": true
    });
    output[key] = json!(locations);
    Ok(Some(output))
}

async fn lsp_call_hierarchy_output(
    path: &Path,
    contents: &str,
    input: &LspInput,
    context: &ToolContext,
    position: (usize, usize),
    max_results: usize,
    request: LspCallHierarchyRequest,
) -> ToolResult<Option<Value>> {
    let Some(session) = start_lsp_session(path, contents, input, context).await? else {
        return Ok(None);
    };
    let prepare_result = send_lsp_session_request(
        &session,
        "textDocument/prepareCallHierarchy",
        json!({
            "textDocument": { "uri": session.uri.clone() },
            "position": {
                "line": position.0.saturating_sub(1),
                "character": position.1.saturating_sub(1)
            }
        }),
    )
    .await?;

    if request == LspCallHierarchyRequest::Prepare {
        return Ok(Some(json!({
            "action": "prepare_call_hierarchy",
            "path": path.to_string_lossy(),
            "server": session.server_name,
            "lsp": true,
            "call_hierarchy": lsp_call_hierarchy_items_to_values(&prepare_result, max_results)
        })));
    }

    let Some(item) = prepare_result
        .as_array()
        .and_then(|items| items.first())
        .cloned()
    else {
        let (action, key) = call_hierarchy_action_key(request);
        let mut output = json!({
            "action": action,
            "path": path.to_string_lossy(),
            "server": session.server_name,
            "lsp": true
        });
        output[key] = json!([]);
        return Ok(Some(output));
    };
    let method = match request {
        LspCallHierarchyRequest::Prepare => unreachable!(),
        LspCallHierarchyRequest::Incoming => "callHierarchy/incomingCalls",
        LspCallHierarchyRequest::Outgoing => "callHierarchy/outgoingCalls",
    };
    let result = send_lsp_session_request(&session, method, json!({ "item": item })).await?;
    let (action, key) = call_hierarchy_action_key(request);
    let mut output = json!({
        "action": action,
        "path": path.to_string_lossy(),
        "server": session.server_name,
        "lsp": true
    });
    output[key] = match request {
        LspCallHierarchyRequest::Incoming => {
            json!(lsp_incoming_calls_to_values(&result, max_results))
        }
        LspCallHierarchyRequest::Outgoing => {
            json!(lsp_outgoing_calls_to_values(&result, max_results))
        }
        LspCallHierarchyRequest::Prepare => json!([]),
    };
    Ok(Some(output))
}

async fn send_lsp_session_request(
    session: &ActiveLspSession,
    method: &str,
    params: Value,
) -> ToolResult<Value> {
    let mut clients = lsp_clients().lock().await;
    let managed = clients.get_mut(&session.key).ok_or_else(|| {
        ToolError::Other(format!(
            "LSP session '{}' was not available",
            session.server_name
        ))
    })?;
    let result = managed
        .client
        .send_request(method, params)
        .await
        .map_err(|error| ToolError::Other(error.to_string()))?;
    register_client_diagnostic_updates(&session.server_name, &mut managed.client);
    Ok(result)
}

async fn start_lsp_session(
    path: &Path,
    contents: &str,
    input: &LspInput,
    context: &ToolContext,
) -> ToolResult<Option<ActiveLspSession>> {
    let Some(server) = resolve_plugin_lsp_server(path, input.server.as_deref(), context)? else {
        return Ok(None);
    };
    let uri = file_uri(path)?;
    let key = lsp_client_key(&server);
    let mut clients = lsp_clients().lock().await;
    if !clients.contains_key(&key) {
        let mut client = LspClient::new(server.config.clone());
        client
            .start()
            .await
            .map_err(|error| ToolError::Other(error.to_string()))?;
        register_client_diagnostic_updates(&server.name, &mut client);
        clients.insert(
            key.clone(),
            ManagedLspClient {
                client,
                opened_files: HashMap::new(),
            },
        );
    }
    let managed = clients.get_mut(&key).ok_or_else(|| {
        ToolError::Other(format!("LSP session '{}' was not available", server.name))
    })?;
    match managed.opened_files.get(&uri) {
        None => {
            managed
                .client
                .send_notification(
                    "textDocument/didOpen",
                    json!({
                        "textDocument": {
                            "uri": uri.clone(),
                            "languageId": server.language_id,
                            "version": 1,
                            "text": contents
                        }
                    }),
                )
                .await
                .map_err(|error| ToolError::Other(error.to_string()))?;
            managed
                .opened_files
                .insert(uri.clone(), contents.to_string());
        }
        Some(existing) if existing != contents => {
            clear_delivered_lsp_diagnostics_for_file(&path.to_string_lossy());
            managed
                .client
                .send_notification(
                    "textDocument/didChange",
                    json!({
                        "textDocument": {
                            "uri": uri.clone(),
                            "version": 1
                        },
                        "contentChanges": [{ "text": contents }]
                    }),
                )
                .await
                .map_err(|error| ToolError::Other(error.to_string()))?;
            managed
                .opened_files
                .insert(uri.clone(), contents.to_string());
        }
        Some(_) => {}
    }

    Ok(Some(ActiveLspSession {
        key,
        server_name: server.name,
        uri,
    }))
}

fn lsp_client_key(server: &ResolvedPluginLspServer) -> String {
    format!(
        "{}\n{}\n{}\n{}",
        server.name,
        server.config.command,
        server.config.args.join("\u{1f}"),
        server.config.root_path
    )
}

fn resolve_plugin_lsp_server(
    path: &Path,
    requested: Option<&str>,
    context: &ToolContext,
) -> ToolResult<Option<ResolvedPluginLspServer>> {
    let mut servers = load_plugin_lsp_servers().into_iter().collect::<Vec<_>>();
    servers.sort_by(|left, right| left.0.cmp(&right.0));
    let mut matched_requested_name = false;

    for (name, config) in servers {
        if let Some(requested) = requested {
            if !lsp_server_name_matches(&name, requested) {
                continue;
            }
            matched_requested_name = true;
        }

        let Some(language_id) = language_id_for_path(&config, path) else {
            if requested.is_some() {
                return Err(ToolError::ValidationError(format!(
                    "LSP server '{}' does not support {}",
                    name,
                    path.extension()
                        .and_then(|extension| extension.to_str())
                        .map(|extension| format!(".{}", extension))
                        .unwrap_or_else(|| "files without an extension".to_string())
                )));
            }
            continue;
        };

        return resolved_plugin_lsp_server(name, config, language_id, context).map(Some);
    }

    if let Some(requested) = requested {
        if matched_requested_name {
            Ok(None)
        } else {
            Err(ToolError::ValidationError(format!(
                "LSP server '{}' was not found",
                requested
            )))
        }
    } else {
        Ok(None)
    }
}

fn resolved_plugin_lsp_server(
    name: String,
    config: Value,
    language_id: String,
    context: &ToolContext,
) -> ToolResult<ResolvedPluginLspServer> {
    let object = config.as_object().ok_or_else(|| {
        ToolError::ValidationError(format!("LSP server '{}' config must be an object", name))
    })?;
    let parsed = parse_plugin_lsp_config(&name, object, context)?;

    Ok(ResolvedPluginLspServer {
        name,
        language_id,
        config: LspServerConfig {
            command: parsed.command,
            args: parsed.args,
            root_path: parsed.root_path.to_string_lossy().to_string(),
            env: parsed.env,
            initialization_options: parsed.initialization_options,
            settings: parsed.settings,
            startup_timeout_ms: parsed.startup_timeout_ms,
            shutdown_timeout_ms: parsed.shutdown_timeout_ms,
            restart_on_crash: parsed.restart_on_crash,
            max_restarts: parsed.max_restarts,
        },
    })
}

fn parse_plugin_lsp_config(
    name: &str,
    object: &Map<String, Value>,
    context: &ToolContext,
) -> ToolResult<ParsedLspConfig> {
    let transport = object
        .get("transport")
        .and_then(Value::as_str)
        .unwrap_or("stdio");
    if transport != "stdio" {
        return Err(ToolError::ValidationError(format!(
            "LSP server '{}' uses unsupported transport '{}'",
            name, transport
        )));
    }
    let command = object
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .ok_or_else(|| {
            ToolError::ValidationError(format!("LSP server '{}' requires command", name))
        })?;
    if command.contains(' ') && !command.starts_with('/') {
        return Err(ToolError::ValidationError(format!(
            "LSP server '{}' command should not contain spaces; use args for arguments",
            name
        )));
    }
    let command = command.to_string();
    let args = object
        .get("args")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let root_path = object
        .get("workspaceFolder")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| resolve_context_path(context, value))
        .unwrap_or_else(|| PathBuf::from(&context.cwd));
    let env = object
        .get("env")
        .and_then(Value::as_object)
        .map(|env| {
            env.iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_string()))
                })
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    let startup_timeout_ms = parse_positive_u64_field(name, object, "startupTimeout")?;
    let shutdown_timeout_ms = parse_positive_u64_field(name, object, "shutdownTimeout")?;
    let restart_on_crash = parse_bool_field(name, object, "restartOnCrash")?.unwrap_or_default();
    let max_restarts = parse_nonnegative_u64_field(name, object, "maxRestarts")?;

    Ok(ParsedLspConfig {
        command,
        args,
        root_path,
        env,
        initialization_options: object
            .get("initializationOptions")
            .cloned()
            .unwrap_or_else(|| json!({})),
        settings: object
            .get("settings")
            .cloned()
            .filter(|value| !value.is_null()),
        startup_timeout_ms,
        shutdown_timeout_ms,
        restart_on_crash,
        max_restarts,
    })
}

fn parse_bool_field(
    server_name: &str,
    object: &Map<String, Value>,
    field: &str,
) -> ToolResult<Option<bool>> {
    let Some(value) = object.get(field) else {
        return Ok(None);
    };
    value.as_bool().map(Some).ok_or_else(|| {
        ToolError::ValidationError(format!(
            "LSP server '{}' field '{}' must be a boolean",
            server_name, field
        ))
    })
}

fn parse_positive_u64_field(
    server_name: &str,
    object: &Map<String, Value>,
    field: &str,
) -> ToolResult<Option<u64>> {
    let Some(value) = object.get(field) else {
        return Ok(None);
    };
    let Some(number) = value.as_u64().filter(|number| *number > 0) else {
        return Err(ToolError::ValidationError(format!(
            "LSP server '{}' field '{}' must be a positive integer",
            server_name, field
        )));
    };
    Ok(Some(number))
}

fn parse_nonnegative_u64_field(
    server_name: &str,
    object: &Map<String, Value>,
    field: &str,
) -> ToolResult<Option<u64>> {
    let Some(value) = object.get(field) else {
        return Ok(None);
    };
    value.as_u64().map(Some).ok_or_else(|| {
        ToolError::ValidationError(format!(
            "LSP server '{}' field '{}' must be a nonnegative integer",
            server_name, field
        ))
    })
}

fn resolve_context_path(context: &ToolContext, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        PathBuf::from(&context.cwd).join(path)
    }
}

fn lsp_server_name_matches(scoped_name: &str, requested: &str) -> bool {
    scoped_name == requested || scoped_name.rsplit(':').next() == Some(requested)
}

fn language_id_for_path(config: &Value, path: &Path) -> Option<String> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| format!(".{}", extension.to_lowercase()))?;
    let mapping = config.get("extensionToLanguage")?.as_object()?;
    mapping.iter().find_map(|(key, value)| {
        let normalized = if key.starts_with('.') {
            key.to_lowercase()
        } else {
            format!(".{}", key.to_lowercase())
        };
        if normalized == extension {
            value.as_str().map(str::to_string)
        } else {
            None
        }
    })
}

fn input_position(input: &LspInput) -> Option<(usize, usize)> {
    match (input.line, input.column) {
        (Some(line), Some(column)) if line > 0 && column > 0 => Some((line, column)),
        _ => None,
    }
}

fn file_uri(path: &Path) -> ToolResult<String> {
    Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| {
            ToolError::ValidationError(format!("failed to build file URI for {}", path.display()))
        })
}

fn diagnostics_to_values(diagnostics: Vec<&kiana_services::lsp::Diagnostic>) -> Vec<Value> {
    diagnostics
        .into_iter()
        .map(|diagnostic| {
            json!({
                "file": &diagnostic.file,
                "line": diagnostic.line,
                "column": diagnostic.column,
                "severity": diagnostic_severity_name(diagnostic.severity),
                "message": &diagnostic.message
            })
        })
        .collect()
}

fn diagnostic_severity_name(severity: kiana_services::lsp::DiagnosticSeverity) -> &'static str {
    match severity {
        kiana_services::lsp::DiagnosticSeverity::Error => "error",
        kiana_services::lsp::DiagnosticSeverity::Warning => "warning",
        kiana_services::lsp::DiagnosticSeverity::Info => "info",
        kiana_services::lsp::DiagnosticSeverity::Hint => "hint",
    }
}

fn lsp_symbols_to_values(
    path: &Path,
    raw: &Value,
    max_results: usize,
    query: Option<&str>,
    outline: bool,
) -> Vec<Value> {
    let query = query.map(str::to_lowercase);
    let mut output = Vec::new();
    if let Some(items) = raw.as_array() {
        for item in items {
            append_lsp_symbol(
                path,
                item,
                0,
                query.as_deref(),
                outline,
                max_results,
                &mut output,
            );
            if output.len() >= max_results {
                break;
            }
        }
    }
    output
}

fn append_lsp_symbol(
    default_path: &Path,
    item: &Value,
    level: usize,
    query: Option<&str>,
    outline: bool,
    max_results: usize,
    output: &mut Vec<Value>,
) {
    if output.len() >= max_results {
        return;
    }
    let Some(name) = item.get("name").and_then(Value::as_str) else {
        return;
    };
    let matches_query = query.map_or(true, |query| name.to_lowercase().contains(query));
    if matches_query {
        let range = item
            .get("selectionRange")
            .or_else(|| item.get("range"))
            .or_else(|| {
                item.get("location")
                    .and_then(|location| location.get("range"))
            });
        let (line, column) = lsp_range_start(range).unwrap_or((1, 1));
        let file = item
            .get("location")
            .and_then(|location| location.get("uri"))
            .and_then(Value::as_str)
            .map(file_path_from_uri)
            .unwrap_or_else(|| default_path.to_string_lossy().to_string());
        let mut value = json!({
            "file": file,
            "line": line,
            "column": column,
            "kind": lsp_symbol_kind_name(item.get("kind").and_then(Value::as_u64).unwrap_or(0)),
            "name": name,
            "text": ""
        });
        if outline {
            value["level"] = json!(level);
        }
        output.push(value);
    }

    if let Some(children) = item.get("children").and_then(Value::as_array) {
        for child in children {
            append_lsp_symbol(
                default_path,
                child,
                level + 1,
                query,
                outline,
                max_results,
                output,
            );
            if output.len() >= max_results {
                break;
            }
        }
    }
}

fn lsp_locations_to_values(raw: &Value, max_results: usize) -> Vec<Value> {
    let mut values = Vec::new();
    match raw {
        Value::Array(items) => {
            for item in items {
                if let Some(value) = lsp_location_to_value(item) {
                    values.push(value);
                }
                if values.len() >= max_results {
                    break;
                }
            }
        }
        Value::Object(_) => {
            if let Some(value) = lsp_location_to_value(raw) {
                values.push(value);
            }
        }
        _ => {}
    }
    values
}

fn hover_to_value(raw: &Value) -> Value {
    if raw.is_null() {
        return Value::Null;
    }
    let text = hover_contents_to_text(raw.get("contents").unwrap_or(raw));
    json!({
        "text": text,
        "raw": raw
    })
}

fn hover_contents_to_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(hover_contents_to_text)
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(object) => object
            .get("value")
            .and_then(Value::as_str)
            .or_else(|| object.get("contents").and_then(Value::as_str))
            .map(str::to_string)
            .unwrap_or_else(|| value.to_string()),
        _ => value.to_string(),
    }
}

fn lsp_location_to_value(item: &Value) -> Option<Value> {
    let uri = item
        .get("targetUri")
        .or_else(|| item.get("uri"))
        .and_then(Value::as_str)?;
    let range = item
        .get("targetSelectionRange")
        .or_else(|| item.get("targetRange"))
        .or_else(|| item.get("range"));
    let (line, column) = lsp_range_start(range).unwrap_or((1, 1));
    Some(json!({
        "file": file_path_from_uri(uri),
        "uri": uri,
        "line": line,
        "column": column,
        "text": ""
    }))
}

fn lsp_call_hierarchy_items_to_values(raw: &Value, max_results: usize) -> Vec<Value> {
    raw.as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(lsp_call_hierarchy_item_to_value)
                .take(max_results)
                .collect()
        })
        .unwrap_or_default()
}

fn lsp_call_hierarchy_item_to_value(item: &Value) -> Option<Value> {
    let uri = item.get("uri").and_then(Value::as_str)?;
    let range = item.get("selectionRange").or_else(|| item.get("range"));
    let (line, column) = lsp_range_start(range).unwrap_or((1, 1));
    Some(json!({
        "name": item.get("name").and_then(Value::as_str).unwrap_or(""),
        "kind": lsp_symbol_kind_name(item.get("kind").and_then(Value::as_u64).unwrap_or(0)),
        "detail": item.get("detail").and_then(Value::as_str).unwrap_or(""),
        "file": file_path_from_uri(uri),
        "uri": uri,
        "line": line,
        "column": column
    }))
}

fn lsp_incoming_calls_to_values(raw: &Value, max_results: usize) -> Vec<Value> {
    raw.as_array()
        .map(|calls| {
            calls
                .iter()
                .filter_map(|call| {
                    let from = call
                        .get("from")
                        .and_then(lsp_call_hierarchy_item_to_value)?;
                    Some(json!({
                        "from": from,
                        "from_ranges": lsp_ranges_to_values(call.get("fromRanges"))
                    }))
                })
                .take(max_results)
                .collect()
        })
        .unwrap_or_default()
}

fn lsp_outgoing_calls_to_values(raw: &Value, max_results: usize) -> Vec<Value> {
    raw.as_array()
        .map(|calls| {
            calls
                .iter()
                .filter_map(|call| {
                    let to = call.get("to").and_then(lsp_call_hierarchy_item_to_value)?;
                    Some(json!({
                        "to": to,
                        "from_ranges": lsp_ranges_to_values(call.get("fromRanges"))
                    }))
                })
                .take(max_results)
                .collect()
        })
        .unwrap_or_default()
}

fn lsp_ranges_to_values(raw: Option<&Value>) -> Vec<Value> {
    raw.and_then(Value::as_array)
        .map(|ranges| ranges.iter().filter_map(lsp_range_to_value).collect())
        .unwrap_or_default()
}

fn lsp_range_to_value(range: &Value) -> Option<Value> {
    let (line, column) = lsp_range_start(Some(range))?;
    Some(json!({
        "line": line,
        "column": column
    }))
}

fn call_hierarchy_action_key(request: LspCallHierarchyRequest) -> (&'static str, &'static str) {
    match request {
        LspCallHierarchyRequest::Prepare => ("prepare_call_hierarchy", "call_hierarchy"),
        LspCallHierarchyRequest::Incoming => ("incoming_calls", "incoming_calls"),
        LspCallHierarchyRequest::Outgoing => ("outgoing_calls", "outgoing_calls"),
    }
}

fn lsp_range_start(range: Option<&Value>) -> Option<(u64, u64)> {
    let start = range?.get("start")?;
    Some((
        start.get("line").and_then(Value::as_u64).unwrap_or(0) + 1,
        start.get("character").and_then(Value::as_u64).unwrap_or(0) + 1,
    ))
}

fn file_path_from_uri(uri: &str) -> String {
    Url::parse(uri)
        .ok()
        .and_then(|url| url.to_file_path().ok())
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| uri.to_string())
}

fn lsp_symbol_kind_name(kind: u64) -> &'static str {
    match kind {
        1 => "file",
        2 => "module",
        3 => "namespace",
        4 => "package",
        5 => "class",
        6 => "method",
        7 => "property",
        8 => "field",
        9 => "constructor",
        10 => "enum",
        11 => "interface",
        12 => "function",
        13 => "variable",
        14 => "constant",
        15 => "string",
        16 => "number",
        17 => "boolean",
        18 => "array",
        19 => "object",
        20 => "key",
        21 => "null",
        22 => "enum_member",
        23 => "struct",
        24 => "event",
        25 => "operator",
        26 => "type_parameter",
        _ => "symbol",
    }
}

fn load_plugin_lsp_servers() -> Map<String, Value> {
    let mut all_servers = Map::new();
    for plugin_root in kiana_types::plugin::installed_plugin_roots() {
        let Some(manifest_path) = kiana_types::plugin::find_manifest_path(&plugin_root) else {
            continue;
        };
        let manifest = read_json_file(&manifest_path);
        let plugin_name = manifest
            .as_ref()
            .and_then(plugin_name_from_manifest)
            .or_else(|| {
                plugin_root
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "plugin".to_string());
        let mut plugin_servers = Map::new();
        merge_plugin_lsp_file(&mut plugin_servers, &plugin_root, ".lsp.json");
        if let Some(manifest) = manifest.as_ref() {
            merge_manifest_lsp_servers(&mut plugin_servers, &plugin_root, manifest);
        }
        for (name, config) in plugin_servers {
            if let Some((scoped_name, config)) =
                scoped_plugin_lsp_config(&name, config, &plugin_root, &plugin_name)
            {
                all_servers.insert(scoped_name, config);
            }
        }
    }
    all_servers
}

fn merge_manifest_lsp_servers(
    servers: &mut Map<String, Value>,
    plugin_root: &Path,
    manifest: &Value,
) {
    let Some(spec) = manifest.get("lspServers") else {
        return;
    };
    match spec {
        Value::String(path) => merge_safe_plugin_lsp_file(servers, plugin_root, path),
        Value::Array(items) => {
            for item in items {
                match item {
                    Value::String(path) => merge_safe_plugin_lsp_file(servers, plugin_root, path),
                    Value::Object(_) => merge_lsp_servers_value(servers, item),
                    _ => {}
                }
            }
        }
        Value::Object(_) => merge_lsp_servers_value(servers, spec),
        _ => {}
    }
}

fn merge_safe_plugin_lsp_file(servers: &mut Map<String, Value>, plugin_root: &Path, path: &str) {
    let Some(path) = safe_relative_plugin_path(path) else {
        return;
    };
    merge_plugin_lsp_file(servers, plugin_root, path);
}

fn merge_plugin_lsp_file(
    servers: &mut Map<String, Value>,
    plugin_root: &Path,
    relative_path: impl AsRef<Path>,
) {
    let path = plugin_root.join(relative_path);
    let Some(value) = read_json_file(&path) else {
        return;
    };
    merge_lsp_servers_value(servers, &value);
}

fn merge_lsp_servers_value(target: &mut Map<String, Value>, value: &Value) {
    let servers = value.get("lspServers").unwrap_or(value);
    let Value::Object(object) = servers else {
        return;
    };
    for (name, config) in object {
        if !name.trim().is_empty() {
            target.insert(name.clone(), config.clone());
        }
    }
}

fn scoped_plugin_lsp_config(
    server_name: &str,
    config: Value,
    plugin_root: &Path,
    plugin_name: &str,
) -> Option<(String, Value)> {
    let server_name = server_name.trim();
    if server_name.is_empty() {
        return None;
    }

    let mut config = substitute_plugin_lsp_variables(&config, plugin_root);
    let object = config.as_object_mut()?;
    let has_command = object
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|command| !command.is_empty());
    if !has_command {
        return None;
    }

    object
        .entry("transport".to_string())
        .or_insert_with(|| json!("stdio"));
    let plugin_root_string = plugin_root.to_string_lossy().to_string();
    let mut env = object
        .get("env")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    env.entry("CLAUDE_PLUGIN_ROOT".to_string())
        .or_insert_with(|| json!(plugin_root_string.clone()));
    env.entry("KIANA_PLUGIN_ROOT".to_string())
        .or_insert_with(|| json!(plugin_root_string));
    object.insert("env".to_string(), Value::Object(env));
    object.insert("scope".to_string(), json!("dynamic"));
    object.insert("source".to_string(), json!(plugin_name));

    Some((format!("plugin:{}:{}", plugin_name, server_name), config))
}

fn substitute_plugin_lsp_variables(value: &Value, plugin_root: &Path) -> Value {
    match value {
        Value::String(value) => {
            let plugin_root = plugin_root.to_string_lossy();
            Value::String(
                value
                    .replace("${CLAUDE_PLUGIN_ROOT}", &plugin_root)
                    .replace("${KIANA_PLUGIN_ROOT}", &plugin_root),
            )
        }
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| substitute_plugin_lsp_variables(value, plugin_root))
                .collect(),
        ),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        substitute_plugin_lsp_variables(value, plugin_root),
                    )
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn plugin_name_from_manifest(manifest: &Value) -> Option<String> {
    manifest
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn safe_relative_plugin_path(path: &str) -> Option<PathBuf> {
    let path = PathBuf::from(path);
    if path.as_os_str().is_empty()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return None;
    }
    Some(path)
}

fn read_json_file(path: &Path) -> Option<Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
}

fn read_required_source_file(path: &Path, action: LspAction) -> ToolResult<String> {
    if !path.is_file() {
        return Err(ToolError::ValidationError(format!(
            "{:?} requires a source file path, got {}",
            action,
            path.display()
        )));
    }
    Ok(fs::read_to_string(path)?)
}

fn target_symbol(input: &LspInput) -> Option<String> {
    input
        .symbol
        .as_deref()
        .or(input.query.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn collect_diagnostics(contents: &str) -> Vec<Value> {
    contents
        .lines()
        .enumerate()
        .filter_map(|(idx, line)| {
            let lower = line.to_lowercase();
            if lower.contains("todo") || lower.contains("fixme") {
                Some(json!({
                    "line": idx + 1,
                    "severity": "hint",
                    "message": line.trim()
                }))
            } else {
                None
            }
        })
        .collect()
}

fn collect_symbols(contents: &str, query: Option<&str>, max_results: usize) -> Vec<SymbolEntry> {
    let query = query.map(str::to_lowercase);
    let mut symbols = Vec::new();

    for (idx, line) in contents.lines().enumerate() {
        for (kind, name) in symbols_from_line(line) {
            if query
                .as_ref()
                .is_some_and(|query| !name.to_lowercase().contains(query))
            {
                continue;
            }
            let column = line.find(&name).map(|value| value + 1).unwrap_or(1);
            symbols.push(SymbolEntry {
                line: idx + 1,
                column,
                kind,
                name,
                text: line.trim().to_string(),
                indent: line.chars().take_while(|ch| ch.is_whitespace()).count(),
            });
            if symbols.len() >= max_results {
                return symbols;
            }
        }
    }

    symbols
}

fn collect_workspace_symbols(
    path: &Path,
    roots: &[PathBuf],
    query: Option<&str>,
    max_results: usize,
) -> ToolResult<Vec<Value>> {
    let mut symbols = Vec::new();
    for root in scan_roots(path, roots) {
        for source_path in source_files_under(&root) {
            let Ok(contents) = fs::read_to_string(&source_path) else {
                continue;
            };
            for symbol in
                collect_symbols(&contents, query, max_results.saturating_sub(symbols.len()))
            {
                symbols.push(symbol_to_value(&source_path, symbol));
                if symbols.len() >= max_results {
                    return Ok(symbols);
                }
            }
        }
    }
    Ok(symbols)
}

fn symbols_from_line(line: &str) -> Vec<(String, String)> {
    let patterns: [(&str, &str, usize); 11] = [
        (
            "function",
            r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\b",
            1,
        ),
        (
            "type",
            r"^\s*(?:pub(?:\([^)]*\))?\s+)?(struct|enum|trait|mod|type|const|static)\s+([A-Za-z_][A-Za-z0-9_]*)\b",
            2,
        ),
        (
            "impl",
            r"^\s*impl(?:\s*<[^>]+>)?\s+(?:(?:[A-Za-z_][A-Za-z0-9_:<>]*)\s+for\s+)?([A-Za-z_][A-Za-z0-9_:<>]*)",
            1,
        ),
        (
            "function",
            r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s+([A-Za-z_$][A-Za-z0-9_$]*)\b",
            1,
        ),
        (
            "type",
            r"^\s*(?:export\s+)?(?:default\s+)?(class|interface|enum|type)\s+([A-Za-z_$][A-Za-z0-9_$]*)\b",
            2,
        ),
        (
            "variable",
            r"^\s*(?:export\s+)?(?:const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*(?:[:=]|$)",
            1,
        ),
        (
            "function",
            r"^\s*(?:async\s+)?def\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(",
            1,
        ),
        ("class", r"^\s*class\s+([A-Za-z_][A-Za-z0-9_]*)\b", 1),
        (
            "function",
            r"^\s*func\s+(?:\([^)]*\)\s*)?([A-Za-z_][A-Za-z0-9_]*)\s*\(",
            1,
        ),
        (
            "type",
            r"^\s*type\s+([A-Za-z_][A-Za-z0-9_]*)\s+(?:struct|interface|=)?",
            1,
        ),
        (
            "type",
            r"^\s*(?:(?:public|private|protected|static|final|abstract)\s+)*(class|interface|enum)\s+([A-Za-z_][A-Za-z0-9_]*)\b",
            2,
        ),
    ];

    let mut result = Vec::new();
    for (default_kind, pattern, name_capture) in patterns {
        let Ok(re) = Regex::new(pattern) else {
            continue;
        };
        let Some(capture) = re.captures(line) else {
            continue;
        };
        let kind = if name_capture == 2 {
            capture
                .get(1)
                .map(|value| value.as_str())
                .unwrap_or(default_kind)
        } else {
            default_kind
        };
        let Some(name) = capture.get(name_capture).map(|value| value.as_str()) else {
            continue;
        };
        result.push((kind.to_string(), trim_symbol_name(name)));
    }
    result
}

fn trim_symbol_name(name: &str) -> String {
    name.trim_matches(|ch: char| matches!(ch, ':' | '<' | '>' | ',' | '&' | '*'))
        .rsplit("::")
        .next()
        .unwrap_or(name)
        .to_string()
}

fn symbol_to_value(path: &Path, symbol: SymbolEntry) -> Value {
    json!({
        "file": path.to_string_lossy(),
        "line": symbol.line,
        "column": symbol.column,
        "kind": symbol.kind,
        "name": symbol.name,
        "text": symbol.text
    })
}

fn outline_to_value(path: &Path, symbol: SymbolEntry) -> Value {
    json!({
        "file": path.to_string_lossy(),
        "line": symbol.line,
        "column": symbol.column,
        "kind": symbol.kind,
        "name": symbol.name,
        "level": symbol.indent / 4,
        "text": symbol.text
    })
}

fn collect_definitions(
    path: &Path,
    roots: &[PathBuf],
    symbol: &str,
    max_results: usize,
) -> ToolResult<Vec<Value>> {
    let mut definitions = Vec::new();
    let mut seen = HashSet::new();

    if path.is_file() {
        let contents = fs::read_to_string(path)?;
        append_definitions_from_file(
            path,
            &contents,
            symbol,
            max_results,
            &mut seen,
            &mut definitions,
        );
    }

    if definitions.len() >= max_results {
        return Ok(definitions);
    }

    for root in scan_roots(path, roots) {
        for source_path in source_files_under(&root) {
            if path.is_file() && same_path(&source_path, path) {
                continue;
            }
            let Ok(contents) = fs::read_to_string(&source_path) else {
                continue;
            };
            append_definitions_from_file(
                &source_path,
                &contents,
                symbol,
                max_results,
                &mut seen,
                &mut definitions,
            );
            if definitions.len() >= max_results {
                return Ok(definitions);
            }
        }
    }

    Ok(definitions)
}

fn append_definitions_from_file(
    path: &Path,
    contents: &str,
    symbol: &str,
    max_results: usize,
    seen: &mut HashSet<String>,
    definitions: &mut Vec<Value>,
) {
    for entry in collect_symbols(contents, None, usize::MAX) {
        if !symbol_name_matches(&entry.name, symbol) {
            continue;
        }
        let key = format!("{}:{}", path.display(), entry.line);
        if seen.insert(key) {
            definitions.push(symbol_to_value(path, entry));
        }
        if definitions.len() >= max_results {
            break;
        }
    }
}

fn collect_references(
    path: &Path,
    roots: &[PathBuf],
    symbol: &str,
    max_results: usize,
) -> ToolResult<Vec<Value>> {
    let re = reference_regex(symbol)?;
    let mut references = Vec::new();
    let mut seen = HashSet::new();

    if path.is_file() {
        let contents = fs::read_to_string(path)?;
        append_references_from_file(
            path,
            &contents,
            symbol,
            &re,
            max_results,
            &mut seen,
            &mut references,
        );
    }

    if references.len() >= max_results {
        return Ok(references);
    }

    for root in scan_roots(path, roots) {
        for source_path in source_files_under(&root) {
            if path.is_file() && same_path(&source_path, path) {
                continue;
            }
            let Ok(contents) = fs::read_to_string(&source_path) else {
                continue;
            };
            append_references_from_file(
                &source_path,
                &contents,
                symbol,
                &re,
                max_results,
                &mut seen,
                &mut references,
            );
            if references.len() >= max_results {
                return Ok(references);
            }
        }
    }

    Ok(references)
}

fn append_references_from_file(
    path: &Path,
    contents: &str,
    symbol: &str,
    re: &Regex,
    max_results: usize,
    seen: &mut HashSet<String>,
    references: &mut Vec<Value>,
) {
    let definition_lines = collect_symbols(contents, None, usize::MAX)
        .into_iter()
        .filter(|entry| symbol_name_matches(&entry.name, symbol))
        .map(|entry| entry.line)
        .collect::<HashSet<_>>();

    for (line_idx, line) in contents.lines().enumerate() {
        let line_number = line_idx + 1;
        for capture in re.captures_iter(line) {
            let Some(matched) = capture.get(2) else {
                continue;
            };
            let key = format!("{}:{}:{}", path.display(), line_number, matched.start() + 1);
            if !seen.insert(key) {
                continue;
            }
            references.push(json!({
                "file": path.to_string_lossy(),
                "line": line_number,
                "column": matched.start() + 1,
                "symbol": symbol,
                "is_definition": definition_lines.contains(&line_number),
                "text": line.trim()
            }));
            if references.len() >= max_results {
                return;
            }
        }
    }
}

fn reference_regex(symbol: &str) -> ToolResult<Regex> {
    Regex::new(&format!(
        r"(^|[^A-Za-z0-9_$])({})([^A-Za-z0-9_$]|$)",
        regex::escape(symbol)
    ))
    .map_err(|error| ToolError::Other(format!("failed to build reference regex: {}", error)))
}

fn scan_roots(path: &Path, roots: &[PathBuf]) -> Vec<PathBuf> {
    if path.is_dir() {
        return vec![path.to_path_buf()];
    }
    roots.to_vec()
}

fn source_files_under(root: &Path) -> Vec<PathBuf> {
    if root.is_file() {
        return vec![root.to_path_buf()];
    }

    WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| entry.depth() == 0 || !is_ignored_dir(entry.path()))
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| is_source_file(path))
        .collect()
}

fn is_ignored_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    matches!(
        name,
        ".git"
            | ".hg"
            | ".svn"
            | "target"
            | "node_modules"
            | "dist"
            | "build"
            | ".next"
            | "coverage"
            | ".venv"
            | "venv"
            | "__pycache__"
    )
}

fn is_source_file(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|value| value.to_str()) else {
        return false;
    };
    matches!(
        ext,
        "rs" | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "py"
            | "go"
            | "java"
            | "c"
            | "h"
            | "cc"
            | "cpp"
            | "hpp"
            | "cs"
            | "kt"
            | "swift"
            | "rb"
            | "php"
    )
}

fn symbol_name_matches(name: &str, symbol: &str) -> bool {
    name == symbol
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[cfg(test)]
mod tests {
    use super::LspTool;
    use crate::file_write::FileWriteTool;
    use crate::tool::ACCESS_ROOTS_APP_STATE_KEY;
    use crate::{Tool, ToolContext};
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command as StdCommand;
    use uuid::Uuid;

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("kiana-lsp-{label}-{}", Uuid::new_v4()))
    }

    fn write_plugin_manifest(plugin_root: &Path, name: &str, extra: Value) {
        fs::create_dir_all(plugin_root.join(".codex-plugin")).unwrap();
        let mut manifest = serde_json::Map::new();
        manifest.insert("name".to_string(), json!(name));
        if let Some(extra) = extra.as_object() {
            for (key, value) in extra {
                manifest.insert(key.clone(), value.clone());
            }
        }
        fs::write(
            plugin_root.join(".codex-plugin").join("plugin.json"),
            Value::Object(manifest).to_string(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn returns_symbols_and_todo_diagnostics() {
        let root = temp_root("symbols");
        fs::create_dir_all(&root).unwrap();
        let source_path = root.join("lib.rs");
        fs::write(
            &source_path,
            "struct Worker {}\n// todo: wire richer diagnostics\nfn run_worker() {}\n",
        )
        .unwrap();

        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: root.to_string_lossy().to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        };
        let output = LspTool::new()
            .call(
                &json!({ "path": "lib.rs", "query": "worker" }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(output.data["diagnostics"].as_array().unwrap().len(), 1);
        assert_eq!(output.data["symbols"].as_array().unwrap().len(), 2);

        let api_result = LspTool::new().map_to_api_result(&output, "toolu_lsp");
        assert_eq!(api_result["type"], "tool_result");
        assert_eq!(api_result["tool_use_id"], "toolu_lsp");
        let content = api_result["content"].as_str().unwrap();
        assert!(content.contains("LSP summary result"));
        assert!(content.contains("Diagnostics: 1"));
        assert!(content.contains("Symbols: 2"));
        assert!(content.contains("Worker"));

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn returns_definition_and_references_across_workspace() {
        let root = temp_root("workspace");
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("lib.rs"),
            "mod worker;\nuse worker::Worker;\nfn main() { let _worker = Worker::new(); }\n",
        )
        .unwrap();
        fs::write(
            src.join("worker.rs"),
            "pub struct Worker {}\nimpl Worker { pub fn new() -> Self { Self {} } }\n",
        )
        .unwrap();

        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: root.to_string_lossy().to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        };
        let tool = LspTool::new();

        let definitions = tool
            .call(
                &json!({
                    "path": "src/lib.rs",
                    "action": "definition",
                    "symbol": "Worker"
                }),
                &mut context,
            )
            .await
            .unwrap();
        let definition_items = definitions.data["definitions"].as_array().unwrap();
        assert!(definition_items.iter().any(|item| {
            item["file"].as_str().unwrap().ends_with("worker.rs")
                && item["kind"] == json!("struct")
                && item["name"] == json!("Worker")
        }));

        let references = tool
            .call(
                &json!({
                    "path": "src/lib.rs",
                    "action": "references",
                    "symbol": "Worker",
                    "max_results": 10
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert!(references.data["references"].as_array().unwrap().len() >= 3);

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn definitions_scan_added_access_roots() {
        let root = temp_root("root");
        let extra = temp_root("extra");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&extra).unwrap();
        fs::write(
            root.join("main.rs"),
            "fn main() { let _ = Worker::new(); }\n",
        )
        .unwrap();
        fs::write(
            extra.join("worker.rs"),
            "pub struct Worker {}\nimpl Worker { pub fn new() -> Self { Self {} } }\n",
        )
        .unwrap();

        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: root.to_string_lossy().to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::from([(
                ACCESS_ROOTS_APP_STATE_KEY.to_string(),
                json!([extra.to_string_lossy()]),
            )]),
            abort_signal: abort_rx,
        };

        let definitions = LspTool::new()
            .call(
                &json!({
                    "path": "main.rs",
                    "action": "definition",
                    "symbol": "Worker"
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert!(definitions.data["definitions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["file"].as_str().unwrap().ends_with("worker.rs")));

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&extra);
    }

    #[tokio::test]
    async fn returns_enabled_plugin_lsp_servers_with_reference_priority() {
        let _guard = crate::test_support::lock_env();
        let root = temp_root("plugin-priority");
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("review-lsp");
        fs::create_dir_all(plugin_root.join("extra")).unwrap();
        fs::create_dir_all(plugin_root.join("bin")).unwrap();
        write_plugin_manifest(
            &plugin_root,
            "review-lsp",
            json!({
                "lspServers": [
                    "extra/lsp.json",
                    {
                        "inline-server": {
                            "command": "${CLAUDE_PLUGIN_ROOT}/bin/inline-lsp",
                            "args": ["--root", "${KIANA_PLUGIN_ROOT}"],
                            "extensionToLanguage": { ".rs": "rust" },
                            "workspaceFolder": "${CLAUDE_PLUGIN_ROOT}/workspace"
                        },
                        "overridden": {
                            "command": "manifest-lsp",
                            "extensionToLanguage": { ".ts": "typescript" }
                        }
                    },
                    "../outside.json"
                ]
            }),
        );
        fs::write(
            plugin_root.join(".lsp.json"),
            json!({
                "default-server": {
                    "command": "default-lsp",
                    "extensionToLanguage": { ".py": "python" }
                },
                "overridden": {
                    "command": "default-lsp",
                    "extensionToLanguage": { ".js": "javascript" }
                }
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            plugin_root.join("extra").join("lsp.json"),
            json!({
                "file-server": {
                    "command": "file-lsp",
                    "transport": "socket",
                    "extensionToLanguage": { ".go": "go" }
                }
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            plugins_dir.join("outside.json"),
            json!({
                "outside-server": {
                    "command": "outside-lsp",
                    "extensionToLanguage": { ".rs": "rust" }
                }
            })
            .to_string(),
        )
        .unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: root.to_string_lossy().to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        };

        let output = LspTool::new()
            .call(&json!({ "path": ".", "action": "servers" }), &mut context)
            .await
            .unwrap();
        let servers = output.data["servers"].as_object().unwrap();
        assert!(servers.contains_key("plugin:review-lsp:default-server"));
        assert!(servers.contains_key("plugin:review-lsp:file-server"));
        assert!(servers.contains_key("plugin:review-lsp:inline-server"));
        assert!(!servers.contains_key("plugin:review-lsp:outside-server"));
        assert_eq!(
            servers["plugin:review-lsp:overridden"]["command"],
            json!("manifest-lsp")
        );

        let inline = &servers["plugin:review-lsp:inline-server"];
        assert_eq!(
            Path::new(inline["command"].as_str().unwrap()),
            plugin_root.join("bin").join("inline-lsp")
        );
        assert_eq!(inline["args"][1], json!(plugin_root.to_string_lossy()));
        assert_eq!(
            Path::new(inline["workspaceFolder"].as_str().unwrap()),
            plugin_root.join("workspace")
        );
        assert_eq!(
            inline["env"]["CLAUDE_PLUGIN_ROOT"],
            json!(plugin_root.to_string_lossy())
        );
        assert_eq!(
            inline["env"]["KIANA_PLUGIN_ROOT"],
            json!(plugin_root.to_string_lossy())
        );
        assert_eq!(inline["scope"], json!("dynamic"));
        assert_eq!(inline["source"], json!("review-lsp"));
        assert_eq!(inline["transport"], json!("stdio"));

        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn lsp_tool_uses_plugin_server_for_symbols_diagnostics_and_locations() {
        let _guard = crate::test_support::lock_env();
        super::shutdown_lsp_clients().await;
        super::reset_all_lsp_diagnostic_state();
        let root = temp_root("plugin-runtime");
        let project = root.join("project");
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("mock-lsp");
        let lsp_log = root.join("lsp-methods.log");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&plugin_root).unwrap();
        let source_path = project.join("lib.rs");
        fs::write(
            &source_path,
            "// source intentionally has no static symbols\n",
        )
        .unwrap();
        let script = write_mock_lsp_server();
        write_plugin_manifest(&plugin_root, "mock-lsp", json!({}));
        fs::write(
            plugin_root.join(".lsp.json"),
            json!({
                "mock": {
                    "command": mock_lsp_python_command(),
                    "args": ["-u", script.to_string_lossy()],
                    "env": { "KIANA_LSP_TEST_LOG": lsp_log.to_string_lossy() },
                    "initializationOptions": { "probe": "tool" },
                    "settings": { "mock": { "strict": true } },
                    "startupTimeout": 1000,
                    "extensionToLanguage": { ".rs": "rust" },
                    "workspaceFolder": project.to_string_lossy()
                }
            })
            .to_string(),
        )
        .unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: project.to_string_lossy().to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        };
        let tool = LspTool::new();

        let symbols = tool
            .call(
                &json!({ "path": "lib.rs", "action": "symbols" }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(symbols.data["lsp"], json!(true));
        assert_eq!(symbols.data["server"], json!("plugin:mock-lsp:mock"));
        assert_eq!(symbols.data["symbols"][0]["name"], json!("FromLsp"));
        assert_eq!(symbols.data["symbols"][0]["kind"], json!("function"));

        let status = super::lsp_runtime_status().await;
        assert_eq!(status["active_clients"], json!(1));
        assert_eq!(status["open_files"], json!(1));
        assert_eq!(status["pending_diagnostics"], json!(1));

        let pending = tool
            .call(
                &json!({ "path": ".", "action": "pending_diagnostics" }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(pending.data["count"], json!(1));
        assert_eq!(
            pending.data["batches"][0]["server"],
            json!("plugin:mock-lsp:mock")
        );
        assert_eq!(
            pending.data["batches"][0]["files"][0]["diagnostics"][0]["message"],
            json!("diagnostic from lsp")
        );

        let repeated = tool
            .call(
                &json!({ "path": ".", "action": "pending_diagnostics" }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(repeated.data["count"], json!(0));

        let diagnostics = tool
            .call(
                &json!({ "path": "lib.rs", "action": "diagnostics" }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(diagnostics.data["lsp"], json!(true));
        assert_eq!(
            diagnostics.data["diagnostics"][0]["message"],
            json!("diagnostic from lsp")
        );

        let definition = tool
            .call(
                &json!({
                    "path": "lib.rs",
                    "action": "definition",
                    "line": 1,
                    "column": 4
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(definition.data["lsp"], json!(true));
        assert_eq!(definition.data["definitions"][0]["line"], json!(2));
        assert_eq!(definition.data["definitions"][0]["column"], json!(3));

        let hover = tool
            .call(
                &json!({
                    "operation": "hover",
                    "filePath": "lib.rs",
                    "line": 1,
                    "character": 4
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(hover.data["lsp"], json!(true));
        assert!(hover.data["hover"]["text"]
            .as_str()
            .unwrap()
            .contains("hover from lsp"));

        let workspace_symbols = tool
            .call(
                &json!({
                    "path": "lib.rs",
                    "action": "workspace_symbol",
                    "query": "Workspace"
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(workspace_symbols.data["lsp"], json!(true));
        assert_eq!(
            workspace_symbols.data["symbols"][0]["name"],
            json!("FromWorkspace")
        );

        let implementation = tool
            .call(
                &json!({
                    "operation": "goToImplementation",
                    "filePath": "lib.rs",
                    "line": 1,
                    "character": 4
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(implementation.data["lsp"], json!(true));
        assert_eq!(implementation.data["implementations"][0]["line"], json!(4));

        let call_hierarchy = tool
            .call(
                &json!({
                    "path": "lib.rs",
                    "action": "prepare_call_hierarchy",
                    "line": 1,
                    "column": 4
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(call_hierarchy.data["lsp"], json!(true));
        assert_eq!(
            call_hierarchy.data["call_hierarchy"][0]["name"],
            json!("FromLsp")
        );

        let incoming = tool
            .call(
                &json!({
                    "operation": "incomingCalls",
                    "filePath": "lib.rs",
                    "line": 1,
                    "character": 4
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(
            incoming.data["incoming_calls"][0]["from"]["name"],
            json!("Caller")
        );

        let outgoing = tool
            .call(
                &json!({
                    "operation": "outgoingCalls",
                    "filePath": "lib.rs",
                    "line": 1,
                    "character": 4
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(
            outgoing.data["outgoing_calls"][0]["to"]["name"],
            json!("Callee")
        );

        let write = FileWriteTool::new()
            .call(
                &json!({
                    "file_path": "lib.rs",
                    "content": "// changed through Write\n"
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert!(write
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("lsp_warning"))
            .is_none());
        let method_log = fs::read_to_string(&lsp_log).unwrap();
        assert!(method_log.contains("textDocument/didChange"));
        assert!(method_log.contains("textDocument/didSave"));
        assert!(method_log.contains("\"probe\": \"tool\""));
        assert!(method_log.contains("\"strict\": true"));

        super::shutdown_lsp_clients().await;
        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_file(script);
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn lsp_runtime_accepts_manifest_lifecycle_fields() {
        let _guard = crate::test_support::lock_env();
        super::shutdown_lsp_clients().await;
        super::reset_all_lsp_diagnostic_state();
        let root = temp_root("plugin-lifecycle-fields");
        let project = root.join("project");
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("lifecycle-lsp");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&plugin_root).unwrap();
        let source = project.join("lib.rs");
        fs::write(&source, "fn main() {}\n").unwrap();
        let script = write_mock_lsp_server();
        write_plugin_manifest(&plugin_root, "lifecycle-lsp", json!({}));
        fs::write(
            plugin_root.join(".lsp.json"),
            json!({
                "mock": {
                    "command": mock_lsp_python_command(),
                    "args": ["-u", script.to_string_lossy()],
                    "restartOnCrash": true,
                    "shutdownTimeout": 1000,
                    "maxRestarts": 1,
                    "extensionToLanguage": { ".rs": "rust" },
                    "workspaceFolder": project.to_string_lossy()
                }
            })
            .to_string(),
        )
        .unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: project.to_string_lossy().to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        };

        let output = LspTool::new()
            .call(
                &json!({ "path": "lib.rs", "action": "symbols" }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(output.data["lsp"], json!(true));
        assert_eq!(output.data["server"], json!("plugin:lifecycle-lsp:mock"));
        assert_eq!(output.data["symbols"][0]["name"], json!("FromLsp"));

        super::shutdown_lsp_clients().await;
        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_file(script);
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn disabled_plugin_lsp_servers_are_not_returned() {
        let _guard = crate::test_support::lock_env();
        let root = temp_root("plugin-disabled");
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("review-lsp");
        write_plugin_manifest(&plugin_root, "review-lsp", json!({}));
        fs::write(
            plugin_root.join(".lsp.json"),
            json!({
                "review-server": {
                    "command": "review-lsp",
                    "extensionToLanguage": { ".rs": "rust" }
                }
            })
            .to_string(),
        )
        .unwrap();
        kiana_types::plugin::set_plugin_enabled(&plugins_dir, "review-lsp", false).unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: root.to_string_lossy().to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        };
        let output = LspTool::new()
            .call(&json!({ "path": ".", "action": "servers" }), &mut context)
            .await
            .unwrap();

        assert_eq!(output.data["count"], json!(0));
        assert!(output.data["servers"].as_object().unwrap().is_empty());

        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }

    fn write_mock_lsp_server() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "kiana-tool-mock-lsp-{}-{}.py",
            std::process::id(),
            Uuid::new_v4()
        ));
        fs::write(
            &path,
            r#"
import json
import os
import sys

opened_uri = None
method_log = os.environ.get("KIANA_LSP_TEST_LOG")

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

def log_method(method):
    if method_log:
        with open(method_log, "a", encoding="utf-8") as fh:
            fh.write(method + "\n")

def log_event(kind, payload):
    if method_log:
        with open(method_log, "a", encoding="utf-8") as fh:
            fh.write(kind + ":" + json.dumps(payload, sort_keys=True) + "\n")

def call_item(name, uri, line):
    return {
        "name": name,
        "kind": 12,
        "uri": uri,
        "range": {
            "start": {"line": line, "character": 0},
            "end": {"line": line, "character": 10}
        },
        "selectionRange": {
            "start": {"line": line, "character": 1},
            "end": {"line": line, "character": 8}
        }
    }

while True:
    msg = read_message()
    if msg is None:
        break
    method = msg.get("method")
    log_method(method or "")
    if method == "initialize":
        log_event("initializationOptions", msg.get("params", {}).get("initializationOptions"))
        write_message({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {
                "capabilities": {
                    "textDocumentSync": 1,
                    "definitionProvider": True,
                    "referencesProvider": True,
                    "documentSymbolProvider": True,
                    "hoverProvider": True,
                    "workspaceSymbolProvider": True,
                    "implementationProvider": True,
                    "callHierarchyProvider": True
                }
            }
        })
    elif method == "initialized":
        continue
    elif method == "workspace/didChangeConfiguration":
        log_event("settings", msg.get("params", {}).get("settings"))
        continue
    elif method == "textDocument/didOpen":
        opened_uri = msg["params"]["textDocument"]["uri"]
        write_message({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": opened_uri,
                "diagnostics": [{
                    "range": {
                        "start": {"line": 0, "character": 1},
                        "end": {"line": 0, "character": 5}
                    },
                    "severity": 2,
                    "message": "diagnostic from lsp"
                }]
            }
        })
    elif method == "textDocument/didChange":
        opened_uri = msg["params"]["textDocument"]["uri"]
    elif method == "textDocument/didSave":
        opened_uri = msg["params"]["textDocument"]["uri"]
        write_message({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": opened_uri,
                "diagnostics": []
            }
        })
    elif method == "textDocument/documentSymbol":
        write_message({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": [{
                "name": "FromLsp",
                "kind": 12,
                "range": {
                    "start": {"line": 0, "character": 0},
                    "end": {"line": 0, "character": 10}
                },
                "selectionRange": {
                    "start": {"line": 0, "character": 2},
                    "end": {"line": 0, "character": 9}
                }
            }]
        })
    elif method == "textDocument/hover":
        write_message({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {
                "contents": {
                    "kind": "markdown",
                    "value": "`FromLsp` hover from lsp"
                }
            }
        })
    elif method == "workspace/symbol":
        uri = opened_uri or "file:///tmp/lib.rs"
        write_message({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": [{
                "name": "FromWorkspace",
                "kind": 12,
                "location": {
                    "uri": uri,
                    "range": {
                        "start": {"line": 0, "character": 2},
                        "end": {"line": 0, "character": 9}
                    }
                }
            }]
        })
    elif method == "textDocument/definition":
        write_message({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {
                "uri": opened_uri or msg["params"]["textDocument"]["uri"],
                "range": {
                    "start": {"line": 1, "character": 2},
                    "end": {"line": 1, "character": 9}
                }
            }
        })
    elif method == "textDocument/references":
        write_message({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": [{
                "uri": opened_uri or msg["params"]["textDocument"]["uri"],
                "range": {
                    "start": {"line": 2, "character": 4},
                    "end": {"line": 2, "character": 11}
                }
            }]
        })
    elif method == "textDocument/implementation":
        write_message({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": [{
                "uri": opened_uri or msg["params"]["textDocument"]["uri"],
                "range": {
                    "start": {"line": 3, "character": 0},
                    "end": {"line": 3, "character": 9}
                }
            }]
        })
    elif method == "textDocument/prepareCallHierarchy":
        uri = opened_uri or msg["params"]["textDocument"]["uri"]
        write_message({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": [call_item("FromLsp", uri, 0)]
        })
    elif method == "callHierarchy/incomingCalls":
        uri = msg["params"]["item"]["uri"]
        write_message({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": [{
                "from": call_item("Caller", uri, 5),
                "fromRanges": [{
                    "start": {"line": 5, "character": 2},
                    "end": {"line": 5, "character": 8}
                }]
            }]
        })
    elif method == "callHierarchy/outgoingCalls":
        uri = msg["params"]["item"]["uri"]
        write_message({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": [{
                "to": call_item("Callee", uri, 6),
                "fromRanges": [{
                    "start": {"line": 6, "character": 3},
                    "end": {"line": 6, "character": 9}
                }]
            }]
        })
    elif method == "shutdown":
        write_message({"jsonrpc": "2.0", "id": msg["id"], "result": None})
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
            .find(|candidate| StdCommand::new(candidate).arg("--version").output().is_ok())
            .unwrap_or("python3")
            .to_string()
    }
}
