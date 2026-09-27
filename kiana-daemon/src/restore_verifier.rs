//! PD-23 daemon restore verifier adapter.
//!
//! This adapter validates a sealed manifest and replacement-fence fact only. It never copies a
//! root, acquires an instance lock, revives a trigger or publishes the restored root as active.

use kiana_domain::{RestoreVerificationFact, SnapshotManifest};

pub fn verify_restore(
    manifest: &SnapshotManifest,
    fact: &RestoreVerificationFact,
) -> Result<(), &'static str> {
    manifest
        .validate()
        .map_err(|_| "restore_manifest_invalid")?;
    fact.validate()?;
    if fact.snapshot_manifest_digest != manifest.manifest_digest
        || fact.owner_scope != manifest.owner_scope
        || fact.store_id != manifest.store_id
        || fact.previous_instance_id.as_deref() != Some(manifest.instance_id.as_str())
        || fact.source_cursor != manifest.source_cursor
        || fact.previous_data_epoch != Some(manifest.data_epoch)
        || fact.data_epoch <= manifest.data_epoch
    {
        return Err("restore_manifest_fact_binding_invalid");
    }
    Ok(())
}
