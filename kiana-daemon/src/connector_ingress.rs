//! INT-24 signed Webhook/A2A ingress verifier.
//!
//! The verifier owns source keys and replay memory, then returns an occurrence for the existing
//! ControlPlane/workflow route. It never appends an EventLog fact or invokes a connector itself.

use kiana_domain::{ConnectorIngressEvent, ConnectorIngressOccurrence, ConnectorIngressPolicy};
use kiana_ports::PortError;
use ring::hmac;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

#[derive(Clone)]
struct SourceVerifier {
    policy: ConnectorIngressPolicy,
    key: Arc<hmac::Key>,
}

/// Server-owned source/key allowlist with source+event idempotent dedupe.
#[derive(Clone, Default)]
pub struct ConnectorIngressVerifier {
    sources: Arc<BTreeMap<String, SourceVerifier>>,
    seen: Arc<Mutex<BTreeMap<String, String>>>,
}

impl std::fmt::Debug for ConnectorIngressVerifier {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConnectorIngressVerifier")
            .field("source_count", &self.sources.len())
            .finish_non_exhaustive()
    }
}

impl ConnectorIngressVerifier {
    pub fn new(
        entries: impl IntoIterator<Item = (ConnectorIngressPolicy, Vec<u8>)>,
    ) -> Result<Self, PortError> {
        let mut sources = BTreeMap::new();
        for (policy, secret) in entries {
            policy.validate().map_err(PortError::Failed)?;
            if secret.len() < 16 || secret.len() > 4_096 {
                return Err(PortError::Failed(
                    "connector_ingress_signing_key_invalid".to_owned(),
                ));
            }
            if sources.contains_key(&policy.source_id) {
                return Err(PortError::Conflict(
                    "connector_ingress_source_duplicate".to_owned(),
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

    /// Verify source/tenant/payload policy, HMAC signature, timestamp and nonce-bound occurrence,
    /// then deduplicate by source plus event ID. A same-digest replay returns `Ok(None)`.
    pub fn verify(
        &self,
        event: ConnectorIngressEvent,
        now_unix_ms: u64,
    ) -> Result<Option<ConnectorIngressOccurrence>, PortError> {
        event.validate().map_err(PortError::Failed)?;
        let source = self.sources.get(&event.source_id).ok_or_else(|| {
            PortError::Conflict("connector_ingress_source_not_allowlisted".to_owned())
        })?;
        source
            .policy
            .matches(&event, now_unix_ms)
            .map_err(PortError::Conflict)?;
        let signature = decode_hex(&event.signature)?;
        hmac::verify(&source.key, event.signing_digest().as_bytes(), &signature)
            .map_err(|_| PortError::Conflict("connector_ingress_signature_invalid".to_owned()))?;
        let occurrence_key = event.occurrence_key();
        let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(previous) = seen.get(&occurrence_key) {
            if previous == &event.ingress_digest {
                return Ok(None);
            }
            return Err(PortError::Conflict(
                "connector_ingress_dedupe_payload_conflict".to_owned(),
            ));
        }
        let occurrence = ConnectorIngressOccurrence::from_verified(&event, &source.policy)
            .map_err(PortError::Failed)?;
        seen.insert(occurrence_key, event.ingress_digest);
        Ok(Some(occurrence))
    }
}

fn decode_hex(value: &str) -> Result<Vec<u8>, PortError> {
    if value.len() != 64 {
        return Err(PortError::Failed(
            "connector_ingress_signature_encoding_invalid".to_owned(),
        ));
    }
    let mut output = Vec::with_capacity(32);
    for pair in value.as_bytes().chunks_exact(2) {
        let high = hex_digit(pair[0]).ok_or_else(|| {
            PortError::Failed("connector_ingress_signature_encoding_invalid".to_owned())
        })?;
        let low = hex_digit(pair[1]).ok_or_else(|| {
            PortError::Failed("connector_ingress_signature_encoding_invalid".to_owned())
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
