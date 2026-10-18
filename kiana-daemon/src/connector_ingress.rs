//! INT-24 signed Webhook/A2A ingress verifier.
//!
//! The verifier owns source keys and the replay window, then returns an occurrence for the
//! existing ControlPlane/workflow route. It never appends an EventLog fact or invokes a connector
//! itself.
//!
//! ```text
//! untrusted ConnectorIngressEvent + server now
//!     -> structure / source allowlist / tenant / payload / clock policy
//!     -> HMAC-SHA256 over signing_digest (constant time)
//!     -> occurrence (digest-only projection)
//!     -> replay window: source+event_id and source+nonce
//!     -> Some(occurrence) | None (idempotent replay) | stable conflict
//! ```
//!
//! Two replay backends exist:
//!
//! - [`ConnectorIngressReplayStore`] (durable, cross-process, bounded by time and count). This is
//!   what the production `DaemonHost` route uses.
//! - an in-process `BTreeMap` window used when no store is supplied (tests/embedded callers). It
//!   is pruned by time but still lost on restart, and is documented as a limitation rather than
//!   being presented as durable.

use kiana_domain::{
    ConnectorIngressEvent, ConnectorIngressOccurrence, ConnectorIngressPolicy,
    ConnectorIngressReplayEntry, ConnectorIngressReplayOutcome,
    CONNECTOR_INGRESS_REPLAY_DEFAULT_WINDOW_MS, CONNECTOR_INGRESS_REPLAY_MAX_ENTRIES,
    CONNECTOR_INGRESS_REPLAY_MAX_WINDOW_MS,
};
use kiana_ports::{ConnectorIngressReplayStore, PortError};
use ring::hmac;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

#[derive(Clone)]
struct SourceVerifier {
    policy: ConnectorIngressPolicy,
    key: Arc<hmac::Key>,
}

/// One retained in-process identity. `accepted_at_unix_ms` bounds the in-memory window in time.
#[derive(Clone)]
struct SeenEntry {
    ingress_digest: String,
    nonce: String,
    source_id: String,
    accepted_at_unix_ms: u64,
}

/// Server-owned source/key allowlist with source+event and source+nonce idempotent replay
/// protection.
#[derive(Clone)]
pub struct ConnectorIngressVerifier {
    sources: Arc<BTreeMap<String, SourceVerifier>>,
    /// Server-derived ingress audience per source. The inbound token audience must differ from
    /// the downstream connector audience; this value is never taken from the wire.
    audiences: Arc<BTreeMap<String, String>>,
    seen: Arc<Mutex<BTreeMap<String, SeenEntry>>>,
    replay: Option<Arc<dyn ConnectorIngressReplayStore>>,
    replay_window_ms: u64,
}

impl std::fmt::Debug for ConnectorIngressVerifier {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConnectorIngressVerifier")
            .field("source_count", &self.sources.len())
            .field("durable_replay", &self.replay.is_some())
            .field("replay_window_ms", &self.replay_window_ms)
            .finish_non_exhaustive()
    }
}

impl Default for ConnectorIngressVerifier {
    fn default() -> Self {
        Self {
            sources: Arc::new(BTreeMap::new()),
            audiences: Arc::new(BTreeMap::new()),
            seen: Arc::new(Mutex::new(BTreeMap::new())),
            replay: None,
            replay_window_ms: CONNECTOR_INGRESS_REPLAY_DEFAULT_WINDOW_MS,
        }
    }
}

impl ConnectorIngressVerifier {
    /// In-process verifier. Replay memory is bounded by the default window but is not durable.
    pub fn new(
        entries: impl IntoIterator<Item = (ConnectorIngressPolicy, Vec<u8>)>,
    ) -> Result<Self, PortError> {
        Self::build(entries, None, CONNECTOR_INGRESS_REPLAY_DEFAULT_WINDOW_MS)
    }

    /// Verifier backed by a durable, cross-process replay store.
    ///
    /// This is the production constructor: the daemon route admits every occurrence through
    /// `store.admit` before returning it, so a restart or a second process cannot accept the same
    /// occurrence again.
    pub fn new_durable(
        entries: impl IntoIterator<Item = (ConnectorIngressPolicy, Vec<u8>)>,
        store: Arc<dyn ConnectorIngressReplayStore>,
    ) -> Result<Self, PortError> {
        Self::build(
            entries,
            Some(store),
            CONNECTOR_INGRESS_REPLAY_DEFAULT_WINDOW_MS,
        )
    }

    /// Build with an explicit replay window (time bound only; the entry-count bound is fixed by
    /// [`kiana_domain::CONNECTOR_INGRESS_REPLAY_MAX_ENTRIES`]).
    pub fn new_durable_with_window(
        entries: impl IntoIterator<Item = (ConnectorIngressPolicy, Vec<u8>)>,
        store: Arc<dyn ConnectorIngressReplayStore>,
        replay_window_ms: u64,
    ) -> Result<Self, PortError> {
        Self::build(entries, Some(store), replay_window_ms)
    }

    fn build(
        entries: impl IntoIterator<Item = (ConnectorIngressPolicy, Vec<u8>)>,
        replay: Option<Arc<dyn ConnectorIngressReplayStore>>,
        replay_window_ms: u64,
    ) -> Result<Self, PortError> {
        if replay_window_ms == 0 || replay_window_ms > CONNECTOR_INGRESS_REPLAY_MAX_WINDOW_MS {
            return Err(PortError::Failed(
                "connector_ingress_replay_window_invalid".to_owned(),
            ));
        }
        let mut sources = BTreeMap::new();
        let mut audiences = BTreeMap::new();
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
            audiences.insert(
                policy.source_id.clone(),
                format!("kiana-ingress:{}", policy.source_id),
            );
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
            audiences: Arc::new(audiences),
            seen: Arc::new(Mutex::new(BTreeMap::new())),
            replay,
            replay_window_ms,
        })
    }

    /// Override the server-owned ingress audience for one allowlisted source.
    ///
    /// The audience describes the token that authenticated the ingress, not a connector
    /// credential. `ConnectorIngressIntent::from_verified_occurrence` refuses an audience equal to
    /// the downstream connector audience, so the inbound token can never be reused.
    pub fn with_ingress_audience(
        mut self,
        source_id: &str,
        audience: impl Into<String>,
    ) -> Result<Self, PortError> {
        let audience = audience.into();
        if !self.sources.contains_key(source_id) {
            return Err(PortError::Conflict(
                "connector_ingress_source_not_allowlisted".to_owned(),
            ));
        }
        if audience.trim().is_empty()
            || audience.len() > kiana_domain::CONNECTOR_INGRESS_INTENT_MAX_AUDIENCE_BYTES
        {
            return Err(PortError::Failed(
                "connector_ingress_audience_invalid".to_owned(),
            ));
        }
        Arc::make_mut(&mut self.audiences).insert(source_id.to_owned(), audience);
        Ok(self)
    }

    /// Server-owned ingress audience for an allowlisted source.
    pub fn ingress_audience(&self, source_id: &str) -> Option<&str> {
        self.audiences.get(source_id).map(String::as_str)
    }

    /// Whether occurrences are admitted through a durable replay store.
    pub fn has_durable_replay(&self) -> bool {
        self.replay.is_some()
    }

    /// Configured replay window.
    pub fn replay_window_ms(&self) -> u64 {
        self.replay_window_ms
    }

    /// Server clock used by synchronous callers is still the caller's responsibility; see
    /// [`ConnectorIngressVerifier::verify`].
    ///
    /// Verify source/tenant/payload policy, HMAC signature, wall-clock skew and replay identity.
    ///
    /// `now_unix_ms` must be the server clock at admission time. A same-digest replay returns
    /// `Ok(None)`. A different payload for the same source/event is a conflict, and a nonce reused
    /// by another event of the same source inside the window is a replay.
    ///
    /// This synchronous entry point uses only the in-process window even if the verifier was
    /// built with a durable store; use [`ConnectorIngressVerifier::verify_durable`] on the
    /// production path so replay evidence survives a restart.
    pub fn verify(
        &self,
        event: ConnectorIngressEvent,
        now_unix_ms: u64,
    ) -> Result<Option<ConnectorIngressOccurrence>, PortError> {
        let occurrence = self.authenticate(&event, now_unix_ms)?;
        if self.admit_in_memory(&occurrence, now_unix_ms)? {
            Ok(Some(occurrence))
        } else {
            Ok(None)
        }
    }

    /// Durable verification path used by the `DaemonHost` ingress route.
    ///
    /// Deny-first order is fixed: structure, source allowlist, tenant/payload/clock policy, HMAC,
    /// occurrence construction, then the replay store. No occurrence is returned and no intent
    /// can be derived unless the replay store durably retained it.
    pub async fn verify_durable(
        &self,
        event: ConnectorIngressEvent,
        now_unix_ms: u64,
    ) -> Result<Option<ConnectorIngressOccurrence>, PortError> {
        let occurrence = self.authenticate(&event, now_unix_ms)?;
        let Some(store) = self.replay.as_ref() else {
            return if self.admit_in_memory(&occurrence, now_unix_ms)? {
                Ok(Some(occurrence))
            } else {
                Ok(None)
            };
        };
        let entry =
            ConnectorIngressReplayEntry::new(&occurrence, now_unix_ms, self.replay_window_ms)
                .map_err(PortError::Failed)?;
        match store.admit(entry, now_unix_ms).await? {
            ConnectorIngressReplayOutcome::Accepted => Ok(Some(occurrence)),
            ConnectorIngressReplayOutcome::Duplicate => Ok(None),
            ConnectorIngressReplayOutcome::EventConflict => Err(PortError::Conflict(
                "connector_ingress_dedupe_payload_conflict".to_owned(),
            )),
            ConnectorIngressReplayOutcome::NonceReplay => Err(PortError::Conflict(
                "connector_ingress_nonce_replay".to_owned(),
            )),
        }
    }

    /// Read the live replay window for one source (audit/inspection only).
    pub async fn replay_window(
        &self,
        source_id: &str,
        now_unix_ms: u64,
        limit: usize,
    ) -> Result<Vec<ConnectorIngressReplayEntry>, PortError> {
        let Some(store) = self.replay.as_ref() else {
            return Err(PortError::Unavailable(
                "connector_ingress_durable_replay_unavailable".to_owned(),
            ));
        };
        store.window(source_id, now_unix_ms, limit).await
    }

    fn authenticate(
        &self,
        event: &ConnectorIngressEvent,
        now_unix_ms: u64,
    ) -> Result<ConnectorIngressOccurrence, PortError> {
        event.validate().map_err(PortError::Failed)?;
        let source = self.sources.get(&event.source_id).ok_or_else(|| {
            PortError::Conflict("connector_ingress_source_not_allowlisted".to_owned())
        })?;
        // The clock check is only real because `now_unix_ms` is the server clock, never the
        // attacker-controlled `occurred_at_unix_ms`.
        source
            .policy
            .matches(event, now_unix_ms)
            .map_err(PortError::Conflict)?;
        let signature = decode_hex(&event.signature)?;
        hmac::verify(&source.key, event.signing_digest().as_bytes(), &signature)
            .map_err(|_| PortError::Conflict("connector_ingress_signature_invalid".to_owned()))?;
        let occurrence =
            ConnectorIngressOccurrence::from_verified(event, &source.policy, now_unix_ms)
                .map_err(PortError::Failed)?;
        Ok(occurrence)
    }

    fn admit_in_memory(
        &self,
        occurrence: &ConnectorIngressOccurrence,
        now_unix_ms: u64,
    ) -> Result<bool, PortError> {
        let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        seen.retain(|_, entry| {
            now_unix_ms.saturating_sub(entry.accepted_at_unix_ms) < self.replay_window_ms
        });
        if let Some(previous) = seen.get(&occurrence.occurrence_key) {
            if previous.ingress_digest == occurrence.ingress_digest {
                return Ok(false);
            }
            return Err(PortError::Conflict(
                "connector_ingress_dedupe_payload_conflict".to_owned(),
            ));
        }
        if seen
            .values()
            .any(|entry| entry.nonce == occurrence.nonce && entry.source_id == occurrence.source_id)
        {
            return Err(PortError::Conflict(
                "connector_ingress_nonce_replay".to_owned(),
            ));
        }
        if seen.len() >= CONNECTOR_INGRESS_REPLAY_MAX_ENTRIES {
            return Err(PortError::Failed(
                "connector_ingress_replay_capacity_exceeded".to_owned(),
            ));
        }
        seen.insert(
            occurrence.occurrence_key.clone(),
            SeenEntry {
                ingress_digest: occurrence.ingress_digest.clone(),
                nonce: occurrence.nonce.clone(),
                source_id: occurrence.source_id.clone(),
                accepted_at_unix_ms: now_unix_ms,
            },
        );
        Ok(true)
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
