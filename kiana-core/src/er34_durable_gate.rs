//! Core read-only facade for ER-34 durable-gate evidence.

use kiana_domain::Er34DurableGateEvidence;

pub fn validate_er34_durable_gate_evidence(
    evidence: &Er34DurableGateEvidence,
) -> Result<(), String> {
    evidence.validate()
}
