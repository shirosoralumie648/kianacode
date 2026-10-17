//! Typed client state for the provider diagnostics projection.
//!
//! The client stores a server-owned snapshot and cursor only. It does not infer model state,
//! retry requests, or turn a settings read into a connection test; callers must use an explicit
//! command carrying [`ProviderConnectionTestRequest`] when they want a gateway admission.

use kiana_domain::{
    admit_provider_upstream_effect, ProviderConnectionTestPlan, ProviderConnectionTestRequest,
    ProviderDiagnosticsCursor, ProviderDiagnosticsSnapshot, ProviderDisplayMode,
    ProviderSettingsView, ProviderStreamDisplay, ProviderSurface, ProviderSurfaceProjection,
    ProviderTerminalReplay, ProviderUiAction, ProviderUiActionDisposition,
    ProviderUpstreamAdmission, ProviderUpstreamEffect, PROVIDER_DIAGNOSTICS_PROJECTION_STALE,
    PROVIDER_LIVE_EFFECT_ADMISSION_REQUIRED, PROVIDER_UI_ACTION_STALE,
};

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ProviderDiagnosticsClientError {
    #[error("provider_diagnostics_snapshot_invalid:{0}")]
    Invalid(String),
    #[error("provider_diagnostics_projection_stale")]
    StaleEpoch,
    #[error("provider_diagnostics_cursor_gap")]
    CursorGap,
    #[error("provider_ui_action_stale")]
    StaleAction,
    #[error("provider_live_effect_requires_gateway_admission")]
    AdmissionRequired,
}

fn map_reason(reason: String) -> ProviderDiagnosticsClientError {
    match reason.as_str() {
        PROVIDER_DIAGNOSTICS_PROJECTION_STALE => ProviderDiagnosticsClientError::StaleEpoch,
        PROVIDER_UI_ACTION_STALE => ProviderDiagnosticsClientError::StaleAction,
        PROVIDER_LIVE_EFFECT_ADMISSION_REQUIRED => {
            ProviderDiagnosticsClientError::AdmissionRequired
        }
        _ => ProviderDiagnosticsClientError::Invalid(reason),
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProviderDiagnosticsClientState {
    snapshot: Option<ProviderDiagnosticsSnapshot>,
    cursor: Option<ProviderDiagnosticsCursor>,
    request_attempts: u32,
}

impl ProviderDiagnosticsClientState {
    pub fn snapshot(&self) -> Option<&ProviderDiagnosticsSnapshot> {
        self.snapshot.as_ref()
    }

    pub fn cursor(&self) -> Option<&ProviderDiagnosticsCursor> {
        self.cursor.as_ref()
    }

    /// Number of admitted model attempts observed by this client. Reconnects and settings reads
    /// never change it.
    pub const fn request_attempts(&self) -> u32 {
        self.request_attempts
    }

    fn require_snapshot(
        &self,
    ) -> Result<&ProviderDiagnosticsSnapshot, ProviderDiagnosticsClientError> {
        self.snapshot.as_ref().ok_or_else(|| {
            ProviderDiagnosticsClientError::Invalid(
                "provider_diagnostics_snapshot_missing".to_owned(),
            )
        })
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
            .map_err(map_reason)?;
        self.cursor = Some(snapshot.cursor.clone());
        self.snapshot = Some(snapshot);
        Ok(())
    }

    /// Replace the projection after a reconnect. Epochs must match; a gap is recovered by
    /// requesting a snapshot rather than replaying a possibly stale model action. No model
    /// request is retried here.
    pub fn reconnect(
        &mut self,
        snapshot: ProviderDiagnosticsSnapshot,
        authority_epoch: u64,
        config_epoch: u64,
        expected_sequence: Option<u64>,
    ) -> Result<(), ProviderDiagnosticsClientError> {
        snapshot
            .validate_for_epoch(authority_epoch, config_epoch)
            .map_err(map_reason)?;
        if expected_sequence.is_some_and(|sequence| snapshot.cursor.sequence != sequence) {
            self.snapshot = None;
            self.cursor = None;
            return Err(ProviderDiagnosticsClientError::CursorGap);
        }
        self.cursor = Some(snapshot.cursor.clone());
        self.snapshot = Some(snapshot);
        Ok(())
    }

    /// Record the first admitted model attempt. A second attempt is refused so a display or
    /// reconnect path cannot silently re-issue model work.
    pub fn begin_model_attempt(&mut self) -> Result<(), ProviderDiagnosticsClientError> {
        if self.request_attempts != 0 {
            return Err(ProviderDiagnosticsClientError::Invalid(
                "provider_model_attempt_already_started".to_owned(),
            ));
        }
        self.request_attempts = 1;
        Ok(())
    }

    /// Return the durable terminal receipt, if it is newer than the subscriber cursor.
    pub fn terminal_after(
        &self,
        sequence: u64,
    ) -> Result<Option<ProviderTerminalReplay>, ProviderDiagnosticsClientError> {
        self.require_snapshot()?
            .terminal_after(sequence)
            .map(|terminal| terminal.cloned())
            .map_err(map_reason)
    }

    /// Return the shared surface projection. CLI, Workbench, Web and Desktop all call this with
    /// the same snapshot; the renderer only labels the surface.
    pub fn surface_projection(
        &self,
        surface: ProviderSurface,
    ) -> Result<ProviderSurfaceProjection, ProviderDiagnosticsClientError> {
        ProviderSurfaceProjection::new(surface, self.require_snapshot()?.clone())
            .map_err(map_reason)
    }

    /// A settings/catalog read never triggers inference or creates a connection-test plan.
    pub fn settings_view(&self) -> Result<ProviderSettingsView, ProviderDiagnosticsClientError> {
        ProviderSettingsView::read_only(self.require_snapshot()?.clone()).map_err(map_reason)
    }

    /// Validate an explicit connection test against the current projection. The caller must
    /// supply the same gateway admission digest used by model execution; this method never sends.
    pub fn explicit_connection_test(
        &self,
        request: &ProviderConnectionTestRequest,
        authority_epoch: u64,
        config_epoch: u64,
    ) -> Result<ProviderConnectionTestPlan, ProviderDiagnosticsClientError> {
        self.require_snapshot()?
            .connection_test_plan(request, authority_epoch, config_epoch)
            .map_err(map_reason)
    }

    /// Settings/catalog views expose this request shape but cannot manufacture an admitted test.
    pub fn connection_test_request(
        request: &ProviderConnectionTestRequest,
    ) -> Result<(), ProviderDiagnosticsClientError> {
        request.validate().map_err(map_reason)
    }

    /// Gate an explicit live catalog/smoke/connection test. Offline paths return `None` and do not
    /// need an admission.
    pub fn admit_upstream_effect(
        &self,
        effect: ProviderUpstreamEffect,
        live: bool,
        request: Option<&ProviderConnectionTestRequest>,
    ) -> Result<Option<ProviderUpstreamAdmission>, ProviderDiagnosticsClientError> {
        let snapshot = self.require_snapshot()?;
        admit_provider_upstream_effect(
            effect,
            live,
            request,
            snapshot.authority_epoch,
            snapshot.config_epoch,
        )
        .map_err(map_reason)
    }

    /// Authorize a UI action against the run/epochs/sequence the surface actually displayed.
    /// Stale actions are refused; they can only trigger a fresh snapshot read elsewhere.
    pub fn authorize_ui_action(
        &self,
        action: &ProviderUiAction,
        active_run_id: Option<&str>,
        authority_epoch: u64,
        config_epoch: u64,
        current_sequence: u64,
    ) -> Result<ProviderUiActionDisposition, ProviderDiagnosticsClientError> {
        self.require_snapshot()?
            .validate_for_epoch(authority_epoch, config_epoch)
            .map_err(|_| ProviderDiagnosticsClientError::StaleAction)?;
        if action.authority_epoch != authority_epoch || action.config_epoch != config_epoch {
            return Err(ProviderDiagnosticsClientError::StaleAction);
        }
        action
            .authorize(
                active_run_id,
                authority_epoch,
                config_epoch,
                current_sequence,
            )
            .map_err(map_reason)
    }

    /// Fold already-observed display events. The returned value has no gateway handle and cannot
    /// create a second model request.
    pub fn stream_display(
        &self,
        display_mode: ProviderDisplayMode,
    ) -> Result<ProviderStreamDisplay, ProviderDiagnosticsClientError> {
        let snapshot = self.require_snapshot()?;
        ProviderStreamDisplay::new(
            display_mode,
            snapshot.authority_epoch,
            snapshot.config_epoch,
            self.request_attempts,
        )
        .map_err(map_reason)
    }
}
