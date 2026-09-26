use kiana_domain::*;

fn sha(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn manifest() -> DeliveryManifest {
    let mut value = DeliveryManifest {
        schema: DELIVERY_MANIFEST_SCHEMA.to_owned(),
        manifest_id: "manifest-1".to_owned(),
        delivery_id: "delivery-1".to_owned(),
        project_id: "project-1".to_owned(),
        baseline_version: 4,
        acceptance_id: "acceptance-1".to_owned(),
        acceptance_status: AcceptanceStatus::Accepted,
        acceptance_digest: sha('a'),
        channel: "local_package".to_owned(),
        destination: "lessons/deliveries/delivery-1.json".to_owned(),
        recipient_ref: "principal:recipient-1".to_owned(),
        residual_obligations: vec!["document follow-up".to_owned()],
        artifacts: vec![DeliveryManifestArtifact {
            artifact_ref: "artifact:report".to_owned(),
            project_id: "project-1".to_owned(),
            packet_id: Some("packet-1".to_owned()),
            artifact_version: 2,
            relative_path: "report/report.md".to_owned(),
            content_hash: sha('c'),
            content_size: 42,
            producer_run_refs: vec!["run:1".to_owned()],
        }],
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

fn package(manifest: &DeliveryManifest) -> LocalDeliveryPackage {
    let mut value = LocalDeliveryPackage {
        schema: LOCAL_DELIVERY_PACKAGE_SCHEMA.to_owned(),
        package_id: "package-1".to_owned(),
        manifest_id: manifest.manifest_id.clone(),
        manifest_digest: manifest.digest.clone(),
        package_root: "lessons/packages/package-1".to_owned(),
        entries: vec![LocalPackageEntry {
            artifact_ref: "artifact:report".to_owned(),
            relative_path: "report/report.md".to_owned(),
            content_hash: sha('c'),
            content_size: 42,
            symlink: false,
        }],
        package_digest: String::new(),
    };
    value.package_digest = value.canonical_digest();
    value
}

#[test]
fn delivery_preparation_rejects_unaccepted_changed_or_foreign_artifacts() {
    let mut unaccepted = manifest();
    unaccepted.acceptance_status = AcceptanceStatus::ReadyForDecision;
    unaccepted.digest = unaccepted.canonical_digest();
    assert_eq!(
        unaccepted.validate().unwrap_err(),
        "delivery_manifest_header_invalid"
    );

    let mut foreign = manifest();
    foreign.artifacts[0].project_id = "project-foreign".to_owned();
    foreign.digest = foreign.canonical_digest();
    assert_eq!(
        foreign.validate().unwrap_err(),
        "delivery_manifest_artifact_binding_invalid"
    );

    let mut changed = manifest();
    changed.artifacts[0].content_hash = sha('d');
    changed.digest = changed.canonical_digest();
    assert!(changed.validate().is_ok());
    let mut ledger = DeliveryManifestLedger::default();
    ledger.publish_manifest(manifest()).expect("original");
    assert_eq!(
        ledger.publish_manifest(changed).unwrap_err(),
        "delivery_manifest_duplicate_digest_mismatch"
    );
}

#[test]
fn local_package_matches_the_accepted_manifest_and_rejects_path_or_symlink_escape() {
    let manifest = manifest();
    let mut ledger = DeliveryManifestLedger::default();
    ledger.publish_manifest(manifest.clone()).expect("manifest");
    let package = package(&manifest);
    ledger.record_package(package.clone()).expect("package");
    ledger.record_package(package).expect("idempotent package");

    let mut symlink = package(&manifest);
    symlink.entries[0].symlink = true;
    symlink.package_digest = symlink.canonical_digest();
    assert_eq!(
        symlink.validate_against(&manifest).unwrap_err(),
        "local_package_symlink_forbidden"
    );

    let mut escape = package(&manifest);
    escape.entries[0].relative_path = "../report.md".to_owned();
    escape.package_digest = escape.canonical_digest();
    assert_eq!(
        escape.validate_against(&manifest).unwrap_err(),
        "local_package_path_invalid"
    );
}

#[test]
fn manifest_rejects_empty_delivery_and_stale_or_foreign_package_entries() {
    let mut empty = manifest();
    empty.artifacts.clear();
    empty.digest = empty.canonical_digest();
    assert_eq!(
        empty.validate().unwrap_err(),
        "delivery_manifest_header_invalid"
    );

    let manifest = manifest();
    let mut stale = package(&manifest);
    stale.manifest_digest = sha('z');
    stale.package_digest = stale.canonical_digest();
    assert_eq!(
        stale.validate_against(&manifest).unwrap_err(),
        "local_package_manifest_binding_invalid"
    );

    let mut foreign = package(&manifest);
    foreign.entries[0].artifact_ref = "artifact:other".to_owned();
    foreign.package_digest = foreign.canonical_digest();
    assert_eq!(
        foreign.validate_against(&manifest).unwrap_err(),
        "local_package_manifest_entries_mismatch"
    );
}
