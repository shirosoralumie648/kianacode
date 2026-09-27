//! CAP-31 failure-first fixtures: a macOS backend is refused, never substituted, on this host.
//!
//! This file runs on Linux. It therefore cannot and does not prove anything about Seatbelt
//! confinement, GUI/Apple Events denial, descendant escape or process supervision on macOS. What
//! it proves is the decision contract: a macOS backend selected on a non-macOS host is refused,
//! and the refusal does not name a substitute.

use kiana_domain::{
    backend_is_selectable, BackendSelection, BackendSelectionDecision, BackendSelectionRequest,
    PlatformBackendDisposition, PlatformBackendReport, PlatformTarget, HOST_EXECUTION_BACKEND,
};

const LIMITATIONS: [&str; 2] = [
    "seatbelt not implemented in this checkout",
    "no macOS target-machine receipt exists",
];

fn target_only_macos() -> PlatformBackendReport {
    PlatformBackendReport::new(
        PlatformTarget::Macos,
        "seatbelt",
        PlatformBackendDisposition::TargetOnly,
        false,
        true,
        LIMITATIONS.iter().map(|text| (*text).to_owned()).collect(),
    )
    .expect("target-only report")
}

fn target_only_windows() -> PlatformBackendReport {
    PlatformBackendReport::new(
        PlatformTarget::Windows,
        "job-object",
        PlatformBackendDisposition::TargetOnly,
        false,
        true,
        LIMITATIONS.iter().map(|text| (*text).to_owned()).collect(),
    )
    .expect("target-only report")
}

#[test]
fn macos_missing_backend_has_no_host_fallback() {
    // The card's headline case: the macOS backend is unknown to the server. The answer is a
    // refusal, and the decision carries no substitute backend name at all.
    let request = BackendSelectionRequest::new(
        PlatformTarget::Linux,
        "seatbelt",
        PlatformTarget::Linux,
        None,
    )
    .expect("request");
    let decision = BackendSelectionDecision::evaluate(&request).expect("decision");
    assert!(!decision.usable());
    assert_eq!(decision.selection, BackendSelection::NotSupported);
    assert!(decision.effective_backend.is_empty());
    assert!(decision.no_host_fallback);
    assert!(!decision.grants_execution);
    assert_eq!(decision.reason, "backend_selection_unknown_backend");
    assert!(!backend_is_selectable(
        PlatformTarget::Linux,
        "seatbelt",
        None
    ));

    // A known-but-target-only macOS backend on a Linux host is refused for the same reason, and
    // in particular the host's real backend is NOT named as a replacement.
    let request = BackendSelectionRequest::new(
        PlatformTarget::Linux,
        "seatbelt",
        PlatformTarget::Linux,
        Some(target_only_macos()),
    )
    .expect("request");
    let decision = BackendSelectionDecision::evaluate(&request).expect("decision");
    assert_eq!(decision.selection, BackendSelection::TargetOnly);
    assert!(decision.effective_backend.is_empty());
    assert!(!decision.effective_backend.contains(HOST_EXECUTION_BACKEND));
    assert!(!backend_is_selectable(
        PlatformTarget::Linux,
        "seatbelt",
        Some(&target_only_macos())
    ));

    // A caller claiming it is on macOS while the host is Linux is a configuration or forgery
    // signal, and is blocked before the backend's own disposition is consulted.
    let request = BackendSelectionRequest::new(
        PlatformTarget::Macos,
        "seatbelt",
        PlatformTarget::Linux,
        Some(target_only_macos()),
    )
    .expect("request");
    let decision = BackendSelectionDecision::evaluate(&request).expect("decision");
    assert_eq!(decision.selection, BackendSelection::Blocked);
    assert_eq!(decision.reason, "backend_selection_target_mismatch");
    assert!(decision.effective_backend.is_empty());

    // A decision that names a substitute backend is refused by its own validation, so
    // "no host fallback" is a property of the value and not only of the code path.
    let mut substituted = BackendSelectionDecision::evaluate(
        &BackendSelectionRequest::new(
            PlatformTarget::Linux,
            "seatbelt",
            PlatformTarget::Linux,
            Some(target_only_macos()),
        )
        .expect("request"),
    )
    .expect("decision");
    substituted.effective_backend = HOST_EXECUTION_BACKEND.to_owned();
    assert_eq!(
        substituted
            .validate_against(
                &BackendSelectionRequest::new(
                    PlatformTarget::Linux,
                    "seatbelt",
                    PlatformTarget::Linux,
                    Some(target_only_macos()),
                )
                .expect("request")
            )
            .unwrap_err(),
        "backend_selection_substitute_backend_named"
    );
}

#[test]
fn macos_backend_denies_host_secrets_and_gui_escape() {
    // What this file can honestly assert about the macOS backend is its *declared* surface: a
    // report that claims macOS support must state its limits, and may not claim verified
    // behaviour without a receipt. The confinement itself is not exercised here and is not
    // proven by this test.
    let report = target_only_macos();
    assert_eq!(report.target, PlatformTarget::Macos);
    assert_eq!(report.disposition, PlatformBackendDisposition::TargetOnly);
    assert!(!report.behavior_verified);
    assert!(report.no_host_fallback);
    assert!(report.validate().is_ok());

    // An unverified backend may never be presented as implemented, however it is spelled.
    for backend in ["seatbelt", "sandbox-exec", "host-shell", "none"] {
        let claimed = PlatformBackendReport::new(
            PlatformTarget::Macos,
            backend,
            PlatformBackendDisposition::Implemented,
            false,
            true,
            vec!["no target receipt".to_owned()],
        );
        assert_eq!(
            claimed.unwrap_err(),
            "platform_backend_implemented_requires_behavior",
            "backend {backend} was allowed to claim Implemented without behaviour"
        );
    }

    // A host fallback is rejected for every disposition, including Implemented.
    for disposition in [
        PlatformBackendDisposition::TargetOnly,
        PlatformBackendDisposition::NotSupported,
        PlatformBackendDisposition::Blocked,
    ] {
        let fallback = PlatformBackendReport::new(
            PlatformTarget::Macos,
            "seatbelt",
            disposition,
            false,
            false,
            vec!["target only".to_owned()],
        );
        assert_eq!(
            fallback.unwrap_err(),
            "platform_backend_host_fallback_forbidden"
        );
    }

    // An unimplemented backend without limitations is refused, so a refusal always carries a
    // reason the caller can read.
    assert_eq!(
        PlatformBackendReport::new(
            PlatformTarget::Macos,
            "seatbelt",
            PlatformBackendDisposition::NotSupported,
            false,
            true,
            Vec::new(),
        )
        .unwrap_err(),
        "platform_backend_limitation_required"
    );
}

#[test]
fn macos_descendant_escape_or_stop_failure_is_visible() {
    // "Stop failure is visible" is a property of the decision vocabulary, not of a process. The
    // contract has no value meaning "probably stopped": a backend that is not proven is
    // `TargetOnly`, and a backend whose report is invalid is `Blocked` rather than `Selected`.
    // A caller therefore cannot observe a soft success for an unproven macOS backend.
    let invalid = BackendSelectionRequest::new(
        PlatformTarget::Linux,
        "seatbelt",
        PlatformTarget::Linux,
        Some(target_only_macos()),
    )
    .expect("request");
    let mut corrupted = invalid.clone();
    corrupted.report.as_mut().expect("report").behavior_verified = true;
    let decision = BackendSelectionDecision::evaluate(&corrupted).expect("decision");
    assert_eq!(decision.selection, BackendSelection::Blocked);
    assert_eq!(decision.reason, "backend_selection_report_invalid");
    assert!(!decision.usable());

    // A digest-sealed report is what makes this checkable: editing a field breaks the seal, and a
    // broken seal is a refusal rather than a silently different capability claim.
    let mut edited = target_only_macos();
    edited.behavior_verified = true;
    assert_eq!(
        edited.validate().unwrap_err(),
        "platform_backend_unverified_disposition_mismatch"
    );

    // The Windows backend shares the same rule through the same vocabulary, so a card that says
    // "escape is visible" gets the same treatment without a second mechanism.
    let request = BackendSelectionRequest::new(
        PlatformTarget::Linux,
        "job-object",
        PlatformTarget::Linux,
        Some(target_only_windows()),
    )
    .expect("request");
    let decision = BackendSelectionDecision::evaluate(&request).expect("decision");
    assert_eq!(decision.selection, BackendSelection::TargetOnly);
    assert!(!decision.usable());
    assert!(decision.effective_backend.is_empty());
}
