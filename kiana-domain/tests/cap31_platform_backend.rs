use kiana_domain::{PlatformBackendDisposition, PlatformBackendReport, PlatformTarget};

#[test]
fn target_only_platforms_cannot_claim_behavior_or_host_fallback() {
    let macos = PlatformBackendReport::new(
        PlatformTarget::Macos,
        "seatbelt-target",
        PlatformBackendDisposition::TargetOnly,
        false,
        true,
        vec![
            "target macOS CI only".to_owned(),
            "Seatbelt not implemented".to_owned(),
        ],
    )
    .unwrap();
    assert!(!macos.behavior_verified);
    assert!(macos.no_host_fallback);
    assert!(macos.validate().is_ok());

    let implemented_without_behavior = PlatformBackendReport::new(
        PlatformTarget::Macos,
        "seatbelt",
        PlatformBackendDisposition::Implemented,
        false,
        true,
        vec!["missing runtime receipt".to_owned()],
    );
    assert_eq!(
        implemented_without_behavior.unwrap_err(),
        "platform_backend_implemented_requires_behavior"
    );
}

#[test]
fn host_fallback_and_missing_limits_are_rejected() {
    let fallback = PlatformBackendReport::new(
        PlatformTarget::Macos,
        "seatbelt-target",
        PlatformBackendDisposition::TargetOnly,
        false,
        false,
        vec!["target only".to_owned()],
    );
    assert_eq!(
        fallback.unwrap_err(),
        "platform_backend_host_fallback_forbidden"
    );

    let missing_limits = PlatformBackendReport::new(
        PlatformTarget::Macos,
        "seatbelt-target",
        PlatformBackendDisposition::TargetOnly,
        false,
        true,
        Vec::new(),
    );
    assert_eq!(
        missing_limits.unwrap_err(),
        "platform_backend_limitation_required"
    );
}
