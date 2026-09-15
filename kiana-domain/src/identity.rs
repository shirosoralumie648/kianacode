//! Server-owned identity and project scope snapshots.
//!
//! These values are immutable inputs to ControlPlane decisions. They deliberately contain only
//! opaque credential references and filesystem identity metadata; a wire actor/role/trust flag is
//! never an authenticated principal.

use crate::{canonical_journal_bytes, json_digest, ProjectId, SchemaVersion, SessionId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const AUTHENTICATED_PRINCIPAL_SCHEMA: &str = "kiana.authenticated-principal.v1";
pub const PROJECT_IDENTITY_SCHEMA: &str = "kiana.project-identity.v1";
pub const SESSION_ASSIGNMENT_SCHEMA: &str = "kiana.session-assignment.v1";
pub const IDENTITY_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
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

fn clean_digest(value: &str) -> String {
    if value.starts_with("sha256:") {
        value.to_owned()
    } else {
        json_digest(&serde_json::json!(value))
    }
}

fn stable_project_id(canonical_root: &str) -> ProjectId {
    let digest = Sha256::digest(canonical_root.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    // Preserve UUID variant/version bits while keeping the value deterministic per root.
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    ProjectId::from_uuid(Uuid::from_bytes(bytes))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthenticatedPrincipalRef {
    pub schema: String,
    pub version: SchemaVersion,
    pub principal_id: String,
    pub authentication_method: String,
    pub issuer: String,
    pub credential_generation: u64,
    pub expires_at_unix_ms: u64,
    pub principal_digest: String,
}

impl AuthenticatedPrincipalRef {
    pub fn local() -> Self {
        let mut principal = Self {
            schema: AUTHENTICATED_PRINCIPAL_SCHEMA.to_owned(),
            version: IDENTITY_SCHEMA_VERSION,
            principal_id: "local-user".to_owned(),
            authentication_method: "local_os".to_owned(),
            issuer: "kiana-local".to_owned(),
            credential_generation: 1,
            expires_at_unix_ms: u64::MAX,
            principal_digest: String::new(),
        };
        principal.principal_digest = principal.digest();
        principal
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUTHENTICATED_PRINCIPAL_SCHEMA
            || !self.version.is_compatible_with(&IDENTITY_SCHEMA_VERSION)
            || self.credential_generation == 0
            || self.expires_at_unix_ms == 0
        {
            return Err("authenticated_principal_header_invalid".to_owned());
        }
        required(&self.principal_id, "principal_id", 256)?;
        required(&self.authentication_method, "authentication_method", 64)?;
        required(&self.issuer, "principal_issuer", 256)?;
        digest(&self.principal_digest, "principal_digest")?;
        if self.principal_digest != self.digest() {
            return Err("principal_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "principal_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectIdentity {
    pub schema: String,
    pub version: SchemaVersion,
    pub project_id: ProjectId,
    pub root: String,
    pub canonical_root: String,
    #[serde(default)]
    pub device: Option<u64>,
    #[serde(default)]
    pub inode: Option<u64>,
    pub trust_revision: String,
    pub identity_digest: String,
}

impl ProjectIdentity {
    pub fn new(
        root: impl Into<String>,
        canonical_root: impl Into<String>,
        device: Option<u64>,
        inode: Option<u64>,
        trust_revision: impl Into<String>,
    ) -> Result<Self, String> {
        let canonical_root = canonical_root.into();
        let project_id = stable_project_id(&canonical_root);
        let mut identity = Self {
            schema: PROJECT_IDENTITY_SCHEMA.to_owned(),
            version: IDENTITY_SCHEMA_VERSION,
            project_id,
            root: root.into(),
            canonical_root,
            device,
            inode,
            trust_revision: clean_digest(&trust_revision.into()),
            identity_digest: String::new(),
        };
        identity.identity_digest = identity.digest();
        identity.validate()?;
        Ok(identity)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROJECT_IDENTITY_SCHEMA
            || !self.version.is_compatible_with(&IDENTITY_SCHEMA_VERSION)
        {
            return Err("project_identity_header_invalid".to_owned());
        }
        required(&self.root, "project_root", 4_096)?;
        required(&self.canonical_root, "project_canonical_root", 4_096)?;
        required(&self.trust_revision, "project_trust_revision", 256)?;
        digest(&self.trust_revision, "project_trust_revision")?;
        digest(&self.identity_digest, "project_identity_digest")?;
        if self.identity_digest != self.digest() {
            return Err("project_identity_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            // The display `root` may be an alias; identity is keyed by canonical_root.
            object.insert(
                "root".to_owned(),
                serde_json::Value::String(self.canonical_root.clone()),
            );
            object.insert(
                "identity_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionAssignment {
    pub schema: String,
    pub version: SchemaVersion,
    pub principal: AuthenticatedPrincipalRef,
    pub project: ProjectIdentity,
    pub session_id: SessionId,
    pub role_id: String,
    pub department_id: String,
    pub assignment_revision: u64,
    pub authority_epoch: u64,
    pub assignment_digest: String,
}

impl SessionAssignment {
    pub fn new(
        principal: AuthenticatedPrincipalRef,
        project: ProjectIdentity,
        session_id: impl Into<String>,
        role_id: impl Into<String>,
        department_id: impl Into<String>,
        assignment_revision: u64,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        let mut assignment = Self {
            schema: SESSION_ASSIGNMENT_SCHEMA.to_owned(),
            version: IDENTITY_SCHEMA_VERSION,
            principal,
            project,
            session_id: SessionId::new(session_id),
            role_id: role_id.into(),
            department_id: department_id.into(),
            assignment_revision,
            authority_epoch,
            assignment_digest: String::new(),
        };
        assignment.assignment_digest = assignment.digest();
        assignment.validate()?;
        Ok(assignment)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SESSION_ASSIGNMENT_SCHEMA
            || !self.version.is_compatible_with(&IDENTITY_SCHEMA_VERSION)
            || self.assignment_revision == 0
            || self.authority_epoch == 0
            || self.session_id.is_empty()
        {
            return Err("session_assignment_header_invalid".to_owned());
        }
        self.principal.validate()?;
        self.project.validate()?;
        required(&self.role_id, "assignment_role", 128)?;
        required(&self.department_id, "assignment_department", 128)?;
        digest(&self.assignment_digest, "assignment_digest")?;
        if self.assignment_digest != self.digest() {
            return Err("session_assignment_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "assignment_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}
