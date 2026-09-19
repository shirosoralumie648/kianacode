//! Signed workflow event ingress verification.
//!
//! This adapter verifies an allowlisted source before it creates an occurrence. It never appends
//! an EventLog fact or calls ControlPlane itself; the caller must persist the accepted ingress and
//! route the resulting occurrence through the existing workflow command path.

use kiana_domain::{WorkflowEventIngress, WorkflowEventOccurrence, WorkflowEventSourcePolicy};
use kiana_ports::PortError;
use ring::hmac;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

#[derive(Clone)]
struct SourceVerifier {
    policy: WorkflowEventSourcePolicy,
    key: Arc<hmac::Key>,
}

/// Server-owned source/key allowlist with idempotent event dedupe.
#[derive(Clone, Default)]
pub struct WorkflowEventVerifier {
    sources: Arc<BTreeMap<String, SourceVerifier>>,
    seen: Arc<Mutex<BTreeMap<String, String>>>,
}

impl std::fmt::Debug for WorkflowEventVerifier {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkflowEventVerifier")
            .field("source_count", &self.sources.len())
            .finish_non_exhaustive()
    }
}

impl WorkflowEventVerifier {
    pub fn new(
        entries: impl IntoIterator<Item = (WorkflowEventSourcePolicy, Vec<u8>)>,
    ) -> Result<Self, PortError> {
        let mut sources = BTreeMap::new();
        for (policy, secret) in entries {
            policy.validate().map_err(PortError::Failed)?;
            if secret.len() < 16 || secret.len() > 4_096 {
                return Err(PortError::Failed(
                    "workflow_event_signing_key_invalid".to_owned(),
                ));
            }
            if sources.contains_key(&policy.source_id) {
                return Err(PortError::Conflict(
                    "workflow_event_source_duplicate".to_owned(),
                ));
            }
            sources.insert(
                policy.source_id.clone(),
                SourceVerifier {
                    policy,
                    key: Arc::new(hmac::Key::new(hmac::HMAC_SHA256, &secret)),
                },
            );
        }
        Ok(Self {
            sources: Arc::new(sources),
            seen: Arc::new(Mutex::new(BTreeMap::new())),
        })
    }

    /// Verify signature, source/project/event filter and timestamp, then deduplicate by source
    /// plus event ID. A same-digest replay returns `Ok(None)` and never creates a second key.
    pub fn verify(
        &self,
        event: WorkflowEventIngress,
        now_unix_ms: u64,
    ) -> Result<Option<WorkflowEventOccurrence>, PortError> {
        event.validate().map_err(PortError::Failed)?;
        let source = self.sources.get(&event.source_id).ok_or_else(|| {
            PortError::Conflict("workflow_event_source_not_allowlisted".to_owned())
        })?;
        source
            .policy
            .matches(&event, now_unix_ms)
            .map_err(PortError::Conflict)?;
        let signature = decode_hex(&event.signature)?;
        hmac::verify(&source.key, event.signing_digest().as_bytes(), &signature)
            .map_err(|_| PortError::Conflict("workflow_event_signature_invalid".to_owned()))?;
        let occurrence_key = event.occurrence_key();
        let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(previous) = seen.get(&occurrence_key) {
            if previous == &event.ingress_digest {
                return Ok(None);
            }
            return Err(PortError::Conflict(
                "workflow_event_dedupe_payload_conflict".to_owned(),
            ));
        }
        let occurrence = WorkflowEventOccurrence::from_verified(&event, &source.policy)
            .map_err(PortError::Failed)?;
        seen.insert(occurrence_key, event.ingress_digest);
        Ok(Some(occurrence))
    }
}

fn decode_hex(value: &str) -> Result<Vec<u8>, PortError> {
    if value.len() != 64 {
        return Err(PortError::Failed(
            "workflow_event_signature_encoding_invalid".to_owned(),
        ));
    }
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(32);
    for pair in bytes.chunks_exact(2) {
        let high = hex_digit(pair[0]).ok_or_else(|| {
            PortError::Failed("workflow_event_signature_encoding_invalid".to_owned())
        })?;
        let low = hex_digit(pair[1]).ok_or_else(|| {
            PortError::Failed("workflow_event_signature_encoding_invalid".to_owned())
        })?;
        output.push((high << 4) | low);
    }
    Ok(output)
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}
