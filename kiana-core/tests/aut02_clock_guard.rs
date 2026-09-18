#[test]
fn automation_clock_guard_has_no_unchecked_deadline_extension_path() {
    let domain = include_str!("../../kiana-domain/src/clock.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let automation = include_str!("../src/automation.rs");
    for marker in [
        "ClockObservation",
        "ClockTrust",
        "clock_zero_sample",
        "clock_wall_overflow",
        "clock_transition_rollback_untrusted",
        "clock_untrusted",
        "allows_before",
        "clamp_deadline",
    ] {
        assert!(
            domain.contains(marker),
            "clock domain marker missing: {marker}"
        );
    }
    for marker in [
        "pub trait ClockPort",
        "require_trusted_deadline",
        "monotonic_now_ms",
        "ClockObservation::observe",
    ] {
        assert!(
            ports.contains(marker),
            "clock port marker missing: {marker}"
        );
    }
    assert!(automation.contains("now_ms"));
    assert!(automation.contains("AutomationAuthority"));
    assert!(!automation.contains("tokio::spawn(async move {"));
}
