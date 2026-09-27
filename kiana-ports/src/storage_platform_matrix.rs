//! PD-32 platform and filesystem capability matrix.
//!
//! Linux ext4, Linux tmpfs, a Windows or macOS fallback, a network filesystem, and a clock or
//! encoding that does not behave — the card names them as a matrix, and asks for the *capability
//! decisions* plus a per-platform pre-install check.
//!
//! This module is those decisions as data. It does not `statfs` a mount, open a directory, take an
//! `flock`, call `rename`, read a wall clock, or ask an OS which platform it is. A cell is a
//! **declaration** of what a `(platform, filesystem)` pair supports, and the contract refuses any
//! cell that lets an unsupported semantic be treated as an equivalent durable one.
//!
//! ## Honesty boundary
//!
//! The dispositions here are stated as decisions, not as observations. The Linux rows are written
//! to match what this checkout's adapters actually claim, and the ext4/local cells are the only
//! ones an in-process run can say anything about; `tmpfs`, `network_fs`, `windows` and `macos` are
//! recorded as *declared* dispositions that no test in this slice verified. A fixture asserts the
//! **disposition**, never an observed filesystem behaviour, because asserting an unobserved
//! behaviour is how a matrix turns into fiction.
//!
//! ## Vocabulary is reused, not reinvented
//!
//! * [`StorageSemantic`] is the one list of durability-affecting primitives. It deliberately
//!   covers exactly the three the card names — `rename`, `fsync`, `lock` — plus the two the
//!   existing adapters already branch on.
//! * [`StorageSecurityCapabilities`] (PD-28) is the authority for which *guards* a platform can
//!   actually enforce; a cell may not claim a guard its own platform record disallows.
//! * [`StorageSecurityCapabilities::platform`] is read as the *declared* platform string, so a
//!   cell is bound to a platform record rather than to `std::env::consts`.
//! * [`PersistenceCapacityProofLevel`] and PD-31's [`ProofCeiling`] keep the durable/local_behavior
//!   boundary a single ladder rather than a second scale.
//!
//! ## What an unsupported cell does
//!
//! An `Unsupported` cell is **refused, not degraded**. [`StoragePlatformCell::admits`] is
//! `false` for it, [`StoragePlatformMatrix::install_preflight`] names it, and
//! [`StoragePlatformMatrix::negotiated_capabilities`] drops the semantic rather than returning a
//! weaker one. Silently substituting "best effort" is exactly the drift the card rejects.

use crate::adapter_conformance::ProofCeiling;
use kiana_domain::{
    json_digest, redact_text, scan_secret_sentinels, SchemaVersion, SecretScanChannel,
    StorageCapabilities, StorageSecurityCapabilities,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const STORAGE_PLATFORM_MATRIX_SCHEMA: &str = "kiana.pd32-storage-platform-matrix.v1";
pub const STORAGE_PLATFORM_CELL_SCHEMA: &str = "kiana.pd32-storage-platform-cell.v1";
pub const STORAGE_PLATFORM_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// One cell per [`StoragePlatformTarget::ALL`] x [`StorageFilesystemClass::ALL`] pair.
pub const MAX_STORAGE_PLATFORM_CELLS: usize = 8;
pub const MAX_STORAGE_PLATFORM_NOTES: usize = 8;
pub const MAX_STORAGE_PLATFORM_TEXT: usize = 256;

/// The platforms the card names. `OtherUnix` is the fail-closed catch-all: an unrecognised Unix
/// gets a cell that refuses every semantic rather than inheriting the Linux row.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoragePlatformTarget {
    Linux,
    /// Windows: no `flock`, and the atomic-replace story differs. Declared fallback only.
    Windows,
    /// macOS: `rename` is atomic but `fsync` does not flush the directory entry the same way.
    Macos,
    /// Anything Unix that is neither Linux nor macOS. Never a supported row.
    OtherUnix,
}

impl StoragePlatformTarget {
    // Multi-line on purpose: `rustfmt` collapses a short array onto one line, which would make the
    // PD-32 source guard's per-variant `assert_exact_once` on this block ambiguous with the
    // identically-shaped `StorageFilesystemClass::ALL` below.
    pub const ALL: [Self; 4] = [Self::Linux, Self::Windows, Self::Macos, Self::OtherUnix];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::Macos => "macos",
            Self::OtherUnix => "other_unix",
        }
    }

    /// The string PD-28's `StorageSecurityCapabilities::platform` uses for this target, so a cell
    /// and a platform record are compared on one vocabulary instead of two.
    pub const fn security_platform(self) -> &'static str {
        match self {
            Self::Linux | Self::OtherUnix => "linux",
            Self::Windows => "non_unix",
            Self::Macos => "unix",
        }
    }
}

/// The filesystems the card names. `Unknown` refuses everything: a filesystem nobody identified
/// cannot be credited with any semantic.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageFilesystemClass {
    /// Local journalled filesystem with `rename` replacement, `fsync` and advisory `flock`.
    Ext4,
    /// In-memory filesystem: writes are not durable across a restart by construction.
    Tmpfs,
    /// A filesystem reached over the network. Rename and fsync semantics are server-dependent.
    NetworkFs,
    /// The filesystem could not be identified.
    Unknown,
}

impl StorageFilesystemClass {
    // Multi-line for the same reason as `StoragePlatformTarget::ALL` above: the PD-32 source guard
    // pins each variant to its own block, and a collapsed one-line array would not be
    // distinguishable from the target list.
    pub const ALL: [Self; 4] = [Self::Ext4, Self::Tmpfs, Self::NetworkFs, Self::Unknown];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ext4 => "ext4",
            Self::Tmpfs => "tmpfs",
            Self::NetworkFs => "network_fs",
            Self::Unknown => "unknown",
        }
    }

    /// Whether an unsupported semantic on this filesystem is a refusal or a documented
    /// degradation. `ext4` is the only class where all three semantics are available; `tmpfs`
    /// provides the rename and the lock but loses durability with the process; a network
    /// filesystem cannot be assumed to provide any of the three.
    pub const fn is_local_journalled(self) -> bool {
        matches!(self, Self::Ext4)
    }
}

/// The three durability-affecting primitives the card names, plus the two the existing adapters
/// already branch on. A semantic that is not `Supported` in a cell is never substituted with a
/// weaker one: it is dropped from the negotiation and the cell is refused.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageSemantic {
    /// Atomic replacement of a published file.
    Rename,
    /// Flush of file contents to stable storage.
    Fsync,
    /// Exclusive advisory lock over the journal.
    AdvisoryLock,
    /// Permission-bit inspection, which PD-28's `permission_guard` already gates.
    PermissionGuard,
    /// Symlink/hardlink inspection, which PD-28's guards already gate.
    LinkGuard,
}

impl StorageSemantic {
    pub const ALL: [Self; 5] = [
        Self::Rename,
        Self::Fsync,
        Self::AdvisoryLock,
        Self::PermissionGuard,
        Self::LinkGuard,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rename => "rename",
            Self::Fsync => "fsync",
            Self::AdvisoryLock => "advisory_lock",
            Self::PermissionGuard => "permission_guard",
            Self::LinkGuard => "link_guard",
        }
    }

    /// The PD-28 `StorageSecurityCapabilities` guard this semantic depends on, if any.
    ///
    /// `AdvisoryLock` has no guard record in `kiana-domain`: nothing reports whether a platform's
    /// lock is advisory or mandatory, so a cell's own disposition is the only statement about it.
    /// `Fsync` is gated by `StorageCapabilities`' own `fsync` bit instead, which is checked
    /// separately in `validate_against_capabilities`.
    pub const fn security_capability(self) -> Option<SecurityCapability> {
        match self {
            Self::Rename => Some(SecurityCapability::AtomicReplace),
            Self::Fsync => None,
            Self::AdvisoryLock => None,
            Self::PermissionGuard => Some(SecurityCapability::PermissionGuard),
            Self::LinkGuard => Some(SecurityCapability::LinkGuard),
        }
    }

    /// Whether the storage-health `StorageCapabilities` record's own `fsync` bit backs this
    /// semantic. `StorageCapabilities` already refuses `durable_commits` or `atomic_transitions`
    /// without it, so reusing the bit keeps one fsync claim in the workspace.
    pub const fn uses_storage_fsync(self) -> bool {
        matches!(self, Self::Fsync)
    }
}

/// The guards a [`StorageSecurityCapabilities`] record reports. Reused rather than restated so a
/// cell cannot claim a guard the PD-28 record disallows.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityCapability {
    AtomicReplace,
    PermissionGuard,
    LinkGuard,
}

impl SecurityCapability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AtomicReplace => "atomic_replace",
            Self::PermissionGuard => "permission_guard",
            Self::LinkGuard => "link_guard",
        }
    }

    pub fn reported(self, capabilities: &StorageSecurityCapabilities) -> bool {
        match self {
            Self::AtomicReplace => capabilities.atomic_replace,
            Self::PermissionGuard => capabilities.permission_guard,
            Self::LinkGuard => capabilities.symlink_guard && capabilities.hardlink_guard,
        }
    }
}

/// What a `(platform, filesystem)` cell does with a primitive the card names.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoragePlatformDisposition {
    /// The primitive behaves as the storage contract requires and may be relied on.
    Supported,
    /// The primitive behaves differently and the difference is recorded. It may not be treated as
    /// an equivalent durable guarantee: the cell is refused for the durable claim either way.
    Degraded,
    /// The primitive does not exist here. Refused, never substituted.
    Unsupported,
}

impl StoragePlatformDisposition {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Degraded => "degraded",
            Self::Unsupported => "unsupported",
        }
    }

    /// Only `Supported` counts for a durable claim. `Degraded` is not a weaker `Supported`.
    pub const fn admits_durable_claim(self) -> bool {
        matches!(self, Self::Supported)
    }
}

/// The wall-clock and text-encoding shape a cell is assumed to have.
///
/// A clock that moves backwards must not silently extend a TTL, a lease or a retention watermark,
/// so a cell whose clock is not monotonic is degraded on the clock dimension even when its
/// filesystem semantics are all supported.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageClockShape {
    /// Wall clock is expected to move forwards. PD-27 leases may use it.
    MonotonicWall,
    /// The wall clock can be set backwards (NTP step, manual change, VM snapshot restore).
    AdjustableWall,
    /// No usable wall clock on this platform.
    AbsentWall,
}

impl StorageClockShape {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MonotonicWall => "monotonic_wall",
            Self::AdjustableWall => "adjustable_wall",
            Self::AbsentWall => "absent_wall",
        }
    }

    /// A TTL, lease or retention deadline may only be evaluated against a monotonic wall clock.
    pub const fn admits_expiry(self) -> bool {
        matches!(self, Self::MonotonicWall)
    }
}

/// The text encoding a path or payload is written in. Non-UTF-8 is refused rather than
/// transliterated, because a transliterated path is a different path.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageEncoding {
    Utf8,
    /// The platform has a native encoding and a non-UTF-8 byte sequence must be refused.
    NonUtf8,
}

impl StorageEncoding {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Utf8 => "utf8",
            Self::NonUtf8 => "non_utf8",
        }
    }
}

/// One `(platform, filesystem)` cell with an explicit disposition per primitive.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoragePlatformCell {
    pub schema: String,
    pub version: SchemaVersion,
    pub target: StoragePlatformTarget,
    pub filesystem: StorageFilesystemClass,
    pub rename: StoragePlatformDisposition,
    pub fsync: StoragePlatformDisposition,
    pub advisory_lock: StoragePlatformDisposition,
    pub permission_guard: StoragePlatformDisposition,
    pub link_guard: StoragePlatformDisposition,
    pub clock: StorageClockShape,
    pub encoding: StorageEncoding,
    /// The strongest claim a cell may carry. A `Degraded` filesystem may not reach `Durable`.
    pub proof_ceiling: ProofCeiling,
    /// Per-platform limitations. Required for anything below `Supported` on all five primitives.
    pub limitations: Vec<String>,
    pub cell_digest: String,
}

impl StoragePlatformCell {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        target: StoragePlatformTarget,
        filesystem: StorageFilesystemClass,
        rename: StoragePlatformDisposition,
        fsync: StoragePlatformDisposition,
        advisory_lock: StoragePlatformDisposition,
        permission_guard: StoragePlatformDisposition,
        link_guard: StoragePlatformDisposition,
        clock: StorageClockShape,
        encoding: StorageEncoding,
        proof_ceiling: ProofCeiling,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: STORAGE_PLATFORM_CELL_SCHEMA.to_owned(),
            version: STORAGE_PLATFORM_VERSION,
            target,
            filesystem,
            rename,
            fsync,
            advisory_lock,
            permission_guard,
            link_guard,
            clock,
            encoding,
            proof_ceiling,
            limitations,
            cell_digest: String::new(),
        };
        value.cell_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn dispositions(&self) -> BTreeSet<(&'static str, StoragePlatformDisposition)> {
        let mut set = BTreeSet::new();
        set.insert((StorageSemantic::Rename.as_str(), self.rename));
        set.insert((StorageSemantic::Fsync.as_str(), self.fsync));
        set.insert((StorageSemantic::AdvisoryLock.as_str(), self.advisory_lock));
        set.insert((
            StorageSemantic::PermissionGuard.as_str(),
            self.permission_guard,
        ));
        set.insert((StorageSemantic::LinkGuard.as_str(), self.link_guard));
        set
    }

    /// The disposition for one primitive. An `Unsupported` primitive is refused, which is what
    /// keeps a weaker semantic from being substituted for a stronger one.
    pub fn admits(&self, semantic: StorageSemantic) -> bool {
        self.disposition(semantic) == StoragePlatformDisposition::Supported
    }

    pub fn disposition(&self, semantic: StorageSemantic) -> StoragePlatformDisposition {
        match semantic {
            StorageSemantic::Rename => self.rename,
            StorageSemantic::Fsync => self.fsync,
            StorageSemantic::AdvisoryLock => self.advisory_lock,
            StorageSemantic::PermissionGuard => self.permission_guard,
            StorageSemantic::LinkGuard => self.link_guard,
        }
    }

    /// The first primitive this cell does not support, in the fixed order of
    /// [`StorageSemantic::ALL`]. An empty result means the cell supports all five.
    pub fn first_unsupported(&self) -> Option<StorageSemantic> {
        StorageSemantic::ALL
            .into_iter()
            .find(|semantic| !self.admits(*semantic))
    }

    /// A cell that cannot support every primitive is not a usable root, and one whose clock shape
    /// cannot evaluate an expiry is not usable for a leased or retained store.
    pub fn usable_root(&self) -> bool {
        self.first_unsupported().is_none() && self.clock.admits_expiry()
    }

    /// Whether a wall-clock deadline (TTL, lease, retention watermark) may be evaluated here.
    /// Spelled out as its own method so the expiry rule is not re-derived at each call site.
    pub fn clock_can_expire(&self) -> bool {
        self.clock.admits_expiry()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_PLATFORM_CELL_SCHEMA
            || !self.version.is_compatible_with(&STORAGE_PLATFORM_VERSION)
        {
            return Err("storage_platform_cell_header_invalid".to_owned());
        }
        for (name, disposition) in self.dispositions() {
            if disposition == StoragePlatformDisposition::Unsupported && self.usable_root() {
                return Err("storage_platform_unsupported_with_usable_root".to_owned());
            }
            if disposition == StoragePlatformDisposition::Degraded
                && self.proof_ceiling > ProofCeiling::Source
                && self.all_supported()
            {
                return Err(format!("storage_platform_degraded_ceiling:{name}"));
            }
        }
        // A degraded primitive may never be presented as a durable claim. Only a cell that is
        // `Supported` on all five, with a monotonic clock, may reach `LocalBehavior`.
        if self.proof_ceiling == ProofCeiling::LocalBehavior
            && !(self.all_supported() && self.clock.admits_expiry())
        {
            return Err("storage_platform_ceiling_outranks_dispositions".to_owned());
        }
        if self.proof_ceiling > ProofCeiling::LocalBehavior {
            return Err("storage_platform_ceiling_not_negotiable".to_owned());
        }
        if self.encoding == StorageEncoding::NonUtf8 && self.usable_root() {
            return Err("storage_platform_non_utf8_usable_root".to_owned());
        }
        // An unknown filesystem cannot be credited with anything, whatever the platform says.
        if self.filesystem == StorageFilesystemClass::Unknown
            && self.rename != StoragePlatformDisposition::Unsupported
        {
            return Err("storage_platform_unknown_filesystem_supported".to_owned());
        }
        if self.target == StoragePlatformTarget::OtherUnix
            && self.proof_ceiling != ProofCeiling::Source
        {
            return Err("storage_platform_other_unix_ceiling".to_owned());
        }
        if !self.all_supported() && self.limitations.is_empty() {
            return Err("storage_platform_limitation_required".to_owned());
        }
        if self.limitations.len() > MAX_STORAGE_PLATFORM_NOTES {
            return Err("storage_platform_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            safe_text(limitation, "storage_platform_limitation")?;
        }
        validate_digest(&self.cell_digest, "storage_platform_cell_digest")?;
        if self.cell_digest != self.digest() {
            return Err("storage_platform_cell_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn all_supported(&self) -> bool {
        self.dispositions()
            .iter()
            .all(|(_, disposition)| disposition.admits_durable_claim())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "target": self.target,
            "filesystem": self.filesystem,
            "rename": self.rename,
            "fsync": self.fsync,
            "advisory_lock": self.advisory_lock,
            "permission_guard": self.permission_guard,
            "link_guard": self.link_guard,
            "clock": self.clock,
            "encoding": self.encoding,
            "proof_ceiling": self.proof_ceiling,
            "limitations": self.limitations,
        }))
    }
}

/// The matrix: one cell per named `(platform, filesystem)` pair, plus the negotiation it authorises.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoragePlatformMatrix {
    pub schema: String,
    pub version: SchemaVersion,
    pub baseline_digest: String,
    pub cells: Vec<StoragePlatformCell>,
    pub report_digest: String,
}

impl StoragePlatformMatrix {
    pub fn new(
        baseline_digest: impl Into<String>,
        cells: Vec<StoragePlatformCell>,
    ) -> Result<Self, String> {
        let mut matrix = Self {
            schema: STORAGE_PLATFORM_MATRIX_SCHEMA.to_owned(),
            version: STORAGE_PLATFORM_VERSION,
            baseline_digest: baseline_digest.into(),
            cells,
            report_digest: String::new(),
        };
        matrix.report_digest = matrix.digest();
        matrix.validate()?;
        Ok(matrix)
    }

    pub fn cell(
        &self,
        target: StoragePlatformTarget,
        filesystem: StorageFilesystemClass,
    ) -> Option<&StoragePlatformCell> {
        self.cells
            .iter()
            .find(|cell| cell.target == target && cell.filesystem == filesystem)
    }

    /// The platform and filesystem pairs the card names, in the matrix's own order.
    pub fn scope(&self) -> BTreeSet<(&'static str, &'static str)> {
        self.cells
            .iter()
            .map(|cell| (cell.target.as_str(), cell.filesystem.as_str()))
            .collect()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_PLATFORM_MATRIX_SCHEMA
            || !self.version.is_compatible_with(&STORAGE_PLATFORM_VERSION)
            || self.cells.is_empty()
            || self.cells.len() > MAX_STORAGE_PLATFORM_CELLS
        {
            return Err("storage_platform_matrix_header_invalid".to_owned());
        }
        validate_digest(
            &self.baseline_digest,
            "storage_platform_matrix_baseline_digest",
        )?;
        validate_digest(&self.report_digest, "storage_platform_matrix_report_digest")?;
        let mut seen = BTreeSet::new();
        for cell in &self.cells {
            cell.validate()?;
            if !seen.insert((cell.target, cell.filesystem)) {
                return Err("storage_platform_cell_duplicate".to_owned());
            }
        }
        // Every platform the card names must have a row, or "unsupported" is invisible rather than
        // decided. The filesystem column may vary; the platform column may not.
        let targets: BTreeSet<StoragePlatformTarget> =
            self.cells.iter().map(|cell| cell.target).collect();
        for target in StoragePlatformTarget::ALL {
            if !targets.contains(&target) {
                return Err(format!(
                    "storage_platform_target_missing:{}",
                    target.as_str()
                ));
            }
        }
        if self.report_digest != self.digest() {
            return Err("storage_platform_matrix_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// The pre-install check the card asks for: a root may only be placed on a cell that supports
    /// every primitive and has a monotonic wall clock. Everything else is refused with the cell
    /// named, so the refusal is actionable rather than a generic "unsupported platform".
    pub fn install_preflight(
        &self,
        target: StoragePlatformTarget,
        filesystem: StorageFilesystemClass,
    ) -> Result<ProofCeiling, String> {
        let cell = self
            .cell(target, filesystem)
            .ok_or_else(|| format!("storage_platform_target_missing:{}", target.as_str()))?;
        cell.validate()?;
        if let Some(semantic) = cell.first_unsupported() {
            return Err(format!(
                "storage_platform_semantic_unsupported:{}:{}",
                cell.target.as_str(),
                semantic.as_str()
            ));
        }
        if !cell.clock_can_expire() {
            return Err(format!(
                "storage_platform_clock_not_expirable:{}",
                cell.target.as_str()
            ));
        }
        if cell.encoding != StorageEncoding::Utf8 {
            return Err(format!(
                "storage_platform_encoding_refused:{}",
                cell.target.as_str()
            ));
        }
        Ok(cell.proof_ceiling)
    }

    /// The capabilities a caller may actually negotiate on a cell. A semantic the cell does not
    /// support is *absent*, not present-and-false, so a caller cannot read a weaker primitive as
    /// available. A cell that supports everything and has an adjustable wall clock yields
    /// `LocalBehavior`; anything else yields `Source`.
    pub fn negotiated_capabilities(
        &self,
        target: StoragePlatformTarget,
        filesystem: StorageFilesystemClass,
    ) -> Result<BTreeSet<&'static str>, String> {
        let cell = self
            .cell(target, filesystem)
            .ok_or_else(|| format!("storage_platform_target_missing:{}", target.as_str()))?;
        cell.validate()?;
        let mut granted: BTreeSet<&'static str> = cell
            .dispositions()
            .into_iter()
            .filter(|(_, disposition)| disposition.admits_durable_claim())
            .map(|(name, _)| name)
            .collect();
        if !cell.clock_can_expire() {
            granted.remove(StorageSemantic::Fsync.as_str());
        }
        Ok(granted)
    }

    /// Cross-check a cell against the two capability records that will actually run on it.
    ///
    /// A cell that claims a guard its own `StorageSecurityCapabilities` disallows is refused, so
    /// the two vocabularies cannot drift apart silently. The `fsync` bit is checked against
    /// `StorageCapabilities` — the record that already refuses `durable_commits` without it — so
    /// there is exactly one fsync claim in the workspace rather than two.
    pub fn validate_against_capabilities(
        &self,
        cell: &StoragePlatformCell,
        security: &StorageSecurityCapabilities,
        storage: &StorageCapabilities,
    ) -> Result<(), String> {
        cell.validate()?;
        security.validate()?;
        storage.validate()?;
        if security.platform != cell.target.security_platform() {
            return Err("storage_platform_security_platform_mismatch".to_owned());
        }
        for semantic in StorageSemantic::ALL {
            let Some(guarded) = semantic.security_capability() else {
                continue;
            };
            if cell.admits(semantic) && !guarded.reported(security) {
                return Err(format!(
                    "storage_platform_guard_not_enforceable:{}",
                    guarded.as_str()
                ));
            }
        }
        if cell.admits(StorageSemantic::Fsync) && !storage.fsync {
            return Err("storage_platform_guard_not_enforceable:fsync".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "baseline_digest": self.baseline_digest,
            "cells": self.cells,
        }))
    }
}

/// Admit — or refuse — a wall-clock deadline (a TTL, a lease expiry, a retention watermark) on a
/// cell, given the clock observation's own trust.
///
/// The card rejects 时钟回退破坏 TTL/retention: a wall clock that has rolled back, or that a
/// previous sample already moved backwards, cannot be used to decide that a deadline has not
/// passed. The refusal is the PD-32 half of the rule; `ClockObservation`'s own `Rollback` trust is
/// the domain half, and both have to agree. A clock observation is supplied by the caller because
/// this module never reads a clock.
pub fn admit_platform_expiry(
    cell: &StoragePlatformCell,
    clock_trusted: bool,
    wall_now_unix_ms: u64,
    expires_at_unix_ms: u64,
) -> Result<u64, String> {
    cell.validate()?;
    if !cell.clock_can_expire() {
        return Err(format!(
            "storage_platform_expiry_refused:{}",
            cell.clock.as_str()
        ));
    }
    if !clock_trusted {
        return Err("storage_platform_expiry_refused:untrusted_clock".to_owned());
    }
    if wall_now_unix_ms == 0 || expires_at_unix_ms == 0 {
        return Err("storage_platform_expiry_sample_invalid".to_owned());
    }
    if wall_now_unix_ms >= expires_at_unix_ms {
        return Err("storage_platform_expiry_elapsed".to_owned());
    }
    Ok(expires_at_unix_ms)
}

/// The `platform` string PD-28 records for the host this binary was built for.
///
/// This is a compile-time constant, not a probe: it says which row of the matrix a Linux build
/// should be pointed at, and it is deliberately *not* a substitute for statting a mount. A build on
/// Linux pointed at a `network_fs` cell still gets that cell's refusal.
pub const STORAGE_PLATFORM_BUILD_TARGET: &str = "linux";

pub fn storage_platform_scope() -> BTreeSet<&'static str> {
    StoragePlatformTarget::ALL
        .into_iter()
        .map(|target| target.as_str())
        .collect()
}

fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_STORAGE_PLATFORM_TEXT
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_not_redacted"));
    }
    scan_secret_sentinels(SecretScanChannel::Receipt, value)
        .map_err(|_| format!("{field}_secret_detected"))
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(format!("{field}_invalid"))
    }
}
