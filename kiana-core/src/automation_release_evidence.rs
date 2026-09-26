//! Core read-only facade for AUT-24 release evidence.

use kiana_domain::Aut24ReleaseGate;

pub fn validate_automation_release_gate(gate: &Aut24ReleaseGate) -> Result<(), String> {
    gate.validate()
}
