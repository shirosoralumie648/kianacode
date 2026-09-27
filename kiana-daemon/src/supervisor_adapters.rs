//! Narrow supervisor adapter registry.
//!
//! This source slice validates the server-owned request and identifies the selected platform
//! adapter. It deliberately returns `Unavailable` until a platform implementation can provide a
//! real observation; no shell/model/capability path or pid fallback is introduced here.

use async_trait::async_trait;
use kiana_domain::{SupervisorAction, SupervisorBackend, SupervisorObservation, SupervisorRequest};
use kiana_ports::{PortError, SupervisorPort};

pub const SUPPORTED_SUPERVISOR_BACKENDS: &[SupervisorBackend] = &[
    SupervisorBackend::Systemd,
    SupervisorBackend::Launchd,
    SupervisorBackend::WindowsService,
    SupervisorBackend::Container,
    SupervisorBackend::Fake,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NarrowSupervisorAdapter {
    backend: SupervisorBackend,
}

impl NarrowSupervisorAdapter {
    pub const fn new(backend: SupervisorBackend) -> Self {
        Self { backend }
    }

    pub const fn backend(&self) -> SupervisorBackend {
        self.backend
    }

    fn unavailable(
        &self,
        request: SupervisorRequest,
        expected_action: SupervisorAction,
    ) -> Result<SupervisorObservation, PortError> {
        request.validate().map_err(PortError::Failed).and_then(|_| {
            if request.backend != self.backend {
                return Err(PortError::Conflict(
                    "supervisor_backend_binding_mismatch".to_owned(),
                ));
            }
            if request.action != expected_action {
                return Err(PortError::Conflict(
                    "supervisor_action_binding_mismatch".to_owned(),
                ));
            }
            Err(PortError::Unavailable(format!(
                "supervisor_{}_adapter_unavailable",
                self.backend.adapter_name()
            )))
        })
    }
}

#[async_trait]
impl SupervisorPort for NarrowSupervisorAdapter {
    async fn start(&self, request: SupervisorRequest) -> Result<SupervisorObservation, PortError> {
        self.unavailable(request, SupervisorAction::Start)
    }

    async fn stop(&self, request: SupervisorRequest) -> Result<SupervisorObservation, PortError> {
        self.unavailable(request, SupervisorAction::Stop)
    }

    async fn restart(
        &self,
        request: SupervisorRequest,
    ) -> Result<SupervisorObservation, PortError> {
        self.unavailable(request, SupervisorAction::Restart)
    }
}
