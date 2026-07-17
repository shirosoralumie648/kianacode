use crate::integrity::{
    load_local_hmac_key, sha256_prefixed, IntegrityEnvelope, IntegrityError, LocalHmacKey,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const WORKFLOW_ID_ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

pub type WorkflowResult<T> = Result<T, WorkflowError>;

#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    #[error("workflow request cannot be empty")]
    EmptyRequest,
    #[error("workflow run id is invalid: {0}")]
    InvalidRunId(String),
    #[error("workflow run was not found: {0}")]
    RunNotFound(String),
    #[error("no workflow runs were found")]
    NoRuns,
    #[error("workflow state is inconsistent: {0}")]
    InconsistentState(String),
    #[error("workflow eventlog is invalid: {0}")]
    InvalidEventLog(String),
    #[error("workflow eventlog writer is busy: {0}")]
    WriterBusy(String),
    #[error("workflow event data field {field} must be unique: {value}")]
    DuplicateEventData { field: String, value: String },
    #[error("workflow artifact path is invalid: {0}")]
    InvalidArtifactPath(String),
    #[error("workflow artifact already exists with different content: {path}")]
    ArtifactConflict { path: String },
    #[error("failed to access workflow filesystem: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to serialize workflow data: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Integrity(#[from] IntegrityError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowInputKind {
    Feature,
    Bug,
    Refactor,
    Research,
    Prd,
    Issue,
    Qa,
    Review,
    Eda,
    Ship,
    Task,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowProfile {
    Quick,
    Standard,
    Gated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStatus {
    Initialized,
    Running,
    Blocked,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowNodeStage {
    Runtime,
    Capture,
    Product,
    Context,
    Routing,
    Research,
    Design,
    Plan,
    Decision,
    WorkPacket,
    Execute,
    Quality,
    Verify,
    Review,
    Security,
    Fix,
    Ship,
    Learn,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowNodeType {
    Action,
    Gate,
    Router,
    Branch,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateDecision {
    Continue,
    ProceedWithCaution,
    AskUser,
    Replan,
    Reroute,
    Fix,
    AcceptRisk,
    Blocked,
    Cancelled,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInit {
    pub request: String,
    pub input_kind: WorkflowInputKind,
    pub profile: WorkflowProfile,
    pub approval_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub schema: String,
    pub workflow_id: String,
    pub run_id: String,
    pub template_id: String,
    pub status: WorkflowStatus,
    pub current_node: String,
    pub request: String,
    pub input_kind: WorkflowInputKind,
    pub profile: WorkflowProfile,
    pub approval_required: bool,
    pub artifact_dir: PathBuf,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunSummary {
    pub schema: String,
    pub workflow_id: String,
    pub run_id: String,
    pub status: WorkflowStatus,
    pub current_node: String,
    pub request: String,
    pub input_kind: WorkflowInputKind,
    pub profile: WorkflowProfile,
    pub approval_required: bool,
    pub artifact_dir: PathBuf,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowResumeStatus {
    Ready,
    Repaired,
    Blocked,
}

impl WorkflowResumeStatus {
    pub fn can_continue(self) -> bool {
        matches!(self, Self::Ready | Self::Repaired)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Repaired => "repaired",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowResumeReport {
    pub schema: String,
    pub resume_status: WorkflowResumeStatus,
    pub run: WorkflowRunSummary,
    pub event_count: usize,
    pub eventlog_last_seq: u64,
    pub state_last_event_seq: u64,
    pub eventlog_consistent: bool,
    pub blocker: Option<String>,
    pub recommended_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowState {
    pub schema: String,
    pub workflow_id: String,
    pub run_id: String,
    pub status: WorkflowStatus,
    pub current_node: String,
    pub request: String,
    pub input_kind: WorkflowInputKind,
    pub profile: WorkflowProfile,
    pub approval_required: bool,
    pub checkpoint: String,
    pub pending_approvals: Vec<String>,
    pub last_event_seq: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDagTemplate {
    pub schema: String,
    pub id: String,
    pub version: u32,
    pub description: String,
    pub nodes: Vec<WorkflowNodeSpec>,
    pub edges: Vec<WorkflowEdge>,
    pub artifact_contract: Vec<String>,
}

impl WorkflowDagTemplate {
    pub fn node(&self, id: &str) -> Option<&WorkflowNodeSpec> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub fn has_edge(&self, from: &str, to: &str, decision: &str) -> bool {
        self.edges
            .iter()
            .any(|edge| edge.from == from && edge.to == to && edge.decision == decision)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNodeSpec {
    pub id: String,
    pub title: String,
    pub stage: WorkflowNodeStage,
    pub node_type: WorkflowNodeType,
    pub actions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate: Option<WorkflowGateSpec>,
    pub reads: Vec<String>,
    pub writes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowGateSpec {
    pub id: String,
    pub decisions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowEdge {
    pub from: String,
    pub to: String,
    pub decision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowTransitionEvidence {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTransitionInput {
    pub from: String,
    pub to: String,
    pub decision: String,
    pub evidence: Vec<WorkflowTransitionEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTransitionResult {
    pub transition_id: String,
    pub gate_event: WorkflowEvent,
    pub node_exited_event: WorkflowEvent,
    pub node_entered_event: WorkflowEvent,
    pub state: WorkflowState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowEventKind {
    WorkflowCreated,
    TaskCreated,
    TaskStatusChanged,
    WorkPacketCreated,
    SwarmWorkersPrepared,
    SwarmProjectBaselineCaptured,
    WorkerStarted,
    WorkerCancelled,
    SwarmIntegrationPlanned,
    SwarmIntegrationStarted,
    SwarmWorkerApplied,
    SwarmWorkerIntegrated,
    SwarmIntegrationBlocked,
    SwarmIntegrationRecoveryStarted,
    SwarmIntegrationRecoveryRevisionCommitted,
    SwarmIntegrationRecoveryArchived,
    SwarmIntegrationRolledBack,
    SwarmIntegrationCompleted,
    SwarmIsolationCleaned,
    CommandStarted,
    CommandCompleted,
    ResultPacketCreated,
    EvidenceRecorded,
    VerificationCompleted,
    ReviewCompleted,
    NodeEntered,
    NodeExited,
    GateEvaluated,
    ArtifactWritten,
    ApprovalRequested,
    ApprovalResolved,
    RetryRecorded,
    WorkflowBlocked,
    WorkflowCompleted,
    WorkflowCancelled,
    WorkflowResumed,
    WorkflowForked,
    Learned,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowEvent {
    pub schema: String,
    pub seq: u64,
    pub at_ms: u64,
    pub kind: WorkflowEventKind,
    pub node_id: String,
    pub data: Value,
}

pub const WORKFLOW_INTEGRITY_GENESIS_SCHEMA: &str = "kiana.workflow-integrity-genesis.v1";
pub const WORKFLOW_INTEGRITY_REPORT_SCHEMA: &str = "kiana.workflow-integrity-report.v1";
pub const WORKFLOW_ARTIFACT_DESCRIPTOR_SCHEMA: &str = "kiana.workflow-artifact-descriptor.v1";
const WORKFLOW_INTEGRITY_GENESIS_PATH: &str = "integrity/genesis-seal.json";
const WORKFLOW_INTEGRITY_ARTIFACTS_FIELD: &str = "integrity_artifacts";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowArtifactDescriptor {
    pub schema: String,
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub media_type: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowTrustStatus {
    UnsignedLegacy,
    SealedLegacyPrefix,
    Verified,
    UnverifiableKeyMissing,
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowIntegrityGenesisSeal {
    pub schema: String,
    pub workflow_id: String,
    pub legacy_event_count: u64,
    pub legacy_eventlog_sha256: String,
    pub sealed_at_ms: u64,
    pub key_id: String,
    pub integrity: IntegrityEnvelope,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowIntegrityReport {
    pub schema: String,
    pub workflow_id: String,
    pub status: WorkflowTrustStatus,
    pub event_count: u64,
    pub legacy_prefix_count: u64,
    pub verified_event_count: u64,
    pub first_authenticated_seq: Option<u64>,
    pub last_authenticated_seq: Option<u64>,
    pub key_id: Option<String>,
    pub genesis_seal_path: Option<String>,
    pub artifact_descriptor_count: u64,
    pub verified_artifact_count: u64,
    pub orphan_artifact_count: u64,
}

#[derive(Serialize)]
struct WorkflowIntegrityGenesisPayload<'a> {
    schema: &'a str,
    workflow_id: &'a str,
    legacy_event_count: u64,
    legacy_eventlog_sha256: &'a str,
    sealed_at_ms: u64,
    key_id: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkflowEventRecord {
    #[serde(flatten)]
    event: WorkflowEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    integrity: Option<IntegrityEnvelope>,
}

#[derive(Debug, Clone)]
pub struct WorkflowArtifactInput {
    pub relative_path: String,
    pub contents: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct WorkflowArtifactBatch {
    pub artifacts: Vec<WorkflowArtifactInput>,
    pub event_data: Value,
}

#[derive(Debug, Clone)]
pub struct WorkflowArtifactCommit {
    pub event: WorkflowEvent,
    pub artifact_paths: Vec<String>,
    pub reused_event: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateResult {
    pub schema: String,
    pub node_id: String,
    pub decision: GateDecision,
    pub reason: String,
    pub confidence: f32,
    pub approval_required: bool,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkPacket {
    pub schema: String,
    pub id: String,
    pub goal: String,
    pub in_scope: Vec<String>,
    pub not_building: Vec<String>,
    pub allowed_files: Vec<String>,
    pub forbidden_files: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub verification_commands: Vec<String>,
    pub evidence_requirements: Vec<String>,
    pub rollback_plan: Vec<String>,
    pub review_focus: Vec<String>,
    pub depends_on: Vec<String>,
    pub retry_policy: String,
    pub approval_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidencePacket {
    pub schema: String,
    pub id: String,
    pub workpacket_id: String,
    pub status: String,
    pub items: Vec<EvidenceItem>,
    pub acceptance_results: Vec<String>,
    pub skipped_checks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceItem {
    pub kind: String,
    pub command: Option<String>,
    pub artifact_path: Option<String>,
    pub summary: String,
    pub passed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewSeverity {
    Block,
    High,
    Medium,
    Low,
    Note,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewPacket {
    pub schema: String,
    pub id: String,
    #[serde(default)]
    pub workflow_id: String,
    #[serde(default)]
    pub run_id: String,
    pub mode: String,
    #[serde(default)]
    pub reviewer_type: String,
    #[serde(default)]
    pub scope: Vec<String>,
    pub findings: Vec<ReviewFinding>,
    #[serde(default)]
    pub score: f32,
    #[serde(default)]
    pub blocking_count: usize,
    #[serde(default)]
    pub recommendation: String,
    #[serde(default)]
    pub decision: String,
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub created_at_ms: u64,
    #[serde(default)]
    pub next_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewFinding {
    pub severity: ReviewSeverity,
    pub confidence: f32,
    #[serde(default)]
    pub category: String,
    pub title: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    #[serde(default)]
    pub evidence: String,
    #[serde(default)]
    pub risk: String,
    pub recommendation: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub decision: String,
    #[serde(default)]
    pub next_action: String,
}

pub fn default_workflow_template() -> WorkflowDagTemplate {
    let nodes = vec![
        node(
            "runtime_init",
            "Workflow Runtime Init",
            WorkflowNodeStage::Runtime,
            WorkflowNodeType::Action,
            &[
                "create workflow run",
                "assign workflow and run identifiers",
                "create artifact directory",
                "initialize event log and state",
                "select DAG template",
                "create initial checkpoint",
            ],
            &["workflow_dag.json", "eventlog.jsonl", "state.json"],
        ),
        node(
            "capture",
            "Capture",
            WorkflowNodeStage::Capture,
            WorkflowNodeType::Gate,
            &[
                "extract goal",
                "extract success criteria",
                "extract constraints",
                "extract risk and approval needs",
                "extract explicit NOT_BUILDING",
                "classify input type",
                "estimate ambiguity and automation risk",
            ],
            &["state.json"],
        ),
        node(
            "clarify_question",
            "Clarify Question",
            WorkflowNodeStage::Capture,
            WorkflowNodeType::Branch,
            &["ask one targeted clarification question"],
            &["eventlog.jsonl"],
        ),
        node(
            "split_goal",
            "Split Goal",
            WorkflowNodeStage::Capture,
            WorkflowNodeType::Branch,
            &["split oversized goal into subgoals"],
            &["task_plan.md"],
        ),
        node(
            "approval_gate",
            "Approval Gate",
            WorkflowNodeStage::Decision,
            WorkflowNodeType::Gate,
            &["request explicit human approval for high-risk action"],
            &["eventlog.jsonl", "state.json"],
        ),
        node(
            "product_definition",
            "Product Definition Gate",
            WorkflowNodeStage::Product,
            WorkflowNodeType::Gate,
            &[
                "decide if product clarification is needed",
                "route clear work to context intake",
            ],
            &["problem-definition.md"],
        ),
        node(
            "product_office_hours",
            "Product Office Hours",
            WorkflowNodeStage::Product,
            WorkflowNodeType::Action,
            &[
                "define actor",
                "define real problem",
                "define workflow",
                "define scope",
                "define non-goals",
                "define success metrics",
                "define approval boundaries",
            ],
            &["problem-definition.md"],
        ),
        node(
            "context_intake",
            "Context Intake",
            WorkflowNodeStage::Context,
            WorkflowNodeType::Gate,
            &[
                "restore previous state",
                "load memory summary",
                "check project fingerprint",
                "load handoff reports",
                "build context pack",
                "decide isolation",
            ],
            &["context_pack.md", "state.json"],
        ),
        node(
            "refresh_memory",
            "Refresh Memory Index",
            WorkflowNodeStage::Context,
            WorkflowNodeType::Branch,
            &["refresh stale memory index"],
            &["context_pack.md"],
        ),
        node(
            "create_isolation",
            "Create Worktree or Branch",
            WorkflowNodeStage::Context,
            WorkflowNodeType::Branch,
            &["isolate risky changes"],
            &["state.json"],
        ),
        node(
            "intent_router",
            "Intent Router",
            WorkflowNodeStage::Routing,
            WorkflowNodeType::Router,
            &["route by primary intent"],
            &["routing-packet.json"],
        ),
        node(
            "qa_planning",
            "QA Planning",
            WorkflowNodeStage::Verify,
            WorkflowNodeType::Branch,
            &["select QA flow and verification profile"],
            &["evidence/"],
        ),
        node(
            "investigate",
            "Investigate",
            WorkflowNodeStage::Fix,
            WorkflowNodeType::Branch,
            &["reproduce failure", "collect logs", "trace root cause"],
            &["findings.md"],
        ),
        node(
            "research",
            "Research",
            WorkflowNodeStage::Research,
            WorkflowNodeType::Gate,
            &[
                "search code and docs",
                "check external docs if needed",
                "trace affected symbols and data flow",
                "check historical decisions",
                "record findings",
            ],
            &["findings.md"],
        ),
        node(
            "design",
            "Design Candidate",
            WorkflowNodeStage::Design,
            WorkflowNodeType::Gate,
            &[
                "generate design candidate",
                "map modules",
                "define interfaces and errors",
                "define test, migration, security, and evidence surfaces",
                "define NOT_BUILDING",
            ],
            &["decision-briefs.md"],
        ),
        node(
            "design_alternative",
            "Design Alternative",
            WorkflowNodeStage::Design,
            WorkflowNodeType::Branch,
            &["produce lower-risk alternative"],
            &["decision-briefs.md"],
        ),
        node(
            "plan",
            "Plan",
            WorkflowNodeStage::Plan,
            WorkflowNodeType::Gate,
            &[
                "create task_plan.md",
                "decompose tasks",
                "define dependencies",
                "define verification, rollback, review, DAG, evidence, approval boundaries",
            ],
            &["task_plan.md"],
        ),
        node(
            "plan_confirmation",
            "Plan Confirmation",
            WorkflowNodeStage::Plan,
            WorkflowNodeType::Gate,
            &[
                "verify referenced files",
                "verify target patterns",
                "verify commands",
                "verify branch state",
                "verify acceptance criteria and evidence",
            ],
            &["plan-confirmation.md"],
        ),
        node(
            "autoplan_review",
            "Autoplan Review",
            WorkflowNodeStage::Review,
            WorkflowNodeType::Gate,
            &[
                "review completeness",
                "run product, engineering, security, design, and DevEx plan review",
                "classify findings",
            ],
            &["autoplan-review.md"],
        ),
        node(
            "decision_briefs",
            "Decision Briefs",
            WorkflowNodeStage::Decision,
            WorkflowNodeType::Gate,
            &[
                "identify direction-changing decisions",
                "compare options",
                "recommend default",
                "record decision or ADR",
            ],
            &["decision-briefs.md"],
        ),
        node(
            "build_workpacket",
            "Build WorkPacket",
            WorkflowNodeStage::WorkPacket,
            WorkflowNodeType::Gate,
            &[
                "define goal, scope, NOT_BUILDING, files, tests, acceptance, verification, evidence, rollback, risk, and DAG metadata",
            ],
            &["workpackets/"],
        ),
        node(
            "skill_router",
            "Skill / Agent / Rules Router",
            WorkflowNodeStage::Routing,
            WorkflowNodeType::Gate,
            &[
                "select commands and skills",
                "select specialist agents",
                "select rules pack",
                "select verification profile",
                "select execution mode",
            ],
            &["routing-packet.json"],
        ),
        node(
            "execute",
            "Execute",
            WorkflowNodeStage::Execute,
            WorkflowNodeType::Gate,
            &[
                "read immutable WorkPacket",
                "lock scope",
                "create pre-edit snapshot",
                "apply TDD if required",
                "make bounded edit",
                "collect changed files",
                "detect scope drift and secret leakage",
            ],
            &["resultpackets/"],
        ),
        node(
            "quality_gate",
            "Quality Gate",
            WorkflowNodeStage::Quality,
            WorkflowNodeType::Gate,
            &[
                "format",
                "lint",
                "type check",
                "build",
                "security scan",
                "dependency check",
                "diff scope check",
            ],
            &["evidence/"],
        ),
        node(
            "behavior_verify",
            "Behavior Verify",
            WorkflowNodeStage::Verify,
            WorkflowNodeType::Gate,
            &[
                "run targeted tests",
                "run regression tests if needed",
                "run browser, CLI, or API flow if needed",
                "capture logs and outputs",
                "check acceptance criteria and NOT_BUILDING",
            ],
            &["evidence/"],
        ),
        node(
            "review_scope",
            "Review Scope",
            WorkflowNodeStage::Review,
            WorkflowNodeType::Action,
            &[
                "collect changed files",
                "collect diff summary",
                "copy rules and scope",
                "add evidence and decision refs",
            ],
            &["review/scope.md"],
        ),
        node(
            "multi_review",
            "Multi Review",
            WorkflowNodeStage::Review,
            WorkflowNodeType::Gate,
            &[
                "run product, architecture, code, security, QA, docs/DX, error handling, and red-team review as needed",
                "deduplicate findings",
                "assign severity and confidence",
            ],
            &["reviews/", "consolidated-review.md"],
        ),
        node(
            "review_triage",
            "Review Triage",
            WorkflowNodeStage::Review,
            WorkflowNodeType::Gate,
            &[
                "classify all findings",
                "resolve every finding before ship",
            ],
            &["consolidated-review.md"],
        ),
        node(
            "security_fix",
            "Security Fix / Block",
            WorkflowNodeStage::Security,
            WorkflowNodeType::Gate,
            &["classify security severity", "fix or block"],
            &["evidence/"],
        ),
        node(
            "fix_loop",
            "Fix Loop",
            WorkflowNodeStage::Fix,
            WorkflowNodeType::Gate,
            &[
                "read failure packet",
                "classify root cause",
                "apply minimal fix",
                "record attempt",
                "check scope",
                "increment retry counter",
            ],
            &["resultpackets/"],
        ),
        node(
            "ship",
            "Ship",
            WorkflowNodeStage::Ship,
            WorkflowNodeType::Gate,
            &[
                "prepare delivery summary",
                "summarize files, checks, evidence, review triage, risks, and follow-ups",
                "generate handoff",
            ],
            &["next-agent-handoff.md"],
        ),
        node(
            "release_action",
            "Push / Merge / Deploy",
            WorkflowNodeStage::Ship,
            WorkflowNodeType::Branch,
            &["perform approved external release action"],
            &["eventlog.jsonl"],
        ),
        node(
            "learn",
            "Learn",
            WorkflowNodeStage::Learn,
            WorkflowNodeType::Gate,
            &[
                "append final events",
                "update progress",
                "classify durable and temporary learnings",
                "update memory candidates",
            ],
            &["learnings.md"],
        ),
        node(
            "loop_controller",
            "Loop Controller",
            WorkflowNodeStage::Learn,
            WorkflowNodeType::Router,
            &["select next workpacket, next goal, or completion"],
            &["state.json"],
        ),
        node(
            "block_handoff",
            "Block / Handoff Report",
            WorkflowNodeStage::Terminal,
            WorkflowNodeType::Terminal,
            &["write block reason and next-agent handoff"],
            &["next-agent-handoff.md"],
        ),
        node(
            "completed",
            "Completed",
            WorkflowNodeStage::Terminal,
            WorkflowNodeType::Terminal,
            &["mark workflow completed"],
            &["state.json"],
        ),
    ];

    WorkflowDagTemplate {
        schema: "kiana.workflow-dag.v1".to_string(),
        id: "kiana-full-workflow".to_string(),
        version: 1,
        description:
            "Complete Kiana workflow DAG for capture, context, plan, execution, review, ship, and learn."
                .to_string(),
        nodes,
        edges: default_edges(),
        artifact_contract: default_artifact_contract(),
    }
}

pub fn initialize_workflow_run(
    root: impl AsRef<Path>,
    init: WorkflowInit,
) -> WorkflowResult<WorkflowRun> {
    let request = init.request.trim();
    if request.is_empty() {
        return Err(WorkflowError::EmptyRequest);
    }

    let now = now_ms();
    let workflow_id = format!("wf-{}", random_suffix(10));
    let run_id = format!("run-{}-{}", now, random_suffix(6));
    let artifact_dir = root.as_ref().join(".kiana").join("workflows").join(&run_id);
    fs::create_dir_all(&artifact_dir)?;
    create_artifact_shape(&artifact_dir)?;

    let template = default_workflow_template();
    write_json_pretty(&artifact_dir.join("workflow_dag.json"), &template)?;
    let dag_sha256 = sha256_prefixed(&fs::read(artifact_dir.join("workflow_dag.json"))?);

    let state = WorkflowState {
        schema: "kiana.workflow-state.v1".to_string(),
        workflow_id: workflow_id.clone(),
        run_id: run_id.clone(),
        status: WorkflowStatus::Initialized,
        current_node: "capture".to_string(),
        request: request.to_string(),
        input_kind: init.input_kind,
        profile: init.profile,
        approval_required: init.approval_required,
        checkpoint: "initial".to_string(),
        pending_approvals: if init.approval_required {
            vec!["capture:approval_required".to_string()]
        } else {
            Vec::new()
        },
        last_event_seq: 0,
        created_at_ms: now,
        updated_at_ms: now,
    };
    write_workflow_state_atomic(&artifact_dir.join("state.json"), &state)?;
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(artifact_dir.join("eventlog.jsonl"))?;

    append_workflow_event(
        &artifact_dir,
        WorkflowEventKind::WorkflowCreated,
        "runtime_init",
        json!({
            "workflow_id": workflow_id,
            "run_id": run_id,
            "template_id": template.id,
            "dag_sha256": dag_sha256,
            "initial_node": "capture",
            "input_kind": init.input_kind,
            "profile": init.profile,
            "approval_required": init.approval_required,
            "request": request
        }),
    )?;
    append_workflow_event(
        &artifact_dir,
        WorkflowEventKind::NodeEntered,
        "capture",
        json!({
            "reason": "initial workflow node"
        }),
    )?;

    let state = read_state(&artifact_dir)?;
    Ok(WorkflowRun {
        schema: "kiana.workflow-run.v1".to_string(),
        workflow_id: state.workflow_id,
        run_id: state.run_id,
        template_id: template.id,
        status: state.status,
        current_node: state.current_node,
        request: state.request,
        input_kind: state.input_kind,
        profile: state.profile,
        approval_required: state.approval_required,
        artifact_dir,
        created_at_ms: state.created_at_ms,
        updated_at_ms: state.updated_at_ms,
    })
}

pub fn append_workflow_event(
    artifact_dir: impl AsRef<Path>,
    kind: WorkflowEventKind,
    node_id: impl Into<String>,
    data: Value,
) -> WorkflowResult<WorkflowEvent> {
    append_workflow_event_with_data(artifact_dir, kind, node_id, move |_, _| data)
}

pub fn append_workflow_event_with_data(
    artifact_dir: impl AsRef<Path>,
    kind: WorkflowEventKind,
    node_id: impl Into<String>,
    build_data: impl FnOnce(u64, u64) -> Value,
) -> WorkflowResult<WorkflowEvent> {
    append_workflow_event_internal(artifact_dir, kind, node_id, None, build_data)
}

pub fn append_workflow_event_with_unique_data_value(
    artifact_dir: impl AsRef<Path>,
    kind: WorkflowEventKind,
    node_id: impl Into<String>,
    field: impl Into<String>,
    value: impl Into<String>,
    build_data: impl FnOnce(u64, u64) -> Value,
) -> WorkflowResult<WorkflowEvent> {
    append_workflow_event_internal(
        artifact_dir,
        kind,
        node_id,
        Some((field.into(), value.into())),
        build_data,
    )
}

pub fn append_workflow_event_idempotent_with_unique_data_value(
    artifact_dir: impl AsRef<Path>,
    kind: WorkflowEventKind,
    node_id: impl Into<String>,
    field: impl Into<String>,
    value: impl Into<String>,
    build_data: impl FnOnce(u64, u64) -> Value,
) -> WorkflowResult<WorkflowEvent> {
    let artifact_dir = artifact_dir.as_ref();
    let _writer_lease = WorkflowWriterLease::acquire(artifact_dir)?;
    let events = read_workflow_events(artifact_dir)?;
    ensure_state_matches_events(artifact_dir, &events)?;
    let node_id = node_id.into();
    let field = field.into();
    let value = value.into();
    let existing = events
        .iter()
        .find(|event| event.data.get(&field).and_then(Value::as_str) == Some(value.as_str()))
        .cloned();
    let (seq, at_ms) = existing
        .as_ref()
        .map(|event| (event.seq, event.at_ms))
        .unwrap_or_else(|| {
            (
                events.last().map(|event| event.seq + 1).unwrap_or(1),
                now_ms(),
            )
        });
    let data = build_data(seq, at_ms);
    if data.get(&field).and_then(Value::as_str) != Some(value.as_str()) {
        return Err(WorkflowError::InvalidEventLog(format!(
            "event data must contain {field}={value}"
        )));
    }
    if let Some(event) = existing {
        if event.kind != kind || event.node_id != node_id || event.data != data {
            return Err(WorkflowError::InconsistentState(format!(
                "existing unique event {field}={value} does not match requested event"
            )));
        }
        return Ok(event);
    }
    let event = WorkflowEvent {
        schema: "kiana.workflow-event.v1".to_string(),
        seq,
        at_ms,
        kind,
        node_id,
        data,
    };
    write_workflow_event_under_lease(artifact_dir, event)
}

pub fn append_workflow_transition(
    artifact_dir: impl AsRef<Path>,
    input: WorkflowTransitionInput,
) -> WorkflowResult<WorkflowTransitionResult> {
    let artifact_dir = artifact_dir.as_ref();
    let _writer_lease = WorkflowWriterLease::acquire(artifact_dir)?;
    let mut records = read_workflow_event_records(artifact_dir)?;
    let events = records
        .iter()
        .map(|record| record.event.clone())
        .collect::<Vec<_>>();
    ensure_state_matches_events(artifact_dir, &events)?;
    let state = read_state(artifact_dir)?;
    if state.status != WorkflowStatus::Running || state.current_node != input.from {
        return Err(WorkflowError::InconsistentState(format!(
            "workflow must be running at {}, current status is {:?} at {}",
            input.from, state.status, state.current_node
        )));
    }
    if input.from.trim().is_empty()
        || input.to.trim().is_empty()
        || input.decision.trim().is_empty()
        || input.evidence.is_empty()
    {
        return Err(WorkflowError::InvalidEventLog(
            "workflow transition requires from, to, decision, and evidence".to_string(),
        ));
    }
    let dag = read_verified_workflow_dag(artifact_dir, &events)?;
    if !dag.has_edge(&input.from, &input.to, &input.decision) {
        return Err(WorkflowError::InconsistentState(format!(
            "workflow DAG edge {} --{}--> {} is not permitted",
            input.from, input.decision, input.to
        )));
    }

    let transition_id = format!("tr_{}_{}", now_ms(), random_suffix(6));
    let base_seq = records
        .last()
        .map(|record| record.event.seq + 1)
        .unwrap_or(1);
    let at_ms = now_ms();
    let common = json!({
        "transition_id": transition_id,
        "from": input.from,
        "to": input.to,
        "decision": input.decision,
        "evidence": input.evidence,
        "note": input.note,
    });
    let drafts = [
        (WorkflowEventKind::GateEvaluated, input.from.clone()),
        (WorkflowEventKind::NodeExited, input.from.clone()),
        (WorkflowEventKind::NodeEntered, input.to.clone()),
    ];
    let mut appended = Vec::with_capacity(drafts.len());
    for (index, (kind, node_id)) in drafts.into_iter().enumerate() {
        let event = WorkflowEvent {
            schema: "kiana.workflow-event.v1".to_string(),
            seq: base_seq + index as u64,
            at_ms,
            kind,
            node_id,
            data: common.clone(),
        };
        let record = build_workflow_event_record(artifact_dir, &records, event)?;
        appended.push(record.event.clone());
        records.push(record);
    }

    let new_records = &records[records.len() - appended.len()..];
    let mut bytes = Vec::new();
    for record in new_records {
        serde_json::to_writer(&mut bytes, record)?;
        bytes.push(b'\n');
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(artifact_dir.join("eventlog.jsonl"))?;
    file.write_all(&bytes)?;
    file.flush()?;
    file.sync_data()?;

    let node_entered_event = appended.pop().expect("three transition events");
    let node_exited_event = appended.pop().expect("three transition events");
    let gate_event = appended.pop().expect("three transition events");
    update_state_after_event(artifact_dir, &node_entered_event)?;
    let state = read_state(artifact_dir)?;
    Ok(WorkflowTransitionResult {
        transition_id,
        gate_event,
        node_exited_event,
        node_entered_event,
        state,
    })
}

pub fn commit_immutable_artifacts_with_unique_event(
    artifact_dir: impl AsRef<Path>,
    kind: WorkflowEventKind,
    node_id: impl Into<String>,
    field: impl Into<String>,
    value: impl Into<String>,
    build_batch: impl FnOnce(u64, u64) -> WorkflowResult<WorkflowArtifactBatch>,
) -> WorkflowResult<WorkflowArtifactCommit> {
    let artifact_dir = artifact_dir.as_ref();
    let _writer_lease = WorkflowWriterLease::acquire(artifact_dir)?;
    let events = read_workflow_events(artifact_dir)?;
    ensure_state_matches_events(artifact_dir, &events)?;

    let node_id = node_id.into();
    let field = field.into();
    let value = value.into();
    let existing_event = events
        .iter()
        .find(|event| event.data.get(&field).and_then(Value::as_str) == Some(value.as_str()))
        .cloned();
    let (seq, at_ms) = existing_event
        .as_ref()
        .map(|event| (event.seq, event.at_ms))
        .unwrap_or_else(|| {
            (
                events.last().map(|event| event.seq + 1).unwrap_or(1),
                now_ms(),
            )
        });
    let mut batch = build_batch(seq, at_ms)?;
    if batch.event_data.get(&field).and_then(Value::as_str) != Some(value.as_str()) {
        return Err(WorkflowError::InvalidEventLog(format!(
            "event data must contain {field}={value}"
        )));
    }
    let event_data = batch.event_data.as_object_mut().ok_or_else(|| {
        WorkflowError::InvalidEventLog("artifact event data must be an object".to_string())
    })?;
    if event_data.contains_key(WORKFLOW_INTEGRITY_ARTIFACTS_FIELD) {
        return Err(WorkflowError::InvalidEventLog(
            "integrity_artifacts is reserved".to_string(),
        ));
    }

    let resolved = preflight_artifacts(artifact_dir, &batch.artifacts)?;
    let descriptors = workflow_artifact_descriptors(&resolved);
    event_data.insert(
        WORKFLOW_INTEGRITY_ARTIFACTS_FIELD.to_string(),
        serde_json::to_value(descriptors)?,
    );
    if let Some(event) = existing_event {
        if event.kind != kind || event.node_id != node_id || event.data != batch.event_data {
            return Err(WorkflowError::InconsistentState(format!(
                "existing unique event {field}={value} does not match requested dispatch"
            )));
        }
        if resolved.iter().any(|artifact| !artifact.existed) {
            return Err(WorkflowError::InconsistentState(format!(
                "committed event {field}={value} references missing artifacts"
            )));
        }
        return Ok(WorkflowArtifactCommit {
            event,
            artifact_paths: resolved
                .into_iter()
                .map(|artifact| artifact.relative_path)
                .collect(),
            reused_event: true,
        });
    }

    write_missing_artifacts(artifact_dir, &resolved)?;
    let event = WorkflowEvent {
        schema: "kiana.workflow-event.v1".to_string(),
        seq,
        at_ms,
        kind,
        node_id,
        data: batch.event_data,
    };
    let event = write_workflow_event_under_lease(artifact_dir, event)?;
    Ok(WorkflowArtifactCommit {
        event,
        artifact_paths: resolved
            .into_iter()
            .map(|artifact| artifact.relative_path)
            .collect(),
        reused_event: false,
    })
}

fn append_workflow_event_internal(
    artifact_dir: impl AsRef<Path>,
    kind: WorkflowEventKind,
    node_id: impl Into<String>,
    unique_data: Option<(String, String)>,
    build_data: impl FnOnce(u64, u64) -> Value,
) -> WorkflowResult<WorkflowEvent> {
    let artifact_dir = artifact_dir.as_ref();
    let _writer_lease = WorkflowWriterLease::acquire(artifact_dir)?;
    if let Some((field, value)) = unique_data.as_ref() {
        let events = read_workflow_events(artifact_dir)?;
        if events
            .iter()
            .any(|event| event.data.get(field).and_then(Value::as_str) == Some(value.as_str()))
        {
            return Err(WorkflowError::DuplicateEventData {
                field: field.clone(),
                value: value.clone(),
            });
        }
    }
    let eventlog_path = artifact_dir.join("eventlog.jsonl");
    let seq = next_event_seq(&eventlog_path)?;
    let node_id = node_id.into();
    let at_ms = now_ms();
    let event = WorkflowEvent {
        schema: "kiana.workflow-event.v1".to_string(),
        seq,
        at_ms,
        kind,
        node_id: node_id.clone(),
        data: build_data(seq, at_ms),
    };

    write_workflow_event_under_lease(artifact_dir, event)
}

fn write_workflow_event_under_lease(
    artifact_dir: &Path,
    event: WorkflowEvent,
) -> WorkflowResult<WorkflowEvent> {
    let records = read_workflow_event_records(artifact_dir)?;
    let record = build_workflow_event_record(artifact_dir, &records, event)?;
    let eventlog_path = artifact_dir.join("eventlog.jsonl");
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&eventlog_path)?;
    serde_json::to_writer(&mut file, &record)?;
    file.write_all(b"\n")?;
    file.flush()?;
    file.sync_data()?;

    update_state_after_event(artifact_dir, &record.event)?;
    Ok(record.event)
}

fn build_workflow_event_record(
    artifact_dir: &Path,
    records: &[WorkflowEventRecord],
    mut event: WorkflowEvent,
) -> WorkflowResult<WorkflowEventRecord> {
    let Some(key) = load_local_hmac_key()? else {
        if records.iter().any(|record| record.integrity.is_some()) {
            return Err(IntegrityError::KeyMissing.into());
        }
        event.schema = "kiana.workflow-event.v1".to_string();
        return Ok(WorkflowEventRecord {
            event,
            integrity: None,
        });
    };
    if records
        .last()
        .is_some_and(|record| record.integrity.is_none())
    {
        validate_first_authenticated_event(artifact_dir, records, &event, &key)?;
    }
    event.schema = "kiana.workflow-event.v2".to_string();
    let previous_record_sha256 = records
        .last()
        .map(workflow_event_record_sha256)
        .transpose()?
        .unwrap_or_else(|| "genesis".to_string());
    let payload = serde_json::to_vec(&event)?;
    let integrity =
        key.sign_payload("kiana.workflow-event.v2", &payload, &previous_record_sha256)?;
    Ok(WorkflowEventRecord {
        event,
        integrity: Some(integrity),
    })
}

fn workflow_event_record_sha256(record: &WorkflowEventRecord) -> WorkflowResult<String> {
    Ok(sha256_prefixed(&serde_json::to_vec(record)?))
}

fn canonical_eventlog_bytes(records: &[WorkflowEventRecord]) -> WorkflowResult<Vec<u8>> {
    let mut bytes = Vec::new();
    for record in records {
        serde_json::to_writer(&mut bytes, record)?;
        bytes.push(b'\n');
    }
    Ok(bytes)
}

fn validate_first_authenticated_event(
    artifact_dir: &Path,
    legacy_records: &[WorkflowEventRecord],
    event: &WorkflowEvent,
    key: &LocalHmacKey,
) -> WorkflowResult<()> {
    let (seal, seal_sha256) = verify_genesis_seal(artifact_dir, legacy_records, key)?;
    let data = event
        .data
        .as_object()
        .ok_or_else(|| WorkflowError::InvalidEventLog("integrity_seal_required".to_string()))?;
    if event.kind != WorkflowEventKind::ArtifactWritten
        || event.node_id != "runtime_integrity"
        || data.get("integrity_genesis_path").and_then(Value::as_str)
            != Some(WORKFLOW_INTEGRITY_GENESIS_PATH)
        || data.get("integrity_genesis_sha256").and_then(Value::as_str)
            != Some(seal_sha256.as_str())
        || data.get("key_id").and_then(Value::as_str) != Some(seal.key_id.as_str())
        || data.get("legacy_event_count").and_then(Value::as_u64) != Some(seal.legacy_event_count)
    {
        return Err(WorkflowError::InvalidEventLog(
            "integrity_seal_required".to_string(),
        ));
    }
    Ok(())
}

fn verify_genesis_seal(
    artifact_dir: &Path,
    legacy_records: &[WorkflowEventRecord],
    key: &LocalHmacKey,
) -> WorkflowResult<(WorkflowIntegrityGenesisSeal, String)> {
    let path = artifact_dir.join(WORKFLOW_INTEGRITY_GENESIS_PATH);
    let bytes = fs::read(&path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            WorkflowError::InvalidEventLog("integrity_seal_required".to_string())
        } else {
            WorkflowError::Io(error)
        }
    })?;
    let seal: WorkflowIntegrityGenesisSeal = serde_json::from_slice(&bytes)?;
    if seal.schema != WORKFLOW_INTEGRITY_GENESIS_SCHEMA
        || seal.key_id != key.key_id()
        || seal.legacy_event_count != legacy_records.len() as u64
    {
        return Err(IntegrityError::ChainMismatch.into());
    }
    let legacy_eventlog_sha256 = sha256_prefixed(&canonical_eventlog_bytes(legacy_records)?);
    if seal.legacy_eventlog_sha256 != legacy_eventlog_sha256 {
        return Err(IntegrityError::ChainMismatch.into());
    }
    let payload = WorkflowIntegrityGenesisPayload {
        schema: &seal.schema,
        workflow_id: &seal.workflow_id,
        legacy_event_count: seal.legacy_event_count,
        legacy_eventlog_sha256: &seal.legacy_eventlog_sha256,
        sealed_at_ms: seal.sealed_at_ms,
        key_id: &seal.key_id,
    };
    key.verify_payload(
        "kiana.workflow-integrity-genesis.v1",
        &serde_json::to_vec(&payload)?,
        &seal.integrity,
    )?;
    Ok((seal, sha256_prefixed(&bytes)))
}

pub fn seal_legacy_workflow(
    artifact_dir: impl AsRef<Path>,
) -> WorkflowResult<WorkflowIntegrityGenesisSeal> {
    let artifact_dir = artifact_dir.as_ref();
    let _writer_lease = WorkflowWriterLease::acquire(artifact_dir)?;
    let records = read_workflow_event_records(artifact_dir)?;
    let key = load_local_hmac_key()?.ok_or(IntegrityError::KeyMissing)?;
    if records.iter().any(|record| record.integrity.is_some()) {
        let legacy_count = records
            .iter()
            .take_while(|record| record.integrity.is_none())
            .count();
        return verify_genesis_seal(artifact_dir, &records[..legacy_count], &key)
            .map(|(seal, _)| seal);
    }
    let state = read_state(artifact_dir)?;
    let legacy_eventlog_sha256 = sha256_prefixed(&canonical_eventlog_bytes(&records)?);
    let sealed_at_ms = now_ms();
    let key_id = key.key_id().to_string();
    let payload = WorkflowIntegrityGenesisPayload {
        schema: WORKFLOW_INTEGRITY_GENESIS_SCHEMA,
        workflow_id: &state.workflow_id,
        legacy_event_count: records.len() as u64,
        legacy_eventlog_sha256: &legacy_eventlog_sha256,
        sealed_at_ms,
        key_id: &key_id,
    };
    let integrity = key.sign_payload(
        "kiana.workflow-integrity-genesis.v1",
        &serde_json::to_vec(&payload)?,
        "genesis",
    )?;
    let seal = WorkflowIntegrityGenesisSeal {
        schema: WORKFLOW_INTEGRITY_GENESIS_SCHEMA.to_string(),
        workflow_id: state.workflow_id,
        legacy_event_count: records.len() as u64,
        legacy_eventlog_sha256,
        sealed_at_ms,
        key_id,
        integrity,
    };
    let seal_bytes = serde_json::to_vec_pretty(&seal)?;
    let artifacts = [WorkflowArtifactInput {
        relative_path: WORKFLOW_INTEGRITY_GENESIS_PATH.to_string(),
        contents: seal_bytes.clone(),
    }];
    let resolved = preflight_artifacts(artifact_dir, &artifacts).map_err(|error| match error {
        WorkflowError::ArtifactConflict { .. } => {
            WorkflowError::InvalidEventLog("integrity_genesis_conflict".to_string())
        }
        other => other,
    })?;
    write_missing_artifacts(artifact_dir, &resolved)?;
    let seal_sha256 = sha256_prefixed(&seal_bytes);
    let event = WorkflowEvent {
        schema: "kiana.workflow-event.v1".to_string(),
        seq: records.len() as u64 + 1,
        at_ms: now_ms(),
        kind: WorkflowEventKind::ArtifactWritten,
        node_id: "runtime_integrity".to_string(),
        data: json!({
            "integrity_genesis_path": WORKFLOW_INTEGRITY_GENESIS_PATH,
            "integrity_genesis_sha256": seal_sha256,
            "legacy_event_count": seal.legacy_event_count,
            "key_id": seal.key_id,
        }),
    };
    write_workflow_event_under_lease(artifact_dir, event)?;
    Ok(seal)
}

pub fn inspect_workflow_integrity(
    artifact_dir: impl AsRef<Path>,
) -> WorkflowResult<WorkflowIntegrityReport> {
    let artifact_dir = artifact_dir.as_ref();
    let (records, key_missing) = match read_workflow_event_records(artifact_dir) {
        Ok(records) => (records, false),
        Err(WorkflowError::Integrity(IntegrityError::KeyMissing)) => {
            (read_workflow_event_records_without_key(artifact_dir)?, true)
        }
        Err(error) => return Err(error),
    };
    let events = records
        .iter()
        .map(|record| record.event.clone())
        .collect::<Vec<_>>();
    ensure_state_matches_events(artifact_dir, &events)?;
    read_verified_workflow_dag(artifact_dir, &events)?;
    let state = read_state(artifact_dir)?;
    let legacy_prefix_count = records
        .iter()
        .take_while(|record| record.integrity.is_none())
        .count() as u64;
    let verified_event_count = records.len() as u64 - legacy_prefix_count;
    let first_authenticated_seq = records
        .iter()
        .find(|record| record.integrity.is_some())
        .map(|record| record.event.seq);
    let last_authenticated_seq = records
        .iter()
        .rev()
        .find(|record| record.integrity.is_some())
        .map(|record| record.event.seq);
    let key_id = records
        .iter()
        .find_map(|record| record.integrity.as_ref().map(|value| value.key_id.clone()));
    let status = if key_missing {
        WorkflowTrustStatus::UnverifiableKeyMissing
    } else if verified_event_count == 0 {
        WorkflowTrustStatus::UnsignedLegacy
    } else if legacy_prefix_count == 0 {
        WorkflowTrustStatus::Verified
    } else {
        WorkflowTrustStatus::SealedLegacyPrefix
    };
    let (artifact_descriptor_count, verified_artifact_count, orphan_artifact_count) =
        if key_missing || verified_event_count == 0 {
            (0, 0, 0)
        } else {
            verify_workflow_artifact_integrity(artifact_dir, &records)?
        };
    Ok(WorkflowIntegrityReport {
        schema: WORKFLOW_INTEGRITY_REPORT_SCHEMA.to_string(),
        workflow_id: state.workflow_id,
        status,
        event_count: records.len() as u64,
        legacy_prefix_count,
        verified_event_count,
        first_authenticated_seq,
        last_authenticated_seq,
        key_id,
        genesis_seal_path: (legacy_prefix_count > 0 && verified_event_count > 0)
            .then(|| WORKFLOW_INTEGRITY_GENESIS_PATH.to_string()),
        artifact_descriptor_count,
        verified_artifact_count,
        orphan_artifact_count,
    })
}

#[derive(Debug)]
struct ResolvedWorkflowArtifact {
    relative_path: String,
    target_path: PathBuf,
    contents: Vec<u8>,
    existed: bool,
}

fn workflow_artifact_descriptors(
    resolved: &[ResolvedWorkflowArtifact],
) -> Vec<WorkflowArtifactDescriptor> {
    let mut descriptors = resolved
        .iter()
        .map(|artifact| WorkflowArtifactDescriptor {
            schema: WORKFLOW_ARTIFACT_DESCRIPTOR_SCHEMA.to_string(),
            path: artifact.relative_path.clone(),
            size: artifact.contents.len() as u64,
            sha256: sha256_prefixed(&artifact.contents),
            media_type: workflow_artifact_media_type(&artifact.relative_path).to_string(),
            role: workflow_artifact_role(&artifact.relative_path).to_string(),
        })
        .collect::<Vec<_>>();
    descriptors.sort_by(|left, right| left.path.cmp(&right.path));
    descriptors
}

fn workflow_artifact_media_type(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("json") => "application/json",
        Some("md") => "text/markdown",
        Some("yaml" | "yml") => "application/yaml",
        Some("toml") => "application/toml",
        _ => "application/octet-stream",
    }
}

fn workflow_artifact_role(path: &str) -> &'static str {
    let components = Path::new(path)
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    if components.len() == 5
        && components[0] == "integrations"
        && components[2] == "recovery"
        && components[3] == "history"
        && components[4].ends_with(".json")
    {
        "recovery_journal_archive"
    } else if path.starts_with("verification/") || path.ends_with("/verification.json") {
        "verification_packet"
    } else if path.starts_with("review/") {
        "review_packet"
    } else if path.ends_with("/result-packet.json") {
        "result_packet"
    } else if path.ends_with("/workpacket.json") {
        "workpacket"
    } else if path.ends_with("/packet.json") {
        "packet"
    } else if path.ends_with("/manifest.json") {
        "manifest"
    } else {
        "workflow_artifact"
    }
}

fn verify_workflow_artifact_integrity(
    artifact_dir: &Path,
    records: &[WorkflowEventRecord],
) -> WorkflowResult<(u64, u64, u64)> {
    let artifact_root = fs::canonicalize(artifact_dir)?;
    let mut described_paths = BTreeSet::new();
    let mut descriptor_count = 0u64;
    let mut verified_count = 0u64;

    for record in records.iter().filter(|record| record.integrity.is_some()) {
        let Some(raw_descriptors) = record.event.data.get(WORKFLOW_INTEGRITY_ARTIFACTS_FIELD)
        else {
            continue;
        };
        let descriptors: Vec<WorkflowArtifactDescriptor> =
            serde_json::from_value(raw_descriptors.clone()).map_err(|error| {
                IntegrityError::ArtifactMismatch(format!("descriptor_schema:{error}"))
            })?;
        let mut previous_path: Option<&str> = None;
        for descriptor in &descriptors {
            descriptor_count += 1;
            if descriptor.schema != WORKFLOW_ARTIFACT_DESCRIPTOR_SCHEMA {
                return Err(IntegrityError::ArtifactMismatch(descriptor.path.clone()).into());
            }
            let normalized = normalize_artifact_relative_path(&descriptor.path)
                .map_err(|_| IntegrityError::ArtifactMismatch(descriptor.path.clone()))?;
            if normalized != descriptor.path
                || previous_path.is_some_and(|previous| previous >= descriptor.path.as_str())
                || !described_paths.insert(descriptor.path.clone())
                || descriptor.media_type != workflow_artifact_media_type(&descriptor.path)
                || descriptor.role != workflow_artifact_role(&descriptor.path)
            {
                return Err(IntegrityError::ArtifactMismatch(descriptor.path.clone()).into());
            }
            previous_path = Some(&descriptor.path);
            let target = resolve_artifact_target(&artifact_root, &descriptor.path)
                .map_err(|_| IntegrityError::ArtifactMismatch(descriptor.path.clone()))?;
            let metadata = fs::symlink_metadata(&target)
                .map_err(|_| IntegrityError::ArtifactMismatch(descriptor.path.clone()))?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(IntegrityError::ArtifactMismatch(descriptor.path.clone()).into());
            }
            let contents = fs::read(&target)
                .map_err(|_| IntegrityError::ArtifactMismatch(descriptor.path.clone()))?;
            if descriptor.size != contents.len() as u64
                || descriptor.sha256 != sha256_prefixed(&contents)
            {
                return Err(IntegrityError::ArtifactMismatch(descriptor.path.clone()).into());
            }
            verified_count += 1;
        }
    }

    let orphan_paths = workflow_managed_orphan_artifacts(artifact_dir, &described_paths)?;
    if let Some(path) = orphan_paths.first() {
        return Err(IntegrityError::ArtifactMismatch(format!("orphan:{path}")).into());
    }
    Ok((descriptor_count, verified_count, orphan_paths.len() as u64))
}

fn workflow_managed_orphan_artifacts(
    artifact_dir: &Path,
    described_paths: &BTreeSet<String>,
) -> WorkflowResult<Vec<String>> {
    const ROOTS: &[&str] = &[
        "verification",
        "review",
        "workers",
        "swarm",
        "integrations",
        "results",
    ];
    let mut files = Vec::new();
    for root in ROOTS {
        let path = artifact_dir.join(root);
        if path.exists() {
            collect_workflow_artifact_files(artifact_dir, &path, &mut files)?;
        }
    }
    files.sort();
    files.dedup();
    Ok(files
        .into_iter()
        .filter(|path| workflow_artifact_requires_binding(path))
        .filter(|path| !described_paths.contains(path))
        .collect())
}

fn collect_workflow_artifact_files(
    artifact_dir: &Path,
    current: &Path,
    files: &mut Vec<String>,
) -> WorkflowResult<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            let relative = path
                .strip_prefix(artifact_dir)
                .map_err(|_| WorkflowError::InvalidArtifactPath(path.display().to_string()))?;
            return Err(IntegrityError::ArtifactMismatch(
                relative.to_string_lossy().replace('\\', "/"),
            )
            .into());
        }
        if metadata.is_dir() {
            collect_workflow_artifact_files(artifact_dir, &path, files)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(artifact_dir)
                .map_err(|_| WorkflowError::InvalidArtifactPath(path.display().to_string()))?;
            files.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

fn workflow_artifact_requires_binding(path: &str) -> bool {
    if path == "review/scope.md" {
        return false;
    }
    let components = Path::new(path)
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    if components.len() == 4
        && components[0] == "integrations"
        && components[2] == "recovery"
        && components[3] == "journal.json"
    {
        return false;
    }
    let name = Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if name.starts_with('.')
        || name.ends_with(".tmp")
        || name.ends_with(".lock")
        || name.contains(".tmp-")
        || name.contains(".recovery-")
    {
        return false;
    }
    workflow_artifact_role(path) != "workflow_artifact"
}

fn ensure_state_matches_events(
    artifact_dir: &Path,
    events: &[WorkflowEvent],
) -> WorkflowResult<()> {
    let state = read_state(artifact_dir)?;
    let eventlog_seq = events.last().map(|event| event.seq).unwrap_or(0);
    ensure_state_matches_events_through(artifact_dir, &state, events, eventlog_seq)
}

fn ensure_state_matches_events_through(
    artifact_dir: &Path,
    state: &WorkflowState,
    events: &[WorkflowEvent],
    expected_seq: u64,
) -> WorkflowResult<()> {
    let directory_run_id = artifact_dir
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            WorkflowError::InconsistentState(
                "workflow artifact directory has no valid run id".to_string(),
            )
        })?;
    let mut mismatches = Vec::new();
    if state.schema != "kiana.workflow-state.v1" {
        mismatches.push(format!(
            "schema {} does not match kiana.workflow-state.v1",
            state.schema
        ));
    }
    if state.run_id != directory_run_id {
        mismatches.push(format!(
            "run_id {} does not match directory {}",
            state.run_id, directory_run_id
        ));
    }
    if state.last_event_seq != expected_seq {
        mismatches.push(format!(
            "state last_event_seq {} does not match eventlog {}",
            state.last_event_seq, expected_seq
        ));
    }
    if let Some(projection) = project_workflow_state_through(events, expected_seq)? {
        if state.workflow_id != projection.workflow_id {
            mismatches.push(format!(
                "workflow_id {} does not match EventLog {}",
                state.workflow_id, projection.workflow_id
            ));
        }
        if state.run_id != projection.run_id {
            mismatches.push(format!(
                "run_id {} does not match EventLog {}",
                state.run_id, projection.run_id
            ));
        }
        if state.status != projection.status {
            mismatches.push(format!(
                "status {:?} does not match EventLog {:?}",
                state.status, projection.status
            ));
        }
        if state.current_node != projection.current_node {
            mismatches.push(format!(
                "current_node {} does not match EventLog {}",
                state.current_node, projection.current_node
            ));
        }
        if state.request != projection.request {
            mismatches.push("request does not match EventLog".to_string());
        }
        if state.input_kind != projection.input_kind {
            mismatches.push("input_kind does not match EventLog".to_string());
        }
        if state.profile != projection.profile {
            mismatches.push("profile does not match EventLog".to_string());
        }
        if state.approval_required != projection.approval_required {
            mismatches.push("approval_required does not match EventLog".to_string());
        }
        if state.checkpoint != projection.checkpoint {
            mismatches.push("checkpoint does not match EventLog".to_string());
        }
        if state.pending_approvals != projection.pending_approvals {
            mismatches.push("pending_approvals do not match EventLog".to_string());
        }
        if state.created_at_ms != projection.created_at_ms {
            mismatches.push("created_at_ms does not match run identity".to_string());
        }
        if state.updated_at_ms != projection.updated_at_ms {
            mismatches.push("updated_at_ms does not match EventLog".to_string());
        }
    }
    if !mismatches.is_empty() {
        return Err(WorkflowError::InconsistentState(format!(
            "state projection mismatch: {}",
            mismatches.join("; ")
        )));
    }
    Ok(())
}

#[derive(Debug)]
struct WorkflowStateProjection {
    workflow_id: String,
    run_id: String,
    status: WorkflowStatus,
    current_node: String,
    request: String,
    input_kind: WorkflowInputKind,
    profile: WorkflowProfile,
    approval_required: bool,
    checkpoint: String,
    pending_approvals: Vec<String>,
    created_at_ms: u64,
    updated_at_ms: u64,
}

fn workflow_created_event(events: &[WorkflowEvent]) -> WorkflowResult<Option<&WorkflowEvent>> {
    let created = events
        .iter()
        .filter(|event| event.kind == WorkflowEventKind::WorkflowCreated)
        .collect::<Vec<_>>();
    if created.len() > 1 {
        return Err(WorkflowError::InvalidEventLog(
            "workflow contains multiple WorkflowCreated events".to_string(),
        ));
    }
    Ok(created.into_iter().next())
}

fn project_workflow_state(
    events: &[WorkflowEvent],
) -> WorkflowResult<Option<WorkflowStateProjection>> {
    let through_seq = events.last().map(|event| event.seq).unwrap_or(0);
    project_workflow_state_through(events, through_seq)
}

fn project_workflow_state_through(
    events: &[WorkflowEvent],
    through_seq: u64,
) -> WorkflowResult<Option<WorkflowStateProjection>> {
    let Some(created) = workflow_created_event(events)? else {
        return Ok(None);
    };
    let required_string = |field: &str| {
        created
            .data
            .get(field)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                WorkflowError::InvalidEventLog(format!(
                    "WorkflowCreated is missing required field {field}"
                ))
            })
    };
    let workflow_id = required_string("workflow_id")?;
    let run_id = required_string("run_id")?;
    let created_at_ms = workflow_run_created_at_ms(&run_id)?;
    let request = required_string("request")?;
    let input_kind =
        serde_json::from_value(created.data.get("input_kind").cloned().ok_or_else(|| {
            WorkflowError::InvalidEventLog("WorkflowCreated is missing input_kind".to_string())
        })?)?;
    let profile =
        serde_json::from_value(created.data.get("profile").cloned().ok_or_else(|| {
            WorkflowError::InvalidEventLog("WorkflowCreated is missing profile".to_string())
        })?)?;
    let approval_required = created
        .data
        .get("approval_required")
        .and_then(Value::as_bool)
        .ok_or_else(|| {
            WorkflowError::InvalidEventLog(
                "WorkflowCreated is missing approval_required".to_string(),
            )
        })?;
    let updated_at_ms = events
        .iter()
        .take_while(|event| event.seq <= through_seq)
        .last()
        .map(|event| event.at_ms)
        .unwrap_or(created_at_ms);
    let mut projection = WorkflowStateProjection {
        workflow_id,
        run_id,
        status: WorkflowStatus::Initialized,
        current_node: created
            .data
            .get("initial_node")
            .and_then(Value::as_str)
            .unwrap_or("capture")
            .to_string(),
        request,
        input_kind,
        profile,
        approval_required,
        checkpoint: "initial".to_string(),
        pending_approvals: if approval_required {
            vec!["capture:approval_required".to_string()]
        } else {
            Vec::new()
        },
        created_at_ms,
        updated_at_ms,
    };
    for event in events
        .iter()
        .skip_while(|event| event.seq < created.seq)
        .take_while(|event| event.seq <= through_seq)
    {
        match event.kind {
            WorkflowEventKind::NodeEntered => {
                projection.current_node = event.node_id.clone();
                projection.status = WorkflowStatus::Running;
            }
            WorkflowEventKind::WorkflowBlocked => {
                projection.status = WorkflowStatus::Blocked;
            }
            WorkflowEventKind::WorkflowCompleted => {
                projection.status = WorkflowStatus::Completed;
            }
            WorkflowEventKind::WorkflowCancelled => {
                projection.status = WorkflowStatus::Cancelled;
            }
            _ => {}
        }
    }
    Ok(Some(projection))
}

fn read_verified_workflow_dag(
    artifact_dir: &Path,
    events: &[WorkflowEvent],
) -> WorkflowResult<WorkflowDagTemplate> {
    let bytes = fs::read(artifact_dir.join("workflow_dag.json"))?;
    let dag: WorkflowDagTemplate = serde_json::from_slice(&bytes)?;
    if dag.schema != "kiana.workflow-dag.v1" {
        return Err(WorkflowError::InconsistentState(format!(
            "workflow DAG schema is incompatible: {}",
            dag.schema
        )));
    }
    let created = workflow_created_event(events)?.ok_or_else(|| {
        WorkflowError::InvalidEventLog("WorkflowCreated event is missing".to_string())
    })?;
    if let Some(expected) = created.data.get("dag_sha256").and_then(Value::as_str) {
        let actual = sha256_prefixed(&bytes);
        if actual != expected {
            return Err(WorkflowError::InconsistentState(format!(
                "workflow DAG integrity mismatch: expected {expected}, found {actual}"
            )));
        }
    } else if serde_json::to_value(&dag)? != serde_json::to_value(default_workflow_template())? {
        return Err(WorkflowError::InconsistentState(
            "workflow DAG integrity mismatch for legacy run".to_string(),
        ));
    }
    let node_ids = dag
        .nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<BTreeSet<_>>();
    if node_ids.len() != dag.nodes.len()
        || dag.edges.iter().any(|edge| {
            edge.decision.trim().is_empty()
                || !node_ids.contains(edge.from.as_str())
                || !node_ids.contains(edge.to.as_str())
        })
    {
        return Err(WorkflowError::InconsistentState(
            "workflow DAG integrity validation failed".to_string(),
        ));
    }
    Ok(dag)
}

fn preflight_artifacts(
    artifact_dir: &Path,
    artifacts: &[WorkflowArtifactInput],
) -> WorkflowResult<Vec<ResolvedWorkflowArtifact>> {
    if artifacts.is_empty() {
        return Err(WorkflowError::InvalidArtifactPath(
            "artifact batch cannot be empty".to_string(),
        ));
    }
    let artifact_root = fs::canonicalize(artifact_dir)?;
    let mut seen = BTreeSet::new();
    let mut resolved = Vec::with_capacity(artifacts.len());
    for artifact in artifacts {
        let relative_path = normalize_artifact_relative_path(&artifact.relative_path)?;
        if !seen.insert(relative_path.clone()) {
            return Err(WorkflowError::InvalidArtifactPath(format!(
                "duplicate artifact path {relative_path}"
            )));
        }
        let target_path = resolve_artifact_target(&artifact_root, &relative_path)?;
        let existed = match fs::symlink_metadata(&target_path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(WorkflowError::InvalidArtifactPath(relative_path));
                }
                let existing = fs::read(&target_path)?;
                if existing != artifact.contents {
                    return Err(WorkflowError::ArtifactConflict {
                        path: relative_path,
                    });
                }
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(WorkflowError::Io(error)),
        };
        resolved.push(ResolvedWorkflowArtifact {
            relative_path,
            target_path,
            contents: artifact.contents.clone(),
            existed,
        });
    }
    Ok(resolved)
}

fn normalize_artifact_relative_path(raw: &str) -> WorkflowResult<String> {
    let raw = raw.trim().replace('\\', "/");
    let path = Path::new(&raw);
    if raw.is_empty()
        || path.is_absolute()
        || raw.as_bytes().get(1) == Some(&b':')
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(WorkflowError::InvalidArtifactPath(raw));
    }
    let components = path
        .components()
        .filter_map(|component| match component {
            Component::CurDir => None,
            Component::Normal(value) => value.to_str().map(str::to_string),
            _ => None,
        })
        .collect::<Vec<_>>();
    if components.is_empty() {
        return Err(WorkflowError::InvalidArtifactPath(raw));
    }
    Ok(components.join("/"))
}

fn resolve_artifact_target(artifact_root: &Path, relative_path: &str) -> WorkflowResult<PathBuf> {
    let requested = artifact_root.join(relative_path);
    if fs::symlink_metadata(&requested).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(WorkflowError::InvalidArtifactPath(
            relative_path.to_string(),
        ));
    }

    let mut existing = requested.as_path();
    let mut missing = Vec::new();
    loop {
        match fs::symlink_metadata(existing) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(existing.file_name().ok_or_else(|| {
                    WorkflowError::InvalidArtifactPath(relative_path.to_string())
                })?);
                existing = existing
                    .parent()
                    .ok_or_else(|| WorkflowError::InvalidArtifactPath(relative_path.to_string()))?;
            }
            Err(error) => return Err(WorkflowError::Io(error)),
        }
    }
    let mut resolved = fs::canonicalize(existing)?;
    if !resolved.starts_with(artifact_root) {
        return Err(WorkflowError::InvalidArtifactPath(
            relative_path.to_string(),
        ));
    }
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    if !resolved.starts_with(artifact_root) {
        return Err(WorkflowError::InvalidArtifactPath(
            relative_path.to_string(),
        ));
    }
    Ok(resolved)
}

fn write_missing_artifacts(
    artifact_root: &Path,
    artifacts: &[ResolvedWorkflowArtifact],
) -> WorkflowResult<()> {
    let canonical_root = fs::canonicalize(artifact_root)?;
    for artifact in artifacts.iter().filter(|artifact| !artifact.existed) {
        let current_target = resolve_artifact_target(&canonical_root, &artifact.relative_path)?;
        if current_target != artifact.target_path {
            return Err(WorkflowError::InvalidArtifactPath(
                artifact.relative_path.clone(),
            ));
        }
        let parent = artifact
            .target_path
            .parent()
            .ok_or_else(|| WorkflowError::InvalidArtifactPath(artifact.relative_path.clone()))?;
        fs::create_dir_all(parent)?;
        let canonical_parent = fs::canonicalize(parent)?;
        if !canonical_parent.starts_with(&canonical_root) || canonical_parent != parent {
            return Err(WorkflowError::InvalidArtifactPath(
                artifact.relative_path.clone(),
            ));
        }
        let file_name = artifact
            .target_path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| WorkflowError::InvalidArtifactPath(artifact.relative_path.clone()))?;
        let temp_path = canonical_parent.join(format!(
            ".{file_name}.tmp-{}-{}",
            std::process::id(),
            random_suffix(8)
        ));
        let mut temp = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)?;
        temp.write_all(&artifact.contents)?;
        temp.flush()?;
        temp.sync_data()?;
        let latest_parent = fs::canonicalize(parent)?;
        let latest_target = resolve_artifact_target(&canonical_root, &artifact.relative_path)?;
        if latest_parent != canonical_parent
            || !latest_parent.starts_with(&canonical_root)
            || latest_target != artifact.target_path
            || latest_target.parent() != Some(latest_parent.as_path())
        {
            let _ = fs::remove_file(&temp_path);
            return Err(WorkflowError::InvalidArtifactPath(
                artifact.relative_path.clone(),
            ));
        }
        match fs::hard_link(&temp_path, &latest_target) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing = fs::read(&artifact.target_path)?;
                if existing != artifact.contents {
                    let _ = fs::remove_file(&temp_path);
                    return Err(WorkflowError::ArtifactConflict {
                        path: artifact.relative_path.clone(),
                    });
                }
            }
            Err(error) => {
                let _ = fs::remove_file(&temp_path);
                return Err(WorkflowError::Io(error));
            }
        }
        fs::remove_file(&temp_path)?;
    }
    Ok(())
}

struct WorkflowWriterLease {
    _file: fs::File,
}

impl WorkflowWriterLease {
    fn acquire(artifact_dir: &Path) -> WorkflowResult<Self> {
        let path = artifact_dir.join(".eventlog.lock");
        if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(WorkflowError::InvalidArtifactPath(
                path.display().to_string(),
            ));
        }
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&path)?;
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            match file.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    return Err(WorkflowError::WriterBusy(path.display().to_string()));
                }
                Err(std::fs::TryLockError::Error(error)) => {
                    return Err(WorkflowError::Io(error));
                }
            }
        }
        file.set_len(0)?;
        writeln!(
            file,
            "pid={} acquired_at_ms={}",
            std::process::id(),
            now_ms()
        )?;
        file.flush()?;
        file.sync_data()?;
        Ok(Self { _file: file })
    }
}

pub fn list_workflow_runs(root: impl AsRef<Path>) -> WorkflowResult<Vec<WorkflowRunSummary>> {
    let workflows_dir = root.as_ref().join(".kiana").join("workflows");
    if !workflows_dir.exists() {
        return Ok(Vec::new());
    }

    let mut runs = Vec::new();
    for entry in fs::read_dir(&workflows_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let artifact_dir = entry.path();
        let directory_run_id = entry.file_name().to_string_lossy().to_string();
        validate_run_id(&directory_run_id)?;
        let state = read_state(&artifact_dir)?;
        if state.run_id != directory_run_id {
            return Err(WorkflowError::InconsistentState(format!(
                "directory run_id {} does not match state run_id {}",
                directory_run_id, state.run_id
            )));
        }
        let events = read_eventlog(&artifact_dir)?;
        ensure_state_matches_events(&artifact_dir, &events)?;
        read_verified_workflow_dag(&artifact_dir, &events)?;
        runs.push(summary_from_state(state, artifact_dir));
    }

    runs.sort_by(|left, right| {
        right
            .updated_at_ms
            .cmp(&left.updated_at_ms)
            .then_with(|| right.run_id.cmp(&left.run_id))
    });
    Ok(runs)
}

pub fn resume_workflow_run(
    root: impl AsRef<Path>,
    requested_run_id: Option<&str>,
) -> WorkflowResult<WorkflowResumeReport> {
    let root = root.as_ref();
    let artifact_dir = if let Some(run_id) = requested_run_id {
        validate_run_id(run_id)?;
        let artifact_dir = root.join(".kiana").join("workflows").join(run_id);
        if !artifact_dir.is_dir() {
            return Err(WorkflowError::RunNotFound(run_id.to_string()));
        }
        artifact_dir
    } else {
        latest_workflow_artifact_dir(root)?
    };

    let _writer_lease = WorkflowWriterLease::acquire(&artifact_dir)?;
    let events = read_eventlog(&artifact_dir)?;
    let eventlog_last_seq = events.last().map(|event| event.seq).unwrap_or(0);
    let mut state = read_state(&artifact_dir)?;
    let state_before_resume_seq = state.last_event_seq;
    let (resume_status, blocker) = if state.last_event_seq == eventlog_last_seq {
        match ensure_state_matches_events_through(&artifact_dir, &state, &events, eventlog_last_seq)
        {
            Ok(()) => (WorkflowResumeStatus::Ready, None),
            Err(error) => (WorkflowResumeStatus::Blocked, Some(error.to_string())),
        }
    } else if state.last_event_seq < eventlog_last_seq {
        match ensure_state_matches_events_through(
            &artifact_dir,
            &state,
            &events,
            state.last_event_seq,
        ) {
            Ok(()) => {
                state = rebuild_workflow_state_from_events(state, &events)?;
                write_workflow_state_atomic(&artifact_dir.join("state.json"), &state)?;
                ensure_state_matches_events_through(
                    &artifact_dir,
                    &state,
                    &events,
                    eventlog_last_seq,
                )?;
                (WorkflowResumeStatus::Repaired, None)
            }
            Err(error) => (WorkflowResumeStatus::Blocked, Some(error.to_string())),
        }
    } else {
        (
            WorkflowResumeStatus::Blocked,
            Some(
                WorkflowError::InconsistentState(format!(
                    "state last_event_seq {} does not match eventlog {}",
                    state.last_event_seq, eventlog_last_seq
                ))
                .to_string(),
            ),
        )
    };
    let eventlog_consistent = blocker.is_none();
    let run = summary_from_state(state.clone(), artifact_dir);
    let recommended_action = if eventlog_consistent {
        format!("continue:{}", state.current_node)
    } else {
        "repair:reconcile-eventlog".to_string()
    };

    Ok(WorkflowResumeReport {
        schema: "kiana.workflow-resume.v1".to_string(),
        resume_status,
        run,
        event_count: events.len(),
        eventlog_last_seq,
        state_last_event_seq: if resume_status == WorkflowResumeStatus::Repaired {
            state.last_event_seq
        } else {
            state_before_resume_seq
        },
        eventlog_consistent,
        blocker,
        recommended_action,
    })
}

fn latest_workflow_artifact_dir(root: &Path) -> WorkflowResult<PathBuf> {
    let workflows_dir = root.join(".kiana").join("workflows");
    if !workflows_dir.is_dir() {
        return Err(WorkflowError::NoRuns);
    }
    let mut candidates = Vec::new();
    for entry in fs::read_dir(workflows_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let artifact_dir = entry.path();
        let directory_run_id = entry.file_name().to_string_lossy().to_string();
        validate_run_id(&directory_run_id)?;
        let state = read_state(&artifact_dir)?;
        if state.run_id != directory_run_id {
            return Err(WorkflowError::InconsistentState(format!(
                "directory run_id {} does not match state run_id {}",
                directory_run_id, state.run_id
            )));
        }
        candidates.push((state.updated_at_ms, directory_run_id, artifact_dir));
    }
    candidates.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    candidates
        .into_iter()
        .next()
        .map(|(_, _, artifact_dir)| artifact_dir)
        .ok_or(WorkflowError::NoRuns)
}

fn rebuild_workflow_state_from_events(
    mut state: WorkflowState,
    events: &[WorkflowEvent],
) -> WorkflowResult<WorkflowState> {
    let projection = project_workflow_state(events)?.ok_or_else(|| {
        WorkflowError::InvalidEventLog(
            "workflow eventlog cannot rebuild state without WorkflowCreated".to_string(),
        )
    })?;
    state.workflow_id = projection.workflow_id;
    state.run_id = projection.run_id;
    state.status = projection.status;
    state.current_node = projection.current_node;
    state.request = projection.request;
    state.input_kind = projection.input_kind;
    state.profile = projection.profile;
    state.approval_required = projection.approval_required;
    state.schema = "kiana.workflow-state.v1".to_string();
    state.checkpoint = projection.checkpoint;
    state.pending_approvals = projection.pending_approvals;
    state.created_at_ms = projection.created_at_ms;
    state.updated_at_ms = projection.updated_at_ms;
    if let Some(last) = events.last() {
        state.last_event_seq = last.seq;
    }
    Ok(state)
}

fn workflow_run_created_at_ms(run_id: &str) -> WorkflowResult<u64> {
    let timestamp = run_id
        .strip_prefix("run-")
        .and_then(|value| value.split_once('-').map(|(timestamp, _)| timestamp))
        .ok_or_else(|| WorkflowError::InvalidRunId(run_id.to_string()))?;
    timestamp
        .parse::<u64>()
        .map_err(|_| WorkflowError::InvalidRunId(run_id.to_string()))
}

fn node(
    id: &str,
    title: &str,
    stage: WorkflowNodeStage,
    node_type: WorkflowNodeType,
    actions: &[&str],
    writes: &[&str],
) -> WorkflowNodeSpec {
    WorkflowNodeSpec {
        id: id.to_string(),
        title: title.to_string(),
        stage,
        node_type,
        actions: actions.iter().map(|value| value.to_string()).collect(),
        gate: if matches!(node_type, WorkflowNodeType::Gate | WorkflowNodeType::Router) {
            Some(WorkflowGateSpec {
                id: format!("{id}_gate"),
                decisions: Vec::new(),
            })
        } else {
            None
        },
        reads: Vec::new(),
        writes: writes.iter().map(|value| value.to_string()).collect(),
    }
}

fn default_edges() -> Vec<WorkflowEdge> {
    let edges = [
        ("runtime_init", "capture", "initialized"),
        ("capture", "product_definition", "clear"),
        ("capture", "clarify_question", "vague"),
        ("capture", "split_goal", "too_large"),
        ("capture", "approval_gate", "high_risk"),
        ("capture", "block_handoff", "unsafe"),
        ("clarify_question", "capture", "answered"),
        ("split_goal", "capture", "split"),
        ("approval_gate", "product_definition", "approved"),
        ("approval_gate", "block_handoff", "rejected"),
        ("product_definition", "context_intake", "not_needed"),
        ("product_definition", "product_office_hours", "needed"),
        ("product_office_hours", "context_intake", "accepted"),
        ("product_office_hours", "capture", "goal_changed"),
        ("context_intake", "intent_router", "fresh_enough"),
        ("context_intake", "refresh_memory", "stale"),
        ("context_intake", "create_isolation", "dirty_git_state"),
        ("refresh_memory", "context_intake", "refreshed"),
        ("create_isolation", "intent_router", "ready"),
        ("intent_router", "product_office_hours", "product_scope"),
        ("intent_router", "investigate", "bug_error"),
        ("intent_router", "qa_planning", "qa"),
        ("intent_router", "review_scope", "review_diff"),
        ("intent_router", "ship", "ship"),
        ("intent_router", "research", "feature_refactor_research"),
        ("investigate", "fix_loop", "root_cause_reported"),
        ("qa_planning", "behavior_verify", "planned"),
        ("research", "design", "enough"),
        ("research", "research", "missing_facts"),
        ("design", "plan", "accepted"),
        ("design", "decision_briefs", "needs_tradeoff"),
        ("design", "capture", "goal_mismatch"),
        ("design", "design_alternative", "architecture_risk"),
        ("design", "approval_gate", "requires_approval"),
        ("design_alternative", "design", "ready"),
        ("plan", "plan_confirmation", "executable"),
        ("plan", "plan", "needs_fix"),
        ("plan", "capture", "goal_mismatch"),
        ("plan_confirmation", "autoplan_review", "continue"),
        ("plan_confirmation", "plan", "invalid"),
        ("plan_confirmation", "context_intake", "repo_changed"),
        ("autoplan_review", "decision_briefs", "approved"),
        ("autoplan_review", "plan", "needs_plan_fix"),
        (
            "autoplan_review",
            "product_office_hours",
            "needs_product_reframe",
        ),
        ("autoplan_review", "research", "needs_research"),
        ("autoplan_review", "approval_gate", "needs_approval"),
        ("autoplan_review", "block_handoff", "blocked"),
        ("decision_briefs", "build_workpacket", "all_decisions_made"),
        ("decision_briefs", "research", "needs_research"),
        ("decision_briefs", "capture", "changes_goal"),
        ("decision_briefs", "plan", "changes_plan"),
        ("build_workpacket", "skill_router", "valid"),
        ("build_workpacket", "plan", "missing_bounds"),
        ("build_workpacket", "approval_gate", "risky_action"),
        ("skill_router", "execute", "ready"),
        ("skill_router", "decision_briefs", "low_confidence"),
        ("skill_router", "create_isolation", "needs_isolation"),
        ("skill_router", "plan", "wrong_task_type"),
        ("execute", "quality_gate", "step_done"),
        ("execute", "fix_loop", "failed"),
        ("execute", "plan", "requirement_problem"),
        ("execute", "decision_briefs", "architecture_problem"),
        ("execute", "approval_gate", "risk_discovered"),
        ("execute", "plan", "scope_drift"),
        ("execute", "security_fix", "secret_detected"),
        ("quality_gate", "behavior_verify", "pass"),
        ("quality_gate", "fix_loop", "fixable_failure"),
        ("quality_gate", "security_fix", "security_finding"),
        ("quality_gate", "plan", "scope_drift"),
        ("behavior_verify", "review_scope", "pass"),
        ("behavior_verify", "fix_loop", "test_failed"),
        ("behavior_verify", "quality_gate", "tests_added"),
        ("behavior_verify", "plan", "goal_not_satisfied"),
        ("behavior_verify", "block_handoff", "cannot_verify"),
        ("review_scope", "multi_review", "ready"),
        ("multi_review", "ship", "no_blocking_issues"),
        ("multi_review", "review_triage", "findings_exist"),
        ("multi_review", "capture", "requirement_drift"),
        ("multi_review", "plan", "architecture_issue"),
        ("multi_review", "security_fix", "security_issue"),
        ("multi_review", "quality_gate", "insufficient_tests"),
        ("review_triage", "ship", "all_findings_resolved"),
        ("review_triage", "fix_loop", "fix_required"),
        ("review_triage", "block_handoff", "blocked"),
        ("review_triage", "plan", "replan_required"),
        ("security_fix", "block_handoff", "critical"),
        ("security_fix", "fix_loop", "high"),
        ("security_fix", "review_triage", "medium"),
        ("security_fix", "behavior_verify", "low"),
        ("fix_loop", "quality_gate", "fixed"),
        ("fix_loop", "fix_loop", "retryable"),
        ("fix_loop", "plan", "need_replan"),
        ("fix_loop", "security_fix", "security_problem"),
        ("fix_loop", "block_handoff", "budget_exceeded"),
        ("ship", "learn", "report_only"),
        ("ship", "approval_gate", "push_merge_deploy"),
        ("ship", "quality_gate", "docs_updated"),
        ("approval_gate", "release_action", "release_approved"),
        ("release_action", "learn", "done"),
        ("block_handoff", "learn", "handoff_written"),
        ("learn", "loop_controller", "learned"),
        ("loop_controller", "context_intake", "next_workpacket"),
        ("loop_controller", "context_intake", "next_goal"),
        ("loop_controller", "completed", "done"),
    ];
    edges
        .into_iter()
        .map(|(from, to, decision)| WorkflowEdge {
            from: from.to_string(),
            to: to.to_string(),
            decision: decision.to_string(),
        })
        .collect()
}

fn default_artifact_contract() -> Vec<String> {
    [
        "workflow_dag.json",
        "eventlog.jsonl",
        "state.json",
        "context_pack.md",
        "problem-definition.md",
        "findings.md",
        "task_plan.md",
        "plan-confirmation.md",
        "autoplan-review.md",
        "decision-briefs.md",
        "review/scope.md",
        "reviews/",
        "workpackets/",
        "resultpackets/",
        "evidence/",
        "next-agent-handoff.md",
        "learnings.md",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn create_artifact_shape(artifact_dir: &Path) -> WorkflowResult<()> {
    for dir in [
        "review",
        "reviews",
        "workpackets",
        "resultpackets",
        "evidence",
        "verification",
    ] {
        fs::create_dir_all(artifact_dir.join(dir))?;
    }
    for file in [
        ("context_pack.md", "# Context Pack\n\n"),
        ("problem-definition.md", "# Problem Definition\n\n"),
        ("findings.md", "# Findings\n\n"),
        ("task_plan.md", "# Task Plan\n\n"),
        ("plan-confirmation.md", "# Plan Confirmation\n\n"),
        ("autoplan-review.md", "# Autoplan Review\n\n"),
        ("decision-briefs.md", "# Decision Briefs\n\n"),
        ("next-agent-handoff.md", "# Next Agent Handoff\n\n"),
        ("learnings.md", "# Learnings\n\n"),
        ("review/scope.md", "# Review Scope\n\n"),
    ] {
        let path = artifact_dir.join(file.0);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if !path.exists() {
            fs::write(path, file.1)?;
        }
    }
    Ok(())
}

fn update_state_after_event(artifact_dir: &Path, event: &WorkflowEvent) -> WorkflowResult<()> {
    let mut state = read_state(artifact_dir)?;
    state.updated_at_ms = event.at_ms;
    state.last_event_seq = event.seq;
    match event.kind {
        WorkflowEventKind::NodeEntered => {
            state.current_node = event.node_id.clone();
            state.status = WorkflowStatus::Running;
        }
        WorkflowEventKind::WorkflowBlocked => {
            state.status = WorkflowStatus::Blocked;
        }
        WorkflowEventKind::WorkflowCompleted => {
            state.status = WorkflowStatus::Completed;
        }
        WorkflowEventKind::WorkflowCancelled => {
            state.status = WorkflowStatus::Cancelled;
        }
        _ => {}
    }
    write_workflow_state_atomic(&artifact_dir.join("state.json"), &state)
}

fn read_state(artifact_dir: &Path) -> WorkflowResult<WorkflowState> {
    let contents = fs::read_to_string(artifact_dir.join("state.json"))?;
    Ok(serde_json::from_str(&contents)?)
}

pub fn read_workflow_state(artifact_dir: impl AsRef<Path>) -> WorkflowResult<WorkflowState> {
    read_state(artifact_dir.as_ref())
}

pub fn read_workflow_events(artifact_dir: impl AsRef<Path>) -> WorkflowResult<Vec<WorkflowEvent>> {
    Ok(read_workflow_event_records(artifact_dir.as_ref())?
        .into_iter()
        .map(|record| record.event)
        .collect())
}

fn read_workflow_event_records(artifact_dir: &Path) -> WorkflowResult<Vec<WorkflowEventRecord>> {
    let contents = fs::read_to_string(artifact_dir.join("eventlog.jsonl"))?;
    let mut records: Vec<WorkflowEventRecord> = Vec::new();
    let mut key: Option<LocalHmacKey> = None;
    let mut signed_chain_started = false;
    for (index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record: WorkflowEventRecord = serde_json::from_str(line).map_err(|error| {
            WorkflowError::InvalidEventLog(format!("line {}: {}", index + 1, error))
        })?;
        let expected_seq = records.len() as u64 + 1;
        if record.event.seq != expected_seq {
            return Err(WorkflowError::InvalidEventLog(format!(
                "line {} has seq {}, expected {}",
                index + 1,
                record.event.seq,
                expected_seq
            )));
        }
        match record.integrity.as_ref() {
            Some(integrity) => {
                let expected_previous = records
                    .last()
                    .map(workflow_event_record_sha256)
                    .transpose()?
                    .unwrap_or_else(|| "genesis".to_string());
                if integrity.previous_record_sha256 != expected_previous {
                    return Err(IntegrityError::ChainMismatch.into());
                }
                if key.is_none() {
                    key = load_local_hmac_key()?;
                }
                let key = key.as_ref().ok_or(IntegrityError::KeyMissing)?;
                if !signed_chain_started && !records.is_empty() {
                    validate_first_authenticated_event(artifact_dir, &records, &record.event, key)?;
                }
                let payload = serde_json::to_vec(&record.event)?;
                key.verify_payload("kiana.workflow-event.v2", &payload, integrity)?;
                signed_chain_started = true;
            }
            None if signed_chain_started => {
                return Err(IntegrityError::DowngradeDetected.into());
            }
            None => {}
        }
        records.push(record);
    }
    Ok(records)
}

fn read_workflow_event_records_without_key(
    artifact_dir: &Path,
) -> WorkflowResult<Vec<WorkflowEventRecord>> {
    let contents = fs::read_to_string(artifact_dir.join("eventlog.jsonl"))?;
    let mut records: Vec<WorkflowEventRecord> = Vec::new();
    let mut signed_chain_started = false;
    for (index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record: WorkflowEventRecord = serde_json::from_str(line).map_err(|error| {
            WorkflowError::InvalidEventLog(format!("line {}: {}", index + 1, error))
        })?;
        let expected_seq = records.len() as u64 + 1;
        if record.event.seq != expected_seq {
            return Err(WorkflowError::InvalidEventLog(format!(
                "line {} has seq {}, expected {}",
                index + 1,
                record.event.seq,
                expected_seq
            )));
        }
        match record.integrity.as_ref() {
            Some(integrity) => {
                let expected_previous = records
                    .last()
                    .map(workflow_event_record_sha256)
                    .transpose()?
                    .unwrap_or_else(|| "genesis".to_string());
                if integrity.previous_record_sha256 != expected_previous {
                    return Err(IntegrityError::ChainMismatch.into());
                }
                signed_chain_started = true;
            }
            None if signed_chain_started => {
                return Err(IntegrityError::DowngradeDetected.into());
            }
            None => {}
        }
        records.push(record);
    }
    Ok(records)
}

fn read_eventlog(artifact_dir: &Path) -> WorkflowResult<Vec<WorkflowEvent>> {
    read_workflow_events(artifact_dir)
}

fn summary_from_state(state: WorkflowState, artifact_dir: PathBuf) -> WorkflowRunSummary {
    WorkflowRunSummary {
        schema: "kiana.workflow-run-summary.v1".to_string(),
        workflow_id: state.workflow_id,
        run_id: state.run_id,
        status: state.status,
        current_node: state.current_node,
        request: state.request,
        input_kind: state.input_kind,
        profile: state.profile,
        approval_required: state.approval_required,
        artifact_dir,
        created_at_ms: state.created_at_ms,
        updated_at_ms: state.updated_at_ms,
    }
}

fn validate_run_id(run_id: &str) -> WorkflowResult<()> {
    if run_id.is_empty()
        || run_id.contains('/')
        || run_id.contains('\\')
        || run_id.contains("..")
        || !run_id
            .chars()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_'))
    {
        return Err(WorkflowError::InvalidRunId(run_id.to_string()));
    }
    Ok(())
}

fn write_json_pretty(path: &Path, value: &impl Serialize) -> WorkflowResult<()> {
    let contents = serde_json::to_string_pretty(value)?;
    fs::write(path, contents)?;
    Ok(())
}

fn write_workflow_state_atomic(path: &Path, value: &WorkflowState) -> WorkflowResult<()> {
    let parent = path.parent().ok_or_else(|| {
        WorkflowError::InvalidArtifactPath(format!(
            "workflow state path has no parent: {}",
            path.display()
        ))
    })?;
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| WorkflowError::InvalidArtifactPath(path.display().to_string()))?;
    let temp_path = parent.join(format!(
        ".{file_name}.tmp-{}-{}",
        std::process::id(),
        random_suffix(8)
    ));
    let result = (|| -> WorkflowResult<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)?;
        file.write_all(&serde_json::to_vec_pretty(value)?)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        replace_file_atomically(&temp_path, path)?;
        sync_parent_directory(parent)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

#[cfg(windows)]
fn replace_file_atomically(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_file_atomically(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(unix)]
fn sync_parent_directory(parent: &Path) -> io::Result<()> {
    fs::File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_directory(_parent: &Path) -> io::Result<()> {
    Ok(())
}

fn next_event_seq(path: &Path) -> WorkflowResult<u64> {
    if !path.exists() {
        return Ok(1);
    }
    let artifact_dir = path
        .parent()
        .ok_or_else(|| WorkflowError::InvalidEventLog("eventlog path has no parent".to_string()))?;
    Ok(read_workflow_events(artifact_dir)?.len() as u64 + 1)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn random_suffix(len: usize) -> String {
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| {
            let idx = rng.gen_range(0..WORKFLOW_ID_ALPHABET.len());
            WORKFLOW_ID_ALPHABET[idx] as char
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn artifact_write_rejects_parent_replaced_by_symlink_after_preflight() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "kiana-workflow-artifact-parent-race-{}-{}",
            std::process::id(),
            random_suffix(8)
        ));
        let outside = std::env::temp_dir().join(format!(
            "kiana-workflow-artifact-parent-outside-{}-{}",
            std::process::id(),
            random_suffix(8)
        ));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let resolved = preflight_artifacts(
            &root,
            &[WorkflowArtifactInput {
                relative_path: "eda/reviews/review-1/report.json".to_string(),
                contents: b"{}\n".to_vec(),
            }],
        )
        .unwrap();
        fs::create_dir_all(root.join("eda/reviews")).unwrap();
        symlink(&outside, root.join("eda/reviews/review-1")).unwrap();

        let error = write_missing_artifacts(&root, &resolved).unwrap_err();

        assert!(matches!(error, WorkflowError::InvalidArtifactPath(_)));
        assert!(!outside.join("report.json").exists());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn writer_lease_drop_does_not_remove_a_replacement_lock_file() {
        let root = std::env::temp_dir().join(format!(
            "kiana-workflow-writer-lease-replacement-{}-{}",
            std::process::id(),
            random_suffix(8)
        ));
        fs::create_dir_all(&root).unwrap();
        let lock_path = root.join(".eventlog.lock");
        fs::write(&lock_path, "pid=999999999 acquired_at_ms=0\n").unwrap();

        let lease = WorkflowWriterLease::acquire(&root).unwrap();
        fs::remove_file(&lock_path).unwrap();
        fs::write(&lock_path, "replacement-lock\n").unwrap();
        drop(lease);

        assert_eq!(
            fs::read_to_string(&lock_path).unwrap(),
            "replacement-lock\n"
        );
        let _ = fs::remove_dir_all(root);
    }
}
