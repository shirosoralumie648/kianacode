//! AUT-23 five-surface DaemonHost/ControlPlane UAT parity contract.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const AUT23_SURFACE_UAT_SCHEMA: &str = "kiana.aut23-surface-uat.v1";
fn required(value: &str, field: &'static str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 4096 || value.contains(['\0', '\r', '\n']) {
        Err(field.to_owned())
    } else {
        Ok(())
    }
}
fn digest(value: &str, field: &'static str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field.to_owned());
    };
    if hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field.to_owned())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationUatSurface {
    Cli,
    Web,
    Workbench,
    Desktop,
    Mcp,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSurfaceUatCase {
    pub schema: String,
    pub surface: AutomationUatSurface,
    pub command_digest: String,
    pub snapshot_digest: String,
    pub source_cursor: u64,
    pub authority_epoch: u64,
    pub denied: bool,
    pub broker_calls: u32,
    pub handler_calls: u32,
    pub direct_route: bool,
    pub digest: String,
}

impl AutomationSurfaceUatCase {
    fn validate(&self) -> Result<(), String> {
        if self.schema != AUT23_SURFACE_UAT_SCHEMA
            || self.source_cursor == 0
            || self.authority_epoch == 0
            || self.direct_route
            || (self.denied && (self.broker_calls != 0 || self.handler_calls != 0))
        {
            return Err("aut23_surface_route_or_deny_invalid".to_owned());
        }
        digest(&self.command_digest, "aut23_command_digest_invalid")?;
        digest(&self.snapshot_digest, "aut23_snapshot_digest_invalid")?;
        digest(&self.digest, "aut23_case_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("aut23_case_digest_mismatch".to_owned());
        }
        Ok(())
    }
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "surface": self.surface,
            "command_digest": self.command_digest,
            "snapshot_digest": self.snapshot_digest,
            "source_cursor": self.source_cursor,
            "authority_epoch": self.authority_epoch,
            "denied": self.denied,
            "broker_calls": self.broker_calls,
            "handler_calls": self.handler_calls,
            "direct_route": self.direct_route,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSurfaceUat {
    pub schema: String,
    pub command_digest: String,
    pub snapshot_digest: String,
    pub source_cursor: u64,
    pub authority_epoch: u64,
    pub cases: Vec<AutomationSurfaceUatCase>,
    pub digest: String,
}

impl AutomationSurfaceUat {
    pub fn new(cases: Vec<AutomationSurfaceUatCase>) -> Self {
        let first = cases.first();
        let mut value = Self {
            schema: AUT23_SURFACE_UAT_SCHEMA.to_owned(),
            command_digest: first.map(|c| c.command_digest.clone()).unwrap_or_default(),
            snapshot_digest: first.map(|c| c.snapshot_digest.clone()).unwrap_or_default(),
            source_cursor: first.map(|c| c.source_cursor).unwrap_or(0),
            authority_epoch: first.map(|c| c.authority_epoch).unwrap_or(0),
            cases,
            digest: String::new(),
        };
        value.digest = value.canonical_digest();
        value
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUT23_SURFACE_UAT_SCHEMA || self.cases.len() != 5 {
            return Err("aut23_uat_header_invalid".to_owned());
        }
        required(&self.command_digest, "aut23_command_required")?;
        digest(&self.command_digest, "aut23_command_digest_invalid")?;
        digest(&self.snapshot_digest, "aut23_snapshot_digest_invalid")?;
        let mut surfaces = BTreeSet::new();
        for case in &self.cases {
            case.validate()?;
            if case.command_digest != self.command_digest
                || case.snapshot_digest != self.snapshot_digest
                || case.source_cursor != self.source_cursor
                || case.authority_epoch != self.authority_epoch
                || !surfaces.insert(case.surface)
            {
                return Err("aut23_surface_snapshot_parity_invalid".to_owned());
            }
        }
        let expected = [
            AutomationUatSurface::Cli,
            AutomationUatSurface::Web,
            AutomationUatSurface::Workbench,
            AutomationUatSurface::Desktop,
            AutomationUatSurface::Mcp,
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        if surfaces != expected {
            return Err("aut23_surface_set_incomplete".to_owned());
        }
        digest(&self.digest, "aut23_uat_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("aut23_uat_digest_mismatch".to_owned());
        }
        Ok(())
    }
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "command_digest": self.command_digest,
            "snapshot_digest": self.snapshot_digest,
            "source_cursor": self.source_cursor,
            "authority_epoch": self.authority_epoch,
            "cases": self.cases,
        }))
    }
}
