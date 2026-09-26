//! Core adapter for accepted-artifact manifests and local package descriptors.
//!
//! Filesystem/package effects remain behind the existing Broker path; this adapter only records
//! the validated manifest and package facts.

use kiana_domain::{DeliveryManifest, DeliveryManifestLedger, LocalDeliveryPackage};

pub(crate) fn publish_delivery_manifest(
    ledger: &mut DeliveryManifestLedger,
    manifest: DeliveryManifest,
) -> Result<(), &'static str> {
    ledger.publish_manifest(manifest)
}

pub(crate) fn record_local_delivery_package(
    ledger: &mut DeliveryManifestLedger,
    package: LocalDeliveryPackage,
) -> Result<(), &'static str> {
    ledger.record_package(package)
}
