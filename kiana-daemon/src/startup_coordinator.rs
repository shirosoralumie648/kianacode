//! DaemonHost adapter for the pure deployment startup coordinator.
//!
//! The composition root accepts already validated adapter evidence and routes it through Core;
//! it does not open stores, run migrations, rebuild projections or acquire a lease here.

use kiana_domain::{StartupCoordinatorReport, StartupCoordinatorRequest};
use kiana_ports::PortError;

pub(crate) fn evaluate(
    request: &StartupCoordinatorRequest,
) -> Result<StartupCoordinatorReport, PortError> {
    kiana_core::evaluate_startup(request).map_err(PortError::Failed)
}
