//! SC-37/SC-38/SC-39 deny-first, property and red-team fixture contracts.
//!
//! These contracts are a SOURCE-level evidence index over the security deny paths. They record
//! which stable reason a refusal produced, how many handler/provider/adapter dispatches were
//! observed before the refusal, and which limitation blocks a stronger proof. They do not
//! execute a capability, resolve a secret, contact a provider, or turn a fixture into a runtime
//! enforcement claim: a fixture that says "denied, zero dispatch" describes a recorded
//! observation, not a guarantee about any future adapter.

use crate::{json_digest, OperationId, RequestId, RunId, SchemaVersion, SecurityReasonCode};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

/// Maximum number of cases any one SC-37/38/39 corpus may carry.
pub const MAX_SECURITY_FIXTURE_CASES: usize = 128;
/// Maximum number of dispatch counters recorded on one case.
pub const MAX_SECURITY_FIXTURE_DISPATCHES: usize = 4;
/// Maximum number of limitation strings recorded on one case.
pub const MAX_SECURITY_FIXTURE_LIMITATIONS: usize = 8;
/// Maximum byte length of a corpus id or case name.
pub const MAX_SECURITY_FIXTURE_NAME_BYTES: usize = 128;

// ---------------------------------------------------------------------------
// SC-37: deny-first matrix with zero-effect dispatch counters.
// ---------------------------------------------------------------------------

pub const SC37_DENY_MATRIX_SCHEMA: &str = "kiana.sc37-deny-matrix.v1";
pub const SC37_DENY_CASE_SCHEMA: &str = "kiana.sc37-deny-case.v1";
pub const SC37_DENY_MATRIX_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// The refusal families SC-37 requires a named zero-effect proof for.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sc37DenyFamily {
    /// 越权: principal, project, role or session claims that do not match the server snapshot.
    PrivilegeEscalation,
    /// 重放: a command/permit/approval presented a second time or under a stale idempotency key.
    Replay,
    /// 泄露: a secret sentinel reaching a prompt, event, receipt, argv, env, stdout or stderr sink.
    Leakage,
    /// TOCTOU: path, root, generation or fence drift detected between check and effect.
    Toctou,
    /// 删除: a deletion blocked by legal hold, retention, stale epoch or an unknown target.
    Deletion,
    /// 入口绕过: a UI, CLI, scheduler or connector route that tries to skip the control plane.
    EntrypointBypass,
}

impl Sc37DenyFamily {
    pub const ALL: &'static [Self] = &[
        Self::PrivilegeEscalation,
        Self::Replay,
        Self::Leakage,
        Self::Toctou,
        Self::Deletion,
        Self::EntrypointBypass,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PrivilegeEscalation => "privilege_escalation",
            Self::Replay => "replay",
            Self::Leakage => "leakage",
            Self::Toctou => "toctou",
            Self::Deletion => "deletion",
            Self::EntrypointBypass => "entrypoint_bypass",
        }
    }
}

/// Named dispatch surfaces. A deny that dispatched any of these is not a deny.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sc37DispatchSurface {
    /// The ControlPlane handler that would perform the effect.
    ControlPlaneHandler,
    /// The capability broker dispatch.
    CapabilityBroker,
    /// The model/provider call.
    ModelProvider,
    /// An external adapter (filesystem, network, connector, MCP server).
    ExternalAdapter,
}

impl Sc37DispatchSurface {
    pub const ALL: &'static [Self] = &[
        Self::ControlPlaneHandler,
        Self::CapabilityBroker,
        Self::ModelProvider,
        Self::ExternalAdapter,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ControlPlaneHandler => "control_plane_handler",
            Self::CapabilityBroker => "capability_broker",
            Self::ModelProvider => "model_provider",
            Self::ExternalAdapter => "external_adapter",
        }
    }
}

/// One recorded refusal with the dispatch counts observed before it.
///
/// A deny-first case is only meaningful when every surface counter is exactly zero: a refusal
/// that still reached a handler or a provider is recorded as a defect, never as a denial.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sc37DenyCase {
    pub schema: String,
    pub case_name: String,
    pub family: Sc37DenyFamily,
    pub expected_reason: SecurityReasonCode,
    pub observed_reason: String,
    /// Dispatch count per surface, keyed by [`Sc37DispatchSurface::as_str`].
    pub dispatch_counts: BTreeMap<String, u32>,
    pub secret_free: bool,
    pub same_spine: bool,
    pub reason_stable: bool,
    pub limitations: Vec<String>,
    pub case_digest: String,
}

impl Sc37DenyCase {
    pub fn new(
        case_name: impl Into<String>,
        family: Sc37DenyFamily,
        expected_reason: SecurityReasonCode,
        observed_reason: impl Into<String>,
    ) -> Result<Self, String> {
        let case = Self {
            schema: SC37_DENY_CASE_SCHEMA.to_owned(),
            case_name: case_name.into(),
            family,
            expected_reason,
            observed_reason: observed_reason.into(),
            dispatch_counts: Sc37DenyCase::zero_dispatch_counts(),
            secret_free: true,
            same_spine: true,
            reason_stable: true,
            limitations: vec![
                "recorded source fixture observation; not a live adapter dispatch counter"
                    .to_owned(),
            ],
            case_digest: String::new(),
        };
        let mut case = case;
        case.case_digest = case.digest();
        case.validate()?;
        Ok(case)
    }

    /// Every known dispatch surface observed exactly zero times.
    pub fn zero_dispatch_counts() -> BTreeMap<String, u32> {
        Sc37DispatchSurface::ALL
            .iter()
            .map(|surface| (surface.as_str().to_owned(), 0))
            .collect()
    }

    pub fn dispatch_count(&self, surface: Sc37DispatchSurface) -> u32 {
        self.dispatch_counts
            .get(surface.as_str())
            .copied()
            .unwrap_or(0)
    }

    /// Build the same refusal with a recorded non-zero dispatch on one surface.
    ///
    /// This exists so the matrix guard can prove the contract REJECTS a deny that still
    /// dispatched; it must never be used to describe a passing case.
    pub fn with_dispatch(&self, surface: Sc37DispatchSurface, count: u32) -> Result<Self, String> {
        let mut case = self.clone();
        case.dispatch_counts
            .insert(surface.as_str().to_owned(), count);
        case.case_digest = case.digest();
        case.validate()?;
        Ok(case)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SC37_DENY_CASE_SCHEMA
            || self.case_name.trim().is_empty()
            || self.case_name.len() > MAX_SECURITY_FIXTURE_NAME_BYTES
            || self.case_name.contains(['\0', '\r', '\n'])
            || self.observed_reason.trim().is_empty()
            || self.observed_reason.len() > MAX_SECURITY_FIXTURE_NAME_BYTES
            || self.observed_reason.contains(['\0', '\r', '\n'])
            || !self.secret_free
            || !self.same_spine
            || !self.reason_stable
            || self.observed_reason != self.expected_reason.as_str()
        {
            return Err("sc37_deny_case_reason_or_spine_invalid".to_owned());
        }
        if self.dispatch_counts.len() != Sc37DispatchSurface::ALL.len()
            || self.dispatch_counts.len() > MAX_SECURITY_FIXTURE_DISPATCHES
        {
            return Err("sc37_deny_case_dispatch_surface_set_invalid".to_owned());
        }
        for surface in Sc37DispatchSurface::ALL {
            let Some(count) = self.dispatch_counts.get(surface.as_str()) else {
                return Err("sc37_deny_case_dispatch_surface_set_invalid".to_owned());
            };
            if *count != 0 {
                return Err("sc37_deny_case_effect_not_zero".to_owned());
            }
        }
        if self.limitations.is_empty() || self.limitations.len() > MAX_SECURITY_FIXTURE_LIMITATIONS
        {
            return Err("sc37_deny_case_limitation_missing".to_owned());
        }
        for limitation in &self.limitations {
            if limitation.trim().is_empty()
                || limitation.len() > MAX_SECURITY_FIXTURE_NAME_BYTES
                || limitation.contains(['\0', '\r', '\n'])
            {
                return Err("sc37_deny_case_limitation_invalid".to_owned());
            }
        }
        validate_digest(&self.case_digest, "sc37_deny_case_digest")?;
        if self.case_digest != self.digest() {
            return Err("sc37_deny_case_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "case_name": self.case_name,
            "family": self.family,
            "expected_reason": self.expected_reason,
            "observed_reason": self.observed_reason,
            "dispatch_counts": self.dispatch_counts,
            "secret_free": self.secret_free,
            "same_spine": self.same_spine,
            "reason_stable": self.reason_stable,
            "limitations": self.limitations,
        }))
    }
}

/// The complete SC-37 deny-first matrix: one or more cases per refusal family.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sc37DenyMatrix {
    pub schema: String,
    pub version: SchemaVersion,
    pub matrix_id: String,
    pub cases: Vec<Sc37DenyCase>,
    pub matrix_digest: String,
}

impl Sc37DenyMatrix {
    pub fn new(matrix_id: impl Into<String>, cases: Vec<Sc37DenyCase>) -> Result<Self, String> {
        let matrix_id = matrix_id.into();
        if matrix_id.trim().is_empty() || matrix_id.len() > MAX_SECURITY_FIXTURE_NAME_BYTES {
            return Err("sc37_deny_matrix_id_invalid".to_owned());
        }
        if cases.is_empty() || cases.len() > MAX_SECURITY_FIXTURE_CASES {
            return Err("sc37_deny_matrix_header_invalid".to_owned());
        }
        let mut matrix = Self {
            schema: SC37_DENY_MATRIX_SCHEMA.to_owned(),
            version: SC37_DENY_MATRIX_VERSION,
            matrix_id,
            cases,
            matrix_digest: String::new(),
        };
        matrix.matrix_digest = matrix.digest();
        matrix.validate()?;
        Ok(matrix)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SC37_DENY_MATRIX_SCHEMA
            || !self.version.is_compatible_with(&SC37_DENY_MATRIX_VERSION)
            || self.matrix_id.trim().is_empty()
            || self.matrix_id.len() > MAX_SECURITY_FIXTURE_NAME_BYTES
            || self.cases.is_empty()
            || self.cases.len() > MAX_SECURITY_FIXTURE_CASES
        {
            return Err("sc37_deny_matrix_header_invalid".to_owned());
        }
        let mut names = BTreeSet::new();
        let mut families = BTreeSet::new();
        for case in &self.cases {
            case.validate()?;
            if !names.insert(case.case_name.clone()) {
                return Err("sc37_deny_case_duplicate".to_owned());
            }
            families.insert(case.family);
        }
        for family in Sc37DenyFamily::ALL {
            if !families.contains(family) {
                return Err("sc37_deny_family_uncovered".to_owned());
            }
        }
        validate_digest(&self.matrix_digest, "sc37_deny_matrix_digest")?;
        if self.matrix_digest != self.digest() {
            return Err("sc37_deny_matrix_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn family_cases(&self, family: Sc37DenyFamily) -> Vec<&Sc37DenyCase> {
        self.cases
            .iter()
            .filter(|case| case.family == family)
            .collect()
    }

    pub fn total_dispatch_count(&self) -> u32 {
        self.cases
            .iter()
            .flat_map(|case| case.dispatch_counts.values().copied())
            .sum()
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "matrix_id": self.matrix_id,
            "cases": self.cases,
        }))
    }
}

// ---------------------------------------------------------------------------
// SC-38: deterministic property / fuzz / serialization / replay corpus.
// ---------------------------------------------------------------------------

pub const SC38_PROPERTY_CORPUS_SCHEMA: &str = "kiana.sc38-property-corpus.v1";
pub const SC38_PROPERTY_CASE_SCHEMA: &str = "kiana.sc38-property-case.v1";
pub const SC38_PROPERTY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// The input shapes SC-38 must prove cannot turn into an allow.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sc38InputShape {
    /// Arbitrary bytes handed to a decoder.
    RandomPayload,
    /// A frame that stops mid-value.
    TruncatedFrame,
    /// The same frame delivered twice.
    DuplicateFrame,
    /// Cursors delivered out of order or with a gap.
    OutOfOrderCursor,
    /// A version whose major does not match the registered contract.
    UnknownMajor,
}

impl Sc38InputShape {
    pub const ALL: &'static [Self] = &[
        Self::RandomPayload,
        Self::TruncatedFrame,
        Self::DuplicateFrame,
        Self::OutOfOrderCursor,
        Self::UnknownMajor,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RandomPayload => "random_payload",
            Self::TruncatedFrame => "truncated_frame",
            Self::DuplicateFrame => "duplicate_frame",
            Self::OutOfOrderCursor => "out_of_order_cursor",
            Self::UnknownMajor => "unknown_major",
        }
    }
}

/// The invariant the corpus proves for one generated input.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sc38Invariant {
    /// Decoding is total: the input is refused, never partially accepted.
    DecodeFailsClosed,
    /// Schema compatibility refuses an unknown major instead of upgrading silently.
    UnknownMajorRejected,
    /// A duplicate frame is deduplicated instead of re-effecting.
    DuplicateDeduplicated,
    /// Cursor order is enforced; a gap is refused rather than skipped.
    CursorOrderEnforced,
    /// Replay of the same input yields a byte-identical decision digest.
    ReplayDeterministic,
}

impl Sc38Invariant {
    pub const ALL: &'static [Self] = &[
        Self::DecodeFailsClosed,
        Self::UnknownMajorRejected,
        Self::DuplicateDeduplicated,
        Self::CursorOrderEnforced,
        Self::ReplayDeterministic,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DecodeFailsClosed => "decode_fails_closed",
            Self::UnknownMajorRejected => "unknown_major_rejected",
            Self::DuplicateDeduplicated => "duplicate_deduplicated",
            Self::CursorOrderEnforced => "cursor_order_enforced",
            Self::ReplayDeterministic => "replay_deterministic",
        }
    }
}

/// The refusal a generated input must produce. `Allow` is not a legal value.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sc38Outcome {
    Denied,
    Deduplicated,
    Unknown,
}

impl Sc38Outcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Denied => "denied",
            Self::Deduplicated => "deduplicated",
            Self::Unknown => "unknown",
        }
    }

    /// The only legal outcome for a fuzzed input. There is deliberately no `Allowed` variant.
    pub const fn is_refusal(self) -> bool {
        true
    }
}

/// One deterministic generated case. `seed` is always passed explicitly; the corpus never reads
/// a wall clock and never draws from an unseeded source of randomness.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sc38PropertyCase {
    pub schema: String,
    pub case_name: String,
    pub seed: u64,
    pub iteration: u32,
    pub shape: Sc38InputShape,
    pub invariant: Sc38Invariant,
    pub outcome: Sc38Outcome,
    pub expected_reason: SecurityReasonCode,
    pub observed_reason: String,
    pub input_digest: String,
    pub decision_digest: String,
    pub replay_decision_digest: String,
    pub limitations: Vec<String>,
    pub case_digest: String,
}

impl Sc38PropertyCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        case_name: impl Into<String>,
        seed: u64,
        iteration: u32,
        shape: Sc38InputShape,
        invariant: Sc38Invariant,
        expected_reason: SecurityReasonCode,
        input_digest: impl Into<String>,
        decision_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let case_name = case_name.into();
        let decision_digest = decision_digest.into();
        let case = Self {
            schema: SC38_PROPERTY_CASE_SCHEMA.to_owned(),
            case_name,
            seed,
            iteration,
            shape,
            invariant,
            // 【为什么 outcome 必须由 invariant 推导，而不是写死 Denied】
            // `validate()` 要求 `invariant == DuplicateDeduplicated` 的 case 其 outcome
            // 必须是 `Deduplicated`；而 `new()` 原本无条件写 `Denied`，紧接着又在同一函数
            // 末尾调用 `validate()`。两者相加的结果是：**这个构造器永远无法构造出
            // DuplicateDeduplicated 的 case**，每次都以
            // `sc38_property_case_duplicate_outcome_invalid` 失败。
            // 夹具层已经在事后手工纠正 outcome（再重算 case_digest），但那发生在构造之后，
            // 根本走不到。
            //
            // 这里让 outcome 随 invariant 走：只有 DuplicateDeduplicated 用 Deduplicated，
            // 其余保持 Denied。需要「把重复帧记成一次新的拒绝」这种**错误**组合的用例，
            // 仍然可以照常在构造之后改写 outcome 再断言 `validate()` 拒绝——那条路径不受影响。
            outcome: if invariant == Sc38Invariant::DuplicateDeduplicated {
                Sc38Outcome::Deduplicated
            } else {
                Sc38Outcome::Denied
            },
            expected_reason,
            observed_reason: expected_reason.as_str().to_owned(),
            input_digest: input_digest.into(),
            replay_decision_digest: decision_digest.clone(),
            decision_digest,
            limitations: vec![
                "deterministic fixture replay; no fuzzing engine, clock or live adapter involved"
                    .to_owned(),
            ],
            case_digest: String::new(),
        };
        let mut case = case;
        case.case_digest = case.digest();
        case.validate()?;
        Ok(case)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SC38_PROPERTY_CASE_SCHEMA
            || self.case_name.trim().is_empty()
            || self.case_name.len() > MAX_SECURITY_FIXTURE_NAME_BYTES
            || self.case_name.contains(['\0', '\r', '\n'])
            || self.seed == 0
        {
            return Err("sc38_property_case_header_invalid".to_owned());
        }
        if self.observed_reason != self.expected_reason.as_str() {
            return Err("sc38_property_case_reason_unstable".to_owned());
        }
        if self.invariant == Sc38Invariant::ReplayDeterministic
            && self.decision_digest != self.replay_decision_digest
        {
            return Err("sc38_property_case_replay_diverged".to_owned());
        }
        if self.invariant == Sc38Invariant::DuplicateDeduplicated
            && self.outcome != Sc38Outcome::Deduplicated
        {
            return Err("sc38_property_case_duplicate_outcome_invalid".to_owned());
        }
        if self.invariant == Sc38Invariant::UnknownMajorRejected
            && self.shape != Sc38InputShape::UnknownMajor
        {
            return Err("sc38_property_case_unknown_major_shape_invalid".to_owned());
        }
        validate_digest(&self.input_digest, "sc38_property_case_input_digest")?;
        validate_digest(&self.decision_digest, "sc38_property_case_decision_digest")?;
        validate_digest(
            &self.replay_decision_digest,
            "sc38_property_case_replay_decision_digest",
        )?;
        if self.limitations.is_empty() || self.limitations.len() > MAX_SECURITY_FIXTURE_LIMITATIONS
        {
            return Err("sc38_property_case_limitation_missing".to_owned());
        }
        for limitation in &self.limitations {
            if limitation.trim().is_empty()
                || limitation.len() > MAX_SECURITY_FIXTURE_NAME_BYTES
                || limitation.contains(['\0', '\r', '\n'])
            {
                return Err("sc38_property_case_limitation_invalid".to_owned());
            }
        }
        validate_digest(&self.case_digest, "sc38_property_case_digest")?;
        if self.case_digest != self.digest() {
            return Err("sc38_property_case_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "case_name": self.case_name,
            "seed": self.seed,
            "iteration": self.iteration,
            "shape": self.shape,
            "invariant": self.invariant,
            "outcome": self.outcome,
            "expected_reason": self.expected_reason,
            "observed_reason": self.observed_reason,
            "input_digest": self.input_digest,
            "decision_digest": self.decision_digest,
            "replay_decision_digest": self.replay_decision_digest,
            "limitations": self.limitations,
        }))
    }
}

/// A seeded corpus plus the replay digest that proves ordering is not load-bearing.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sc38PropertyCorpus {
    pub schema: String,
    pub version: SchemaVersion,
    pub corpus_id: String,
    pub seed: u64,
    pub source_snapshot: String,
    pub cases: Vec<Sc38PropertyCase>,
    pub corpus_digest: String,
}

impl Sc38PropertyCorpus {
    pub fn new(
        corpus_id: impl Into<String>,
        seed: u64,
        source_snapshot: impl Into<String>,
        cases: Vec<Sc38PropertyCase>,
    ) -> Result<Self, String> {
        let corpus_id = corpus_id.into();
        if corpus_id.trim().is_empty() || corpus_id.len() > MAX_SECURITY_FIXTURE_NAME_BYTES {
            return Err("sc38_property_corpus_id_invalid".to_owned());
        }
        if cases.is_empty() || cases.len() > MAX_SECURITY_FIXTURE_CASES {
            return Err("sc38_property_corpus_header_invalid".to_owned());
        }
        let mut corpus = Self {
            schema: SC38_PROPERTY_CORPUS_SCHEMA.to_owned(),
            version: SC38_PROPERTY_VERSION,
            corpus_id,
            seed,
            source_snapshot: source_snapshot.into(),
            cases,
            corpus_digest: String::new(),
        };
        corpus.corpus_digest = corpus.digest();
        corpus.validate()?;
        Ok(corpus)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SC38_PROPERTY_CORPUS_SCHEMA
            || !self.version.is_compatible_with(&SC38_PROPERTY_VERSION)
            || self.corpus_id.trim().is_empty()
            || self.seed == 0
            || self.source_snapshot.trim().is_empty()
            || self.cases.is_empty()
            || self.cases.len() > MAX_SECURITY_FIXTURE_CASES
        {
            return Err("sc38_property_corpus_header_invalid".to_owned());
        }
        let mut names = BTreeSet::new();
        let mut shapes = BTreeSet::new();
        for case in &self.cases {
            case.validate()?;
            if case.seed != self.seed {
                return Err("sc38_property_case_seed_mismatch".to_owned());
            }
            if !names.insert(case.case_name.clone()) {
                return Err("sc38_property_case_duplicate".to_owned());
            }
            shapes.insert(case.shape);
        }
        for shape in Sc38InputShape::ALL {
            if !shapes.contains(shape) {
                return Err("sc38_property_shape_uncovered".to_owned());
            }
        }
        validate_digest(&self.corpus_digest, "sc38_property_corpus_digest")?;
        if self.corpus_digest != self.digest() {
            return Err("sc38_property_corpus_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Replay the same corpus in reverse and prove the digest is order-independent.
    pub fn replay_reversed(&self) -> Result<Self, String> {
        let mut reversed = self.clone();
        reversed.cases.reverse();
        reversed.corpus_digest = reversed.digest();
        reversed.validate()?;
        Ok(reversed)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "corpus_id": self.corpus_id,
            "seed": self.seed,
            "source_snapshot": self.source_snapshot,
            "cases": self.cases,
        }))
    }
}

// ---------------------------------------------------------------------------
// SC-39: red-team / eval attack corpus.
// ---------------------------------------------------------------------------

pub const SC39_ATTACK_CORPUS_SCHEMA: &str = "kiana.sc39-attack-corpus.v1";
pub const SC39_ATTACK_CASE_SCHEMA: &str = "kiana.sc39-attack-case.v1";
pub const SC39_ATTACK_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// The attack shapes SC-39 must show cannot produce an effect.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sc39AttackShape {
    /// Text that claims to be a system/developer policy change.
    PromptInjection,
    /// Repository, tool output or a retrieved document that carries instructions.
    IndirectInjection,
    /// An attack whose goal is to move a secret value out of the Broker.
    SecretExfiltration,
    /// An extension manifest that declares capabilities it was not granted.
    MaliciousPlugin,
    /// An MCP tool description that asks the model to widen scope.
    MaliciousMcpDescription,
}

impl Sc39AttackShape {
    pub const ALL: &'static [Self] = &[
        Self::PromptInjection,
        Self::IndirectInjection,
        Self::SecretExfiltration,
        Self::MaliciousPlugin,
        Self::MaliciousMcpDescription,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PromptInjection => "prompt_injection",
            Self::IndirectInjection => "indirect_injection",
            Self::SecretExfiltration => "secret_exfiltration",
            Self::MaliciousPlugin => "malicious_plugin",
            Self::MaliciousMcpDescription => "malicious_mcp_description",
        }
    }
}

/// What the control plane is expected to do with an attack. There is no `Allow` value: an
/// attack that is merely unrecognised is `Unknown`, not a success.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sc39ExpectedOutcome {
    Denied,
    RequiresApproval,
    Unknown,
}

impl Sc39ExpectedOutcome {
    pub const ALL: &'static [Self] = &[Self::Denied, Self::RequiresApproval, Self::Unknown];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Denied => "denied",
            Self::RequiresApproval => "requires_approval",
            Self::Unknown => "unknown",
        }
    }
}

/// One attack input and the effect budget it is allowed to consume.
///
/// The corpus stores only a digest of the attack text: an attack corpus must not itself become a
/// place where secret material or a working exploit is stored.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sc39AttackCase {
    pub schema: String,
    pub attack_id: String,
    pub shape: Sc39AttackShape,
    pub attack_text_digest: String,
    pub prompt_authority: PromptTrustClass,
    pub expected_outcome: Sc39ExpectedOutcome,
    pub expected_reason: SecurityReasonCode,
    pub observed_reason: String,
    pub handler_calls: u32,
    pub provider_calls: u32,
    pub effect_count: u32,
    /// True when the fixture text is synthetic and contains no real secret material.
    pub synthetic_text_only: bool,
    pub no_external_effect: bool,
    pub limitations: Vec<String>,
    pub case_digest: String,
}

/// Whether a prompt section may be treated as policy. Only `Product` is policy; everything a
/// model, tool, repository or server sent is `Context` and never widens authority.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptTrustClass {
    Product,
    Context,
    Untrusted,
}

impl PromptTrustClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Product => "product",
            Self::Context => "context",
            Self::Untrusted => "untrusted",
        }
    }

    /// Only the product prompt may be treated as policy. This is the SC-39 injection invariant.
    pub const fn is_authority(self) -> bool {
        matches!(self, Self::Product)
    }
}

impl Sc39AttackCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        attack_id: impl Into<String>,
        shape: Sc39AttackShape,
        attack_text_digest: impl Into<String>,
        prompt_authority: PromptTrustClass,
        expected_outcome: Sc39ExpectedOutcome,
        expected_reason: SecurityReasonCode,
    ) -> Result<Self, String> {
        let attack_id = attack_id.into();
        let case = Self {
            schema: SC39_ATTACK_CASE_SCHEMA.to_owned(),
            attack_id,
            shape,
            attack_text_digest: attack_text_digest.into(),
            prompt_authority,
            expected_outcome,
            expected_reason,
            observed_reason: expected_reason.as_str().to_owned(),
            handler_calls: 0,
            provider_calls: 0,
            effect_count: 0,
            synthetic_text_only: true,
            no_external_effect: true,
            limitations: vec![
                "offline corpus; no model, provider, MCP server or plugin was executed".to_owned(),
            ],
            case_digest: String::new(),
        };
        let mut case = case;
        case.case_digest = case.digest();
        case.validate()?;
        Ok(case)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SC39_ATTACK_CASE_SCHEMA
            || self.attack_id.trim().is_empty()
            || self.attack_id.len() > MAX_SECURITY_FIXTURE_NAME_BYTES
            || self.attack_id.contains(['\0', '\r', '\n'])
            || !self.synthetic_text_only
            || !self.no_external_effect
            || self.observed_reason != self.expected_reason.as_str()
        {
            return Err("sc39_attack_case_header_invalid".to_owned());
        }
        // Injection text is context by construction; a fixture may never mark it as authority.
        if self.prompt_authority.is_authority() {
            return Err("sc39_attack_case_prompt_authority_invalid".to_owned());
        }
        if self.handler_calls != 0 || self.provider_calls != 0 || self.effect_count != 0 {
            return Err("sc39_attack_case_effect_not_zero".to_owned());
        }
        validate_digest(&self.attack_text_digest, "sc39_attack_case_text_digest")?;
        if self.limitations.is_empty() || self.limitations.len() > MAX_SECURITY_FIXTURE_LIMITATIONS
        {
            return Err("sc39_attack_case_limitation_missing".to_owned());
        }
        for limitation in &self.limitations {
            if limitation.trim().is_empty()
                || limitation.len() > MAX_SECURITY_FIXTURE_NAME_BYTES
                || limitation.contains(['\0', '\r', '\n'])
            {
                return Err("sc39_attack_case_limitation_invalid".to_owned());
            }
        }
        validate_digest(&self.case_digest, "sc39_attack_case_digest")?;
        if self.case_digest != self.digest() {
            return Err("sc39_attack_case_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "attack_id": self.attack_id,
            "shape": self.shape,
            "attack_text_digest": self.attack_text_digest,
            "prompt_authority": self.prompt_authority,
            "expected_outcome": self.expected_outcome,
            "expected_reason": self.expected_reason,
            "observed_reason": self.observed_reason,
            "handler_calls": self.handler_calls,
            "provider_calls": self.provider_calls,
            "effect_count": self.effect_count,
            "synthetic_text_only": self.synthetic_text_only,
            "no_external_effect": self.no_external_effect,
            "limitations": self.limitations,
        }))
    }
}

/// The SC-39 attack corpus: at least one synthetic attack per shape.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sc39AttackCorpus {
    pub schema: String,
    pub version: SchemaVersion,
    pub corpus_id: String,
    pub source_snapshot: String,
    pub cases: Vec<Sc39AttackCase>,
    pub corpus_digest: String,
}

impl Sc39AttackCorpus {
    pub fn new(
        corpus_id: impl Into<String>,
        source_snapshot: impl Into<String>,
        cases: Vec<Sc39AttackCase>,
    ) -> Result<Self, String> {
        let corpus_id = corpus_id.into();
        if corpus_id.trim().is_empty() || corpus_id.len() > MAX_SECURITY_FIXTURE_NAME_BYTES {
            return Err("sc39_attack_corpus_id_invalid".to_owned());
        }
        if cases.is_empty() || cases.len() > MAX_SECURITY_FIXTURE_CASES {
            return Err("sc39_attack_corpus_header_invalid".to_owned());
        }
        let mut corpus = Self {
            schema: SC39_ATTACK_CORPUS_SCHEMA.to_owned(),
            version: SC39_ATTACK_VERSION,
            corpus_id,
            source_snapshot: source_snapshot.into(),
            cases,
            corpus_digest: String::new(),
        };
        corpus.corpus_digest = corpus.digest();
        corpus.validate()?;
        Ok(corpus)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SC39_ATTACK_CORPUS_SCHEMA
            || !self.version.is_compatible_with(&SC39_ATTACK_VERSION)
            || self.corpus_id.trim().is_empty()
            || self.source_snapshot.trim().is_empty()
            || self.cases.is_empty()
            || self.cases.len() > MAX_SECURITY_FIXTURE_CASES
        {
            return Err("sc39_attack_corpus_header_invalid".to_owned());
        }
        let mut ids = BTreeSet::new();
        let mut shapes = BTreeSet::new();
        for case in &self.cases {
            case.validate()?;
            if !ids.insert(case.attack_id.clone()) {
                return Err("sc39_attack_case_duplicate".to_owned());
            }
            shapes.insert(case.shape);
        }
        for shape in Sc39AttackShape::ALL {
            if !shapes.contains(shape) {
                return Err("sc39_attack_shape_uncovered".to_owned());
            }
        }
        validate_digest(&self.corpus_digest, "sc39_attack_corpus_digest")?;
        if self.corpus_digest != self.digest() {
            return Err("sc39_attack_corpus_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn total_effect_count(&self) -> u32 {
        self.cases
            .iter()
            .map(|case| case.handler_calls + case.provider_calls + case.effect_count)
            .sum()
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "corpus_id": self.corpus_id,
            "source_snapshot": self.source_snapshot,
            "cases": self.cases,
        }))
    }
}

/// Correlation binding shared by all three corpora.
///
/// A fixture names the request/run/cursor it was recorded against. Correlation is not authority:
/// the binding carries identity and position only, and it can never be used to admit a request.
/// `source_cursor` is `None` for a fixture recorded before any fact was appended.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScFixtureCorrelation {
    pub schema: String,
    pub version: SchemaVersion,
    pub request_id: RequestId,
    pub run_id: Option<RunId>,
    pub operation_id: Option<OperationId>,
    pub source_cursor: Option<u64>,
    pub correlation_digest: String,
}

impl ScFixtureCorrelation {
    pub const SC_FIXTURE_CORRELATION_SCHEMA: &str = "kiana.sc-fixture-correlation.v1";

    pub fn new(
        request_id: RequestId,
        run_id: Option<RunId>,
        operation_id: Option<OperationId>,
        source_cursor: Option<u64>,
    ) -> Result<Self, String> {
        let mut correlation = Self {
            schema: Self::SC_FIXTURE_CORRELATION_SCHEMA.to_owned(),
            version: SC37_DENY_MATRIX_VERSION,
            request_id,
            run_id,
            operation_id,
            source_cursor,
            correlation_digest: String::new(),
        };
        correlation.correlation_digest = correlation.digest();
        correlation.validate()?;
        Ok(correlation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SC_FIXTURE_CORRELATION_SCHEMA
            || !self.version.is_compatible_with(&SC37_DENY_MATRIX_VERSION)
            || self.request_id.as_uuid().is_nil()
        {
            return Err("sc_fixture_correlation_header_invalid".to_owned());
        }
        validate_digest(&self.correlation_digest, "sc_fixture_correlation_digest")?;
        if self.correlation_digest != self.digest() {
            return Err("sc_fixture_correlation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "request_id": self.request_id,
            "run_id": self.run_id,
            "operation_id": self.operation_id,
            "source_cursor": self.source_cursor,
        }))
    }
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
