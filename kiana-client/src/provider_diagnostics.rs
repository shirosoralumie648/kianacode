//! Typed client state for the provider diagnostics projection.
//!
//! The client stores a server-owned snapshot and cursor only. It does not infer model state,
//! retry requests, or turn a settings read into a connection test; callers must use an explicit
//! command carrying [`ProviderConnectionTestRequest`] when they want a gateway admission.

use kiana_protocol::{
    ProviderConnectionTestRequest, ProviderDiagnosticsCursor, ProviderDiagnosticsSnapshot,
    ProviderTerminalReplay, PROVIDER_DIAGNOSTICS_PROJECTION_STALE,
};

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ProviderDiagnosticsClientError {
    #[error("provider_diagnostics_snapshot_invalid:{0}")]
    Invalid(String),
    #[error("provider_diagnostics_projection_stale")]
    StaleEpoch,
    #[error("provider_diagnostics_cursor_gap")]
    CursorGap,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProviderDiagnosticsClientState {
    snapshot: Option<ProviderDiagnosticsSnapshot>,
    cursor: Option<ProviderDiagnosticsCursor>,
}

impl Default for ProviderDiagnosticsClientState {
    fn default() -> Self {
        Self {
            snapshot: None,
            cursor: None,
        }
    }
}

impl ProviderDiagnosticsClientState {
    pub fn snapshot(&self) -> Option<&ProviderDiagnosticsSnapshot> {
        self.snapshot.as_ref()
    }

    pub fn cursor(&self) -> Option<&ProviderDiagnosticsCursor> {
        self.cursor.as_ref()
    }

    /// Hydrate only a snapshot from the current authority/config epochs.
    pub fn hydrate(
        &mut self,
        snapshot: ProviderDiagnosticsSnapshot,
        authority_epoch: u64,
        config_epoch: u64,
    ) -> Result<(), ProviderDiagnosticsClientError> {
        snapshot
            .validate_for_epoch(authority_epoch, config_epoch)
            .map_err(|reason| {
                if reason == PROVIDER_DIAGNOSTICS_PROJECTION_STALE {
                    ProviderDiagnosticsClientError::StaleEpoch
                } else {
                    ProviderDiagnosticsClientError::Invalid(reason)
                }
            })?;
        self.cursor = Some(snapshot.cursor.clone());
        self.snapshot = Some(snapshot);
        Ok(())
    }

    /// Replace the projection after a reconnect. Epochs must match; a gap is recovered by
    /// requesting a snapshot rather than replaying a possibly stale model action.
    pub fn reconnect(
        &mut self,
        snapshot: ProviderDiagnosticsSnapshot,
        authority_epoch: u64,
        config_epoch: u64,
        expected_sequence: Option<u64>,
    ) -> Result<(), ProviderDiagnosticsClientError> {
        snapshot
            .validate_for_epoch(authority_epoch, config_epoch)
            .map_err(|reason| {
                if reason == PROVIDER_DIAGNOSTICS_PROJECTION_STALE {
                    ProviderDiagnosticsClientError::StaleEpoch
                } else {
                    ProviderDiagnosticsClientError::Invalid(reason)
                }
            })?;
        if expected_sequence.is_some_and(|sequence| snapshot.cursor.sequence < sequence) {
            self.snapshot = None;
            self.cursor = None;
            return Err(ProviderDiagnosticsClientError::CursorGap);
        }
        self.cursor = Some(snapshot.cursor.clone());
        self.snapshot = Some(snapshot);
        Ok(())
    }

    /// Return the durable terminal receipt, if it is newer than the subscriber cursor.
    pub fn terminal_after(
        &self,
        sequence: u64,
    ) -> Result<Option<ProviderTerminalReplay>, ProviderDiagnosticsClientError> {
        self.snapshot
            .as_ref()
            .ok_or_else(|| {
                ProviderDiagnosticsClientError::Invalid(
                    "provider_diagnostics_snapshot_missing".to_owned(),
                )
            })?
            .terminal_after(sequence)
            .map(|terminal| terminal.cloned())
            .map_err(ProviderDiagnosticsClientError::Invalid)
    }

    /// Settings/catalog views expose this request shape but cannot manufacture an admitted test.
    pub fn connection_test_request(
        request: &ProviderConnectionTestRequest,
    ) -> Result<(), ProviderDiagnosticsClientError> {
        request
            .validate()
            .map_err(ProviderDiagnosticsClientError::Invalid)
    }
}
