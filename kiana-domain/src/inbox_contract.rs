//! Versioned acknowledgement facts for Harness input admission and consumption.

use crate::{json_digest, InputId, RunId, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const INPUT_RECEIPT_SCHEMA: &str = "kiana.input-receipt.v1";
pub const INPUT_RECEIPT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputDisposition {
    Accepted,
    Duplicate,
    Claimed,
    Rejected,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputReceipt {
    pub schema: String,
    pub version: SchemaVersion,
    pub input_id: InputId,
    pub run_id: RunId,
    pub source: String,
    pub target: String,
    pub received_sequence: u64,
    pub disposition: InputDisposition,
    pub receipt_digest: String,
}

impl InputReceipt {
    pub fn new(
        input_id: InputId,
        run_id: RunId,
        source: impl Into<String>,
        target: impl Into<String>,
        received_sequence: u64,
        disposition: InputDisposition,
    ) -> Result<Self, String> {
        let mut receipt = Self {
            schema: INPUT_RECEIPT_SCHEMA.to_owned(),
            version: INPUT_RECEIPT_VERSION,
            input_id,
            run_id,
            source: source.into(),
            target: target.into(),
            received_sequence,
            disposition,
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest();
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != INPUT_RECEIPT_SCHEMA
            || self.version != INPUT_RECEIPT_VERSION
            || self.input_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self.source.trim().is_empty()
            || self.target.trim().is_empty()
            || self.received_sequence == 0
            || self.receipt_digest != self.digest()
        {
            return Err("input_receipt_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "input_id": self.input_id,
            "run_id": self.run_id,
            "source": self.source,
            "target": self.target,
            "received_sequence": self.received_sequence,
            "disposition": self.disposition,
        }))
    }
}
