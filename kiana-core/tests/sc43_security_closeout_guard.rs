//! SC-43 source guard for security review, module map and status handoff.

#[test]
fn sc43_keeps_review_proof_limits_and_next_actions_explicit() {
    let review = include_str!("../../docs/security/sc43-security-review.md");
    let module_map = include_str!("../../docs/module-map.md");
    let status = include_str!("../../CURRENT_STATUS.md");
    let roadmap = include_str!("../../docs/roadmap.md");
    let baseline = include_str!("../../docs/roadmap/sc43-security-closeout-baseline.md");
    let workflow = include_str!("../../.github/workflows/sc43-security-closeout.yml");
    let script = include_str!("../../scripts/validate-sc43-security-closeout.sh");
    for marker in [
        "review date",
        "reviewer",
        "source snapshot",
        "feature_status",
        "proof_level",
        "partial",
        "source",
        "result_unknown",
        "reconcile",
        "durable",
        "live",
        "physical",
        "Open risks",
        "Explicit non-claims",
        "SupplyChainReleaseEvidence",
        "ReleaseUatEvidence",
        "PersistenceUatEvidence",
        "PersistenceCapacityEvidence",
    ] {
        assert!(
            review.contains(marker),
            "SC-43 review marker missing: {marker}"
        );
    }
    for marker in ["SC-43", "security", "CURRENT_STATUS"] {
        assert!(
            module_map.contains(marker),
            "module map marker missing: {marker}"
        );
    }
    for marker in [
        "validate-sc43-security-closeout.sh",
        "cargo fmt --all --check",
        "cargo check --workspace --tests --locked",
        "cargo test -p kiana-core --test sc43_security_closeout_guard",
        "git diff --check",
    ] {
        assert!(
            workflow.contains(marker),
            "SC-43 workflow marker missing: {marker}"
        );
    }
    for marker in [
        "review record",
        "module map",
        "CURRENT_STATUS",
        "partial",
        "source",
        "limitations",
    ] {
        assert!(
            baseline.contains(marker),
            "SC-43 baseline marker missing: {marker}"
        );
    }
    assert!(status.contains("### SC-43"));
    assert!(roadmap.contains("<a id=\"step-sc-43\"></a>SC-43"));
    assert!(script.contains("feature_status"));
    assert!(script.contains("Explicit non-claims"));
    assert!(!review.contains("compliance certification: approved"));
}
