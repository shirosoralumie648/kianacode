//! Core read-only facade for ER-33 capacity and migration drill evidence.

use kiana_domain::Er33CapacityMigrationDrill;

pub fn validate_er33_capacity_migration_drill(
    drill: &Er33CapacityMigrationDrill,
) -> Result<(), String> {
    drill.validate()
}
