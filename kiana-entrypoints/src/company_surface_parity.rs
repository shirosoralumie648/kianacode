//! Shared Company snapshot/action parity for CLI, Workbench, Web and Desktop.
//!
//! Surfaces render this frame and send typed actions back to the existing DaemonHost. A stale
//! frame is rejected by digest/revision/epoch before any command can be submitted.

use kiana_domain::{CompanyReadModelSnapshot, ProjectStatus};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const COMPANY_SURFACE_PARITY_SCHEMA: &str = "kiana.company-surface-parity.v1";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanySurface {
    Cli,
    Workbench,
    Web,
    Desktop,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanySurfaceFrame {
    pub schema: String,
    pub surface: CompanySurface,
    pub project_id: String,
    pub snapshot_digest: String,
    pub source_cursor: u64,
    pub projection_cursor: Option<u64>,
    pub revision: u64,
    pub authority_epoch: u64,
    pub project_status: ProjectStatus,
    pub next_action: String,
    pub digest: String,
}

impl CompanySurfaceFrame {
    pub fn from_snapshot(
        surface: CompanySurface,
        snapshot: &CompanyReadModelSnapshot,
        next_action: impl Into<String>,
    ) -> Result<Self, String> {
        snapshot.validate().map_err(|error| error.to_string())?;
        let mut frame = Self {
            schema: COMPANY_SURFACE_PARITY_SCHEMA.to_owned(),
            surface,
            project_id: snapshot.project_id.clone(),
            snapshot_digest: snapshot.snapshot_digest.clone(),
            source_cursor: snapshot.source_cursor,
            projection_cursor: snapshot.projection_cursor,
            revision: snapshot.revision,
            authority_epoch: snapshot.authority_epoch,
            project_status: snapshot.view.project_status,
            next_action: next_action.into(),
            digest: String::new(),
        };
        frame.digest = frame.canonical_digest();
        frame.validate_against(snapshot)?;
        Ok(frame)
    }

    pub fn validate_against(&self, snapshot: &CompanyReadModelSnapshot) -> Result<(), String> {
        snapshot.validate().map_err(|error| error.to_string())?;
        if self.schema != COMPANY_SURFACE_PARITY_SCHEMA
            || self.project_id != snapshot.project_id
            || self.snapshot_digest != snapshot.snapshot_digest
            || self.source_cursor != snapshot.source_cursor
            || self.projection_cursor != snapshot.projection_cursor
            || self.revision != snapshot.revision
            || self.authority_epoch != snapshot.authority_epoch
            || self.project_status != snapshot.view.project_status
            || self.next_action.trim().is_empty()
        {
            return Err("company_surface_frame_binding_invalid".to_owned());
        }
        if self.digest != self.canonical_digest() {
            return Err("company_surface_frame_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        kiana_domain::json_digest(&json!({
            "schema": self.schema,
            "surface": self.surface,
            "project_id": self.project_id,
            "snapshot_digest": self.snapshot_digest,
            "source_cursor": self.source_cursor,
            "projection_cursor": self.projection_cursor,
            "revision": self.revision,
            "authority_epoch": self.authority_epoch,
            "project_status": self.project_status,
            "next_action": self.next_action,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StaleSurfaceAction {
    pub project_id: String,
    pub snapshot_digest: String,
    pub revision: u64,
    pub authority_epoch: u64,
    pub action: String,
}

impl StaleSurfaceAction {
    pub fn validate_against(&self, snapshot: &CompanyReadModelSnapshot) -> Result<(), String> {
        if self.project_id != snapshot.project_id
            || self.snapshot_digest != snapshot.snapshot_digest
            || self.revision != snapshot.revision
            || self.authority_epoch != snapshot.authority_epoch
            || self.action.trim().is_empty()
        {
            return Err("company_surface_action_stale".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanySurfaceParity {
    pub schema: String,
    pub project_id: String,
    pub snapshot_digest: String,
    pub frames: Vec<CompanySurfaceFrame>,
    pub parity_digest: String,
}

impl CompanySurfaceParity {
    pub fn from_snapshot(
        snapshot: &CompanyReadModelSnapshot,
        frames: Vec<CompanySurfaceFrame>,
    ) -> Result<Self, String> {
        snapshot.validate().map_err(|error| error.to_string())?;
        let mut parity = Self {
            schema: COMPANY_SURFACE_PARITY_SCHEMA.to_owned(),
            project_id: snapshot.project_id.clone(),
            snapshot_digest: snapshot.snapshot_digest.clone(),
            frames,
            parity_digest: String::new(),
        };
        parity.parity_digest = parity.canonical_digest();
        parity.validate_against(snapshot)?;
        Ok(parity)
    }

    pub fn validate_against(&self, snapshot: &CompanyReadModelSnapshot) -> Result<(), String> {
        snapshot.validate().map_err(|error| error.to_string())?;
        if self.schema != COMPANY_SURFACE_PARITY_SCHEMA
            || self.project_id != snapshot.project_id
            || self.snapshot_digest != snapshot.snapshot_digest
            || self.frames.len() != 4
        {
            return Err("company_surface_parity_header_invalid".to_owned());
        }
        let mut surfaces = BTreeSet::new();
        for frame in &self.frames {
            frame.validate_against(snapshot)?;
            if !surfaces.insert(frame.surface) {
                return Err("company_surface_parity_duplicate_surface".to_owned());
            }
        }
        if surfaces.len() != 4 {
            return Err("company_surface_parity_surface_missing".to_owned());
        }
        if self.parity_digest != self.canonical_digest() {
            return Err("company_surface_parity_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        kiana_domain::json_digest(&json!({
            "schema": self.schema,
            "project_id": self.project_id,
            "snapshot_digest": self.snapshot_digest,
            "frames": self.frames,
        }))
    }
}
