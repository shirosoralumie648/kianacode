use kiana_domain::{PlatformBackendDisposition, PlatformBackendReport, PlatformTarget};

#[test]
fn windows_target_only_requires_limits_and_no_host_fallback() {
    let windows = PlatformBackendReport::new(
        PlatformTarget::Windows,
        "job-object-target",
        PlatformBackendDisposition::TargetOnly,
        false,
        true,
        vec![
            "target Windows CI only".to_owned(),
            "Job Object and ACL backend not implemented".to_owned(),
            "reparse/UNC behavior unverified".to_owned(),
        ],
    )
    .unwrap();
    assert!(!windows.behavior_verified);
    assert!(windows.no_host_fallback);
}

#[test]
fn windows_implemented_without_behavior_is_rejected() {
    let report = PlatformBackendReport::new(
        PlatformTarget::Windows,
        "job-object",
        PlatformBackendDisposition::Implemented,
        false,
        true,
        vec!["target receipt missing".to_owned()],
    );
    assert_eq!(
        report.unwrap_err(),
        "platform_backend_implemented_requires_behavior"
    );
}
