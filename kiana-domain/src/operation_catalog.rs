//! Closed operation contracts and their optional model-visible projections.
use crate::{CapabilityKind, RiskLevel};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DynamicRiskRule {
    Static,
    SandboxMode,
    WorkspaceTransactionAction,
    ListOrInspectExternal,
    ListExternal,
    ConnectorInvocation,
}

#[derive(Clone, Copy)]
pub struct ModelToolProjection {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub schema: &'static str,
    pub side_effecting: bool,
    pub catalog_resources: &'static [&'static str],
    pub parameters: fn() -> Value,
    pub result_schema: fn() -> Value,
}

#[derive(Clone)]
pub struct OperationSpec {
    pub operation: &'static str,
    pub capability: CapabilityKind,
    pub minimum_risk: RiskLevel,
    pub risk_rule: DynamicRiskRule,
    pub operator_only: bool,
    pub resource_fields: &'static [&'static str],
    pub effect: &'static str,
    pub cancellation: &'static str,
    pub reconciliation: &'static str,
    pub idempotency: &'static str,
    pub binding_version: &'static str,
    pub required_arguments: &'static [&'static str],
    pub model: Option<ModelToolProjection>,
}

const DISPATCH_IDEMPOTENCY: &str = "dispatch_permit_once_no_automatic_retry";

macro_rules! define_operation_catalog {
    ($( $operation:literal => {
        capability: $capability:expr;
        minimum_risk: $minimum_risk:expr;
        risk_rule: $risk_rule:expr;
        operator_only: $operator_only:expr;
        resources: $resources:expr;
        effect: $effect:literal;
        cancellation: $cancellation:literal;
        reconciliation: $reconciliation:literal;
        required: $required:expr;
        model: $model:expr;
    }),+ $(,)?) => {
        pub const ACTION_OPERATIONS: &[&str] = &[$($operation),+];
        pub const OPERATION_SPECS: &[OperationSpec] = &[
            $(OperationSpec {
                operation: $operation,
                capability: $capability,
                minimum_risk: $minimum_risk,
                risk_rule: $risk_rule,
                operator_only: $operator_only,
                resource_fields: $resources,
                effect: $effect,
                cancellation: $cancellation,
                reconciliation: $reconciliation,
                idempotency: DISPATCH_IDEMPOTENCY,
                binding_version: crate::ACTION_HANDLER_BINDING_VERSION,
                required_arguments: $required,
                model: $model,
            }),+
        ];
    };
}

define_operation_catalog! {
    "shell.exec" => {
        capability: CapabilityKind::Process;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::SandboxMode;
        operator_only: false;
        resources: &["workdir", "path_allow"];
        effect: "sandbox_bound_process";
        cancellation: "process_group_stop_confirmation";
        reconciliation: "operator_required_on_unknown";
        required: &["command"];
        model: Some(ModelToolProjection {
            name: "shell",
            aliases: &["shell.exec", "bash", "exec", "command_execution"],
            description: "Run a shell command in the project sandbox. Codex-compatible command_execution item. Linux commands are confined with bubblewrap; read-only cannot write the workspace.",
            schema: "kiana.tool.shell.v1",
            side_effecting: true,
            catalog_resources: &["workspace"],
            parameters: shell_parameters,
            result_schema: model_tool_result_schema,
        });
    },
    "apply_patch" => {
        capability: CapabilityKind::Filesystem;
        minimum_risk: RiskLevel::LocalWrite;
        risk_rule: DynamicRiskRule::Static;
        operator_only: false;
        resources: &["patch", "path_allow"];
        effect: "workspace_patch";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &["patch"];
        model: Some(ModelToolProjection {
            name: "apply_patch",
            aliases: &["file_change"],
            description: "Apply a file patch in the project workspace. Codex-compatible file_change item.",
            schema: "kiana.tool.apply-patch.v1",
            side_effecting: true,
            catalog_resources: &["workspace"],
            parameters: apply_patch_parameters,
            result_schema: model_tool_result_schema,
        });
    },
    "mcp.call" => {
        capability: CapabilityKind::Network;
        minimum_risk: RiskLevel::ExternalSideEffect;
        risk_rule: DynamicRiskRule::Static;
        operator_only: false;
        resources: &["server", "tool"];
        effect: "external_tool";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_receipt";
        required: &["tool"];
        model: Some(ModelToolProjection {
            name: "mcp",
            aliases: &["mcp.call"],
            description: "Call a configured MCP server tool through the daemon broker. stdio only. Untrusted projects are denied.",
            schema: "kiana.tool.mcp.v1",
            side_effecting: true,
            catalog_resources: &["workspace"],
            parameters: mcp_parameters,
            result_schema: model_tool_result_schema,
        });
    },
    "mcp.discover" => {
        capability: CapabilityKind::Network;
        minimum_risk: RiskLevel::ExternalSideEffect;
        risk_rule: DynamicRiskRule::Static;
        operator_only: true;
        resources: &["server", "tool"];
        effect: "external_tool";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_receipt";
        required: &[];
        model: None;
    },
    "memory.search" => {
        capability: CapabilityKind::Query;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::Static;
        operator_only: false;
        resources: &["collection"];
        effect: "read";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &["query"];
        model: Some(ModelToolProjection {
            name: "memory.search",
            aliases: &[],
            description: "Search a Kiana memory collection through the daemon broker. Hits include layer, collection, and source. Unsourced hits are not verified conclusions.",
            schema: "kiana.tool.memory-search.v1",
            side_effecting: false,
            catalog_resources: &["memory"],
            parameters: memory_search_parameters,
            result_schema: model_tool_result_schema,
        });
    },
    "memory.write" => {
        capability: CapabilityKind::Filesystem;
        minimum_risk: RiskLevel::LocalWrite;
        risk_rule: DynamicRiskRule::Static;
        operator_only: false;
        resources: &["collection", "source", "promote_to"];
        effect: "memory_write";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &["collection", "text", "source"];
        model: Some(ModelToolProjection {
            name: "memory.write",
            aliases: &[],
            description: "Write an explicit memory record to one collection. Instance scratch does not promote. Chat is never auto-ingested.",
            schema: "kiana.tool.memory-write.v1",
            side_effecting: true,
            catalog_resources: &["workspace"],
            parameters: memory_write_parameters,
            result_schema: model_tool_result_schema,
        });
    },
    "context.repo_map" => {
        capability: CapabilityKind::Query;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::Static;
        operator_only: false;
        resources: &["root"];
        effect: "read";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &[];
        model: None;
    },
    "context.index.read" => {
        capability: CapabilityKind::Query;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::Static;
        operator_only: false;
        resources: &["root"];
        effect: "read";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &[];
        model: None;
    },
    "context.index.cache.write" => {
        capability: CapabilityKind::Query;
        minimum_risk: RiskLevel::LocalWrite;
        risk_rule: DynamicRiskRule::Static;
        operator_only: false;
        resources: &["root", "cache", "source", "store"];
        effect: "context_write";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &["cache"];
        model: None;
    },
    "context.artifacts.read" => {
        capability: CapabilityKind::Query;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::Static;
        operator_only: false;
        resources: &["root"];
        effect: "read";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &[];
        model: None;
    },
    "context.artifacts.cache.write" => {
        capability: CapabilityKind::Query;
        minimum_risk: RiskLevel::LocalWrite;
        risk_rule: DynamicRiskRule::Static;
        operator_only: false;
        resources: &["root", "cache", "source", "store"];
        effect: "context_write";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &["cache"];
        model: None;
    },
    "context.artifact_store.read" => {
        capability: CapabilityKind::Query;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::Static;
        operator_only: false;
        resources: &["root"];
        effect: "read";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &[];
        model: None;
    },
    "context.artifact_store.cache.write" => {
        capability: CapabilityKind::Query;
        minimum_risk: RiskLevel::LocalWrite;
        risk_rule: DynamicRiskRule::Static;
        operator_only: false;
        resources: &["root", "cache", "source", "store"];
        effect: "context_write";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &["cache"];
        model: None;
    },
    "context.artifact_ingest.write" => {
        capability: CapabilityKind::Query;
        minimum_risk: RiskLevel::LocalWrite;
        risk_rule: DynamicRiskRule::Static;
        operator_only: false;
        resources: &["root", "cache", "source", "store"];
        effect: "context_write";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &["source"];
        model: None;
    },
    "context.artifact_graph.read" => {
        capability: CapabilityKind::Query;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::Static;
        operator_only: false;
        resources: &["root"];
        effect: "read";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &[];
        model: None;
    },
    "context.artifact_readiness.read" => {
        capability: CapabilityKind::Query;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::Static;
        operator_only: false;
        resources: &["root"];
        effect: "read";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &[];
        model: None;
    },
    "context.search" => {
        capability: CapabilityKind::Query;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::Static;
        operator_only: false;
        resources: &["root"];
        effect: "read";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &["query"];
        model: None;
    },
    "context.vector_search" => {
        capability: CapabilityKind::Query;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::Static;
        operator_only: false;
        resources: &["root"];
        effect: "read";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &["query"];
        model: None;
    },
    "context.pack" => {
        capability: CapabilityKind::Query;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::Static;
        operator_only: false;
        resources: &["root"];
        effect: "read";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &["query"];
        model: None;
    },
    "memory.review" => {
        capability: CapabilityKind::Filesystem;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::ListOrInspectExternal;
        operator_only: false;
        resources: &["action", "project_root"];
        effect: "operator_management";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &["action"];
        model: None;
    },
    "data.governance" => {
        capability: CapabilityKind::Filesystem;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::ListOrInspectExternal;
        operator_only: false;
        resources: &["action", "project_root"];
        effect: "operator_management";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &["action"];
        model: None;
    },
    "extension.manage" => {
        capability: CapabilityKind::Filesystem;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::ListOrInspectExternal;
        operator_only: false;
        resources: &["action", "project_root"];
        effect: "operator_management";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &["action"];
        model: None;
    },
    "connector.manage" => {
        capability: CapabilityKind::Tool;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::ListExternal;
        operator_only: false;
        resources: &["binding_id", "action"];
        effect: "connector_management";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_receipt";
        required: &["action"];
        model: None;
    },
    "connector.invoke" => {
        capability: CapabilityKind::Tool;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::ConnectorInvocation;
        operator_only: false;
        resources: &["binding_snapshot", "operation"];
        effect: "binding_declared_effect";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_receipt";
        required: &["binding_snapshot", "operation", "payload", "idempotency_key"];
        model: None;
    },
    "connector.health" => {
        capability: CapabilityKind::Tool;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::Static;
        operator_only: false;
        resources: &["binding_snapshot", "probe_kind"];
        effect: "connector_read_only_probe";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_receipt";
        required: &["binding_snapshot", "probe_kind"];
        model: None;
    },
    "connector.mcp_handshake" => {
        capability: CapabilityKind::Tool;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::Static;
        operator_only: false;
        resources: &["binding_id", "server", "session_ref"];
        effect: "connector_mcp_handshake";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_receipt";
        required: &[];
        model: None;
    },
    "apply_patch.preview" => {
        capability: CapabilityKind::Filesystem;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::Static;
        operator_only: false;
        resources: &["patch", "path_allow"];
        effect: "workspace_patch_preview";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &[];
        model: None;
    },
    "workspace.checkpoint.restore" => {
        capability: CapabilityKind::Filesystem;
        minimum_risk: RiskLevel::Critical;
        risk_rule: DynamicRiskRule::Static;
        operator_only: false;
        resources: &["snapshot"];
        effect: "workspace_restore";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &["snapshot"];
        model: None;
    },
    "workspace.transaction" => {
        capability: CapabilityKind::Filesystem;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::WorkspaceTransactionAction;
        operator_only: true;
        resources: &["transaction_id", "resolution"];
        effect: "workspace_recovery";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &["action"];
        model: None;
    },
    "local.package" => {
        capability: CapabilityKind::Filesystem;
        minimum_risk: RiskLevel::LocalWrite;
        risk_rule: DynamicRiskRule::Static;
        operator_only: true;
        resources: &["package_id", "destination", "sources"];
        effect: "local_package_publication";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &["package_id", "destination", "manifest", "sources"];
        model: None;
    },
    "execution.output.read" => {
        capability: CapabilityKind::Query;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::Static;
        operator_only: true;
        resources: &["root"];
        effect: "read";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &["output_id"];
        model: None;
    },
    "process.start" => {
        capability: CapabilityKind::Process;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::SandboxMode;
        operator_only: true;
        resources: &["workdir", "path_allow"];
        effect: "sandbox_bound_process";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &["command"];
        model: None;
    },
    "process.poll" => {
        capability: CapabilityKind::Query;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::Static;
        operator_only: true;
        resources: &["root"];
        effect: "read";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &["process_id"];
        model: None;
    },
    "process.stdin" => {
        capability: CapabilityKind::Process;
        minimum_risk: RiskLevel::LocalWrite;
        risk_rule: DynamicRiskRule::Static;
        operator_only: true;
        resources: &["process_id"];
        effect: "managed_process_control";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &["process_id", "data"];
        model: None;
    },
    "process.resize" => {
        capability: CapabilityKind::Process;
        minimum_risk: RiskLevel::LocalWrite;
        risk_rule: DynamicRiskRule::Static;
        operator_only: true;
        resources: &["process_id"];
        effect: "managed_process_control";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &["process_id", "rows", "cols"];
        model: None;
    },
    "process.stop" => {
        capability: CapabilityKind::Process;
        minimum_risk: RiskLevel::LocalWrite;
        risk_rule: DynamicRiskRule::Static;
        operator_only: true;
        resources: &["process_id"];
        effect: "managed_process_control";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &["process_id"];
        model: None;
    },
    "environment.inspect" => {
        capability: CapabilityKind::Query;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::Static;
        operator_only: true;
        resources: &["root"];
        effect: "read";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &[];
        model: None;
    },
    "tool.search" => {
        capability: CapabilityKind::Query;
        minimum_risk: RiskLevel::ReadOnly;
        risk_rule: DynamicRiskRule::Static;
        operator_only: true;
        resources: &["root"];
        effect: "read";
        cancellation: "cooperative_or_result_unknown";
        reconciliation: "operator_required_on_unknown";
        required: &[];
        model: None;
    },
}

pub fn operation_spec(operation: &str) -> Option<&'static OperationSpec> {
    OPERATION_SPECS
        .iter()
        .find(|spec| spec.operation == operation)
}

pub fn model_tool_projection(name_or_alias: &str) -> Option<&'static ModelToolProjection> {
    OPERATION_SPECS.iter().find_map(|spec| {
        spec.model
            .as_ref()
            .filter(|model| model.name == name_or_alias || model.aliases.contains(&name_or_alias))
    })
}

pub fn model_operation_spec(name_or_alias: &str) -> Option<&'static OperationSpec> {
    OPERATION_SPECS.iter().find(|spec| {
        spec.model.is_some_and(|model| {
            model.name == name_or_alias || model.aliases.contains(&name_or_alias)
        })
    })
}

pub fn model_tool_projection_for_operation(
    operation: &str,
) -> Option<&'static ModelToolProjection> {
    operation_spec(operation)?.model.as_ref()
}

pub fn model_tool_schemas() -> Vec<Value> {
    OPERATION_SPECS
        .iter()
        .filter_map(|spec| {
            let model = spec.model?;
            Some(json!({
                "name": model.name,
                "description": model.description,
                "parameters": (model.parameters)(),
            }))
        })
        .collect()
}

pub fn operator_argument_properties() -> Value {
    json!({
        "action":{"type":"string","minLength":1,"maxLength":64},
        "package_id":{"type":"string","minLength":36,"maxLength":36},
        "destination":{"type":"string","minLength":1,"maxLength":4096},
        "manifest":{"type":"object"},
        "sources":{"type":"array","maxItems":4096},
        "server":{"type":"string","minLength":1,"maxLength":256},
        "transaction_id":{"type":"string","minLength":1,"maxLength":256},
        "resolution":{"type":"string","enum":["commit","rollback"]},
        "output_id":{"type":"string","minLength":1,"maxLength":256},
        "run_id":{"type":"string","minLength":36,"maxLength":36},
        "cursor":{"type":"string","minLength":1,"maxLength":256},
        "process_id":{"type":"string","minLength":1,"maxLength":256},
        "data":{"type":"string","maxLength":65536},
        "rows":{"type":"integer","minimum":1,"maximum":4096},
        "cols":{"type":"integer","minimum":1,"maximum":4096},
        "offset":{"type":"integer","minimum":0},
        "query":{"type":"string","maxLength":16384}
    })
}

pub fn operator_argument_schema(spec: &OperationSpec) -> Value {
    let mut schema = json!({
        "type":"object",
        "required":spec.required_arguments,
        "properties":operator_argument_properties(),
    });
    schema["additionalProperties"] = json!(true);
    schema
}

pub fn action_result_schema() -> Value {
    json!({"type":"object","required":["request_id","success","output","evidence_refs"],
        "additionalProperties":false,"properties":{"request_id":{"type":"string"},"success":{"type":"boolean"},
        "output":{},"evidence_refs":{"type":"array","items":{"type":"string"}}}})
}

pub fn model_tool_result_schema() -> Value {
    json!({"type":"object","additionalProperties":true})
}

fn shell_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "command": {
                "anyOf": [
                    { "type": "string" },
                    { "type": "array", "items": { "type": "string" } }
                ]
            },
            "workdir": { "type": "string" },
            "timeout_ms": { "type": "integer", "minimum": 1 }
        },
        "required": ["command"]
    })
}

fn apply_patch_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "patch": { "type": "string" },
            "path": { "type": "string" }
        },
        "required": ["patch"]
    })
}

fn mcp_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "server": { "type": "string" },
            "tool": { "type": "string" },
            "tool_name": { "type": "string" },
            "arguments": { "type": "object" }
        },
        "required": ["tool"]
    })
}

fn memory_search_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": { "type": "string" },
            "collection": { "type": "string" },
            "limit": { "type": "integer", "minimum": 1 }
        },
        "required": ["query"]
    })
}

fn memory_write_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "collection": { "type": "string" },
            "text": { "type": "string" },
            "source": { "type": "string" },
            "promote_to": { "type": "string" }
        },
        "required": ["collection", "text", "source"]
    })
}
