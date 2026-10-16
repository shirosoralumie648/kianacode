#[test]
fn status_is_reachable_without_color() {
    let page = include_str!("../src/web_page.html");
    let web = include_str!("../src/web.rs");
    let workbench = include_str!("../src/workbench_chat.rs");
    let workbench_tty = include_str!("../src/workbench.rs");
    let desktop = include_str!("../../contrib/desktop/main.js");
    let baseline = include_str!("../../docs/roadmap/p2-m7-01-accessibility-baseline.md");

    for marker in [
        "aria-label",
        "aria-live",
        "aria-atomic",
        "aria-describedby",
        "aria-current",
        "role=\"status\"",
        "role=\"region\"",
        "role=\"dialog\"",
        "aria-modal",
        "tabindex",
        "skip-link",
        "sr-only",
        ":focus-visible",
        "prefers-reduced-motion",
        "prefers-contrast:more",
        "forced-colors:active",
        "max-width:650px",
        "focus()",
        "Ctrl+Enter",
        "KeyCode",
        "Esc",
        "Ctrl-C",
        "ExecutionStatus",
        "result_unknown",
        "cancelled",
        "incomplete",
        "textContent",
        "read_only",
    ] {
        assert!(
            page.contains(marker)
                || web.contains(marker)
                || workbench.contains(marker)
                || workbench_tty.contains(marker)
                || desktop.contains(marker)
                || baseline.contains(marker),
            "accessibility marker missing: {marker}"
        );
    }

    assert!(page.contains("@media (max-width:650px)"));
    assert!(page.contains("@media (prefers-contrast:more)"));
    assert!(page.contains("@media (forced-colors:active)"));
    assert!(page.contains("@media (prefers-reduced-motion:reduce)"));
    assert!(page.contains("document.activeElement"));
    assert!(page.contains("aria-live=\"polite\""));
    assert!(workbench.contains("KeyCode::Esc"));
    assert!(workbench.contains("KeyCode::Char('c')"));
    // The current TTY uses explicit, keyboard-reachable approval commands.
    assert!(workbench.contains("/approvals 查看，再明确批准或拒绝"));
    assert!(workbench.contains("/approve <approval_id> 或 /deny <approval_id>"));
    assert!(workbench.contains("decide_approval_envelope_on_host"));
    assert!(!page.contains("ModelClient"));
    assert!(!workbench.contains("CapabilityBroker"));
}
