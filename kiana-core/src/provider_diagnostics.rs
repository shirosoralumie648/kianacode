//! Server-owned provider diagnostics projection.
//!
//! This fold only validates and republishes a secret-free snapshot. It never calls a provider,
//! creates a model request or mutates a run. Authority/config epoch changes invalidate the old
//! projection and force the caller to hydrate a fresh snapshot.

use kiana_domain::{
    ProviderDiagnosticsSnapshot, ProviderTerminalReplay, PROVIDER_DIAGNOSTICS_PROJECTION_STALE,
};

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ProviderDiagnosticsProjectionError {
    #[error("provider_diagnostics_snapshot_invalid:{0}")]
    Invalid(String),
    #[error("provider_diagnostics_projection_stale")]
    StaleEpoch,
}

/// Validate the daemon-owned snapshot against the current authority and configuration epochs.
pub fn project_provider_diagnostics(
    snapshot: ProviderDiagnosticsSnapshot,
    authority_epoch: u64,
    config_epoch: u64,
) -> Result<ProviderDiagnosticsSnapshot, ProviderDiagnosticsProjectionError> {
    snapshot
        .validate_for_epoch(authority_epoch, config_epoch)
        .map_err(|reason| {
            if reason == PROVIDER_DIAGNOSTICS_PROJECTION_STALE {
                ProviderDiagnosticsProjectionError::StaleEpoch
            } else {
                ProviderDiagnosticsProjectionError::Invalid(reason)
            }
        })?;
    Ok(snapshot)
}

/// Replay only an already committed terminal receipt for a late subscriber. A missing terminal
/// remains an empty result; it is never converted into a new model request.
pub fn replay_provider_terminal(
    snapshot: &ProviderDiagnosticsSnapshot,
    authority_epoch: u64,
    config_epoch: u64,
    after_sequence: u64,
) -> Result<Option<ProviderTerminalReplay>, ProviderDiagnosticsProjectionError> {
    project_provider_diagnostics(snapshot.clone(), authority_epoch, config_epoch)?;
    snapshot
        .terminal_after(after_sequence)
        .map(|terminal| terminal.cloned())
        .map_err(ProviderDiagnosticsProjectionError::Invalid)
}
