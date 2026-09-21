//! Query-side data boundary: revocation and scope decisions are explicit and digest-bound.

use kiana_domain::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const QUERY_DATA_BOUNDARY_SCHEMA: &str = "kiana.query-data-boundary.v1";

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryDataDisposition {
    Allowed,
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryDataBoundary {
    pub schema: String,
    pub project_root: String,
    pub scope_digest: String,
    pub data_epoch: u64,
    pub revoked: bool,
    pub retention_allowed: bool,
    pub memory: QueryDataDisposition,
    pub index: QueryDataDisposition,
    pub cache: QueryDataDisposition,
    pub export: QueryDataDisposition,
    pub decision_digest: String,
}

impl QueryDataBoundary {
    pub fn derive(
        project_root: impl Into<String>,
        scope_digest: impl Into<String>,
        data_epoch: u64,
        revoked: bool,
        retention_allowed: bool,
        export_requested: bool,
    ) -> Result<Self, String> {
        let project_root = project_root.into();
        let scope_digest = scope_digest.into();
        if project_root.trim().is_empty() || data_epoch == 0 {
            return Err("query_data_boundary_header_invalid".to_owned());
        }
        let Some(hex) = scope_digest.strip_prefix("sha256:") else {
            return Err("query_data_boundary_scope_digest_invalid".to_owned());
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("query_data_boundary_scope_digest_invalid".to_owned());
        }
        let allowed = !revoked && retention_allowed;
        let mut boundary = Self {
            schema: QUERY_DATA_BOUNDARY_SCHEMA.to_owned(),
            project_root,
            scope_digest,
            data_epoch,
            revoked,
            retention_allowed,
            memory: if allowed {
                QueryDataDisposition::Allowed
            } else {
                QueryDataDisposition::Denied
            },
            index: if allowed {
                QueryDataDisposition::Allowed
            } else {
                QueryDataDisposition::Denied
            },
            cache: if allowed {
                QueryDataDisposition::Allowed
            } else {
                QueryDataDisposition::Denied
            },
            export: if allowed && export_requested {
                QueryDataDisposition::Allowed
            } else {
                QueryDataDisposition::Denied
            },
            decision_digest: String::new(),
        };
        boundary.decision_digest = boundary.digest();
        boundary.validate()?;
        Ok(boundary)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUERY_DATA_BOUNDARY_SCHEMA
            || self.project_root.trim().is_empty()
            || self.data_epoch == 0
        {
            return Err("query_data_boundary_header_invalid".to_owned());
        }
        let Some(hex) = self.decision_digest.strip_prefix("sha256:") else {
            return Err("query_data_boundary_digest_invalid".to_owned());
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("query_data_boundary_digest_invalid".to_owned());
        }
        if self.revoked || !self.retention_allowed {
            if [self.memory, self.index, self.cache, self.export]
                .iter()
                .any(|value| *value == QueryDataDisposition::Allowed)
            {
                return Err("query_data_boundary_revoked_data_allowed".to_owned());
            }
        }
        if self.decision_digest != self.digest() {
            return Err("query_data_boundary_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "project_root": self.project_root,
            "scope_digest": self.scope_digest,
            "data_epoch": self.data_epoch,
            "revoked": self.revoked,
            "retention_allowed": self.retention_allowed,
            "memory": self.memory,
            "index": self.index,
            "cache": self.cache,
            "export": self.export,
        }))
    }
}
