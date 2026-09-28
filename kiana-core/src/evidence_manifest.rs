//! SC-35 evidence manifest and fixture/cassette registry.
//!
//! Every claim this repository makes about itself — a step landed, a gate passed, a proof level
//! reached — is supposed to arrive wrapped in an evidence block. This module is the shape that
//! block has to take before it is allowed to count.
//!
//! The card names five things that go missing and one thing that happens to them:
//!
//! ```text
//! 命令 (argv)   环境 (cwd + environment)   源码 (source snapshot)
//! fixture/cassette                        限制 (limitations)
//!                                              证据被手工改写
//! ```
//!
//! Each of the first five is a field that may be empty, and each emptiness is a refusal rather
//! than a default. The sixth is different in kind: a manifest is a *record*, and a record that
//! can be edited after the fact is not evidence. So the manifest is sealed with a digest over its
//! own canonical content, every fixture it cites is bound to a digest in a registry, and each
//! manifest may name the manifest it follows. A hand edit breaks the seal, a swapped fixture
//! breaks the registry binding, and a manifest that claims a predecessor it does not have breaks
//! the chain.
//!
//! # Why a manifest cannot carry a proof level above its evidence
//!
//! `proof_level` is an ordered ladder: `source < local_behavior < durable < live < physical`.
//! The ordering is not decoration — the whole failure mode this repository keeps hitting is a
//! source-only contract described in language that sounds like a deployed system. So the manifest
//! refuses a `feature_status` and `proof_level` combination that cannot both be true, and refuses a
//! manifest whose limitations list is empty, because "no limitations" is never the honest answer.
//!
//! # This module is a read-only contract
//!
//! It writes no file, appends no event, runs no command and calls no port. The registry inside it
//! is supplied by the caller: deciding *which* digests are correct belongs to whatever actually
//! read the bytes, and this module only decides whether the claim matches the registry it was
//! given. A manifest is therefore a decision over supplied values, and the proof ceiling of this
//! slice is `source` — the same ceiling it requires of itself.

use kiana_domain::{
    json_digest, redact_text, scan_secret_sentinels, SchemaVersion, SecretScanChannel,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const EVIDENCE_MANIFEST_SCHEMA: &str = "kiana.evidence-manifest.v1";
pub const FIXTURE_REGISTRY_SCHEMA: &str = "kiana.fixture-registry.v1";
pub const EVIDENCE_MANIFEST_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_EVIDENCE_TEXT: usize = 512;
pub const MAX_EVIDENCE_ARGV: usize = 32;
pub const MAX_EVIDENCE_LIMITS: usize = 32;
pub const MAX_EVIDENCE_FIXTURES: usize = 32;
pub const MIN_SOURCE_SNAPSHOT_LEN: usize = 7;
pub const MAX_SOURCE_SNAPSHOT_LEN: usize = 64;

/// How strong a claim is. The ordering is the point: a manifest may not claim a rung it has not
/// reached, so the variants are ordered rather than free-form strings.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceProofLevel {
    /// A contract that exists in source. Says nothing about behaviour.
    Source,
    /// A behaviour that was actually observed by running something.
    LocalBehavior,
    /// Survives a restart and is reconstructible from committed facts.
    Durable,
    /// Exercised against a real external system.
    Live,
    /// Proven on real hardware.
    Physical,
}

impl EvidenceProofLevel {
    pub const ALL: [Self; 5] = [
        Self::Source,
        Self::LocalBehavior,
        Self::Durable,
        Self::Live,
        Self::Physical,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::LocalBehavior => "local_behavior",
            Self::Durable => "durable",
            Self::Live => "live",
            Self::Physical => "physical",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "source" => Ok(Self::Source),
            "local_behavior" => Ok(Self::LocalBehavior),
            "durable" => Ok(Self::Durable),
            "live" => Ok(Self::Live),
            "physical" => Ok(Self::Physical),
            _ => Err("evidence_manifest_proof_level_invalid".to_owned()),
        }
    }
}

/// What a slice says it built. Kept separate from the proof level because a thing can be
/// `deferred` at a high proof level: the claim is about intent, not about evidence.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceFeatureStatus {
    NotSupported,
    Target,
    Deferred,
    Partial,
    Implemented,
}

impl EvidenceFeatureStatus {
    pub const ALL: [Self; 5] = [
        Self::NotSupported,
        Self::Target,
        Self::Deferred,
        Self::Partial,
        Self::Implemented,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotSupported => "not_supported",
            Self::Target => "target",
            Self::Deferred => "deferred",
            Self::Partial => "partial",
            Self::Implemented => "implemented",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "not_supported" => Ok(Self::NotSupported),
            "target" => Ok(Self::Target),
            "deferred" => Ok(Self::Deferred),
            "partial" => Ok(Self::Partial),
            "implemented" => Ok(Self::Implemented),
            _ => Err("evidence_manifest_feature_status_invalid".to_owned()),
        }
    }
}

/// Whether a cited file is a hand-written input or a recorded run.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureKind {
    /// Deterministic input written by a human.
    Fixture,
    /// A recorded transcript of a real run. The difference matters because a cassette can go
    /// stale against a changed source without anyone noticing.
    Cassette,
}

impl FixtureKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fixture => "fixture",
            Self::Cassette => "cassette",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "fixture" => Ok(Self::Fixture),
            "cassette" => Ok(Self::Cassette),
            _ => Err("evidence_manifest_fixture_kind_invalid".to_owned()),
        }
    }
}

/// One cited file and the digest it had when the claim was made.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureRef {
    /// Repository-relative path. The manifest never carries an absolute path: an absolute path
    /// is machine-specific and would make two developers' evidence incomparable.
    pub path: String,
    pub kind: FixtureKind,
    /// `sha256:<64 hex>`. The prefix keeps the algorithm explicit, so a digest copied from a
    /// different algorithm cannot be pasted in and believed.
    pub sha256: String,
}

impl FixtureRef {
    pub fn new(path: impl Into<String>, kind: FixtureKind, sha256: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            kind,
            sha256: sha256.into(),
        }
    }
}

/// The set of files a manifest is allowed to cite, with the digest each one must carry.
///
/// This is a *supplied* registry. Nothing here reads a filesystem to confirm a digest, because
/// doing so would make the evidence check depend on the machine running it; the caller supplies
/// what it read and this module decides whether the claim matches.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureRegistry {
    pub schema: String,
    pub version: SchemaVersion,
    pub entries: Vec<FixtureRef>,
    pub registry_digest: String,
}

impl FixtureRegistry {
    pub fn new(entries: Vec<FixtureRef>) -> Result<Self, String> {
        let mut value = Self {
            schema: FIXTURE_REGISTRY_SCHEMA.to_owned(),
            version: EVIDENCE_MANIFEST_VERSION,
            entries,
            registry_digest: String::new(),
        };
        value.registry_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "entries": self.entries,
        }))
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema != FIXTURE_REGISTRY_SCHEMA
            || !self.version.is_compatible_with(&EVIDENCE_MANIFEST_VERSION)
        {
            return Err("evidence_manifest_registry_header_invalid".to_owned());
        }
        if self.registry_digest != self.digest() {
            return Err("evidence_manifest_registry_digest_mismatch".to_owned());
        }
        let mut seen = BTreeSet::new();
        for entry in &self.entries {
            valid_path(&entry.path)?;
            valid_digest(&entry.sha256, "evidence_manifest_fixture")?;
            if !seen.insert(entry.path.clone()) {
                return Err("evidence_manifest_fixture_duplicate".to_owned());
            }
        }
        Ok(())
    }

    /// The registered entry for a path, if the registry knows it.
    pub fn lookup(&self, path: &str) -> Option<&FixtureRef> {
        self.entries.iter().find(|entry| entry.path == path)
    }
}

/// The command that was run, where it ran, and how it ended.
///
/// `exit_code` is an `Option` on purpose. "The command produced no exit code" is a real and
/// important state — a run that was cancelled, or never started — and giving it a sentinel like
/// `-1` would let it be confused with a real process exit.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandInvocation {
    /// The full argv, including argv[0]. Not a shell string: a string can be re-split, and a
    /// re-split command is not the command that ran.
    pub argv: Vec<String>,
    pub cwd: String,
    pub exit_code: Option<i64>,
}

impl CommandInvocation {
    pub fn new(argv: Vec<String>, cwd: impl Into<String>, exit_code: Option<i64>) -> Self {
        Self {
            argv,
            cwd: cwd.into(),
            exit_code,
        }
    }
}

/// One environment fact. Values are redacted and secret-scanned before they may be sealed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentFact {
    pub key: String,
    pub value: String,
}

impl EnvironmentFact {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

/// A sealed evidence record for one claim.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceManifest {
    pub schema: String,
    pub version: SchemaVersion,
    pub manifest_id: String,
    /// The commit the claim is about. Hex, 7..=64 characters, so a short sha and a full sha are
    /// both acceptable and a sentence is not.
    pub source_snapshot: String,
    /// Who recorded the claim.
    pub recorded_by: String,
    /// Who checked it. May not be the same person; self-review is refused below.
    pub reviewer: String,
    pub command: CommandInvocation,
    pub environment: Vec<EnvironmentFact>,
    /// Fixtures this claim rests on. May be empty only when `fixture_absent_reason` says why.
    pub fixtures: Vec<FixtureRef>,
    pub fixture_absent_reason: String,
    /// The state change this manifest asserts, in words. Free text on purpose: the alternative is
    /// inventing a vocabulary that every future slice would have to extend.
    pub status_change: String,
    pub feature_status: EvidenceFeatureStatus,
    pub proof_level: EvidenceProofLevel,
    /// What this manifest does **not** show. Never allowed to be empty.
    pub limitations: Vec<String>,
    /// The manifest this one follows, so a sequence of claims is a chain rather than a pile.
    pub previous_manifest_digest: Option<String>,
    pub manifest_digest: String,
}

impl EvidenceManifest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        manifest_id: impl Into<String>,
        source_snapshot: impl Into<String>,
        recorded_by: impl Into<String>,
        reviewer: impl Into<String>,
        command: CommandInvocation,
        environment: Vec<EnvironmentFact>,
        fixtures: Vec<FixtureRef>,
        fixture_absent_reason: impl Into<String>,
        status_change: impl Into<String>,
        feature_status: EvidenceFeatureStatus,
        proof_level: EvidenceProofLevel,
        limitations: Vec<String>,
        previous_manifest_digest: Option<String>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: EVIDENCE_MANIFEST_SCHEMA.to_owned(),
            version: EVIDENCE_MANIFEST_VERSION,
            manifest_id: manifest_id.into(),
            source_snapshot: source_snapshot.into(),
            recorded_by: recorded_by.into(),
            reviewer: reviewer.into(),
            command,
            environment,
            fixtures,
            fixture_absent_reason: fixture_absent_reason.into(),
            status_change: status_change.into(),
            feature_status,
            proof_level,
            limitations,
            previous_manifest_digest,
            manifest_digest: String::new(),
        };
        value.manifest_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    /// The seal. Every field above is inside it; `manifest_digest` itself is not, because a digest
    /// cannot contain itself.
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "manifest_id": self.manifest_id,
            "source_snapshot": self.source_snapshot,
            "recorded_by": self.recorded_by,
            "reviewer": self.reviewer,
            "command": self.command,
            "environment": self.environment,
            "fixtures": self.fixtures,
            "fixture_absent_reason": self.fixture_absent_reason,
            "status_change": self.status_change,
            "feature_status": self.feature_status,
            "proof_level": self.proof_level,
            "limitations": self.limitations,
            "previous_manifest_digest": self.previous_manifest_digest,
        }))
    }

    /// Shape and self-consistency. Says nothing about whether the claims inside are true.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EVIDENCE_MANIFEST_SCHEMA
            || !self.version.is_compatible_with(&EVIDENCE_MANIFEST_VERSION)
        {
            return Err("evidence_manifest_header_invalid".to_owned());
        }
        // A hand edit anywhere above shows up here first, which is the point of sealing.
        if self.manifest_digest != self.digest() {
            return Err("evidence_manifest_digest_mismatch".to_owned());
        }
        safe_text(&self.manifest_id, "evidence_manifest_id")?;
        valid_snapshot(&self.source_snapshot)?;
        safe_text(&self.recorded_by, "evidence_manifest_recorded_by")?;
        if self.reviewer.trim().is_empty() {
            return Err("evidence_manifest_reviewer_required".to_owned());
        }
        // Emptiness is checked before the malformed check so "nobody reviewed this" and "this
        // reviewer string is corrupt" stay two different refusals.
        safe_text(&self.reviewer, "evidence_manifest_reviewer")?;
        if self.reviewer == self.recorded_by {
            return Err("evidence_manifest_reviewer_self".to_owned());
        }
        if self.command.argv.is_empty() || self.command.argv.len() > MAX_EVIDENCE_ARGV {
            return Err("evidence_manifest_command_required".to_owned());
        }
        for argument in &self.command.argv {
            safe_text(argument, "evidence_manifest_command_argv")?;
        }
        safe_text(&self.command.cwd, "evidence_manifest_cwd")?;
        if self.command.exit_code.is_none() {
            return Err("evidence_manifest_exit_code_required".to_owned());
        }
        if self.environment.is_empty() {
            return Err("evidence_manifest_environment_required".to_owned());
        }
        let mut keys = BTreeSet::new();
        for fact in &self.environment {
            safe_text(&fact.key, "evidence_manifest_environment_key")?;
            if !keys.insert(fact.key.clone()) {
                return Err("evidence_manifest_environment_duplicate".to_owned());
            }
            safe_text(&fact.value, "evidence_manifest_environment_value")?;
        }
        if self.fixtures.len() > MAX_EVIDENCE_FIXTURES {
            return Err("evidence_manifest_fixture_exhausted".to_owned());
        }
        let mut paths = BTreeSet::new();
        for fixture in &self.fixtures {
            valid_path(&fixture.path)?;
            valid_digest(&fixture.sha256, "evidence_manifest_fixture")?;
            if !paths.insert(fixture.path.clone()) {
                return Err("evidence_manifest_fixture_duplicate".to_owned());
            }
        }
        // No fixture is a legitimate state -- a documentation-only change has none -- but it has
        // to be said out loud, otherwise "we checked nothing" reads like "we checked and it passed".
        if self.fixtures.is_empty() && self.fixture_absent_reason.trim().is_empty() {
            return Err("evidence_manifest_fixture_required".to_owned());
        }
        if !self.fixtures.is_empty() && !self.fixture_absent_reason.trim().is_empty() {
            return Err("evidence_manifest_fixture_absent_reason_conflict".to_owned());
        }
        if !self.fixture_absent_reason.trim().is_empty() {
            safe_text(
                &self.fixture_absent_reason,
                "evidence_manifest_fixture_absent_reason",
            )?;
        }
        if self.status_change.trim().is_empty() {
            return Err("evidence_manifest_status_change_required".to_owned());
        }
        safe_text(&self.status_change, "evidence_manifest_status_change")?;
        if self.limitations.is_empty() || self.limitations.len() > MAX_EVIDENCE_LIMITS {
            return Err("evidence_manifest_limitations_required".to_owned());
        }
        for limitation in &self.limitations {
            safe_text(limitation, "evidence_manifest_limitation")?;
        }
        if let Some(previous) = &self.previous_manifest_digest {
            valid_digest(previous, "evidence_manifest_previous")?;
        }
        // The claim has to be internally possible: a deferred or unsupported thing cannot be
        // carrying a durable or live proof, because there is nothing there to have been proven.
        if self.feature_status <= EvidenceFeatureStatus::Deferred
            && self.proof_level >= EvidenceProofLevel::Durable
        {
            return Err("evidence_manifest_proof_above_feature".to_owned());
        }
        Ok(())
    }

    /// Check the claim against the registry that is supposed to describe this repository.
    ///
    /// This is the binding that stops a manifest from citing a file that does not exist, or from
    /// citing one whose bytes changed after the claim was written. The second case is the common
    /// one: a cassette recorded against an older source is still a perfectly valid file, and it is
    /// no longer evidence for anything.
    pub fn validate_against(&self, registry: &FixtureRegistry) -> Result<(), String> {
        self.validate()?;
        registry.validate()?;
        for fixture in &self.fixtures {
            let Some(registered) = registry.lookup(&fixture.path) else {
                return Err("evidence_manifest_fixture_unknown".to_owned());
            };
            if registered.kind != fixture.kind {
                return Err("evidence_manifest_fixture_kind_conflict".to_owned());
            }
            if registered.sha256 != fixture.sha256 {
                return Err("evidence_manifest_fixture_digest_mismatch".to_owned());
            }
        }
        Ok(())
    }

    /// Link this manifest to the one it follows.
    ///
    /// `previous` is `None` for the first manifest in a chain. Passing `None` while the manifest
    /// claims a predecessor is refused, because that is what a forged chain looks like: the link
    /// is asserted and the thing it points at is not supplied.
    pub fn verify_chain(&self, previous: Option<&EvidenceManifest>) -> Result<(), String> {
        self.validate()?;
        match previous {
            Some(previous) => {
                previous.validate()?;
                if previous.manifest_id == self.manifest_id {
                    return Err("evidence_manifest_chain_identity".to_owned());
                }
                if self.previous_manifest_digest.as_deref()
                    != Some(previous.manifest_digest.as_str())
                {
                    return Err("evidence_manifest_chain_broken".to_owned());
                }
            }
            None => {
                if self.previous_manifest_digest.is_some() {
                    return Err("evidence_manifest_chain_orphan".to_owned());
                }
            }
        }
        Ok(())
    }
}

fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_EVIDENCE_TEXT
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

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// A repository-relative path. Absolute paths and parent traversal are refused because a manifest
/// has to mean the same thing on somebody else's machine.
fn valid_path(value: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_EVIDENCE_TEXT
        || value.contains(['\0', '\r', '\n'])
        || value.starts_with('/')
        || value.contains("..")
        || value.contains("://")
    {
        return Err("evidence_manifest_fixture_path_invalid".to_owned());
    }
    Ok(())
}

fn valid_snapshot(value: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.len() < MIN_SOURCE_SNAPSHOT_LEN
        || trimmed.len() > MAX_SOURCE_SNAPSHOT_LEN
        || !trimmed
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err("evidence_manifest_source_snapshot_invalid".to_owned());
    }
    Ok(())
}
