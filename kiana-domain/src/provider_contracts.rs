//! Offline provider contract matrices, cassette metadata and deterministic fault corpus.
//!
//! These values describe what an adapter claims and how an offline fixture is allowed to run.
//! They do not perform a provider request, open a socket, grant a model permit or turn a fake
//! response into live evidence.  The provider crate consumes the contracts at its existing
//! prepare/decode boundaries; ControlPlane and the Broker remain the only effect authorities.

use crate::{
    canonical_journal_bytes, json_digest, redact_text, CapabilitySupport, ModelProtocol,
    SchemaVersion,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const PROVIDER_CONTRACT_MATRIX_SCHEMA: &str = "kiana.provider-contract-matrix.v1";
pub const PROVIDER_CONTRACT_CELL_SCHEMA: &str = "kiana.provider-contract-cell.v1";
pub const PROVIDER_CASSETTE_SCHEMA: &str = "kiana.provider-cassette.v1";
pub const PROVIDER_FAULT_CASE_SCHEMA: &str = "kiana.provider-fault-case.v1";
pub const PROVIDER_FAULT_CORPUS_SCHEMA: &str = "kiana.provider-fault-corpus.v1";
pub const PROVIDER_CONTRACT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_MATRIX_CELLS: usize = 128;
const MAX_FIXTURE_IDS: usize = 16;
const MAX_FAULT_CASES: usize = 32;

fn bounded(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
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

fn error_code(value: &str, field: &str) -> Result<(), String> {
    bounded(value, field, 128)?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn redacted(value: &str, field: &str, max: usize) -> Result<(), String> {
    bounded(value, field, max)?;
    if redact_text(value) != value {
        return Err(format!("{field}_contains_secret"));
    }
    Ok(())
}

fn clear_digest<T: Serialize>(value: &T, field: &str) -> String {
    let mut value = serde_json::to_value(value).unwrap_or(serde_json::Value::Null);
    if let Some(object) = value.as_object_mut() {
        object.insert(field.to_owned(), serde_json::Value::String(String::new()));
    }
    json_digest(&value)
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderContractCapability {
    Text,
    Tools,
    Streaming,
    Usage,
    Refusal,
    Length,
    Reasoning,
    Structured,
    Image,
}

impl ProviderContractCapability {
    pub const ALL: [Self; 9] = [
        Self::Text,
        Self::Tools,
        Self::Streaming,
        Self::Usage,
        Self::Refusal,
        Self::Length,
        Self::Reasoning,
        Self::Structured,
        Self::Image,
    ];

    pub fn model_support(self, capabilities: &crate::ModelCapabilities) -> CapabilitySupport {
        match self {
            Self::Text | Self::Usage | Self::Refusal | Self::Length => CapabilitySupport::Supported,
            Self::Tools => capabilities.tools,
            Self::Streaming => capabilities.streaming,
            Self::Reasoning => capabilities.reasoning_replay,
            Self::Structured => capabilities.structured_output,
            Self::Image => capabilities.images,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderContractSupport {
    Supported,
    Unsupported,
    Unknown,
}

impl ProviderContractSupport {
    pub fn from_model_support(value: CapabilitySupport) -> Self {
        match value {
            CapabilitySupport::Supported => Self::Supported,
            CapabilitySupport::Unsupported => Self::Unsupported,
            CapabilitySupport::Unknown => Self::Unknown,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderContractCell {
    pub schema: String,
    pub protocol: ModelProtocol,
    pub capability: ProviderContractCapability,
    pub support: ProviderContractSupport,
    pub fixture_ids: Vec<String>,
    pub preflight_error: Option<String>,
}

impl ProviderContractCell {
    pub fn new(
        protocol: ModelProtocol,
        capability: ProviderContractCapability,
        support: ProviderContractSupport,
        fixture_ids: Vec<String>,
        preflight_error: Option<String>,
    ) -> Result<Self, String> {
        let cell = Self {
            schema: PROVIDER_CONTRACT_CELL_SCHEMA.to_owned(),
            protocol,
            capability,
            support,
            fixture_ids,
            preflight_error,
        };
        cell.validate()?;
        Ok(cell)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_CONTRACT_CELL_SCHEMA || self.fixture_ids.len() > MAX_FIXTURE_IDS
        {
            return Err("provider_contract_cell_header_invalid".to_owned());
        }
        let mut fixture_ids = BTreeSet::new();
        for fixture_id in &self.fixture_ids {
            redacted(fixture_id, "provider_contract_fixture_id", 128)?;
            if !fixture_ids.insert(fixture_id) {
                return Err("provider_contract_fixture_duplicate".to_owned());
            }
        }
        if let Some(error) = &self.preflight_error {
            error_code(error, "provider_contract_preflight_error")?;
        }
        match self.support {
            ProviderContractSupport::Supported => {
                if self.fixture_ids.is_empty() || self.preflight_error.is_some() {
                    return Err("provider_contract_supported_cell_incomplete".to_owned());
                }
            }
            ProviderContractSupport::Unsupported => {
                if !self.fixture_ids.is_empty() || self.preflight_error.is_none() {
                    return Err("provider_contract_unsupported_cell_incomplete".to_owned());
                }
            }
            ProviderContractSupport::Unknown => {
                if !self.fixture_ids.is_empty() || self.preflight_error.is_some() {
                    return Err("provider_contract_unknown_cell_claimed".to_owned());
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderContractMatrix {
    pub schema: String,
    pub version: SchemaVersion,
    pub adapter_id: String,
    pub source_revision: String,
    pub cells: Vec<ProviderContractCell>,
    pub matrix_digest: String,
}

impl ProviderContractMatrix {
    pub fn new(
        adapter_id: impl Into<String>,
        source_revision: impl Into<String>,
        cells: Vec<ProviderContractCell>,
    ) -> Result<Self, String> {
        let mut matrix = Self {
            schema: PROVIDER_CONTRACT_MATRIX_SCHEMA.to_owned(),
            version: PROVIDER_CONTRACT_VERSION,
            adapter_id: adapter_id.into(),
            source_revision: source_revision.into(),
            cells,
            matrix_digest: String::new(),
        };
        matrix.matrix_digest = matrix.digest();
        matrix.validate()?;
        Ok(matrix)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_CONTRACT_MATRIX_SCHEMA
            || self.version != PROVIDER_CONTRACT_VERSION
        {
            return Err("provider_contract_matrix_header_invalid".to_owned());
        }
        redacted(&self.adapter_id, "provider_contract_adapter_id", 128)?;
        redacted(
            &self.source_revision,
            "provider_contract_source_revision",
            256,
        )?;
        if self.cells.is_empty() || self.cells.len() > MAX_MATRIX_CELLS {
            return Err("provider_contract_matrix_cell_limit".to_owned());
        }
        for (index, cell) in self.cells.iter().enumerate() {
            cell.validate()?;
            if self.cells[..index].iter().any(|previous| {
                previous.protocol == cell.protocol && previous.capability == cell.capability
            }) {
                return Err("provider_contract_matrix_duplicate_cell".to_owned());
            }
        }
        let protocols = self.cells.iter().map(|cell| cell.protocol).fold(
            Vec::new(),
            |mut protocols, protocol| {
                if !protocols.contains(&protocol) {
                    protocols.push(protocol);
                }
                protocols
            },
        );
        for protocol in protocols {
            if ProviderContractCapability::ALL
                .iter()
                .any(|capability| self.cell(protocol, *capability).is_none())
            {
                return Err("provider_contract_matrix_coverage_incomplete".to_owned());
            }
        }
        digest(&self.matrix_digest, "provider_contract_matrix_digest")?;
        if self.matrix_digest != self.digest() {
            return Err("provider_contract_matrix_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn cell(
        &self,
        protocol: ModelProtocol,
        capability: ProviderContractCapability,
    ) -> Option<&ProviderContractCell> {
        self.cells
            .iter()
            .find(|cell| cell.protocol == protocol && cell.capability == capability)
    }

    pub fn unsupported_preflight_error(
        &self,
        protocol: ModelProtocol,
        capability: ProviderContractCapability,
    ) -> Option<&str> {
        self.cell(protocol, capability)
            .filter(|cell| cell.support == ProviderContractSupport::Unsupported)
            .and_then(|cell| cell.preflight_error.as_deref())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        clear_digest(self, "matrix_digest")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCassetteMode {
    Record,
    Replay,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCassetteSource {
    Synthetic,
    Captured,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCassette {
    pub schema: String,
    pub version: SchemaVersion,
    pub cassette_id: String,
    pub protocol: ModelProtocol,
    pub mode: ProviderCassetteMode,
    pub source: ProviderCassetteSource,
    pub payload_digest: String,
    pub redaction_profile_digest: String,
    pub external_connection_allowed: bool,
    pub live_opt_in: bool,
    pub cassette_digest: String,
}

impl ProviderCassette {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cassette_id: impl Into<String>,
        protocol: ModelProtocol,
        mode: ProviderCassetteMode,
        source: ProviderCassetteSource,
        payload_digest: impl Into<String>,
        redaction_profile_digest: impl Into<String>,
        external_connection_allowed: bool,
        live_opt_in: bool,
    ) -> Result<Self, String> {
        let mut cassette = Self {
            schema: PROVIDER_CASSETTE_SCHEMA.to_owned(),
            version: PROVIDER_CONTRACT_VERSION,
            cassette_id: cassette_id.into(),
            protocol,
            mode,
            source,
            payload_digest: payload_digest.into(),
            redaction_profile_digest: redaction_profile_digest.into(),
            external_connection_allowed,
            live_opt_in,
            cassette_digest: String::new(),
        };
        cassette.cassette_digest = cassette.digest();
        cassette.validate()?;
        Ok(cassette)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_CASSETTE_SCHEMA || self.version != PROVIDER_CONTRACT_VERSION {
            return Err("provider_cassette_header_invalid".to_owned());
        }
        redacted(&self.cassette_id, "provider_cassette_id", 128)?;
        digest(&self.payload_digest, "provider_cassette_payload_digest")?;
        digest(
            &self.redaction_profile_digest,
            "provider_cassette_redaction_profile_digest",
        )?;
        match (
            self.mode,
            self.source,
            self.external_connection_allowed,
            self.live_opt_in,
        ) {
            (ProviderCassetteMode::Replay, _, false, false) => {}
            (ProviderCassetteMode::Replay, _, true, _) => {
                return Err("provider_replay_external_connection_forbidden".to_owned())
            }
            (ProviderCassetteMode::Replay, _, _, true) => {
                return Err("provider_replay_live_opt_in_forbidden".to_owned())
            }
            (ProviderCassetteMode::Record, ProviderCassetteSource::Captured, true, true) => {}
            (ProviderCassetteMode::Record, ProviderCassetteSource::Captured, _, _) => {
                return Err("provider_record_requires_explicit_live_opt_in".to_owned())
            }
            (ProviderCassetteMode::Record, ProviderCassetteSource::Synthetic, _, _) => {
                return Err("provider_synthetic_cassette_cannot_record".to_owned())
            }
        }
        digest(&self.cassette_digest, "provider_cassette_digest")?;
        if self.cassette_digest != self.digest() {
            return Err("provider_cassette_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        clear_digest(self, "cassette_digest")
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFaultKind {
    ArbitraryChunking,
    OutOfOrderEvent,
    DuplicateEventId,
    TruncatedFrame,
    OversizedFrame,
    Cancellation,
    IncompleteToolCall,
    MissingUsage,
    UnsupportedCapability,
}

impl ProviderFaultKind {
    pub const ALL: [Self; 9] = [
        Self::ArbitraryChunking,
        Self::OutOfOrderEvent,
        Self::DuplicateEventId,
        Self::TruncatedFrame,
        Self::OversizedFrame,
        Self::Cancellation,
        Self::IncompleteToolCall,
        Self::MissingUsage,
        Self::UnsupportedCapability,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFaultDisposition {
    RejectBeforeSend,
    UnknownNoRetry,
    Cancelled,
    PreserveOutput,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderFaultCase {
    pub schema: String,
    pub version: SchemaVersion,
    pub case_id: String,
    pub protocol: ModelProtocol,
    pub fault: ProviderFaultKind,
    pub seed: u64,
    pub disposition: ProviderFaultDisposition,
    pub expected_error: Option<String>,
    pub expected_attempts: u8,
    pub cassette_id: String,
    pub external_connection_allowed: bool,
    pub case_digest: String,
}

impl ProviderFaultCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        case_id: impl Into<String>,
        protocol: ModelProtocol,
        fault: ProviderFaultKind,
        seed: u64,
        disposition: ProviderFaultDisposition,
        expected_error: Option<String>,
        expected_attempts: u8,
        cassette_id: impl Into<String>,
    ) -> Result<Self, String> {
        let mut case = Self {
            schema: PROVIDER_FAULT_CASE_SCHEMA.to_owned(),
            version: PROVIDER_CONTRACT_VERSION,
            case_id: case_id.into(),
            protocol,
            fault,
            seed,
            disposition,
            expected_error,
            expected_attempts,
            cassette_id: cassette_id.into(),
            external_connection_allowed: false,
            case_digest: String::new(),
        };
        case.case_digest = case.digest();
        case.validate()?;
        Ok(case)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_FAULT_CASE_SCHEMA
            || self.version != PROVIDER_CONTRACT_VERSION
            || self.seed == 0
            || self.external_connection_allowed
            || self.expected_attempts > 1
        {
            return Err("provider_fault_case_header_invalid".to_owned());
        }
        redacted(&self.case_id, "provider_fault_case_id", 128)?;
        redacted(&self.cassette_id, "provider_fault_cassette_id", 128)?;
        if let Some(error) = &self.expected_error {
            error_code(error, "provider_fault_expected_error")?;
        }
        match self.disposition {
            ProviderFaultDisposition::RejectBeforeSend => {
                if self.expected_attempts != 0 || self.expected_error.is_none() {
                    return Err("provider_fault_rejection_expectation_invalid".to_owned());
                }
            }
            ProviderFaultDisposition::UnknownNoRetry => {
                if self.expected_attempts != 1 || self.expected_error.is_none() {
                    return Err("provider_fault_unknown_expectation_invalid".to_owned());
                }
            }
            ProviderFaultDisposition::Cancelled | ProviderFaultDisposition::PreserveOutput => {
                if self.expected_attempts > 1 {
                    return Err("provider_fault_attempt_expectation_invalid".to_owned());
                }
            }
        }
        digest(&self.case_digest, "provider_fault_case_digest")?;
        if self.case_digest != self.digest() {
            return Err("provider_fault_case_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        clear_digest(self, "case_digest")
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderFaultCorpus {
    pub schema: String,
    pub version: SchemaVersion,
    pub corpus_id: String,
    pub source_revision: String,
    pub cases: Vec<ProviderFaultCase>,
    pub corpus_digest: String,
}

impl ProviderFaultCorpus {
    pub fn new(
        corpus_id: impl Into<String>,
        source_revision: impl Into<String>,
        cases: Vec<ProviderFaultCase>,
    ) -> Result<Self, String> {
        let mut corpus = Self {
            schema: PROVIDER_FAULT_CORPUS_SCHEMA.to_owned(),
            version: PROVIDER_CONTRACT_VERSION,
            corpus_id: corpus_id.into(),
            source_revision: source_revision.into(),
            cases,
            corpus_digest: String::new(),
        };
        corpus.corpus_digest = corpus.digest();
        corpus.validate()?;
        Ok(corpus)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_FAULT_CORPUS_SCHEMA
            || self.version != PROVIDER_CONTRACT_VERSION
            || self.cases.is_empty()
            || self.cases.len() > MAX_FAULT_CASES
        {
            return Err("provider_fault_corpus_header_invalid".to_owned());
        }
        redacted(&self.corpus_id, "provider_fault_corpus_id", 128)?;
        redacted(&self.source_revision, "provider_fault_source_revision", 256)?;
        let mut ids = BTreeSet::new();
        let mut kinds = BTreeSet::new();
        for case in &self.cases {
            case.validate()?;
            if !ids.insert(&case.case_id) {
                return Err("provider_fault_case_duplicate".to_owned());
            }
            kinds.insert(case.fault);
        }
        if ProviderFaultKind::ALL
            .iter()
            .any(|kind| !kinds.contains(kind))
        {
            return Err("provider_fault_corpus_coverage_incomplete".to_owned());
        }
        digest(&self.corpus_digest, "provider_fault_corpus_digest")?;
        if self.corpus_digest != self.digest() {
            return Err("provider_fault_corpus_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        clear_digest(self, "corpus_digest")
    }
}
