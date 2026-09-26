//! Deterministic, project-local connector fixtures.
//!
//! A fixture is test input, not an external provider.  The domain owns its schema, bounds,
//! canonical payload matching and receipt projection so that a daemon adapter cannot silently
//! widen the accepted input or claim a physical effect.

use crate::{
    canonical_json, is_sha256_hex, redact_value, valid_extension_identifier,
    ConnectorBindingSnapshot, ProviderOutcome, ProviderReceipt, PROVIDER_RECEIPT_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const CONNECTOR_FIXTURE_SCHEMA: &str = "kiana.connector-fixture.v1";
pub const CONNECTOR_FIXTURE_MAX_BYTES: usize = 1024 * 1024;
pub const CONNECTOR_FIXTURE_MAX_OPERATIONS: usize = 32;
pub const CONNECTOR_FIXTURE_MAX_CASES_PER_OPERATION: usize = 128;
pub const CONNECTOR_FIXTURE_SOURCE: &str = "local_fixture";

/// A bounded fixture loaded from a project-local, hash-pinned file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorFixture {
    pub schema: String,
    pub connector_id: String,
    pub account_id: String,
    pub operations: BTreeMap<String, Vec<ConnectorFixtureCase>>,
    /// A fixture may never represent a real external effect.  The field is accepted only so a
    /// malicious or stale fixture produces a stable denial instead of being ignored.
    #[serde(default, skip_serializing_if = "is_false")]
    pub external_effect: bool,
}

/// One deterministic input/output case for a registered connector operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorFixtureCase {
    pub payload: Value,
    pub outcome: ProviderOutcome,
    pub receipt_id: String,
    pub result: Value,
    /// See [`ConnectorFixture::external_effect`].  `true` is always rejected.
    #[serde(default, skip_serializing_if = "is_false")]
    pub external_effect: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl ConnectorFixture {
    /// Decode a fixture after enforcing the file-size boundary.  Binding identity is checked by
    /// [`Self::validate_for_binding`] because the bytes are not trusted until a binding is known.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > CONNECTOR_FIXTURE_MAX_BYTES {
            return Err("connector_fixture_size_exceeded".to_owned());
        }
        let fixture: Self =
            serde_json::from_slice(bytes).map_err(|_| "connector_fixture_invalid".to_owned())?;
        fixture.validate_shape()?;
        Ok(fixture)
    }

    pub fn validate_for_binding(&self, binding: &ConnectorBindingSnapshot) -> Result<(), String> {
        self.validate_shape()?;
        if self.connector_id != binding.definition.connector_id
            || self.account_id != binding.binding.account_id
        {
            return Err("connector_fixture_identity_mismatch".to_owned());
        }
        for operation in binding.definition.operations.keys() {
            if !self.operations.contains_key(operation) {
                return Err("connector_fixture_operation_missing".to_owned());
            }
        }
        if self
            .operations
            .keys()
            .any(|operation| !binding.definition.operations.contains_key(operation))
        {
            return Err("connector_fixture_operation_unregistered".to_owned());
        }
        Ok(())
    }

    /// Match by canonical JSON so object-key ordering cannot select a different case.
    pub fn find_case(
        &self,
        operation: &str,
        payload: &Value,
    ) -> Result<&ConnectorFixtureCase, String> {
        let cases = self
            .operations
            .get(operation)
            .ok_or_else(|| "connector_fixture_operation_missing".to_owned())?;
        let digest = connector_payload_sha256(payload);
        cases
            .iter()
            .find(|case| {
                connector_payload_sha256(&case.payload) == digest
                    && canonical_json(case.payload.clone()) == canonical_json(payload.clone())
            })
            .ok_or_else(|| "connector_fixture_payload_mismatch".to_owned())
    }

    /// Build the same receipt for the same binding, operation, key and canonical payload.
    pub fn provider_receipt(
        &self,
        binding: &ConnectorBindingSnapshot,
        operation: &str,
        idempotency_key: &str,
        payload: &Value,
    ) -> Result<ProviderReceipt, String> {
        self.validate_for_binding(binding)?;
        binding.operation(operation).map_err(str::to_owned)?;
        if idempotency_key.trim().is_empty()
            || idempotency_key.len() > 128
            || idempotency_key.chars().any(char::is_control)
        {
            return Err("connector_idempotency_key_invalid".to_owned());
        }
        let case = self.find_case(operation, payload)?;
        if case.external_effect || self.external_effect {
            return Err("connector_fixture_external_effect_denied".to_owned());
        }
        if !valid_extension_identifier(&case.receipt_id) {
            return Err("connector_fixture_invalid".to_owned());
        }
        let receipt = ProviderReceipt {
            schema: PROVIDER_RECEIPT_SCHEMA.to_owned(),
            connector_id: binding.definition.connector_id.clone(),
            binding_id: binding.binding.binding_id.clone(),
            account_id: binding.binding.account_id.clone(),
            operation: operation.to_owned(),
            idempotency_key: idempotency_key.to_owned(),
            final_payload_sha256: connector_payload_sha256(payload),
            provider_receipt_id: case.receipt_id.clone(),
            outcome: case.outcome,
            source: CONNECTOR_FIXTURE_SOURCE.to_owned(),
            result: redact_value(&case.result),
        };
        receipt.validate()?;
        Ok(receipt)
    }

    fn validate_shape(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_FIXTURE_SCHEMA {
            return Err("connector_fixture_schema_unsupported".to_owned());
        }
        if self.external_effect {
            return Err("connector_fixture_external_effect_denied".to_owned());
        }
        if !valid_extension_identifier(&self.connector_id)
            || !valid_extension_identifier(&self.account_id)
        {
            return Err("connector_fixture_identity_mismatch".to_owned());
        }
        if self.operations.is_empty() || self.operations.len() > CONNECTOR_FIXTURE_MAX_OPERATIONS {
            return Err("connector_fixture_operation_limit".to_owned());
        }
        if self.operations.values().any(|cases| {
            cases.is_empty() || cases.len() > CONNECTOR_FIXTURE_MAX_CASES_PER_OPERATION
        }) {
            return Err("connector_fixture_case_limit".to_owned());
        }
        for (operation, cases) in &self.operations {
            if !valid_extension_identifier(operation) {
                return Err("connector_fixture_invalid".to_owned());
            }
            let mut payload_digests = std::collections::BTreeSet::new();
            for case in cases {
                if case.external_effect {
                    return Err("connector_fixture_external_effect_denied".to_owned());
                }
                if !valid_extension_identifier(&case.receipt_id)
                    || !payload_digests.insert(connector_payload_sha256(&case.payload))
                {
                    return Err(if !valid_extension_identifier(&case.receipt_id) {
                        "connector_fixture_invalid".to_owned()
                    } else {
                        "connector_fixture_payload_duplicate".to_owned()
                    });
                }
            }
        }
        Ok(())
    }
}

/// SHA-256 of the canonical compact payload bytes.  The digest is deliberately unprefixed to
/// match the existing ProviderReceipt and EffectObservation contract.
pub fn connector_payload_sha256(payload: &Value) -> String {
    let canonical = canonical_json(payload.clone());
    let bytes = serde_json::to_vec(&canonical).expect("JSON values are serializable");
    format!("{:x}", Sha256::digest(bytes))
}

/// SHA-256 of the exact fixture bytes read from disk.
pub fn connector_fixture_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Accept the historical `sha256:<hex>` spelling for existing connector binding snapshots.
pub fn connector_fixture_hash_valid(value: &str) -> bool {
    is_sha256_hex(value) || value.strip_prefix("sha256:").is_some_and(is_sha256_hex)
}

pub fn connector_fixture_hash_matches(expected: &str, bytes: &[u8]) -> bool {
    let actual = connector_fixture_sha256(bytes);
    expected == actual || expected.strip_prefix("sha256:") == Some(actual.as_str())
}
