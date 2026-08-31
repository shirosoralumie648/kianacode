//! Stable domain contracts for the Kiana control plane.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::path::{Component, Path};
use uuid::Uuid;

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

uuid_id!(RequestId);
uuid_id!(RunId);
uuid_id!(TurnId);
uuid_id!(CellId);
uuid_id!(EventId);
uuid_id!(ExecutionId);
uuid_id!(InvocationId);
uuid_id!(ApprovalId);
uuid_id!(ArtifactId);
uuid_id!(ReceiptId);
uuid_id!(OrganizationId);
uuid_id!(ProjectId);
uuid_id!(TemplateId);
uuid_id!(SpawnPlanId);
uuid_id!(BudgetLeaseId);
uuid_id!(CapabilityGrantId);
uuid_id!(SupervisionLeaseId);
uuid_id!(DelegationId);

impl RunId {
    pub fn parse_str(value: &str) -> Option<Self> {
        Uuid::parse_str(value.trim()).ok().map(Self::from_uuid)
    }
}

pub const APPROVAL_CHALLENGE_SCHEMA: &str = "kiana.approval-challenge.v1";

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(String);

impl SessionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkFingerprint(String);

impl WorkFingerprint {
    pub fn from_parts(
        objective: &str,
        input_refs: &[String],
        partition: &str,
        output_contract: &str,
        policy_snapshot: &str,
    ) -> Result<Self, &'static str> {
        if objective.trim().is_empty()
            || partition.trim().is_empty()
            || output_contract.trim().is_empty()
            || policy_snapshot.trim().is_empty()
        {
            return Err("work_fingerprint_input_required");
        }
        let mut inputs = input_refs
            .iter()
            .map(|input| input.trim())
            .filter(|input| !input.is_empty())
            .collect::<Vec<_>>();
        inputs.sort_unstable();
        let canonical = format!(
            "objective={}\ninputs={}\npartition={}\noutput={}\npolicy={}",
            objective.trim(),
            inputs.join("\u{1f}"),
            partition.trim(),
            output_contract.trim(),
            policy_snapshot.trim(),
        );
        Ok(Self(format!(
            "fnv1a64:{:016x}",
            fnv1a64(canonical.as_bytes())
        )))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorkFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionProfile {
    #[default]
    Safe,
    Balanced,
    Autonomous,
}

fn default_role_id() -> String {
    ROLE_BUILDER.to_owned()
}

fn default_department_id() -> String {
    DEPARTMENT_EXECUTING.to_owned()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RequestContext {
    pub request_id: RequestId,
    pub session_id: SessionId,
    pub project_root: String,
    pub actor_id: Option<String>,
    pub project_trusted: bool,
    pub permission_profile: PermissionProfile,
    #[serde(default = "default_role_id")]
    pub role_id: String,
    #[serde(default = "default_department_id")]
    pub department_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_packet_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell_id: Option<CellId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_allow: Vec<String>,
}

impl RequestContext {
    pub fn local(session_id: impl Into<String>, project_root: impl Into<String>) -> Self {
        Self {
            request_id: RequestId::new(),
            session_id: SessionId::new(session_id),
            project_root: project_root.into(),
            actor_id: Some("local-user".to_owned()),
            project_trusted: false,
            permission_profile: PermissionProfile::Safe,
            role_id: default_role_id(),
            department_id: default_department_id(),
            work_packet_id: None,
            cell_id: None,
            path_allow: Vec::new(),
        }
    }

    pub fn assign_role(&mut self, role: &RoleSpec) {
        self.role_id = role.role_id.clone();
        self.department_id = role.department_id.clone();
    }
}

pub const ROLE_BUILDER: &str = "builder";
pub const ROLE_PM: &str = "pm";
pub const ROLE_ARCHITECT: &str = "architect";
pub const ROLE_REVIEWER: &str = "reviewer";
pub const ROLE_SPONSOR: &str = "sponsor";
pub const ROLE_CLOSER: &str = "closer";
pub const DEPARTMENT_EXECUTING: &str = "executing";
pub const DEPARTMENT_PLANNING: &str = "planning";
pub const DEPARTMENT_MONITORING: &str = "monitoring";
pub const DEPARTMENT_INITIATING: &str = "initiating";
pub const DEPARTMENT_CLOSING: &str = "closing";
pub const ROLE_SANDBOX_READ_ONLY: &str = "read-only";
pub const ROLE_SANDBOX_WORKSPACE_WRITE: &str = "workspace-write";
pub const PLANNING_PATH_CHARTER: &str = "charter";
pub const PLANNING_PATH_PLAN: &str = "plan";
pub const PLANNING_PATH_PACKET: &str = "packet";
pub const CLOSING_PATH_LESSONS: &str = "lessons";
pub const EXECUTING_PATH_RECEIPT: &str = "receipt";
pub const WORK_PACKET_SCHEMA: &str = "kiana.work-packet.v1";
pub const DECISION_RECORD_SCHEMA: &str = "kiana.decision-record.v1";
pub const SYMPOSIUM_SCHEMA: &str = "kiana.symposium.v1";
pub const SYMPOSIUM_RESULT_SCHEMA: &str = "kiana.symposium-result.v1";
pub const SYMPOSIUM_TYPE_DECISION: &str = "decision";
pub const SYMPOSIUM_STATUS_CLOSED: &str = "closed";
pub const SYMPOSIUM_STATUS_SKIPPED: &str = "skipped";
pub const DECISION_RECORD_PATH: &str = "plan/DECISION.json";
pub const INITIATING_DECISION_PATH: &str = "charter/DECISION.json";
pub const EXECUTING_DECISION_PATH: &str = "receipt/DECISION.json";
pub const MONITORING_DECISION_PATH: &str = "gate/DECISION.json";
pub const CLOSING_DECISION_PATH: &str = "lessons/DECISION.json";
pub const CLOSING_RECEIPT_PATH: &str = "lessons/CLOSING.json";
pub const WORK_PACKET_PATH: &str = "packet/TASK.json";
pub const REVIEW_PACKET_SCHEMA: &str = "kiana.review-packet.v1";
pub const REVIEW_RESULT_SCHEMA: &str = "kiana.review-result.v1";
pub const REVIEW_PACKET_PATH: &str = "gate/REVIEW.json";
pub const MERGE_RECEIPT_PATH: &str = "gate/MERGE.json";
pub const MONITORING_PATH_GATE: &str = "gate";
pub const MEMORY_LAYER_COMPANY: &str = "company";
pub const MEMORY_LAYER_DEPARTMENT: &str = "department";
pub const MEMORY_LAYER_ROLE: &str = "role";
pub const MEMORY_LAYER_PROJECT: &str = "project";
pub const MEMORY_LAYER_USER: &str = "user";
pub const MEMORY_LAYER_INSTANCE_SCRATCH: &str = "instance-scratch";
pub const MEMORY_COLLECTION_USER_PRIVATE: &str = "user-private";
pub const MEMORY_COLLECTION_USER_PREFS: &str = "user:prefs";
pub const MEMORY_COLLECTION_PLANNING_UNRELEASED: &str = "planning:unreleased-debate";
pub const MEMORY_SEARCH_SCHEMA: &str = "kiana.memory-search.v1";
pub const MEMORY_WRITE_SCHEMA: &str = "kiana.memory-write.v1";
pub const MEMORY_RECORD_SCHEMA: &str = "kiana.memory-record.v1";
pub const AGENT_TEMPLATE_SCHEMA: &str = "kiana.agent-template.v1";
pub const CELL_SCHEMA: &str = "kiana.cell.v1";
pub const SPAWN_PLAN_SCHEMA: &str = "kiana.spawn-plan.v1";
pub const BUDGET_LEASE_SCHEMA: &str = "kiana.budget-lease.v1";
pub const CAPABILITY_GRANT_SCHEMA: &str = "kiana.capability-grant.v1";
pub const SUPERVISION_LEASE_SCHEMA: &str = "kiana.supervision-lease.v1";
pub const DELEGATION_PACKET_SCHEMA: &str = "kiana.delegation-packet.v1";
pub const MERGE_RECEIPT_SCHEMA: &str = "kiana.merge-receipt.v1";
pub const CLOSING_RECEIPT_SCHEMA: &str = "kiana.closing-receipt.v1";
pub const SPAWN_RESULT_SCHEMA: &str = "kiana.spawn-result.v1";
pub const RETIREMENT_RECORD_SCHEMA: &str = "kiana.retirement-record.v1";
pub const MEMORY_LAYERS: [&str; 6] = [
    MEMORY_LAYER_COMPANY,
    MEMORY_LAYER_DEPARTMENT,
    MEMORY_LAYER_ROLE,
    MEMORY_LAYER_PROJECT,
    MEMORY_LAYER_USER,
    MEMORY_LAYER_INSTANCE_SCRATCH,
];

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RoleSpec {
    pub role_id: String,
    pub department_id: String,
    pub prompt: String,
    pub prompt_hash: String,
    pub tools: Vec<String>,
    pub sandbox: String,
    pub path_allow: Vec<String>,
    pub knowledge_grants: Vec<String>,
    pub can_convene: bool,
    pub can_vote: bool,
    pub model_profile: String,
    pub max_steps: u32,
}

impl RoleSpec {
    pub fn builder() -> Self {
        Self::new(
            ROLE_BUILDER,
            DEPARTMENT_EXECUTING,
            "You are Kiana's executing Builder. Use only the provided tools `shell`, `apply_patch`, `mcp`, `memory.search`, and `memory.write`. Never request danger-full-access.",
            vec![
                "shell".to_owned(),
                "apply_patch".to_owned(),
                "mcp".to_owned(),
                "memory.search".to_owned(),
                "memory.write".to_owned(),
            ],
            ROLE_SANDBOX_WORKSPACE_WRITE,
            vec![".".to_owned()],
            vec![
                MEMORY_LAYER_COMPANY.to_owned(),
                MEMORY_LAYER_PROJECT.to_owned(),
                "role:builder".to_owned(),
                "scratch".to_owned(),
            ],
            true,
            false,
            "executing",
            32,
        )
    }

    pub fn pm() -> Self {
        Self::new(
            ROLE_PM,
            DEPARTMENT_PLANNING,
            "You are Kiana's planning PM. Write only charter/plan/packet artifacts. Do not patch source files. Do not run shell.",
            vec![
                "apply_patch".to_owned(),
                "memory.search".to_owned(),
                "memory.write".to_owned(),
            ],
            ROLE_SANDBOX_WORKSPACE_WRITE,
            vec![
                PLANNING_PATH_CHARTER.to_owned(),
                PLANNING_PATH_PLAN.to_owned(),
                PLANNING_PATH_PACKET.to_owned(),
            ],
            vec![
                MEMORY_LAYER_COMPANY.to_owned(),
                "department:planning".to_owned(),
                MEMORY_LAYER_PROJECT.to_owned(),
                MEMORY_COLLECTION_USER_PREFS.to_owned(),
            ],
            true,
            true,
            "planning",
            8,
        )
    }

    pub fn architect() -> Self {
        Self::new(
            ROLE_ARCHITECT,
            DEPARTMENT_PLANNING,
            "You are Kiana's planning Architect. Read and advise. Do not write files or run shell.",
            vec!["memory.search".to_owned()],
            ROLE_SANDBOX_READ_ONLY,
            Vec::new(),
            vec![
                MEMORY_LAYER_COMPANY.to_owned(),
                "department:planning".to_owned(),
                MEMORY_LAYER_PROJECT.to_owned(),
            ],
            false,
            true,
            "planning",
            8,
        )
    }

    pub fn reviewer() -> Self {
        Self::new(
            ROLE_REVIEWER,
            DEPARTMENT_MONITORING,
            "You are Kiana's monitoring Reviewer. Compare the author receipt to acceptance. Do not patch source or run shell. You are never the author.",
            vec!["memory.search".to_owned()],
            ROLE_SANDBOX_READ_ONLY,
            Vec::new(),
            vec![
                MEMORY_LAYER_COMPANY.to_owned(),
                "department:monitoring".to_owned(),
                "project:events".to_owned(),
            ],
            true,
            false,
            "monitoring",
            8,
        )
    }

    pub fn sponsor() -> Self {
        Self::new(
            ROLE_SPONSOR,
            DEPARTMENT_INITIATING,
            "You are Kiana's initiating Sponsor. Write only charter artifacts. Do not patch source files. Do not run shell.",
            vec![
                "apply_patch".to_owned(),
                "memory.search".to_owned(),
                "memory.write".to_owned(),
            ],
            ROLE_SANDBOX_WORKSPACE_WRITE,
            vec![PLANNING_PATH_CHARTER.to_owned()],
            vec![
                MEMORY_LAYER_COMPANY.to_owned(),
                "department:initiating".to_owned(),
                MEMORY_COLLECTION_USER_PREFS.to_owned(),
            ],
            true,
            true,
            "initiating",
            8,
        )
    }

    pub fn closer() -> Self {
        Self::new(
            ROLE_CLOSER,
            DEPARTMENT_CLOSING,
            "You are Kiana's closing Closer. Write only lessons artifacts. Do not patch source files. Do not run shell.",
            vec![
                "apply_patch".to_owned(),
                "memory.search".to_owned(),
                "memory.write".to_owned(),
            ],
            ROLE_SANDBOX_WORKSPACE_WRITE,
            vec![CLOSING_PATH_LESSONS.to_owned()],
            vec![
                MEMORY_LAYER_COMPANY.to_owned(),
                "department:closing".to_owned(),
                "project:events".to_owned(),
            ],
            true,
            false,
            "closing",
            8,
        )
    }

    pub fn catalog() -> [RoleSpec; 6] {
        [
            Self::sponsor(),
            Self::pm(),
            Self::architect(),
            Self::builder(),
            Self::reviewer(),
            Self::closer(),
        ]
    }

    pub fn lookup(role_id: &str) -> Option<Self> {
        let role_id = role_id.trim();
        if role_id.is_empty() {
            return Some(Self::builder());
        }
        Self::catalog()
            .into_iter()
            .find(|role| role.role_id == role_id)
    }

    pub fn allows_tool(&self, tool: &str) -> bool {
        self.tools.iter().any(|allowed| allowed == tool)
    }

    pub fn allows_path(&self, path: &str) -> bool {
        let Some(path) = normalize_role_path(path) else {
            return false;
        };
        if self
            .path_allow
            .iter()
            .any(|allow| allow == "." || allow == "*")
        {
            return true;
        }
        self.path_allow.iter().any(|allow| {
            let allow = allow.trim().trim_matches('/');
            !allow.is_empty() && (path == allow || path.starts_with(&format!("{allow}/")))
        })
    }

    pub fn workspace_write_allowed(&self) -> bool {
        self.sandbox == ROLE_SANDBOX_WORKSPACE_WRITE
    }

    pub fn allows_knowledge(&self, collection: &str) -> bool {
        let Some(parsed) = MemoryCollection::parse(collection) else {
            return false;
        };
        self.knowledge_grants
            .iter()
            .any(|grant| MemoryCollection::parse(grant).is_some_and(|grant| grant.covers(&parsed)))
    }

    pub fn granted_collections(&self) -> Vec<MemoryCollection> {
        self.knowledge_grants
            .iter()
            .filter_map(|grant| MemoryCollection::parse(grant))
            .collect()
    }

    pub fn allows_memory_write(&self, collection: &str) -> bool {
        let Some(parsed) = MemoryCollection::parse(collection) else {
            return false;
        };
        match self.role_id.as_str() {
            ROLE_BUILDER => parsed.collection == MEMORY_LAYER_INSTANCE_SCRATCH,
            ROLE_PM => {
                parsed.collection == "department:planning"
                    || parsed.collection == MEMORY_COLLECTION_PLANNING_UNRELEASED
            }
            ROLE_SPONSOR => parsed.collection == "department:initiating",
            ROLE_CLOSER => parsed.collection == "department:closing",
            _ => false,
        }
    }

    fn new(
        role_id: &str,
        department_id: &str,
        prompt: &str,
        tools: Vec<String>,
        sandbox: &str,
        path_allow: Vec<String>,
        knowledge_grants: Vec<String>,
        can_convene: bool,
        can_vote: bool,
        model_profile: &str,
        max_steps: u32,
    ) -> Self {
        Self {
            role_id: role_id.to_owned(),
            department_id: department_id.to_owned(),
            prompt_hash: prompt_hash(prompt),
            prompt: prompt.to_owned(),
            tools,
            sandbox: sandbox.to_owned(),
            path_allow,
            knowledge_grants,
            can_convene,
            can_vote,
            model_profile: model_profile.to_owned(),
            max_steps,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MemoryCollection {
    pub layer: String,
    pub collection: String,
}

impl MemoryCollection {
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim().trim_matches('/');
        if raw.is_empty() {
            return None;
        }
        let (layer, collection) = match raw {
            MEMORY_LAYER_COMPANY => (MEMORY_LAYER_COMPANY, MEMORY_LAYER_COMPANY),
            MEMORY_LAYER_DEPARTMENT => (MEMORY_LAYER_DEPARTMENT, MEMORY_LAYER_DEPARTMENT),
            MEMORY_LAYER_ROLE => (MEMORY_LAYER_ROLE, MEMORY_LAYER_ROLE),
            MEMORY_LAYER_PROJECT | "project:code" | "project:docs" | "project:events" => {
                (MEMORY_LAYER_PROJECT, raw)
            }
            MEMORY_LAYER_USER | MEMORY_COLLECTION_USER_PREFS | MEMORY_COLLECTION_USER_PRIVATE => {
                (MEMORY_LAYER_USER, raw)
            }
            MEMORY_LAYER_INSTANCE_SCRATCH | "scratch" => {
                (MEMORY_LAYER_INSTANCE_SCRATCH, MEMORY_LAYER_INSTANCE_SCRATCH)
            }
            MEMORY_COLLECTION_PLANNING_UNRELEASED => (
                MEMORY_LAYER_DEPARTMENT,
                MEMORY_COLLECTION_PLANNING_UNRELEASED,
            ),
            other if other.starts_with("department:") => (MEMORY_LAYER_DEPARTMENT, other),
            other if other.starts_with("role:") => (MEMORY_LAYER_ROLE, other),
            _ => return None,
        };
        Some(Self {
            layer: layer.to_owned(),
            collection: collection.to_owned(),
        })
    }

    pub fn covers(&self, requested: &MemoryCollection) -> bool {
        if self.collection == requested.collection {
            return true;
        }
        if self.collection == MEMORY_LAYER_PROJECT
            && matches!(
                requested.collection.as_str(),
                MEMORY_LAYER_PROJECT | "project:code" | "project:docs"
            )
        {
            return true;
        }
        if self.collection == "scratch" && requested.collection == MEMORY_LAYER_INSTANCE_SCRATCH {
            return true;
        }
        if self.collection == "department:planning"
            && requested.collection == MEMORY_COLLECTION_PLANNING_UNRELEASED
        {
            return true;
        }
        false
    }

    pub fn home_scoped(&self) -> bool {
        matches!(
            self.layer.as_str(),
            MEMORY_LAYER_COMPANY | MEMORY_LAYER_USER
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DepartmentSpec {
    pub department_id: String,
    pub pmp_group: String,
    pub mission: String,
    pub roles: Vec<String>,
    pub artifacts: Vec<String>,
    pub rag_collection: String,
    pub can_convene: bool,
    pub gates: Vec<String>,
}

impl DepartmentSpec {
    fn new(
        department_id: &str,
        mission: &str,
        roles: Vec<String>,
        artifacts: Vec<String>,
        can_convene: bool,
        gates: Vec<String>,
    ) -> Self {
        Self {
            department_id: department_id.to_owned(),
            pmp_group: department_id.to_owned(),
            mission: mission.to_owned(),
            roles,
            artifacts,
            rag_collection: format!("department:{department_id}"),
            can_convene,
            gates,
        }
    }

    pub fn executing() -> Self {
        Self::new(
            DEPARTMENT_EXECUTING,
            "Execute work packets. Write files and run allowed verification. Do not change acceptance.",
            vec![ROLE_BUILDER.to_owned()],
            vec![".".to_owned(), EXECUTING_PATH_RECEIPT.to_owned()],
            true,
            vec!["receipt".to_owned()],
        )
    }

    pub fn planning() -> Self {
        Self::new(
            DEPARTMENT_PLANNING,
            "Turn a charter into work packets with frozen acceptance. Do not implement source.",
            vec![ROLE_PM.to_owned(), ROLE_ARCHITECT.to_owned()],
            vec![
                PLANNING_PATH_CHARTER.to_owned(),
                PLANNING_PATH_PLAN.to_owned(),
                PLANNING_PATH_PACKET.to_owned(),
            ],
            true,
            vec!["packet_has_acceptance".to_owned()],
        )
    }

    pub fn monitoring() -> Self {
        Self::new(
            DEPARTMENT_MONITORING,
            "Review author receipts against acceptance. The reviewer is never the author.",
            vec![ROLE_REVIEWER.to_owned()],
            vec![MONITORING_PATH_GATE.to_owned()],
            true,
            vec!["review_packet".to_owned()],
        )
    }

    pub fn initiating() -> Self {
        Self::new(
            DEPARTMENT_INITIATING,
            "Decide whether the work should exist and what success looks like. Do not write source.",
            vec![ROLE_SPONSOR.to_owned()],
            vec![PLANNING_PATH_CHARTER.to_owned()],
            true,
            vec!["charter_has_success_criteria".to_owned()],
        )
    }

    pub fn closing() -> Self {
        Self::new(
            DEPARTMENT_CLOSING,
            "Record lessons and close the receipt. Do not write source.",
            vec![ROLE_CLOSER.to_owned()],
            vec![CLOSING_PATH_LESSONS.to_owned()],
            true,
            vec!["lessons_logged".to_owned()],
        )
    }

    pub fn catalog() -> [DepartmentSpec; 5] {
        [
            Self::initiating(),
            Self::planning(),
            Self::executing(),
            Self::monitoring(),
            Self::closing(),
        ]
    }

    pub fn lookup(department_id: &str) -> Option<Self> {
        let department_id = department_id.trim();
        if department_id.is_empty() {
            return Some(Self::executing());
        }
        Self::catalog()
            .into_iter()
            .find(|department| department.department_id == department_id)
    }

    pub fn convene_chair_id(&self) -> Option<String> {
        self.roles.iter().find_map(|role_id| {
            RoleSpec::lookup(role_id)
                .filter(|role| role.can_convene)
                .map(|role| role.role_id)
        })
    }

    pub fn decision_path(&self) -> &'static str {
        match self.department_id.as_str() {
            DEPARTMENT_INITIATING => INITIATING_DECISION_PATH,
            DEPARTMENT_PLANNING => DECISION_RECORD_PATH,
            DEPARTMENT_EXECUTING => EXECUTING_DECISION_PATH,
            DEPARTMENT_MONITORING => MONITORING_DECISION_PATH,
            DEPARTMENT_CLOSING => CLOSING_DECISION_PATH,
            _ => DECISION_RECORD_PATH,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkPacket {
    pub schema: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<ProjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_packet_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_cell_id: Option<CellId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_scope: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acceptance_tests: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_unix_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_lease_id: Option<BudgetLeaseId>,
    #[serde(default, skip_serializing_if = "is_draft_status")]
    pub status: WorkPacketStatus,
    #[serde(default = "default_from_department")]
    pub from_department: String,
    #[serde(default = "default_department_id")]
    pub to_department: String,
    #[serde(default = "default_role_id")]
    pub assignee_role: String,
    pub goal: String,
    #[serde(default)]
    pub path_allow: Vec<String>,
    #[serde(default)]
    pub acceptance: Vec<String>,
    #[serde(default)]
    pub forbidden: Vec<String>,
}

fn is_draft_status(status: &WorkPacketStatus) -> bool {
    *status == WorkPacketStatus::Draft
}

fn default_from_department() -> String {
    DEPARTMENT_PLANNING.to_owned()
}

impl WorkPacket {
    pub fn builder_task(id: impl Into<String>, goal: impl Into<String>) -> Self {
        Self {
            schema: WORK_PACKET_SCHEMA.to_owned(),
            id: id.into(),
            project_id: None,
            parent_packet_id: None,
            owner_cell_id: None,
            acceptor_id: None,
            inputs: Vec::new(),
            dependencies: Vec::new(),
            data_scope: Vec::new(),
            acceptance_tests: Vec::new(),
            deadline_unix_ms: None,
            budget_lease_id: None,
            status: WorkPacketStatus::Draft,
            from_department: default_from_department(),
            to_department: default_department_id(),
            assignee_role: default_role_id(),
            goal: goal.into(),
            path_allow: Vec::new(),
            acceptance: Vec::new(),
            forbidden: Vec::new(),
        }
    }

    pub fn with_path_allow(mut self, paths: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.path_allow = paths.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_acceptance_tests(
        mut self,
        tests: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.acceptance_tests = tests.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_dependencies(
        mut self,
        dependencies: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.dependencies = dependencies.into_iter().map(Into::into).collect();
        self
    }

    pub fn transition_status(&mut self, next: WorkPacketStatus) -> Result<(), DomainError> {
        self.status = self.status.transition(next)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != WORK_PACKET_SCHEMA {
            return Err("packet_invalid");
        }
        if self.id.trim().is_empty() {
            return Err("packet_id_required");
        }
        if self.goal.trim().is_empty() {
            return Err("packet_goal_required");
        }
        if self.status.is_terminal() {
            return Err("packet_status_terminal");
        }
        let role = RoleSpec::lookup(&self.assignee_role).ok_or("packet_role_unknown")?;
        if role.role_id != ROLE_BUILDER {
            return Err("packet_role_must_be_builder");
        }
        if role.department_id != DEPARTMENT_EXECUTING {
            return Err("packet_department_must_be_executing");
        }
        let to = self.to_department.trim();
        if !to.is_empty() && to != DEPARTMENT_EXECUTING {
            return Err("packet_department_must_be_executing");
        }
        Ok(())
    }

    pub fn as_prompt(&self) -> String {
        let mut lines = vec![
            format!("Work packet {}", self.id.trim()),
            format!(
                "From: {} -> {}/{}",
                self.from_department.trim(),
                self.to_department.trim(),
                self.assignee_role.trim()
            ),
            format!("Goal: {}", self.goal.trim()),
        ];
        push_packet_list(&mut lines, "Inputs", &self.inputs);
        push_packet_list(&mut lines, "Dependencies", &self.dependencies);
        push_packet_list(&mut lines, "Data scope", &self.data_scope);
        push_packet_list(&mut lines, "Path allow", &self.path_allow);
        push_packet_list(&mut lines, "Acceptance tests", &self.acceptance_tests);
        push_packet_list(&mut lines, "Acceptance", &self.acceptance);
        push_packet_list(&mut lines, "Forbidden", &self.forbidden);
        lines.join("\n")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AgentTemplate {
    pub schema: String,
    pub template_id: TemplateId,
    pub version: String,
    pub role_id: String,
    pub mission_schema: String,
    pub input_schema: String,
    pub output_schema: String,
    pub default_capabilities: Vec<String>,
    pub sandbox_profile: String,
    pub estimated_cost: u64,
    pub max_children: u32,
    pub max_depth: u32,
    pub ttl_seconds: u64,
    pub heartbeat_interval_seconds: u64,
    pub checkpoint_policy: String,
    pub merge_strategy: String,
    #[serde(default)]
    pub delegation_allowed: bool,
}

impl AgentTemplate {
    pub fn for_role(role: &RoleSpec, version: impl Into<String>) -> Self {
        Self {
            schema: AGENT_TEMPLATE_SCHEMA.to_owned(),
            template_id: TemplateId::new(),
            version: version.into(),
            role_id: role.role_id.clone(),
            mission_schema: "kiana.mission.v1".to_owned(),
            input_schema: "kiana.input.v1".to_owned(),
            output_schema: "kiana.output.v1".to_owned(),
            default_capabilities: role.tools.clone(),
            sandbox_profile: role.sandbox.clone(),
            estimated_cost: u64::from(role.max_steps),
            max_children: 0,
            max_depth: 0,
            ttl_seconds: 300,
            heartbeat_interval_seconds: 15,
            checkpoint_policy: "on_failure".to_owned(),
            merge_strategy: "receipt_only".to_owned(),
            delegation_allowed: false,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != AGENT_TEMPLATE_SCHEMA {
            return Err("template_invalid");
        }
        if self.version.trim().is_empty() {
            return Err("template_version_required");
        }
        if RoleSpec::lookup(&self.role_id).is_none() {
            return Err("template_role_unknown");
        }
        if self.mission_schema.trim().is_empty()
            || self.input_schema.trim().is_empty()
            || self.output_schema.trim().is_empty()
        {
            return Err("template_schema_required");
        }
        if self.ttl_seconds == 0 || self.heartbeat_interval_seconds == 0 {
            return Err("template_lifetime_invalid");
        }
        if self.heartbeat_interval_seconds > self.ttl_seconds {
            return Err("template_heartbeat_exceeds_ttl");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BudgetLease {
    pub schema: String,
    pub lease_id: BudgetLeaseId,
    pub max_tool_calls: u64,
    pub max_tokens: u64,
    pub max_wall_clock_ms: u64,
    pub max_concurrency: u32,
    pub max_effects: u32,
    #[serde(default)]
    pub max_reserved_budget: u64,
    #[serde(default)]
    pub reserved_budget: u64,
    #[serde(default)]
    pub tool_calls_used: u64,
    #[serde(default)]
    pub tokens_used: u64,
    #[serde(default)]
    pub effects_used: u32,
}

impl BudgetLease {
    pub fn new(
        max_tool_calls: u64,
        max_tokens: u64,
        max_wall_clock_ms: u64,
        max_concurrency: u32,
        max_effects: u32,
    ) -> Self {
        Self {
            schema: BUDGET_LEASE_SCHEMA.to_owned(),
            lease_id: BudgetLeaseId::new(),
            max_tool_calls,
            max_tokens,
            max_wall_clock_ms,
            max_concurrency,
            max_effects,
            max_reserved_budget: max_tool_calls,
            reserved_budget: 0,
            tool_calls_used: 0,
            tokens_used: 0,
            effects_used: 0,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != BUDGET_LEASE_SCHEMA {
            return Err("budget_lease_invalid");
        }
        if self.max_tool_calls == 0
            || self.max_tokens == 0
            || self.max_wall_clock_ms == 0
            || self.max_concurrency == 0
        {
            return Err("budget_lease_limit_required");
        }
        if self.tool_calls_used > self.max_tool_calls
            || self.tokens_used > self.max_tokens
            || self.effects_used > self.max_effects
            || self.reserved_budget > self.reservation_limit()
        {
            return Err("budget_lease_exceeded");
        }
        Ok(())
    }

    fn reservation_limit(&self) -> u64 {
        // Legacy JSON did not carry an admission limit. Treat zero as the
        // backwards-compatible tool-call ceiling; newly-created leases pin it.
        if self.max_reserved_budget == 0 {
            self.max_tool_calls
        } else {
            self.max_reserved_budget
        }
    }

    pub fn can_reserve(&self, amount: u64) -> bool {
        amount
            <= self
                .reservation_limit()
                .saturating_sub(self.reserved_budget)
    }

    pub fn reserve(&mut self, amount: u64) -> Result<(), &'static str> {
        if !self.can_reserve(amount) {
            return Err("spawn_budget_exceeded");
        }
        self.reserved_budget = self
            .reserved_budget
            .checked_add(amount)
            .ok_or("spawn_budget_exceeded")?;
        Ok(())
    }

    pub fn release(&mut self, amount: u64) -> Result<(), &'static str> {
        if amount > self.reserved_budget {
            return Err("spawn_budget_release_invalid");
        }
        self.reserved_budget -= amount;
        Ok(())
    }

    pub fn can_consume(&self, tool_calls: u64, tokens: u64, effects: u32) -> bool {
        tool_calls <= self.max_tool_calls.saturating_sub(self.tool_calls_used)
            && tokens <= self.max_tokens.saturating_sub(self.tokens_used)
            && effects <= self.max_effects.saturating_sub(self.effects_used)
    }

    pub fn consume(
        &mut self,
        tool_calls: u64,
        tokens: u64,
        effects: u32,
    ) -> Result<(), &'static str> {
        if !self.can_consume(tool_calls, tokens, effects) {
            return Err("budget_lease_exceeded");
        }
        self.tool_calls_used = self
            .tool_calls_used
            .checked_add(tool_calls)
            .ok_or("budget_lease_exceeded")?;
        self.tokens_used = self
            .tokens_used
            .checked_add(tokens)
            .ok_or("budget_lease_exceeded")?;
        self.effects_used = self
            .effects_used
            .checked_add(effects)
            .ok_or("budget_lease_exceeded")?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CapabilityGrant {
    pub schema: String,
    pub grant_id: CapabilityGrantId,
    pub capability: CapabilityKind,
    pub operation: String,
    #[serde(default)]
    pub resources: Vec<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    pub expires_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<ApprovalId>,
    #[serde(default)]
    pub delegation_allowed: bool,
}

impl CapabilityGrant {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != CAPABILITY_GRANT_SCHEMA {
            return Err("capability_grant_invalid");
        }
        if self.operation.trim().is_empty() || self.expires_at_unix_ms == 0 {
            return Err("capability_grant_scope_required");
        }
        if self
            .paths
            .iter()
            .any(|path| normalize_role_path(path).is_none())
        {
            return Err("capability_grant_path_invalid");
        }
        Ok(())
    }

    pub fn contains(&self, child: &Self) -> bool {
        self.capability == child.capability
            && self.operation == child.operation
            && child.expires_at_unix_ms <= self.expires_at_unix_ms
            && (!child.delegation_allowed || self.delegation_allowed)
            && child
                .resources
                .iter()
                .all(|resource| self.resources.contains(resource))
            && child
                .paths
                .iter()
                .all(|path| allow_list_covers(&self.paths, path))
    }

    pub fn allows_request(&self, request: &CapabilityRequest) -> bool {
        let exact_scope = self.capability == request.capability && self.operation == request.operation;
        let coding_scope = matches!(&self.capability, CapabilityKind::Other(scope) if scope == "coding")
            && self.operation == "builder.packet"
            && matches!(
                request.operation.as_str(),
                "shell.exec" | "apply_patch" | "mcp.call" | "memory.search" | "memory.write"
            );
        if !exact_scope && !coding_scope {
            return false;
        }
        let paths = capability_request_paths(request);
        paths.is_empty() || paths.iter().all(|path| allow_list_covers(&self.paths, path))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SupervisionLease {
    pub schema: String,
    pub lease_id: SupervisionLeaseId,
    pub heartbeat_interval_seconds: u64,
    pub stall_threshold_seconds: u64,
    pub retry_limit: u32,
    #[serde(default)]
    pub retries_used: u32,
}

impl SupervisionLease {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != SUPERVISION_LEASE_SCHEMA {
            return Err("supervision_lease_invalid");
        }
        if self.heartbeat_interval_seconds == 0
            || self.stall_threshold_seconds < self.heartbeat_interval_seconds
        {
            return Err("supervision_lease_interval_invalid");
        }
        if self.retries_used > self.retry_limit {
            return Err("supervision_lease_retries_exceeded");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CellSpec {
    pub schema: String,
    pub cell_id: CellId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_cell_id: Option<CellId>,
    pub root_run_id: RunId,
    pub template_id: TemplateId,
    pub template_version: String,
    pub role_id: String,
    pub objective: String,
    #[serde(default)]
    pub input_refs: Vec<String>,
    pub output_contract: String,
    pub partition_key: String,
    #[serde(default)]
    pub owned_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_packet_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_actor_id: Option<String>,
    pub capability_grant_id: CapabilityGrantId,
    pub budget_lease_id: BudgetLeaseId,
    pub supervision_lease_id: SupervisionLeaseId,
    pub depth: u32,
    pub spawn_quota: u32,
    #[serde(default)]
    pub lifecycle: CellLifecycle,
}

impl CellSpec {
    pub fn validate(&self, template: &AgentTemplate) -> Result<(), &'static str> {
        if self.schema.trim() != CELL_SCHEMA {
            return Err("cell_invalid");
        }
        template.validate()?;
        if self.template_id != template.template_id
            || self.template_version != template.version
            || self.role_id != template.role_id
        {
            return Err("cell_template_mismatch");
        }
        if self.objective.trim().is_empty() || self.output_contract.trim().is_empty() {
            return Err("cell_contract_required");
        }
        if self.depth > template.max_depth {
            return Err("cell_depth_exceeded");
        }
        if self
            .owned_paths
            .iter()
            .any(|path| normalize_role_path(path).is_none())
        {
            return Err("cell_path_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SpawnPlan {
    pub schema: String,
    pub plan_id: SpawnPlanId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_cell_id: Option<CellId>,
    pub reason_code: String,
    pub candidate_templates: Vec<TemplateId>,
    pub count: u32,
    pub partition: String,
    #[serde(default)]
    pub input_refs: Vec<String>,
    pub output_contract: String,
    #[serde(default)]
    pub requested_capabilities: Vec<String>,
    pub budget_reservation: u64,
    pub deadline_unix_ms: u64,
    pub rollback_policy: String,
    pub idempotency_key: String,
    pub expected_utility: i64,
    #[serde(default)]
    pub status: SpawnPlanStatus,
}

impl SpawnPlan {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != SPAWN_PLAN_SCHEMA {
            return Err("spawn_plan_invalid");
        }
        if self.count == 0 || self.candidate_templates.is_empty() {
            return Err("spawn_plan_count_invalid");
        }
        if self.reason_code.trim().is_empty()
            || self.output_contract.trim().is_empty()
            || self.idempotency_key.trim().is_empty()
        {
            return Err("spawn_plan_contract_required");
        }
        if self.deadline_unix_ms == 0 {
            return Err("spawn_plan_deadline_required");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpawnPlanStatus {
    Proposed,
    Validated,
    Reserved,
    Committed,
    RolledBack,
    Rejected,
}

impl SpawnPlanStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Proposed, Self::Validated | Self::Rejected)
                | (Self::Validated, Self::Reserved | Self::Rejected)
                | (
                    Self::Reserved,
                    Self::Committed | Self::RolledBack | Self::Rejected
                )
                | (Self::Committed, Self::RolledBack)
        )
    }

    pub fn transition(self, next: Self) -> Result<Self, DomainError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(DomainError::InvalidStateTransition {
                aggregate: "spawn_plan",
                from: self.as_str(),
                to: next.as_str(),
            })
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Validated => "validated",
            Self::Reserved => "reserved",
            Self::Committed => "committed",
            Self::RolledBack => "rolled_back",
            Self::Rejected => "rejected",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::RolledBack | Self::Rejected)
    }
}

impl Default for SpawnPlanStatus {
    fn default() -> Self {
        Self::Proposed
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SpawnReceipt {
    pub schema: String,
    pub plan_id: SpawnPlanId,
    pub cell_id: CellId,
    pub root_run_id: RunId,
    pub work_packet_id: String,
    pub fingerprint: WorkFingerprint,
    pub lifecycle: CellLifecycle,
    pub replayed: bool,
}

impl SpawnReceipt {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != SPAWN_RESULT_SCHEMA {
            return Err("spawn_receipt_invalid");
        }
        if self.work_packet_id.trim().is_empty() || self.fingerprint.as_str().trim().is_empty() {
            return Err("spawn_receipt_contract_required");
        }
        if self.lifecycle == CellLifecycle::Proposed {
            return Err("spawn_receipt_lifecycle_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RetirementRecord {
    pub schema: String,
    pub cell_id: CellId,
    pub grant_id: CapabilityGrantId,
    pub budget_lease_id: BudgetLeaseId,
    pub supervision_lease_id: SupervisionLeaseId,
    #[serde(default)]
    pub released_paths: Vec<String>,
    pub reason: String,
    pub retired_at_unix_ms: u64,
}

impl RetirementRecord {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != RETIREMENT_RECORD_SCHEMA {
            return Err("retirement_record_invalid");
        }
        if self.reason.trim().is_empty() || self.retired_at_unix_ms == 0 {
            return Err("retirement_record_contract_required");
        }
        if self
            .released_paths
            .iter()
            .any(|path| normalize_role_path(path).is_none())
        {
            return Err("retirement_record_path_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DelegationPacket {
    pub schema: String,
    pub delegation_id: DelegationId,
    pub parent_cell_id: CellId,
    pub child_cell_id: CellId,
    pub source_packet_id: String,
    #[serde(default)]
    pub capability_scopes: Vec<String>,
    #[serde(default)]
    pub path_scopes: Vec<String>,
    pub budget_lease_id: BudgetLeaseId,
    pub capability_grant_id: CapabilityGrantId,
    pub supervision_lease_id: SupervisionLeaseId,
    pub expires_at_unix_ms: u64,
    #[serde(default)]
    pub delegation_allowed: bool,
}

impl DelegationPacket {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != DELEGATION_PACKET_SCHEMA {
            return Err("delegation_packet_invalid");
        }
        if self.parent_cell_id == self.child_cell_id {
            return Err("delegation_packet_self_parent");
        }
        if self.source_packet_id.trim().is_empty() || self.expires_at_unix_ms == 0 {
            return Err("delegation_packet_contract_required");
        }
        if self
            .path_scopes
            .iter()
            .any(|path| normalize_role_path(path).is_none())
        {
            return Err("delegation_packet_path_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MergeReceipt {
    pub schema: String,
    pub receipt_id: ReceiptId,
    pub author_run_id: RunId,
    pub author_session_id: SessionId,
    pub reviewer_session_id: SessionId,
    pub reviewer_verdict: String,
    #[serde(default)]
    pub files: Vec<String>,
    pub accepted: bool,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl MergeReceipt {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != MERGE_RECEIPT_SCHEMA {
            return Err("merge_receipt_invalid");
        }
        if self.author_session_id.is_empty() || self.reviewer_session_id.is_empty() {
            return Err("merge_receipt_identity_required");
        }
        if self.author_session_id == self.reviewer_session_id {
            return Err("merge_receipt_reviewer_author_same");
        }
        if self.reviewer_verdict != "pass" || !self.accepted {
            return Err("merge_receipt_not_accepted");
        }
        if self
            .files
            .iter()
            .any(|path| normalize_role_path(path).is_none())
        {
            return Err("merge_receipt_path_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClosingReceipt {
    pub schema: String,
    pub receipt_id: ReceiptId,
    pub project_id: Option<ProjectId>,
    pub author_run_id: RunId,
    pub author_session_id: SessionId,
    pub reviewer_session_id: SessionId,
    pub closer_session_id: SessionId,
    pub review_id: String,
    pub verdict: String,
    pub accepted: bool,
    #[serde(default)]
    pub files_verified: Vec<String>,
    #[serde(default)]
    pub exceptions: Vec<String>,
    pub lessons_path: String,
}

impl ClosingReceipt {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != CLOSING_RECEIPT_SCHEMA {
            return Err("closing_receipt_invalid");
        }
        if self.author_session_id.is_empty()
            || self.reviewer_session_id.is_empty()
            || self.closer_session_id.is_empty()
        {
            return Err("closing_receipt_identity_required");
        }
        if self.closer_session_id == self.author_session_id
            || self.closer_session_id == self.reviewer_session_id
        {
            return Err("closing_receipt_role_separation_failed");
        }
        if self.review_id.trim().is_empty() || self.lessons_path.trim() != "lessons/LEARNED.md" {
            return Err("closing_receipt_provenance_required");
        }
        if self.verdict != "pass" || !self.accepted {
            return Err("closing_receipt_not_accepted");
        }
        if self
            .files_verified
            .iter()
            .any(|path| normalize_role_path(path).is_none())
        {
            return Err("closing_receipt_path_invalid");
        }
        Ok(())
    }
}

fn push_packet_list(lines: &mut Vec<String>, label: &str, values: &[String]) {
    let values: Vec<_> = values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect();
    if values.is_empty() {
        return;
    }
    lines.push(format!("{label}: {}", values.join(", ")));
}

pub const EXCLUSIVE_PATH_LOCK: &str = "*";

pub fn builder_lock_paths(path_allow: &[String]) -> Vec<String> {
    let mut paths: Vec<String> = path_allow
        .iter()
        .filter_map(|path| normalize_role_path(path))
        .collect();
    paths.sort();
    paths.dedup();
    if paths.is_empty() {
        vec![EXCLUSIVE_PATH_LOCK.to_owned()]
    } else {
        paths
    }
}

pub fn path_locks_conflict(left: &str, right: &str) -> bool {
    if left == EXCLUSIVE_PATH_LOCK || right == EXCLUSIVE_PATH_LOCK {
        return true;
    }
    left == right
        || left.starts_with(&format!("{right}/"))
        || right.starts_with(&format!("{left}/"))
}

pub fn allow_list_covers(path_allow: &[String], path: &str) -> bool {
    let Some(path) = normalize_role_path(path) else {
        return false;
    };
    if path_allow
        .iter()
        .any(|allow| allow.trim() == "." || allow.trim() == "*")
    {
        return true;
    }
    path_allow.iter().any(|allow| {
        let allow = allow.trim().trim_matches('/');
        !allow.is_empty() && (path == allow || path.starts_with(&format!("{allow}/")))
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReviewPacket {
    pub schema: String,
    pub id: String,
    pub author_session_id: String,
    pub author_role_id: String,
    pub reviewer_session_id: String,
    pub verdict: String,
    pub summary: String,
    #[serde(default)]
    pub files_reviewed: Vec<String>,
}

impl ReviewPacket {
    pub fn closed(
        id: impl Into<String>,
        author_session_id: impl Into<String>,
        author_role_id: impl Into<String>,
        reviewer_session_id: impl Into<String>,
        verdict: impl Into<String>,
        summary: impl Into<String>,
        files_reviewed: Vec<String>,
    ) -> Self {
        Self {
            schema: REVIEW_PACKET_SCHEMA.to_owned(),
            id: id.into(),
            author_session_id: author_session_id.into(),
            author_role_id: author_role_id.into(),
            reviewer_session_id: reviewer_session_id.into(),
            verdict: verdict.into(),
            summary: summary.into(),
            files_reviewed,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != REVIEW_PACKET_SCHEMA {
            return Err("review_packet_invalid");
        }
        if self.id.trim().is_empty() {
            return Err("review_id_required");
        }
        if self.author_session_id.trim().is_empty() {
            return Err("review_author_required");
        }
        if self.reviewer_session_id.trim().is_empty() {
            return Err("review_session_required");
        }
        if self.author_session_id.trim() == self.reviewer_session_id.trim() {
            return Err("review_author_session_denied");
        }
        if self.author_role_id.trim() != ROLE_BUILDER {
            return Err("review_author_must_be_builder");
        }
        match self.verdict.trim() {
            "pass" | "fail" | "needs_change" => Ok(()),
            _ => Err("review_verdict_invalid"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SymposiumClaim {
    pub speaker: String,
    pub text: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

impl SymposiumClaim {
    pub fn new(speaker: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            speaker: speaker.into(),
            text: text.into(),
            evidence_refs: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SymposiumVote {
    pub role: String,
    pub stance: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Blackboard {
    #[serde(default)]
    pub claims: Vec<SymposiumClaim>,
    #[serde(default)]
    pub votes: Vec<SymposiumVote>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_decision: Option<String>,
}

impl Blackboard {
    pub fn as_prompt(&self) -> String {
        let mut lines = Vec::new();
        if self.claims.is_empty() {
            lines.push("Blackboard claims: (none)".to_owned());
        } else {
            lines.push("Blackboard claims:".to_owned());
            for claim in &self.claims {
                lines.push(format!("- {}: {}", claim.speaker.trim(), claim.text.trim()));
            }
        }
        if !self.votes.is_empty() {
            lines.push("Blackboard votes:".to_owned());
            for vote in &self.votes {
                lines.push(format!(
                    "- {}: {} ({})",
                    vote.role.trim(),
                    vote.stance.trim(),
                    vote.reason.trim()
                ));
            }
        }
        if let Some(draft) = self
            .draft_decision
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            lines.push(format!("Draft decision: {draft}"));
        }
        lines.join("\n")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub schema: String,
    pub id: String,
    pub symposium_id: String,
    pub summary: String,
    pub decision: String,
    pub work_packet_id: String,
    pub skipped_meeting: bool,
}

impl DecisionRecord {
    pub fn closed(
        id: impl Into<String>,
        symposium_id: impl Into<String>,
        summary: impl Into<String>,
        decision: impl Into<String>,
        work_packet_id: impl Into<String>,
        skipped_meeting: bool,
    ) -> Self {
        Self {
            schema: DECISION_RECORD_SCHEMA.to_owned(),
            id: id.into(),
            symposium_id: symposium_id.into(),
            summary: summary.into(),
            decision: decision.into(),
            work_packet_id: work_packet_id.into(),
            skipped_meeting,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Symposium {
    pub schema: String,
    pub id: String,
    pub department_id: String,
    #[serde(rename = "type")]
    pub symposium_type: String,
    pub agenda: String,
    pub chair: String,
    pub attendees: Vec<String>,
    pub max_rounds: u32,
    pub blackboard: Blackboard,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_packet_id: Option<String>,
}

impl Symposium {
    pub const DEFAULT_MAX_ROUNDS: u32 = 4;
    pub const MAX_ROUNDS_CAP: u32 = 8;

    pub fn planning(id: impl Into<String>, agenda: impl Into<String>, max_rounds: u32) -> Self {
        Self::department(DEPARTMENT_PLANNING, id, agenda, max_rounds)
            .expect("planning department can convene")
    }

    pub fn department(
        department_id: impl AsRef<str>,
        id: impl Into<String>,
        agenda: impl Into<String>,
        max_rounds: u32,
    ) -> Result<Self, &'static str> {
        let department =
            DepartmentSpec::lookup(department_id.as_ref()).ok_or("department_unknown")?;
        if !department.can_convene {
            return Err("symposium_department_cannot_convene");
        }
        let chair = department
            .convene_chair_id()
            .ok_or("symposium_chair_cannot_convene")?;
        Ok(Self {
            schema: SYMPOSIUM_SCHEMA.to_owned(),
            id: id.into(),
            department_id: department.department_id.clone(),
            symposium_type: SYMPOSIUM_TYPE_DECISION.to_owned(),
            agenda: agenda.into(),
            chair,
            attendees: department.roles.clone(),
            max_rounds,
            blackboard: Blackboard::default(),
            status: "proposed".to_owned(),
            decision_id: None,
            work_packet_id: None,
        })
    }

    pub fn validate_max_rounds(max_rounds: u32) -> Result<u32, &'static str> {
        if max_rounds == 0 || max_rounds > Self::MAX_ROUNDS_CAP {
            Err("symposium_max_rounds_invalid")
        } else {
            Ok(max_rounds)
        }
    }

    pub fn speaker_prompt(&self, role_id: &str) -> String {
        let builder_rule = if self.department_id == DEPARTMENT_EXECUTING {
            "Stay on this department's artifacts. Do not rewrite planning packets."
        } else {
            "Do not invite the Builder."
        };
        format!(
            "{} symposium {}\nChair: {}\nAttendees: {}\nSpeak as {}\nAgenda: {}\n{}\nReply with a claim or vote. Do not patch source. {}",
            self.department_id.trim(),
            self.id.trim(),
            self.chair.trim(),
            self.attendees.join(", "),
            role_id.trim(),
            self.agenda.trim(),
            self.blackboard.as_prompt(),
            builder_rule
        )
    }

    pub fn speaker_session_id(&self, role_id: &str) -> String {
        format!("{}-{}", self.id.trim(), role_id.trim())
    }

    pub fn builder_present(&self) -> bool {
        self.attendees
            .iter()
            .any(|role| role.trim() == ROLE_BUILDER)
    }

    pub fn decision_path(&self) -> &'static str {
        DepartmentSpec::lookup(&self.department_id)
            .map(|department| department.decision_path())
            .unwrap_or(DECISION_RECORD_PATH)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        Self::validate_max_rounds(self.max_rounds)?;
        if self.agenda.trim().is_empty() {
            return Err("symposium_goal_required");
        }
        let department = DepartmentSpec::lookup(&self.department_id).ok_or("department_unknown")?;
        if !department.can_convene {
            return Err("symposium_department_cannot_convene");
        }
        if self.department_id == DEPARTMENT_PLANNING && self.chair.trim() != ROLE_PM {
            return Err("symposium_chair_must_be_pm");
        }
        let Some(chair) = RoleSpec::lookup(&self.chair) else {
            return Err("symposium_chair_cannot_convene");
        };
        if !chair.can_convene || chair.department_id != department.department_id {
            return Err("symposium_chair_cannot_convene");
        }
        if self.department_id != DEPARTMENT_EXECUTING
            && self
                .attendees
                .iter()
                .any(|role| role.trim() == ROLE_BUILDER)
        {
            return Err("symposium_builder_not_attendee");
        }
        for role_id in &self.attendees {
            let Some(role) = RoleSpec::lookup(role_id) else {
                return Err("symposium_attendees_invalid");
            };
            if role.department_id != department.department_id {
                return Err("joint_symposium_frozen");
            }
        }
        if self.attendees.len() != department.roles.len()
            || self
                .attendees
                .iter()
                .zip(department.roles.iter())
                .any(|(got, want)| got.trim() != want.trim())
        {
            return Err("symposium_attendees_invalid");
        }
        if !self
            .attendees
            .iter()
            .any(|role| role.trim() == self.chair.trim())
        {
            return Err("symposium_attendees_invalid");
        }
        Ok(())
    }

    pub fn close(
        &mut self,
        skipped_meeting: bool,
    ) -> Result<(DecisionRecord, Option<WorkPacket>), &'static str> {
        self.validate()?;
        let decision_id = format!("dec-{}", self.id.trim());
        let decision_text = self
            .blackboard
            .draft_decision
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| self.agenda.trim())
            .to_owned();
        if decision_text.is_empty() {
            return Err("symposium_decision_required");
        }
        let summary = if skipped_meeting {
            format!("Anti-meeting: proceed with {decision_text}")
        } else {
            format!(
                "{} symposium closed: {decision_text}",
                self.department_id.trim()
            )
        };
        let packet = if self.department_id == DEPARTMENT_PLANNING {
            let packet_id = format!("wp-{}", self.id.trim());
            let packet = WorkPacket::builder_task(packet_id, self.agenda.trim());
            packet.validate()?;
            Some(packet)
        } else {
            None
        };
        let packet_id = packet
            .as_ref()
            .map(|packet| packet.id.clone())
            .unwrap_or_default();
        let decision = DecisionRecord::closed(
            decision_id,
            self.id.clone(),
            summary,
            decision_text,
            packet_id.clone(),
            skipped_meeting,
        );
        self.status = if skipped_meeting {
            SYMPOSIUM_STATUS_SKIPPED.to_owned()
        } else {
            SYMPOSIUM_STATUS_CLOSED.to_owned()
        };
        self.decision_id = Some(decision.id.clone());
        self.work_packet_id = if packet_id.is_empty() {
            None
        } else {
            Some(packet_id)
        };
        Ok((decision, packet))
    }
}

pub fn normalize_role_path(path: &str) -> Option<String> {
    let path = path.trim().replace('\\', "/");
    if path.is_empty() {
        return None;
    }
    if Path::new(&path).is_absolute() {
        return None;
    }
    if path == "." {
        return Some(".".to_owned());
    }
    let mut parts = Vec::new();
    for component in Path::new(&path).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            _ => return None,
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

pub fn prompt_hash(prompt: &str) -> String {
    format!("fnv1a64:{:016x}", fnv1a64(prompt.as_bytes()))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommandIntent {
    pub name: String,
    pub arguments: Value,
}

impl CommandIntent {
    pub fn new(name: impl Into<String>, arguments: Value) -> Self {
        Self {
            name: name.into(),
            arguments,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    Query,
    Filesystem,
    Process,
    Network,
    Model,
    Secret,
    Sandbox,
    Computer,
    Tool,
    Other(String),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    #[default]
    ReadOnly,
    LocalWrite,
    ExternalSideEffect,
    Critical,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapabilityRequest {
    pub request_id: RequestId,
    pub capability: CapabilityKind,
    pub operation: String,
    pub arguments: Value,
    pub risk: RiskLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell_id: Option<CellId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_grant_id: Option<CapabilityGrantId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_lease_id: Option<BudgetLeaseId>,
}

impl CapabilityRequest {
    pub fn new(
        request_id: RequestId,
        capability: CapabilityKind,
        operation: impl Into<String>,
        arguments: Value,
    ) -> Self {
        Self {
            request_id,
            capability,
            operation: operation.into(),
            arguments,
            risk: RiskLevel::ReadOnly,
            cell_id: None,
            capability_grant_id: None,
            budget_lease_id: None,
        }
    }

    pub fn with_risk(mut self, risk: RiskLevel) -> Self {
        self.risk = risk;
        self
    }
}

fn capability_request_paths(request: &CapabilityRequest) -> Vec<String> {
    let mut paths = Vec::new();
    if let Some(path) = request
        .arguments
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        paths.push(path.to_owned());
    }
    if let Some(patch) = request.arguments.get("patch").and_then(Value::as_str) {
        for line in patch.lines() {
            let line = line.trim();
            for prefix in [
                "*** Add File:",
                "*** Update File:",
                "*** Delete File:",
                "*** Move to:",
            ] {
                if let Some(path) = line.strip_prefix(prefix).map(str::trim) {
                    if !path.is_empty() {
                        paths.push(path.to_owned());
                    }
                }
            }
        }
    }
    paths
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approve,
    Deny,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApprovalChallenge {
    pub schema: String,
    pub approval_id: ApprovalId,
    pub request_id: RequestId,
    pub request_hash: String,
    pub expires_at_unix_ms: u64,
    pub reason: String,
    #[serde(default)]
    pub nonce: String,
    #[serde(default)]
    pub policy_version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PendingApproval {
    pub challenge: ApprovalChallenge,
    pub request: CapabilityRequest,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PendingInvocation {
    pub approval_id: ApprovalId,
    pub challenge: ApprovalChallenge,
    pub request_id: RequestId,
    pub event_request_id: RequestId,
    pub event_sequence: u64,
    pub run_id: RunId,
    pub request: CapabilityRequest,
    pub context: RequestContext,
    pub sandbox: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AuthorizedCapabilityRequest {
    pub authorization_id: String,
    pub request: CapabilityRequest,
}

impl AuthorizedCapabilityRequest {
    pub fn new(
        authorization_id: impl Into<String>,
        request: CapabilityRequest,
    ) -> Result<Self, DomainError> {
        let authorization_id = authorization_id.into();
        if authorization_id.trim().is_empty() {
            return Err(DomainError::EmptyAuthorizationId);
        }
        Ok(Self {
            authorization_id,
            request,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapabilityResult {
    pub request_id: RequestId,
    pub success: bool,
    pub output: Value,
    pub evidence_refs: Vec<String>,
}

impl CapabilityResult {
    pub fn success(request_id: RequestId, output: Value) -> Self {
        Self {
            request_id,
            success: true,
            output,
            evidence_refs: Vec::new(),
        }
    }

    pub fn failure(request_id: RequestId, error: impl Into<String>) -> Self {
        Self {
            request_id,
            success: false,
            output: serde_json::json!({ "error": error.into() }),
            evidence_refs: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum PolicyDecision {
    Allow { authorization_id: String },
    Ask { reason: String },
    Deny { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum GateDecision {
    Allowed { authorization_id: String },
    AwaitingApproval { reason: String },
    Denied { reason: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    Staged,
    Active,
    Approved,
    Denied,
    Expired,
    Cancelled,
    Consumed,
}

impl ApprovalState {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Staged, Self::Active)
                | (Self::Active, Self::Approved)
                | (Self::Active, Self::Denied)
                | (Self::Active, Self::Expired)
                | (Self::Active, Self::Cancelled)
                | (Self::Approved, Self::Consumed)
        )
    }

    pub fn transition(self, next: Self) -> Result<Self, DomainError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(DomainError::InvalidStateTransition {
                aggregate: "approval",
                from: self.as_str(),
                to: next.as_str(),
            })
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Staged => "staged",
            Self::Active => "active",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
            Self::Consumed => "consumed",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Denied | Self::Expired | Self::Cancelled | Self::Consumed
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityExecutionState {
    Requested,
    PolicyChecked,
    AwaitingApproval,
    Authorized,
    Denied,
    Dispatching,
    Executing,
    Succeeded,
    Failed,
    Cancelled,
    Unknown,
}

impl CapabilityExecutionState {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Requested, Self::PolicyChecked)
                | (Self::PolicyChecked, Self::AwaitingApproval)
                | (Self::PolicyChecked, Self::Authorized)
                | (Self::PolicyChecked, Self::Denied)
                | (Self::Authorized, Self::Dispatching)
                | (Self::Dispatching, Self::Executing)
                | (Self::Executing, Self::Succeeded)
                | (Self::Executing, Self::Failed)
                | (Self::Executing, Self::Cancelled)
                | (Self::Executing, Self::Unknown)
                | (Self::AwaitingApproval, Self::Authorized)
                | (Self::AwaitingApproval, Self::Denied)
                | (Self::AwaitingApproval, Self::Cancelled)
                | (Self::AwaitingApproval, Self::Unknown)
        )
    }

    pub fn transition(self, next: Self) -> Result<Self, DomainError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(DomainError::InvalidStateTransition {
                aggregate: "capability_execution",
                from: self.as_str(),
                to: next.as_str(),
            })
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::PolicyChecked => "policy_checked",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Authorized => "authorized",
            Self::Denied => "denied",
            Self::Dispatching => "dispatching",
            Self::Executing => "executing",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Denied | Self::Succeeded | Self::Failed | Self::Cancelled | Self::Unknown
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkPacketStatus {
    Draft,
    Approved,
    Assigned,
    Running,
    Blocked,
    AwaitingApproval,
    Succeeded,
    Reviewed,
    Closed,
    Failed,
    Cancelled,
}

impl WorkPacketStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Draft, Self::Approved)
                | (Self::Approved, Self::Assigned)
                | (Self::Assigned, Self::Running)
                | (Self::Running, Self::Blocked)
                | (Self::Running, Self::AwaitingApproval)
                | (Self::Running, Self::Succeeded)
                | (Self::Running, Self::Failed)
                | (Self::Running, Self::Cancelled)
                | (Self::Blocked, Self::Running)
                | (Self::AwaitingApproval, Self::Running)
                | (Self::Succeeded, Self::Reviewed)
                | (Self::Reviewed, Self::Closed)
        )
    }

    pub fn transition(self, next: Self) -> Result<Self, DomainError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(DomainError::InvalidStateTransition {
                aggregate: "work_packet",
                from: self.as_str(),
                to: next.as_str(),
            })
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Approved => "approved",
            Self::Assigned => "assigned",
            Self::Running => "running",
            Self::Blocked => "blocked",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Succeeded => "succeeded",
            Self::Reviewed => "reviewed",
            Self::Closed => "closed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Closed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellLifecycle {
    Proposed,
    Validated,
    Spawning,
    Ready,
    Running,
    WaitingInput,
    Blocked,
    Checkpointing,
    ReadyToMerge,
    Merging,
    Succeeded,
    Retiring,
    Retired,
    CancelRequested,
    Cancelled,
    Stalled,
    Retrying,
    Failed,
    Quarantined,
}

impl CellLifecycle {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Proposed, Self::Validated)
                | (Self::Validated, Self::Spawning)
                | (Self::Spawning, Self::Ready)
                | (Self::Ready, Self::Running)
                | (Self::Running, Self::WaitingInput)
                | (Self::Running, Self::Blocked)
                | (Self::Running, Self::Checkpointing)
                | (Self::Running, Self::ReadyToMerge)
                | (Self::Running, Self::CancelRequested)
                | (Self::Running, Self::Stalled)
                | (Self::Running, Self::Failed)
                | (Self::Running, Self::Quarantined)
                | (Self::WaitingInput, Self::Running)
                | (Self::Blocked, Self::Running)
                | (Self::Checkpointing, Self::Running)
                | (Self::Checkpointing, Self::ReadyToMerge)
                | (Self::ReadyToMerge, Self::Merging)
                | (Self::ReadyToMerge, Self::Retiring)
                | (Self::Merging, Self::Succeeded)
                | (Self::Succeeded, Self::Retiring)
                | (Self::Retiring, Self::Retired)
                | (Self::Stalled, Self::Retrying)
                | (Self::Retrying, Self::Running)
                | (Self::Retrying, Self::Failed)
                | (Self::Failed, Self::Quarantined)
                | (Self::CancelRequested, Self::Cancelled)
                | (Self::Cancelled, Self::Retiring)
                | (Self::Quarantined, Self::Retiring)
        )
    }

    pub fn transition(self, next: Self) -> Result<Self, DomainError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(DomainError::InvalidStateTransition {
                aggregate: "cell",
                from: self.as_str(),
                to: next.as_str(),
            })
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Validated => "validated",
            Self::Spawning => "spawning",
            Self::Ready => "ready",
            Self::Running => "running",
            Self::WaitingInput => "waiting_input",
            Self::Blocked => "blocked",
            Self::Checkpointing => "checkpointing",
            Self::ReadyToMerge => "ready_to_merge",
            Self::Merging => "merging",
            Self::Succeeded => "succeeded",
            Self::Retiring => "retiring",
            Self::Retired => "retired",
            Self::CancelRequested => "cancel_requested",
            Self::Cancelled => "cancelled",
            Self::Stalled => "stalled",
            Self::Retrying => "retrying",
            Self::Failed => "failed",
            Self::Quarantined => "quarantined",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Retired | Self::Cancelled | Self::Quarantined)
    }
}

impl Default for ApprovalState {
    fn default() -> Self {
        Self::Staged
    }
}

impl Default for CapabilityExecutionState {
    fn default() -> Self {
        Self::Requested
    }
}

impl Default for WorkPacketStatus {
    fn default() -> Self {
        Self::Draft
    }
}

impl Default for CellLifecycle {
    fn default() -> Self {
        Self::Proposed
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Accepted,
    Denied,
    AwaitingApproval,
    Running,
    Completed,
    Failed,
    Cancelled,
    ResultUnknown,
    Blocked,
}

impl ExecutionStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Accepted, Self::Running)
                | (Self::Accepted, Self::AwaitingApproval)
                | (Self::Accepted, Self::Denied)
                | (Self::Accepted, Self::Blocked)
                | (Self::Running, Self::AwaitingApproval)
                | (Self::Running, Self::Completed)
                | (Self::Running, Self::Failed)
                | (Self::Running, Self::Cancelled)
                | (Self::Running, Self::ResultUnknown)
                | (Self::Running, Self::Blocked)
                | (Self::AwaitingApproval, Self::Running)
                | (Self::AwaitingApproval, Self::Denied)
                | (Self::AwaitingApproval, Self::Failed)
                | (Self::AwaitingApproval, Self::Cancelled)
                | (Self::AwaitingApproval, Self::ResultUnknown)
                | (Self::AwaitingApproval, Self::Blocked)
        )
    }

    pub fn transition(self, next: Self) -> Result<Self, DomainError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(DomainError::InvalidStateTransition {
                aggregate: "execution",
                from: self.as_str(),
                to: next.as_str(),
            })
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Denied => "denied",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::ResultUnknown => "result_unknown",
            Self::Blocked => "blocked",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Denied
                | Self::Completed
                | Self::Failed
                | Self::Cancelled
                | Self::ResultUnknown
                | Self::Blocked
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuntimeEvent {
    pub event_id: EventId,
    pub request_id: RequestId,
    pub sequence: u64,
    pub kind: String,
    pub data: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregate_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregate_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

impl RuntimeEvent {
    pub fn new(
        request_id: RequestId,
        sequence: u64,
        kind: impl Into<String>,
        data: Value,
    ) -> Result<Self, DomainError> {
        if sequence == 0 {
            return Err(DomainError::InvalidEventSequence);
        }
        Ok(Self {
            event_id: EventId::new(),
            request_id,
            sequence,
            kind: kind.into(),
            data,
            aggregate_type: None,
            aggregate_id: None,
            stream_version: None,
            idempotency_key: None,
        })
    }

    pub fn with_stream_metadata(
        mut self,
        aggregate_type: impl Into<String>,
        aggregate_id: impl Into<String>,
        stream_version: u64,
    ) -> Self {
        self.aggregate_type = Some(aggregate_type.into());
        self.aggregate_id = Some(aggregate_id.into());
        self.stream_version = Some(stream_version);
        self
    }

    pub fn with_idempotency_key(mut self, idempotency_key: impl Into<String>) -> Self {
        self.idempotency_key = Some(idempotency_key.into());
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CoreResponse {
    pub request_id: RequestId,
    pub status: ExecutionStatus,
    pub output: Value,
    pub error: Option<String>,
}

impl CoreResponse {
    pub fn completed(request_id: RequestId, output: Value) -> Self {
        Self {
            request_id,
            status: ExecutionStatus::Completed,
            output,
            error: None,
        }
    }

    pub fn blocked(request_id: RequestId, reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            request_id,
            status: ExecutionStatus::Blocked,
            output: Value::Null,
            error: Some(reason),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum DomainError {
    #[error("authorization_id_required")]
    EmptyAuthorizationId,
    #[error("event_sequence_must_be_positive")]
    InvalidEventSequence,
    #[error("{aggregate}_invalid_state_transition:{from}->{to}")]
    InvalidStateTransition {
        aggregate: &'static str,
        from: &'static str,
        to: &'static str,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_state_machine_is_single_use_and_terminal() {
        assert_eq!(
            ApprovalState::Staged
                .transition(ApprovalState::Active)
                .unwrap(),
            ApprovalState::Active
        );
        assert_eq!(
            ApprovalState::Active
                .transition(ApprovalState::Approved)
                .unwrap()
                .transition(ApprovalState::Consumed)
                .unwrap(),
            ApprovalState::Consumed
        );
        assert!(ApprovalState::Consumed.is_terminal());
        assert_eq!(
            ApprovalState::Consumed
                .transition(ApprovalState::Active)
                .unwrap_err(),
            DomainError::InvalidStateTransition {
                aggregate: "approval",
                from: "consumed",
                to: "active",
            }
        );
    }

    #[test]
    fn capability_execution_cannot_skip_authorization_or_recover_unknown() {
        assert!(!CapabilityExecutionState::Requested
            .can_transition_to(CapabilityExecutionState::Executing));
        assert!(CapabilityExecutionState::PolicyChecked
            .can_transition_to(CapabilityExecutionState::AwaitingApproval));
        assert!(CapabilityExecutionState::Executing
            .can_transition_to(CapabilityExecutionState::Unknown));
        assert!(CapabilityExecutionState::Unknown.is_terminal());
        assert!(!CapabilityExecutionState::Unknown
            .can_transition_to(CapabilityExecutionState::Succeeded));
    }

    #[test]
    fn cancelled_execution_is_terminal_and_serializes_distinctly() {
        assert!(ExecutionStatus::Running.can_transition_to(ExecutionStatus::Cancelled));
        assert!(ExecutionStatus::AwaitingApproval.can_transition_to(ExecutionStatus::Cancelled));
        assert!(ExecutionStatus::Cancelled.is_terminal());
        assert!(!ExecutionStatus::Cancelled.can_transition_to(ExecutionStatus::Running));
        let encoded = serde_json::to_string(&ExecutionStatus::Cancelled).unwrap();
        assert_eq!(encoded, "\"cancelled\"");
        assert_eq!(ExecutionStatus::Cancelled.as_str(), "cancelled");
    }

    #[test]
    fn work_packet_and_cell_terminals_cannot_return_to_running() {
        assert!(WorkPacketStatus::Closed.is_terminal());
        assert!(!WorkPacketStatus::Closed.can_transition_to(WorkPacketStatus::Running));
        assert!(WorkPacketStatus::Blocked.can_transition_to(WorkPacketStatus::Running));
        assert!(WorkPacketStatus::AwaitingApproval.can_transition_to(WorkPacketStatus::Running));
        assert!(CellLifecycle::Retired.is_terminal());
        assert!(!CellLifecycle::Retired.can_transition_to(CellLifecycle::Running));
        assert!(!CellLifecycle::Failed.is_terminal());
        assert!(CellLifecycle::CancelRequested.can_transition_to(CellLifecycle::Cancelled));
    }

    #[test]
    fn stable_ids_are_distinct_serializable_contract_types() {
        let turn = TurnId::new();
        let cell = CellId::new();
        assert_ne!(turn.to_string(), cell.to_string());
        let encoded = serde_json::to_string(&turn).unwrap();
        assert_eq!(serde_json::from_str::<TurnId>(&encoded).unwrap(), turn);
    }

    #[test]
    fn company_os_contracts_validate_and_child_grants_only_shrink() {
        let role = RoleSpec::builder();
        let template = AgentTemplate::for_role(&role, "1.0.0");
        assert!(template.validate().is_ok());

        let mut parent = CapabilityGrant {
            schema: CAPABILITY_GRANT_SCHEMA.to_owned(),
            grant_id: CapabilityGrantId::new(),
            capability: CapabilityKind::Filesystem,
            operation: "apply_patch".to_owned(),
            resources: vec!["workspace".to_owned()],
            paths: vec!["src".to_owned()],
            expires_at_unix_ms: 200,
            approval_id: None,
            delegation_allowed: true,
        };
        let child = CapabilityGrant {
            schema: CAPABILITY_GRANT_SCHEMA.to_owned(),
            grant_id: CapabilityGrantId::new(),
            capability: CapabilityKind::Filesystem,
            operation: "apply_patch".to_owned(),
            resources: vec!["workspace".to_owned()],
            paths: vec!["src/lib.rs".to_owned()],
            expires_at_unix_ms: 100,
            approval_id: None,
            delegation_allowed: false,
        };
        assert!(parent.validate().is_ok());
        assert!(child.validate().is_ok());
        assert!(parent.contains(&child));
        parent.delegation_allowed = false;
        let mut delegated_child = child.clone();
        delegated_child.delegation_allowed = true;
        assert!(!parent.contains(&delegated_child));
    }

    #[test]
    fn budget_lease_rejects_overconsumption_without_mutating_usage() {
        let lease = BudgetLease::new(2, 100, 1_000, 1, 1);
        assert!(lease.can_consume(2, 100, 1));
        assert!(!lease.can_consume(3, 100, 1));
        assert_eq!(lease.tool_calls_used, 0);
        assert_eq!(lease.effects_used, 0);
    }

    #[test]
    fn work_packet_legacy_json_defaults_to_draft_and_round_trips_new_fields() {
        let packet: WorkPacket = serde_json::from_value(serde_json::json!({
            "schema": WORK_PACKET_SCHEMA,
            "id": "wp-1",
            "goal": "ship a change"
        }))
        .unwrap();
        assert_eq!(packet.status, WorkPacketStatus::Draft);
        assert!(packet.project_id.is_none());

        let mut packet = packet;
        packet.project_id = Some(ProjectId::new());
        packet.acceptance_tests = vec!["cargo test".to_owned()];
        packet
            .transition_status(WorkPacketStatus::Approved)
            .unwrap();
        let encoded = serde_json::to_value(&packet).unwrap();
        assert_eq!(encoded["status"], "approved");
        assert_eq!(encoded["acceptance_tests"][0], "cargo test");

        packet.status = WorkPacketStatus::Closed;
        assert_eq!(packet.validate(), Err("packet_status_terminal"));
    }

    #[test]
    fn runtime_event_supports_optional_company_os_stream_metadata() {
        let request_id = RequestId::new();
        let event = RuntimeEvent::new(request_id, 2, "run.completed", Value::Null)
            .unwrap()
            .with_stream_metadata("request", request_id.to_string(), 2)
            .with_idempotency_key("run-2-completed");

        assert_eq!(event.aggregate_type.as_deref(), Some("request"));
        assert_eq!(
            event.aggregate_id.as_deref().unwrap(),
            request_id.to_string()
        );
        assert_eq!(event.stream_version, Some(2));
        assert_eq!(event.idempotency_key.as_deref(), Some("run-2-completed"));

        let legacy: RuntimeEvent = serde_json::from_value(serde_json::json!({
            "event_id": event.event_id,
            "request_id": request_id,
            "sequence": 1,
            "kind": "run.accepted",
            "data": null
        }))
        .unwrap();
        assert!(legacy.aggregate_type.is_none());
        assert!(legacy.idempotency_key.is_none());
    }

    #[test]
    fn request_context_defaults_to_untrusted() {
        let context = RequestContext::local("session-1", "/repo");
        assert!(!context.project_trusted);
        assert_eq!(context.permission_profile, PermissionProfile::Safe);
    }

    #[test]
    fn v0_2_worker_is_executing_builder() {
        let role = RoleSpec::builder();
        let department = DepartmentSpec::executing();
        assert_eq!(role.role_id, ROLE_BUILDER);
        assert_eq!(role.department_id, DEPARTMENT_EXECUTING);
        assert_eq!(
            role.tools,
            [
                "shell",
                "apply_patch",
                "mcp",
                "memory.search",
                "memory.write"
            ]
        );
        assert_eq!(role.sandbox, ROLE_SANDBOX_WORKSPACE_WRITE);
        assert_eq!(role.path_allow, ["."]);
        assert!(!role.prompt_hash.is_empty());
        assert_eq!(department.department_id, DEPARTMENT_EXECUTING);
        assert_eq!(department.roles, [ROLE_BUILDER]);
        assert!(department.can_convene);
        assert!(role.can_convene);
    }

    #[test]
    fn v0_3_catalog_has_planning_and_executing_roles() {
        let planning = DepartmentSpec::planning();
        assert_eq!(planning.department_id, DEPARTMENT_PLANNING);
        assert_eq!(planning.roles, [ROLE_PM, ROLE_ARCHITECT]);
        assert!(planning.can_convene);

        let pm = RoleSpec::pm();
        assert_eq!(pm.department_id, DEPARTMENT_PLANNING);
        assert_eq!(pm.tools, ["apply_patch", "memory.search", "memory.write"]);
        assert!(pm.can_convene);
        assert!(pm.allows_path("plan/WORK.md"));
        assert!(pm.allows_path("charter/GOAL.md"));
        assert!(pm.allows_path("packet/task.json"));
        assert!(!pm.allows_path("GOLDEN_PATH.txt"));
        assert!(!pm.allows_path("src/lib.rs"));
        assert!(!pm.allows_tool("shell"));

        let architect = RoleSpec::architect();
        assert_eq!(architect.department_id, DEPARTMENT_PLANNING);
        assert_eq!(architect.tools, ["memory.search"]);
        assert!(!architect.workspace_write_allowed());
        assert!(!architect.allows_path("plan/WORK.md"));
        assert!(!architect.can_convene);
        assert!(architect.can_vote);

        assert_eq!(RoleSpec::lookup("pm").unwrap().role_id, ROLE_PM);
        assert_eq!(RoleSpec::lookup("").unwrap().role_id, ROLE_BUILDER);
        assert!(RoleSpec::lookup("ceo").is_none());

        let monitoring = DepartmentSpec::monitoring();
        assert_eq!(monitoring.department_id, DEPARTMENT_MONITORING);
        assert_eq!(monitoring.roles, [ROLE_REVIEWER]);
        assert!(monitoring.can_convene);
        let reviewer = RoleSpec::reviewer();
        assert!(reviewer.can_convene);
        assert_eq!(reviewer.department_id, DEPARTMENT_MONITORING);
        assert_eq!(reviewer.tools, ["memory.search"]);
        assert!(!reviewer.workspace_write_allowed());
        assert!(!reviewer.allows_tool("apply_patch"));
        assert!(!reviewer.allows_path("GOLDEN_PATH.txt"));
        assert_eq!(RoleSpec::lookup("reviewer").unwrap().role_id, ROLE_REVIEWER);
        assert_eq!(
            DepartmentSpec::lookup("planning").unwrap().department_id,
            DEPARTMENT_PLANNING
        );
    }

    #[test]
    fn v0_5_catalog_has_five_departments_without_changing_default_worker() {
        let ids: Vec<_> = DepartmentSpec::catalog()
            .into_iter()
            .map(|department| department.department_id)
            .collect();
        assert_eq!(
            ids,
            [
                DEPARTMENT_INITIATING,
                DEPARTMENT_PLANNING,
                DEPARTMENT_EXECUTING,
                DEPARTMENT_MONITORING,
                DEPARTMENT_CLOSING,
            ]
        );

        let initiating = DepartmentSpec::initiating();
        assert_eq!(initiating.pmp_group, DEPARTMENT_INITIATING);
        assert_eq!(initiating.roles, [ROLE_SPONSOR]);
        assert_eq!(initiating.artifacts, [PLANNING_PATH_CHARTER]);
        assert_eq!(initiating.rag_collection, "department:initiating");
        assert!(initiating.can_convene);
        assert!(!initiating.mission.is_empty());
        assert!(!initiating.gates.is_empty());

        let sponsor = RoleSpec::sponsor();
        assert_eq!(sponsor.department_id, DEPARTMENT_INITIATING);
        assert!(sponsor.allows_path("charter/GOAL.md"));
        assert!(!sponsor.allows_path("GOLDEN_PATH.txt"));
        assert!(!sponsor.allows_path("src/lib.rs"));
        assert!(!sponsor.allows_tool("shell"));
        assert_eq!(RoleSpec::lookup("sponsor").unwrap().role_id, ROLE_SPONSOR);

        let closing = DepartmentSpec::closing();
        assert_eq!(closing.roles, [ROLE_CLOSER]);
        assert_eq!(closing.artifacts, [CLOSING_PATH_LESSONS]);
        assert!(closing.can_convene);

        let closer = RoleSpec::closer();
        assert!(closer.can_convene);
        assert_eq!(closer.department_id, DEPARTMENT_CLOSING);
        assert!(closer.allows_path("lessons/LEARNED.md"));
        assert!(!closer.allows_path("GOLDEN_PATH.txt"));
        assert!(!closer.allows_tool("shell"));
        assert_eq!(RoleSpec::lookup("closer").unwrap().role_id, ROLE_CLOSER);

        assert_eq!(RoleSpec::lookup("").unwrap().role_id, ROLE_BUILDER);
        assert_eq!(
            DepartmentSpec::lookup("").unwrap().department_id,
            DEPARTMENT_EXECUTING
        );
        assert!(RoleSpec::lookup("ceo").is_none());
    }

    #[test]
    fn v0_5_memory_grants_keep_builder_off_private_and_unreleased() {
        assert_eq!(MEMORY_LAYERS.len(), 6);
        let builder = RoleSpec::builder();
        assert!(builder.allows_knowledge(MEMORY_LAYER_COMPANY));
        assert!(builder.allows_knowledge(MEMORY_LAYER_PROJECT));
        assert!(builder.allows_knowledge("project:code"));
        assert!(builder.allows_knowledge("role:builder"));
        assert!(builder.allows_knowledge(MEMORY_LAYER_INSTANCE_SCRATCH));
        assert!(!builder.allows_knowledge(MEMORY_COLLECTION_USER_PRIVATE));
        assert!(!builder.allows_knowledge(MEMORY_COLLECTION_USER_PREFS));
        assert!(!builder.allows_knowledge(MEMORY_COLLECTION_PLANNING_UNRELEASED));
        assert!(!builder.allows_knowledge("project:events"));
        assert!(builder.allows_memory_write(MEMORY_LAYER_INSTANCE_SCRATCH));
        assert!(!builder.allows_memory_write(MEMORY_LAYER_PROJECT));
        assert!(!builder.allows_memory_write(MEMORY_COLLECTION_USER_PRIVATE));

        let pm = RoleSpec::pm();
        assert!(pm.allows_knowledge(MEMORY_COLLECTION_USER_PREFS));
        assert!(pm.allows_knowledge(MEMORY_COLLECTION_PLANNING_UNRELEASED));
        assert!(!pm.allows_knowledge(MEMORY_COLLECTION_USER_PRIVATE));
        assert!(pm.allows_memory_write("department:planning"));
        assert!(!pm.allows_memory_write(MEMORY_LAYER_INSTANCE_SCRATCH));

        let reviewer = RoleSpec::reviewer();
        assert!(reviewer.allows_knowledge("project:events"));
        assert!(!reviewer.allows_knowledge("project:code"));
        assert!(!reviewer.allows_memory_write(MEMORY_LAYER_INSTANCE_SCRATCH));
        assert!(MemoryCollection::parse("not-a-layer").is_none());
    }

    #[test]
    fn request_context_defaults_to_executing_builder() {
        let context = RequestContext::local("session-1", "/repo");
        assert_eq!(context.role_id, ROLE_BUILDER);
        assert_eq!(context.department_id, DEPARTMENT_EXECUTING);
        assert_eq!(context.work_packet_id, None);
        assert!(context.path_allow.is_empty());
    }

    #[test]
    fn builder_lock_paths_exclusive_when_empty_and_overlap_on_prefix() {
        assert_eq!(builder_lock_paths(&[]), vec![EXCLUSIVE_PATH_LOCK]);
        assert_eq!(
            builder_lock_paths(&["ALPHA.txt".to_owned(), "ALPHA.txt".to_owned()]),
            vec!["ALPHA.txt"]
        );
        assert!(path_locks_conflict("ALPHA.txt", "ALPHA.txt"));
        assert!(path_locks_conflict("src", "src/lib.rs"));
        assert!(!path_locks_conflict("ALPHA.txt", "BRAVO.txt"));
        assert!(path_locks_conflict(EXCLUSIVE_PATH_LOCK, "ALPHA.txt"));
        assert!(allow_list_covers(&["ALPHA.txt".to_owned()], "ALPHA.txt"));
        assert!(!allow_list_covers(
            &["ALPHA.txt".to_owned()],
            "GOLDEN_PATH.txt"
        ));
    }

    #[test]
    fn work_packet_prompt_is_only_packet_fields() {
        let mut packet =
            WorkPacket::builder_task("wp-1", "create GOLDEN_PATH.txt containing hello");
        packet.path_allow = vec!["GOLDEN_PATH.txt".to_owned()];
        packet.acceptance = vec!["file exists".to_owned()];
        packet.validate().unwrap();
        let prompt = packet.as_prompt();
        assert!(prompt.contains("Work packet wp-1"));
        assert!(prompt.contains("Goal: create GOLDEN_PATH.txt containing hello"));
        assert!(prompt.contains("Path allow: GOLDEN_PATH.txt"));
        assert!(!prompt.contains("PLANNER_SECRET_TOKEN"));
        let mut architect = packet.clone();
        architect.assignee_role = ROLE_ARCHITECT.to_owned();
        assert_eq!(architect.validate(), Err("packet_role_must_be_builder"));
        let allowed =
            WorkPacket::builder_task("wp-2", "create ALPHA.txt").with_path_allow(["ALPHA.txt"]);
        assert_eq!(allowed.path_allow, ["ALPHA.txt"]);
    }

    #[test]
    fn planning_symposium_excludes_builder_and_prompt_is_blackboard_only() {
        let mut meeting = Symposium::planning("sym-1", "one vertical slice vs two packets", 2);
        meeting
            .blackboard
            .claims
            .push(SymposiumClaim::new(ROLE_PM, "choose the vertical slice"));
        assert_eq!(meeting.chair, ROLE_PM);
        assert_eq!(meeting.attendees, [ROLE_PM, ROLE_ARCHITECT]);
        assert!(!meeting.attendees.iter().any(|role| role == ROLE_BUILDER));
        let prompt = meeting.speaker_prompt(ROLE_ARCHITECT);
        assert!(prompt.contains("Speak as architect"));
        assert!(prompt.contains("Blackboard claims:"));
        assert!(prompt.contains("choose the vertical slice"));
        assert!(!prompt.contains("PLANNER_SECRET_TOKEN"));
        assert_eq!(
            Symposium::validate_max_rounds(0),
            Err("symposium_max_rounds_invalid")
        );
        meeting.validate().unwrap();
        let (decision, packet) = meeting.close(false).unwrap();
        let packet = packet.expect("planning packet");
        assert_eq!(decision.schema, DECISION_RECORD_SCHEMA);
        assert_eq!(packet.assignee_role, ROLE_BUILDER);
        assert!(!decision.skipped_meeting);
        assert_eq!(meeting.status, SYMPOSIUM_STATUS_CLOSED);
        assert_eq!(packet.goal, "one vertical slice vs two packets");

        let mut skipped =
            Symposium::planning("sym-2", "create GOLDEN_PATH.txt containing hello", 4);
        let (decision, packet) = skipped.close(true).unwrap();
        let packet = packet.expect("skipped planning packet");
        assert!(decision.skipped_meeting);
        assert_eq!(skipped.status, SYMPOSIUM_STATUS_SKIPPED);
        assert_eq!(packet.goal, "create GOLDEN_PATH.txt containing hello");

        let mut with_builder =
            Symposium::planning("sym-3", "create GOLDEN_PATH.txt containing hello", 4);
        with_builder.attendees.push(ROLE_BUILDER.to_owned());
        assert_eq!(
            with_builder.validate(),
            Err("symposium_builder_not_attendee")
        );
    }

    #[test]
    fn each_department_can_convene_without_a_joint_meeting() {
        let expected = [
            (
                DEPARTMENT_INITIATING,
                ROLE_SPONSOR,
                INITIATING_DECISION_PATH,
                false,
            ),
            (DEPARTMENT_PLANNING, ROLE_PM, DECISION_RECORD_PATH, true),
            (
                DEPARTMENT_EXECUTING,
                ROLE_BUILDER,
                EXECUTING_DECISION_PATH,
                false,
            ),
            (
                DEPARTMENT_MONITORING,
                ROLE_REVIEWER,
                MONITORING_DECISION_PATH,
                false,
            ),
            (
                DEPARTMENT_CLOSING,
                ROLE_CLOSER,
                CLOSING_DECISION_PATH,
                false,
            ),
        ];
        for (department_id, chair, path, emits_packet) in expected {
            let mut meeting =
                Symposium::department(department_id, format!("sym-{department_id}"), "decide", 2)
                    .unwrap();
            assert_eq!(meeting.chair, chair);
            assert_eq!(meeting.decision_path(), path);
            assert_eq!(
                meeting.builder_present(),
                department_id == DEPARTMENT_EXECUTING
            );
            meeting.validate().unwrap();
            let (decision, packet) = meeting.close(true).unwrap();
            assert!(decision.skipped_meeting);
            assert_eq!(packet.is_some(), emits_packet);
            if department_id != DEPARTMENT_EXECUTING {
                assert!(!meeting.attendees.iter().any(|role| role == ROLE_BUILDER));
            }
        }

        let mut architect_chair =
            Symposium::planning("sym-arch", "one vertical slice vs two packets", 2);
        architect_chair.chair = ROLE_ARCHITECT.to_owned();
        assert_eq!(
            architect_chair.validate(),
            Err("symposium_chair_must_be_pm")
        );

        let mut joint = Symposium::planning("sym-joint", "one vertical slice vs two packets", 2);
        joint.attendees = vec![ROLE_PM.to_owned(), ROLE_REVIEWER.to_owned()];
        assert_eq!(joint.validate(), Err("joint_symposium_frozen"));
    }

    #[test]
    fn capability_request_serialization_contains_reference_not_secret_value() {
        let request = CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Secret,
            "resolve",
            serde_json::json!({ "secret_ref": "provider/anthropic" }),
        );
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("secret_ref"));
        assert!(!json.contains("secret_value"));
    }

    #[test]
    fn authorization_and_event_invariants_fail_closed() {
        let request = CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Query,
            "search",
            Value::Null,
        );
        assert_eq!(
            AuthorizedCapabilityRequest::new("", request).unwrap_err(),
            DomainError::EmptyAuthorizationId
        );
        assert_eq!(
            RuntimeEvent::new(RequestId::new(), 0, "invalid", Value::Null).unwrap_err(),
            DomainError::InvalidEventSequence
        );
    }

    #[test]
    fn review_packet_rejects_author_session() {
        let packet = ReviewPacket::closed(
            "rv-1",
            "builder-1",
            ROLE_BUILDER,
            "builder-1",
            "pass",
            "same session",
            vec!["GOLDEN_PATH.txt".to_owned()],
        );
        assert_eq!(packet.validate(), Err("review_author_session_denied"));
    }

    #[test]
    fn review_packet_requires_builder_author() {
        let packet = ReviewPacket::closed(
            "rv-1",
            "builder-1",
            ROLE_PM,
            "reviewer-1",
            "pass",
            "pm authored",
            Vec::new(),
        );
        assert_eq!(packet.validate(), Err("review_author_must_be_builder"));
    }
}
