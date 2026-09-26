//! Explicit Company/standalone task scope and fresh-run context boundary.
//!
//! A packet is not a permission by itself.  This contract records the server-derived mode,
//! packet/role/path/input binding and fresh-session rule that Core and Runner adapters must carry
//! together.  Private transcript/planning history is never an input reference in a Company task.

use crate::{json_digest, DepartmentPacket, RequestContext, RunId, SessionId, WorkPacket};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const COMPANY_TASK_SCOPE_SCHEMA: &str = "kiana.company-task-scope.v1";
pub const FRESH_TASK_RUN_SCHEMA: &str = "kiana.fresh-task-run.v1";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains('\0') {
        Err(field)
    } else {
        Ok(())
    }
}

fn private_ref(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.contains("private") || value.contains("transcript") || value.contains("prompt-history")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskExecutionMode {
    Company,
    Standalone,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyTaskScope {
    pub schema: String,
    pub mode: TaskExecutionMode,
    pub project_id: Option<String>,
    pub packet_id: Option<String>,
    pub assignment_ref: Option<String>,
    pub attempt_ref: Option<String>,
    pub role_id: String,
    pub department_id: String,
    pub session_id: SessionId,
    pub parent_session_id: Option<SessionId>,
    pub input_refs: Vec<String>,
    pub authorized_retrieval_refs: Vec<String>,
    pub path_allow: Vec<String>,
    pub private_history_allowed: bool,
    pub scope_digest: String,
}

impl CompanyTaskScope {
    pub fn company_builder(
        context: &RequestContext,
        packet: &WorkPacket,
    ) -> Result<Self, &'static str> {
        let project_id = packet
            .project_id
            .map(|id| id.to_string())
            .ok_or("company_task_project_required")?;
        let assignment_ref = context
            .actor_id
            .clone()
            .ok_or("company_task_assignment_required")?;
        if context.work_packet_id.as_deref() != Some(packet.id.as_str()) {
            return Err("company_task_packet_scope_mismatch");
        }
        if context.role_id != "builder" || context.department_id != "executing" {
            return Err("company_task_role_scope_mismatch");
        }
        if context.path_allow != packet.path_allow {
            return Err("company_task_path_scope_mismatch");
        }
        let scope = Self {
            schema: COMPANY_TASK_SCOPE_SCHEMA.to_owned(),
            mode: TaskExecutionMode::Company,
            project_id: Some(project_id),
            packet_id: Some(packet.id.clone()),
            assignment_ref: Some(assignment_ref),
            attempt_ref: None,
            role_id: context.role_id.clone(),
            department_id: context.department_id.clone(),
            session_id: context.session_id.clone(),
            parent_session_id: None,
            input_refs: packet.inputs.clone(),
            authorized_retrieval_refs: packet.inputs.clone(),
            path_allow: packet.path_allow.clone(),
            private_history_allowed: false,
            scope_digest: String::new(),
        };
        scope.with_digest_and_validate()
    }

    pub fn standalone(context: &RequestContext, packet: &WorkPacket) -> Result<Self, &'static str> {
        let scope = Self {
            schema: COMPANY_TASK_SCOPE_SCHEMA.to_owned(),
            mode: TaskExecutionMode::Standalone,
            project_id: None,
            packet_id: None,
            assignment_ref: None,
            attempt_ref: None,
            role_id: context.role_id.clone(),
            department_id: context.department_id.clone(),
            session_id: context.session_id.clone(),
            parent_session_id: None,
            input_refs: packet.inputs.clone(),
            authorized_retrieval_refs: packet.inputs.clone(),
            path_allow: packet.path_allow.clone(),
            private_history_allowed: false,
            scope_digest: String::new(),
        };
        scope.with_digest_and_validate()
    }

    pub fn for_department_packet(
        packet: &DepartmentPacket,
        owner_session_id: SessionId,
        current_session_id: SessionId,
        authorized_retrieval_refs: Vec<String>,
        path_allow: Vec<String>,
    ) -> Result<Self, &'static str> {
        packet.validate()?;
        if owner_session_id == current_session_id {
            return Err("company_task_session_must_be_fresh");
        }
        let scope = Self {
            schema: COMPANY_TASK_SCOPE_SCHEMA.to_owned(),
            mode: TaskExecutionMode::Company,
            project_id: Some(packet.project_id.clone()),
            packet_id: Some(packet.packet_id.clone()),
            assignment_ref: Some(packet.assignment_ref.clone()),
            attempt_ref: None,
            role_id: packet.target_role.clone(),
            department_id: packet.from_department.clone(),
            session_id: current_session_id,
            parent_session_id: Some(owner_session_id),
            input_refs: packet.input_refs.clone(),
            authorized_retrieval_refs,
            path_allow,
            private_history_allowed: false,
            scope_digest: String::new(),
        };
        scope.with_digest_and_validate()
    }

    fn with_digest_and_validate(mut self) -> Result<Self, &'static str> {
        self.scope_digest = self.canonical_digest();
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_TASK_SCOPE_SCHEMA || self.session_id.is_empty() {
            return Err("company_task_scope_invalid");
        }
        required(&self.role_id, "company_task_role_required")?;
        required(&self.department_id, "company_task_department_required")?;
        if self.private_history_allowed
            || self.input_refs.iter().any(|value| private_ref(value))
            || self
                .authorized_retrieval_refs
                .iter()
                .any(|value| private_ref(value))
        {
            return Err("company_task_private_history_forbidden");
        }
        if self
            .input_refs
            .iter()
            .chain(self.authorized_retrieval_refs.iter())
            .any(|value| required(value, "company_task_input_ref_invalid").is_err())
        {
            return Err("company_task_input_ref_invalid");
        }
        if self
            .path_allow
            .iter()
            .any(|path| crate::normalize_role_path(path).is_none())
        {
            return Err("company_task_path_scope_invalid");
        }
        match self.mode {
            TaskExecutionMode::Company => {
                if self.project_id.is_none()
                    || self.packet_id.is_none()
                    || self.assignment_ref.is_none()
                {
                    return Err("company_task_company_binding_required");
                }
                if self
                    .parent_session_id
                    .as_ref()
                    .is_some_and(|parent| parent == &self.session_id)
                {
                    return Err("company_task_session_must_be_fresh");
                }
            }
            TaskExecutionMode::Standalone => {
                if self.project_id.is_some()
                    || self.packet_id.is_some()
                    || self.assignment_ref.is_some()
                    || self.attempt_ref.is_some()
                    || self.parent_session_id.is_some()
                {
                    return Err("standalone_company_binding_forbidden");
                }
            }
        }
        if self.scope_digest != self.canonical_digest() {
            return Err("company_task_scope_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "mode": self.mode,
            "project_id": self.project_id,
            "packet_id": self.packet_id,
            "assignment_ref": self.assignment_ref,
            "attempt_ref": self.attempt_ref,
            "role_id": self.role_id,
            "department_id": self.department_id,
            "session_id": self.session_id,
            "parent_session_id": self.parent_session_id,
            "input_refs": self.input_refs,
            "authorized_retrieval_refs": self.authorized_retrieval_refs,
            "path_allow": self.path_allow,
            "private_history_allowed": self.private_history_allowed,
        }))
    }

    pub fn bind_attempt(mut self, attempt_ref: impl Into<String>) -> Result<Self, &'static str> {
        let attempt_ref = attempt_ref.into();
        required(&attempt_ref, "company_task_attempt_required")?;
        if self.mode != TaskExecutionMode::Company {
            return Err("standalone_attempt_binding_forbidden");
        }
        self.attempt_ref = Some(attempt_ref);
        self.scope_digest = self.canonical_digest();
        self.validate()?;
        Ok(self)
    }

    pub fn fresh_run(
        &self,
        run_id: RunId,
        attempt_ref: impl Into<String>,
        current_session_id: SessionId,
    ) -> Result<FreshTaskRun, &'static str> {
        self.validate()?;
        if self.mode != TaskExecutionMode::Company || current_session_id == self.session_id {
            return Err("company_task_session_must_be_fresh");
        }
        let attempt_ref = attempt_ref.into();
        required(&attempt_ref, "company_task_attempt_required")?;
        FreshTaskRun::new(
            run_id,
            current_session_id,
            Some(self.session_id.clone()),
            self.packet_id
                .clone()
                .ok_or("company_task_packet_required")?,
            attempt_ref,
            self.scope_digest.clone(),
            self.input_refs.clone(),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FreshTaskRun {
    pub schema: String,
    pub run_id: RunId,
    pub session_id: SessionId,
    pub parent_session_id: Option<SessionId>,
    pub packet_id: String,
    pub attempt_ref: String,
    pub scope_digest: String,
    pub input_refs: Vec<String>,
    pub digest: String,
}

impl FreshTaskRun {
    fn new(
        run_id: RunId,
        session_id: SessionId,
        parent_session_id: Option<SessionId>,
        packet_id: String,
        attempt_ref: String,
        scope_digest: String,
        input_refs: Vec<String>,
    ) -> Result<Self, &'static str> {
        let mut run = Self {
            schema: FRESH_TASK_RUN_SCHEMA.to_owned(),
            run_id,
            session_id,
            parent_session_id,
            packet_id,
            attempt_ref,
            scope_digest,
            input_refs,
            digest: String::new(),
        };
        run.digest = run.canonical_digest();
        run.validate()?;
        Ok(run)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != FRESH_TASK_RUN_SCHEMA || self.run_id.as_uuid().is_nil() {
            return Err("fresh_task_run_identity_invalid");
        }
        required(&self.packet_id, "fresh_task_run_packet_required")?;
        required(&self.attempt_ref, "fresh_task_run_attempt_required")?;
        required(&self.scope_digest, "fresh_task_run_scope_required")?;
        if self.session_id.is_empty()
            || self
                .parent_session_id
                .as_ref()
                .is_some_and(|parent| parent == &self.session_id)
        {
            return Err("fresh_task_run_session_invalid");
        }
        if self.input_refs.iter().any(|value| private_ref(value)) {
            return Err("fresh_task_run_private_history_forbidden");
        }
        if self.digest != self.canonical_digest() {
            return Err("fresh_task_run_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "run_id": self.run_id,
            "session_id": self.session_id,
            "parent_session_id": self.parent_session_id,
            "packet_id": self.packet_id,
            "attempt_ref": self.attempt_ref,
            "scope_digest": self.scope_digest,
            "input_refs": self.input_refs,
        }))
    }
}
