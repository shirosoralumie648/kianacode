//! CAP-32 failure-first fixtures: a Windows backend is refused, never faked, on this host.
//!
//! This file runs on Linux. It cannot and does not prove Job Object behaviour, handle
//! inheritance, ACL application, reparse-point resolution or cross-process locking on Windows.
//! What it proves is the decision contract and the honesty requirement the card names last:
//! an unavailable backend must not return an empty success to satisfy a cross-platform trait.

use kiana_domain::{
    backend_is_selectable, BackendSelection, BackendSelectionDecision, BackendSelectionRequest,
    PlatformBackendDisposition, PlatformBackendReport, PlatformTarget,
};

fn target_only_windows() -> PlatformBackendReport {
    PlatformBackendReport::new(
        PlatformTarget::Windows,
        "job-object",
        PlatformBackendDisposition::TargetOnly,
        false,
        true,
        vec![
            "job object / restricted token not implemented".to_owned(),
            "no Windows target-machine receipt exists".to_owned(),
        ],
    )
    .expect("target-only report")
}

fn not_supported_windows() -> PlatformBackendReport {
    PlatformBackendReport::new(
        PlatformTarget::Windows,
        "windows-backend",
        PlatformBackendDisposition::NotSupported,
        false,
        true,
        vec!["no implementation on any host".to_owned()],
    )
    .expect("not-supported report")
}

#[test]
fn windows_reparse_or_unc_path_cannot_escape_scope() {
    // Path-shape enforcement belongs to a Windows handle implementation, which does not exist
    // here. What the decision contract guarantees is that no Windows path handling can be
    // *selected* on this host in the first place, so there is no code path in which a reparse
    // point, UNC path, ADS suffix or case alias could be resolved by a Linux build and reported
    // as Windows behaviour.
    let request = BackendSelectionRequest::new(
        PlatformTarget::Linux,
        "job-object",
        PlatformTarget::Linux,
        Some(target_only_windows()),
    )
    .expect("request");
    let decision = BackendSelectionDecision::evaluate(&request).expect("decision");
    assert!(!decision.usable());
    assert_eq!(decision.selection, BackendSelection::TargetOnly);
    assert!(decision.effective_backend.is_empty());
    assert!(decision.no_host_fallback);
    assert_eq!(decision.reason, "backend_selection_target_only_backend");
    assert!(!backend_is_selectable(
        PlatformTarget::Linux,
        "job-object",
        Some(&target_only_windows())
    ));

    // A caller that claims to be on Windows while the host is Linux is refused, which is the
    // case where a Unix path routine would otherwise be reached through a "Windows" label.
    let forged = BackendSelectionRequest::new(
        PlatformTarget::Windows,
        "job-object",
        PlatformTarget::Linux,
        Some(target_only_windows()),
    )
    .expect("request");
    let decision = BackendSelectionDecision::evaluate(&forged).expect("decision");
    assert_eq!(decision.selection, BackendSelection::Blocked);
    assert_eq!(decision.reason, "backend_selection_target_mismatch");

    // A backend the server has no report for is `NotSupported`, not a permissive default.
    let unknown = BackendSelectionRequest::new(
        PlatformTarget::Linux,
        "job-object",
        PlatformTarget::Linux,
        None,
    )
    .expect("request");
    let decision = BackendSelectionDecision::evaluate(&unknown).expect("decision");
    assert_eq!(decision.selection, BackendSelection::NotSupported);
    assert!(decision.effective_backend.is_empty());
}

#[test]
fn windows_child_cannot_break_away_from_job() {
    // Breakaway prevention is a Job Object property. There is no Job Object here, so this test
    // asserts the boundary that keeps the claim honest: the disposition vocabulary has no value
    // for "implemented but unverified", so a backend that has not produced target evidence
    // cannot be selected, and a report edited to claim it is refused by its own digest.
    let report = target_only_windows();
    assert!(!report.behavior_verified);
    assert!(report.validate().is_ok());

    let forged = PlatformBackendReport::new(
        PlatformTarget::Windows,
        "job-object",
        PlatformBackendDisposition::Implemented,
        false,
        true,
        vec!["no target receipt".to_owned()],
    );
    assert_eq!(
        forged.unwrap_err(),
        "platform_backend_implemented_requires_behavior"
    );

    let mut edited = target_only_windows();
    edited.disposition = PlatformBackendDisposition::Implemented;
    assert!(edited.validate().is_err());

    // A blocked backend is refused for the same reason as a target-only one; the two are kept
    // distinct so a caller can tell "not on this platform" from "present but unusable".
    let blocked = PlatformBackendReport::new(
        PlatformTarget::Windows,
        "job-object",
        PlatformBackendDisposition::Blocked,
        false,
        true,
        vec!["breakaway control unverified".to_owned()],
    )
    .expect("blocked report");
    let request = BackendSelectionRequest::new(
        PlatformTarget::Windows,
        "job-object",
        PlatformTarget::Windows,
        Some(blocked),
    )
    .expect("request");
    let decision = BackendSelectionDecision::evaluate(&request).expect("decision");
    assert_eq!(decision.selection, BackendSelection::Blocked);
    assert_eq!(decision.reason, "backend_selection_blocked");
    assert!(decision.effective_backend.is_empty());
    assert!(!decision.usable());
}

#[test]
fn windows_cross_process_lock_is_real_or_operation_is_denied() {
    // The card requires a real lock or a denial. The decision contract supplies the denial half
    // in source, and refuses the "real lock" half from being claimed without evidence.
    let request = BackendSelectionRequest::new(
        PlatformTarget::Windows,
        "job-object",
        PlatformTarget::Windows,
        Some(target_only_windows()),
    )
    .expect("request");
    let decision = BackendSelectionDecision::evaluate(&request).expect("decision");
    assert_eq!(decision.selection, BackendSelection::TargetOnly);
    assert!(!decision.usable());

    let missing = BackendSelectionRequest::new(
        PlatformTarget::Windows,
        "windows-backend",
        PlatformTarget::Windows,
        Some(not_supported_windows()),
    )
    .expect("request");
    let decision = BackendSelectionDecision::evaluate(&missing).expect("decision");
    assert_eq!(decision.selection, BackendSelection::NotSupported);
    assert!(decision.effective_backend.is_empty());
    assert!(!decision.grants_execution);

    // A tampered decision is recomputed and refused, so an edited status cannot be presented as
    // the outcome of a selection this module did not make.
    let mut tampered = decision.clone();
    tampered.selection = BackendSelection::Selected;
    tampered.effective_backend = "job-object".to_owned();
    assert!(tampered.validate_against(&missing).is_err());

    // Only a target-backed, behaviour-verified report is selectable, and this module cannot
    // construct one: it forwards a report that already passed
    // `platform_backend_implemented_requires_behavior`. That report is fixture data, not
    // evidence: nothing in this repository observes Windows behaviour.
    let verified = PlatformBackendReport::new(
        PlatformTarget::Windows,
        "job-object",
        PlatformBackendDisposition::Implemented,
        true,
        true,
        Vec::new(),
    )
    .expect("verified report");
    let request = BackendSelectionRequest::new(
        PlatformTarget::Windows,
        "job-object",
        PlatformTarget::Windows,
        Some(verified),
    )
    .expect("request");
    let decision = BackendSelectionDecision::evaluate(&request).expect("decision");
    assert_eq!(decision.selection, BackendSelection::Selected);
    assert!(decision.usable());
    assert_eq!(decision.effective_backend, "job-object");
    assert!(!decision.grants_execution);
    assert_eq!(decision.reason, "backend_selection_accepted");
}
