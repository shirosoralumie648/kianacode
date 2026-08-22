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
uuid_id!(EventId);
uuid_id!(ApprovalId);

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
pub const DEPARTMENT_EXECUTING: &str = "executing";
pub const DEPARTMENT_PLANNING: &str = "planning";
pub const DEPARTMENT_MONITORING: &str = "monitoring";
pub const ROLE_SANDBOX_READ_ONLY: &str = "read-only";
pub const ROLE_SANDBOX_WORKSPACE_WRITE: &str = "workspace-write";
pub const PLANNING_PATH_CHARTER: &str = "charter";
pub const PLANNING_PATH_PLAN: &str = "plan";
pub const PLANNING_PATH_PACKET: &str = "packet";
pub const WORK_PACKET_SCHEMA: &str = "kiana.work-packet.v1";
pub const DECISION_RECORD_SCHEMA: &str = "kiana.decision-record.v1";
pub const SYMPOSIUM_SCHEMA: &str = "kiana.symposium.v1";
pub const SYMPOSIUM_RESULT_SCHEMA: &str = "kiana.symposium-result.v1";
pub const SYMPOSIUM_TYPE_DECISION: &str = "decision";
pub const SYMPOSIUM_STATUS_CLOSED: &str = "closed";
pub const SYMPOSIUM_STATUS_SKIPPED: &str = "skipped";
pub const DECISION_RECORD_PATH: &str = "plan/DECISION.json";
pub const WORK_PACKET_PATH: &str = "packet/TASK.json";
pub const REVIEW_PACKET_SCHEMA: &str = "kiana.review-packet.v1";
pub const REVIEW_RESULT_SCHEMA: &str = "kiana.review-result.v1";
pub const REVIEW_PACKET_PATH: &str = "gate/REVIEW.json";
pub const MONITORING_PATH_GATE: &str = "gate";

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
            "You are Kiana's executing Builder. Use only the provided tools `shell` and `apply_patch`. Never request danger-full-access.",
            vec!["shell".to_owned(), "apply_patch".to_owned()],
            ROLE_SANDBOX_WORKSPACE_WRITE,
            vec![".".to_owned()],
            vec!["project".to_owned()],
            false,
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
            vec!["apply_patch".to_owned()],
            ROLE_SANDBOX_WORKSPACE_WRITE,
            vec![
                PLANNING_PATH_CHARTER.to_owned(),
                PLANNING_PATH_PLAN.to_owned(),
                PLANNING_PATH_PACKET.to_owned(),
            ],
            vec!["department:planning".to_owned(), "project".to_owned()],
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
            Vec::new(),
            ROLE_SANDBOX_READ_ONLY,
            Vec::new(),
            vec!["department:planning".to_owned(), "project".to_owned()],
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
            Vec::new(),
            ROLE_SANDBOX_READ_ONLY,
            Vec::new(),
            vec!["department:monitoring".to_owned(), "project:events".to_owned()],
            false,
            false,
            "monitoring",
            8,
        )
    }

    pub fn catalog() -> [RoleSpec; 4] {
        [
            Self::pm(),
            Self::architect(),
            Self::builder(),
            Self::reviewer(),
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
pub struct DepartmentSpec {
    pub department_id: String,
    pub roles: Vec<String>,
    pub can_convene: bool,
}

impl DepartmentSpec {
    pub fn executing() -> Self {
        Self {
            department_id: DEPARTMENT_EXECUTING.to_owned(),
            roles: vec![ROLE_BUILDER.to_owned()],
            can_convene: false,
        }
    }

    pub fn planning() -> Self {
        Self {
            department_id: DEPARTMENT_PLANNING.to_owned(),
            roles: vec![ROLE_PM.to_owned(), ROLE_ARCHITECT.to_owned()],
            can_convene: true,
        }
    }

    pub fn monitoring() -> Self {
        Self {
            department_id: DEPARTMENT_MONITORING.to_owned(),
            roles: vec![ROLE_REVIEWER.to_owned()],
            can_convene: false,
        }
    }

    pub fn catalog() -> [DepartmentSpec; 3] {
        [Self::planning(), Self::executing(), Self::monitoring()]
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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkPacket {
    pub schema: String,
    pub id: String,
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

fn default_from_department() -> String {
    DEPARTMENT_PLANNING.to_owned()
}

impl WorkPacket {
    pub fn builder_task(id: impl Into<String>, goal: impl Into<String>) -> Self {
        Self {
            schema: WORK_PACKET_SCHEMA.to_owned(),
            id: id.into(),
            from_department: default_from_department(),
            to_department: default_department_id(),
            assignee_role: default_role_id(),
            goal: goal.into(),
            path_allow: Vec::new(),
            acceptance: Vec::new(),
            forbidden: Vec::new(),
        }
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
        push_packet_list(&mut lines, "Path allow", &self.path_allow);
        push_packet_list(&mut lines, "Acceptance", &self.acceptance);
        push_packet_list(&mut lines, "Forbidden", &self.forbidden);
        lines.join("\n")
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
        Self {
            schema: SYMPOSIUM_SCHEMA.to_owned(),
            id: id.into(),
            department_id: DEPARTMENT_PLANNING.to_owned(),
            symposium_type: SYMPOSIUM_TYPE_DECISION.to_owned(),
            agenda: agenda.into(),
            chair: ROLE_PM.to_owned(),
            attendees: vec![ROLE_PM.to_owned(), ROLE_ARCHITECT.to_owned()],
            max_rounds,
            blackboard: Blackboard::default(),
            status: "proposed".to_owned(),
            decision_id: None,
            work_packet_id: None,
        }
    }

    pub fn validate_max_rounds(max_rounds: u32) -> Result<u32, &'static str> {
        if max_rounds == 0 || max_rounds > Self::MAX_ROUNDS_CAP {
            Err("symposium_max_rounds_invalid")
        } else {
            Ok(max_rounds)
        }
    }

    pub fn speaker_prompt(&self, role_id: &str) -> String {
        format!(
            "Planning symposium {}\nChair: {}\nAttendees: {}\nSpeak as {}\nAgenda: {}\n{}\nReply with a claim or vote. Do not patch source. Do not invite the Builder.",
            self.id.trim(),
            self.chair.trim(),
            self.attendees.join(", "),
            role_id.trim(),
            self.agenda.trim(),
            self.blackboard.as_prompt()
        )
    }

    pub fn speaker_session_id(&self, role_id: &str) -> String {
        format!("{}-{}", self.id.trim(), role_id.trim())
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        Self::validate_max_rounds(self.max_rounds)?;
        if self.agenda.trim().is_empty() {
            return Err("symposium_goal_required");
        }
        if self.chair.trim() != ROLE_PM {
            return Err("symposium_chair_must_be_pm");
        }
        if self
            .attendees
            .iter()
            .any(|role| role.trim() == ROLE_BUILDER)
        {
            return Err("symposium_builder_not_attendee");
        }
        let expected = [ROLE_PM, ROLE_ARCHITECT];
        if self.attendees.len() != expected.len()
            || self
                .attendees
                .iter()
                .zip(expected)
                .any(|(got, want)| got.trim() != want)
        {
            return Err("symposium_attendees_invalid");
        }
        Ok(())
    }

    pub fn close(
        &mut self,
        skipped_meeting: bool,
    ) -> Result<(DecisionRecord, WorkPacket), &'static str> {
        self.validate()?;
        let packet_id = format!("wp-{}", self.id.trim());
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
            format!("Planning symposium closed: {decision_text}")
        };
        let packet = WorkPacket::builder_task(packet_id.clone(), self.agenda.trim());
        packet.validate()?;
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
        self.work_packet_id = Some(packet_id);
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
        }
    }

    pub fn with_risk(mut self, risk: RiskLevel) -> Self {
        self.risk = risk;
        self
    }
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
}

#[derive(Clone, Debug, PartialEq)]
pub struct PendingApproval {
    pub challenge: ApprovalChallenge,
    pub request: CapabilityRequest,
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
pub enum ExecutionStatus {
    Accepted,
    Denied,
    AwaitingApproval,
    Running,
    Completed,
    Failed,
    ResultUnknown,
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuntimeEvent {
    pub event_id: EventId,
    pub request_id: RequestId,
    pub sequence: u64,
    pub kind: String,
    pub data: Value,
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
        })
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(role.tools, ["shell", "apply_patch"]);
        assert_eq!(role.sandbox, ROLE_SANDBOX_WORKSPACE_WRITE);
        assert_eq!(role.path_allow, ["."]);
        assert!(!role.prompt_hash.is_empty());
        assert_eq!(department.department_id, DEPARTMENT_EXECUTING);
        assert_eq!(department.roles, [ROLE_BUILDER]);
        assert!(!department.can_convene);
    }

    #[test]
    fn v0_3_catalog_has_planning_and_executing_roles() {
        let planning = DepartmentSpec::planning();
        assert_eq!(planning.department_id, DEPARTMENT_PLANNING);
        assert_eq!(planning.roles, [ROLE_PM, ROLE_ARCHITECT]);
        assert!(planning.can_convene);

        let pm = RoleSpec::pm();
        assert_eq!(pm.department_id, DEPARTMENT_PLANNING);
        assert_eq!(pm.tools, ["apply_patch"]);
        assert!(pm.can_convene);
        assert!(pm.allows_path("plan/WORK.md"));
        assert!(pm.allows_path("charter/GOAL.md"));
        assert!(pm.allows_path("packet/task.json"));
        assert!(!pm.allows_path("GOLDEN_PATH.txt"));
        assert!(!pm.allows_path("src/lib.rs"));
        assert!(!pm.allows_tool("shell"));

        let architect = RoleSpec::architect();
        assert_eq!(architect.department_id, DEPARTMENT_PLANNING);
        assert!(architect.tools.is_empty());
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
        assert!(!monitoring.can_convene);
        let reviewer = RoleSpec::reviewer();
        assert_eq!(reviewer.department_id, DEPARTMENT_MONITORING);
        assert!(reviewer.tools.is_empty());
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
    fn request_context_defaults_to_executing_builder() {
        let context = RequestContext::local("session-1", "/repo");
        assert_eq!(context.role_id, ROLE_BUILDER);
        assert_eq!(context.department_id, DEPARTMENT_EXECUTING);
        assert_eq!(context.work_packet_id, None);
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
        assert_eq!(decision.schema, DECISION_RECORD_SCHEMA);
        assert_eq!(packet.assignee_role, ROLE_BUILDER);
        assert!(!decision.skipped_meeting);
        assert_eq!(meeting.status, SYMPOSIUM_STATUS_CLOSED);
        assert_eq!(packet.goal, "one vertical slice vs two packets");

        let mut skipped =
            Symposium::planning("sym-2", "create GOLDEN_PATH.txt containing hello", 4);
        let (decision, packet) = skipped.close(true).unwrap();
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
