//! Server-owned identity for a long-running supervised process.
//!
//! A PID is only an implementation detail. Continuation operations must prove the originating
//! invocation, Run/Turn, owner, project and authority snapshot before stdin, poll or stop.

use crate::{json_digest, InvocationId, RequestId, RunId, SchemaVersion, SessionId, TurnId};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const JOB_HANDLE_SCHEMA: &str = "kiana.job-handle.v1";
pub const JOB_HANDLE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        Err(format!("{field}_invalid"))
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// Immutable continuation identity for one process start.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobHandle {
    pub schema: String,
    pub version: SchemaVersion,
    pub job_id: RequestId,
    pub start_request_id: RequestId,
    pub start_invocation_id: InvocationId,
    pub run_id: Option<RunId>,
    pub turn_id: Option<TurnId>,
    pub owner_id: String,
    pub session_id: SessionId,
    pub project_digest: String,
    pub authority_epoch: u64,
    /// The process-group ID is evidence, never authority. Missing evidence prevents continuation.
    pub process_group_id: Option<u32>,
    pub expires_at_unix_ms: u64,
    pub handle_digest: String,
}

impl JobHandle {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        job_id: RequestId,
        start_request_id: RequestId,
        run_id: Option<RunId>,
        turn_id: Option<TurnId>,
        owner_id: impl Into<String>,
        session_id: SessionId,
        project_digest: impl Into<String>,
        authority_epoch: u64,
        process_group_id: Option<u32>,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut handle = Self {
            schema: JOB_HANDLE_SCHEMA.to_owned(),
            version: JOB_HANDLE_VERSION,
            job_id,
            start_request_id,
            start_invocation_id: InvocationId::from_uuid(start_request_id.as_uuid()),
            run_id,
            turn_id,
            owner_id: owner_id.into(),
            session_id,
            project_digest: project_digest.into(),
            authority_epoch,
            process_group_id,
            expires_at_unix_ms,
            handle_digest: String::new(),
        };
        handle.handle_digest = handle.digest();
        handle.validate()?;
        Ok(handle)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != JOB_HANDLE_SCHEMA
            || self.version != JOB_HANDLE_VERSION
            || self.job_id.as_uuid().is_nil()
            || self.start_request_id.as_uuid().is_nil()
            || self.start_invocation_id != InvocationId::from_uuid(self.start_request_id.as_uuid())
            || self.authority_epoch == 0
            || self.process_group_id.is_none_or(|pid| pid == 0)
            || self.expires_at_unix_ms == 0
            || self.session_id.is_empty()
        {
            return Err("job_handle_header_invalid".to_owned());
        }
        required(&self.owner_id, "job_handle_owner", 256)?;
        digest(&self.project_digest, "job_handle_project_digest")?;
        digest(&self.handle_digest, "job_handle_digest")?;
        if self.handle_digest != self.digest() {
            return Err("job_handle_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "job_id": self.job_id,
            "start_request_id": self.start_request_id,
            "start_invocation_id": self.start_invocation_id,
            "run_id": self.run_id,
            "turn_id": self.turn_id,
            "owner_id": self.owner_id,
            "session_id": self.session_id,
            "project_digest": self.project_digest,
            "authority_epoch": self.authority_epoch,
            "process_group_id": self.process_group_id,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}
