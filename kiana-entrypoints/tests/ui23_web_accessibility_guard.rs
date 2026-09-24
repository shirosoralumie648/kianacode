#[test]
fn ui23_focus_and_sanitizer_guards_are_wired_without_a_second_authority() {
    let page = include_str!("../src/web_page.html");
    for forbidden in [
        "new KianaHarness",
        "new CapabilityBroker",
        "ControlPlane.execute",
        "window.actor",
        "window.scope",
        "innerHTML",
    ] {
        assert!(
            !page.contains(forbidden),
            "UI-23 page gained authority: {forbidden}"
        );
    }
    let human = page
        .split("function humanActionAllowed(")
        .nth(1)
        .and_then(|rest| rest.split("async function refreshApprovals").next())
        .expect("human action deny-first helper");
    for marker in [
        "allowed_decisions",
        "humanActionsAllowed",
        "humanActionExpired",
        "projectionStatus === 'unknown'",
        "projectionStatus === 'result_unknown'",
        "scope.session_id",
    ] {
        assert!(
            human.contains(marker),
            "hidden/unknown action guard missing: {marker}"
        );
    }
    let trap = page
        .split("function focusTrapKeydown(")
        .nth(1)
        .and_then(|rest| rest.split("function setCoach(").next())
        .expect("focus trap helper");
    assert!(trap.contains("event.shiftKey"));
    assert!(trap.contains("first.focus()"));
    assert!(trap.contains("last.focus()"));
    let csp = include_str!("../src/web.rs");
    assert!(csp.contains("Content-Security-Policy") || csp.contains("content-security-policy"));
    assert!(csp.contains("report-uri /api/csp-report"));
    assert!(!csp.contains("'unsafe-inline'"));
}
