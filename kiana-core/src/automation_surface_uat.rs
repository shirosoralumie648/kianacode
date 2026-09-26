//! Core read-only facade for AUT-23 five-surface UAT parity.

use kiana_domain::AutomationSurfaceUat;

pub fn validate_automation_surface_uat(uat: &AutomationSurfaceUat) -> Result<(), String> {
    uat.validate()
}
