#[test]
fn security_baseline_covers_constitution_assets_and_spine() {
    let constitution = include_str!("../../docs/company-os-security-constitution.md");
    let design = include_str!("../../docs/roadmap/security-compliance.md");
    let baseline = include_str!("../../docs/roadmap/security-compliance-baseline.md");
    let current = include_str!("../../CURRENT_STATUS.md");
    let module_map = include_str!("../../docs/module-map.md");
    let roadmap = include_str!("../../docs/roadmap.md");

    for control in [
        "SEC-01", "SEC-02", "SEC-03", "SEC-04", "SEC-05", "SEC-06", "SEC-07", "SEC-08", "SEC-09",
        "SEC-10", "SEC-11", "SEC-12",
    ] {
        assert!(
            constitution.contains(control),
            "missing constitution control {control}"
        );
    }
    for threat in [
        "T01", "T02", "T03", "T04", "T05", "T06", "T07", "T08", "T09", "T10", "T11", "T12",
    ] {
        assert!(design.contains(threat), "missing threat {threat}");
    }
    for step in ["SC-00", "SC-01", "SC-12", "SC-24", "SC-30", "SC-43"] {
        assert!(design.contains(step), "missing security step {step}");
    }
    for anchor in [
        "kiana-entrypoints",
        "kiana-client / kiana-protocol",
        "kiana-daemon::DaemonHost",
        "kiana-core::ControlPlane",
        "kiana-capability-broker / kiana-runner",
        "EventLog",
        "Receipt",
    ] {
        assert!(
            baseline.contains(anchor),
            "missing execution-spine anchor {anchor}"
        );
    }
    assert!(baseline.contains("资产、入口、信任边界"));
    assert!(baseline.contains("proof_level"));
    assert!(baseline.contains("not_supported"));
    assert!(baseline.contains("SecretRef"));
    assert!(baseline.contains("call_id"));
    assert!(baseline.contains("不在本地运行测试"));
    assert!(module_map.contains("安全宪法"));
    assert!(module_map.contains("security-compliance-baseline.md"));
    assert!(roadmap.contains("SC-00"));
    assert!(current.contains("partial"));
    assert!(current.contains("proof-level"));
}

#[test]
fn security_baseline_preserves_partial_and_unsupported_boundaries() {
    let baseline = include_str!("../../docs/roadmap/security-compliance-baseline.md");
    for boundary in [
        "OS credential/OAuth/tenant",
        "SecretRef/SecretStore",
        "跨进程 Resume",
        "not_supported",
        "live/physical",
        "不把类型/单测/CI 文件存在写成 enforcement",
    ] {
        assert!(
            baseline.contains(boundary),
            "boundary was erased: {boundary}"
        );
    }
}
