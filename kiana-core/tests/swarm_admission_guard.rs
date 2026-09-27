#[test]
fn swarm_admission_is_one_read_only_control_plane_boundary() {
    let domain = include_str!("../../kiana-domain/src/swarm_admission.rs");
    let core = include_str!("../src/swarm_admission.rs");
    for marker in [
        "SwarmAdmissionRequest",
        "SwarmAdmissionLedger",
        "SwarmAdmissionResource",
        "admit_with_fault",
        "idempotency",
        "work_fingerprint",
        "authority_epoch",
        "path_locks",
        "data_locks",
        "admit_swarm_resources",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "SW-05 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "KianaHarness",
        "std::process::Command",
        "tokio::spawn",
        "EventStore",
        "write_artifact",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "SW-05 admission boundary must not execute effects: {forbidden}"
        );
    }
}
