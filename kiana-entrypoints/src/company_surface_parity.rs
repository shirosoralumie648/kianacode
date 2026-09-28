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

/// 一个跨界面共享的 Company 快照帧。
///
/// 【共享帧的价值】
/// CLI、Workbench、Web、Desktop 渲染的是**同一个形状**的帧。
/// 各界面各造一份，就等于给「同一个项目在不同界面显示不同状态」留了位置——
/// 而那种差异往往要到出事那天才被发现。
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

/// 一个「从某个快照帧上发出的动作」，以及它**自认为**基于哪个快照。
///
/// 【⚠ 这个类型是本文件里最要紧的】
/// 它要解决的问题是：用户盯着一个**已经过时**的界面点了「批准」。
///
/// 现实里这件事很常见——页面开着不动，后台已经推进了三步。用户看到的是
/// 「待批准」，而实际上那个待批可能早就被别人批了，或者条件已经变了。
/// 如果照用户所见执行，操作会作用在一个**不是他以为的那个对象**上。
///
/// 【四个字段一起比，缺一不可】
/// - `project_id`：连项目都要对上，防止把 A 项目的动作送到 B 项目；
/// - `snapshot_digest`：内容变了就会被发现，哪怕 revision 计数器没动；
/// - `revision`：单调递增的版本号，能发现「变了又变回来」之外的情况；
/// - `authority_epoch`：**授权世代**。这一项最容易被忽略——快照内容可能完全没变，
///   但控制面已经换了一个 epoch（重启、换实例、发生重新授权）。
///   只比 digest 会漏掉这种情况。
///
/// 【为什么只给一个错误码】
/// 四种不匹配对调用方的**修法**是一样的：刷新快照、重新看一眼、再决定一次。
/// 分成四个码不会让修法变多，只会让日志更难读。
///
/// 【为什么不「刷新一下再执行」】
/// 自动刷新等于替用户做决定：他点的是**旧状态**下的那个动作。
/// 正确做法是拒绝，让他看到新状态再决定。
///
/// 【⚠ 它在提交之前】
/// 文件头那句「A stale frame is rejected by digest/revision/epoch before any command can
/// be submitted」就是这个意思：先拒，再谈执行。顺序反过来就变成
/// 「先执行，然后发现刚才那个依据已经不成立了」。
impl StaleSurfaceAction {
    pub fn validate_against(&self, snapshot: &CompanyReadModelSnapshot) -> Result<(), String> {
        if self.project_id != snapshot.project_id
            || self.snapshot_digest != snapshot.snapshot_digest
            || self.revision != snapshot.revision
            || self.authority_epoch != snapshot.authority_epoch
            || self.action.trim().is_empty()
        // 上面任何一个字段对不上，都落到这里。
        //
        // 合并成一个码是有意的：四种不匹配对调用方的修法完全一样
        // （刷新快照、重新看一眼、再决定一次），分成四个码不会让修法变多，
        // 只会让日志更难读——而日志是这件事唯一的线索。
        //
        // ⚠ 这里没有「部分放行」。四个字段是一个整体：允许其中三个对上、
        // 第四个不对就放行，等于承认了「基于旧授权的操作」这件事。
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
