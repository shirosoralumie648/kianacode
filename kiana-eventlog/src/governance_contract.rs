//! EventLog checks for data-governance propagation facts.
//!
//! Governance receipts are ordinary immutable RuntimeEvent facts. The adapter validates their
//! typed envelope before CAS/idempotency/write and never edits or removes the source event.

use kiana_domain::{DataPropagationReceipt, ImmutableEventSeal, RuntimeEvent};

pub(crate) fn validate_runtime_event(event: &RuntimeEvent) -> Result<(), String> {
    match event.kind.as_str() {
        "data.propagation_receipt" => {
            let receipt: DataPropagationReceipt = serde_json::from_value(event.data.clone())
                .map_err(|_| "eventlog_data_propagation_payload_invalid".to_owned())?;
            receipt
                .validate()
                .map_err(|error| format!("eventlog_data_propagation_receipt:{error}"))?;
            if event.data_epoch != Some(receipt.data_epoch) {
                return Err("eventlog_data_propagation_epoch_mismatch".to_owned());
            }
        }
        "data.immutable_event_seal" => {
            let seal: ImmutableEventSeal = serde_json::from_value(event.data.clone())
                .map_err(|_| "eventlog_immutable_event_seal_payload_invalid".to_owned())?;
            seal.validate()
                .map_err(|error| format!("eventlog_immutable_event_seal:{error}"))?;
            if event.payload_recoverable != Some(false) || !event.artifact_refs.is_empty() {
                return Err("eventlog_immutable_event_seal_payload_boundary".to_owned());
            }
            if event.data_epoch != Some(seal.data_epoch) {
                return Err("eventlog_immutable_event_seal_epoch_mismatch".to_owned());
            }
        }
        _ => {}
    }
    Ok(())
}
