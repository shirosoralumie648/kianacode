//! Core read-only facade for CP-30 product-flow evidence.
//!
//! The facade compares already committed flow facts. It never creates a DaemonHost, starts a
//! Runner, dispatches a Broker effect or decides an approval.

use kiana_domain::Cp30ProductFlowBundle;

pub fn validate_control_plane_product_bundle(bundle: &Cp30ProductFlowBundle) -> Result<(), String> {
    bundle.validate()
}
