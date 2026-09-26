#[test]
fn delivery_manifest_keeps_acceptance_artifact_and_broker_boundaries() {
    let manifest = include_str!("../../kiana-domain/src/delivery_manifest.rs");
    let company = include_str!("../../kiana-domain/src/company.rs");
    let closeout = include_str!("../../kiana-domain/src/company_closeout.rs");
    let core = include_str!("../src/delivery_manifest.rs");
    for marker in [
        "DELIVERY_MANIFEST_SCHEMA",
        "DeliveryManifestArtifact",
        "DeliveryManifest",
        "LocalDeliveryPackage",
        "DeliveryManifestLedger",
        "AcceptanceStatus::Accepted",
        "delivery_manifest_artifact_binding_invalid",
        "delivery_manifest_destination_invalid",
        "local_package_symlink_forbidden",
        "local_package_manifest_entries_mismatch",
        "PrepareDelivery",
        "Delivery",
        "CompanyArtifact",
        "publish_delivery_manifest",
        "record_local_delivery_package",
        "Broker",
    ] {
        assert!(
            manifest.contains(marker)
                || company.contains(marker)
                || closeout.contains(marker)
                || core.contains(marker),
            "CO-33 marker missing: {marker}"
        );
    }
    for forbidden in ["std::fs::write", "Command::new", "ModelClient::new"] {
        assert!(
            !manifest.contains(forbidden),
            "CO-33 effect bypass marker present: {forbidden}"
        );
    }
}
