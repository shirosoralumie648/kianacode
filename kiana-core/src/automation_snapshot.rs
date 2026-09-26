//! Core read-only facade for AUT-22 automation snapshots.

use kiana_domain::AutomationSnapshot;

pub fn validate_automation_snapshot(snapshot: &AutomationSnapshot) -> Result<(), String> {
    snapshot.validate()
}
