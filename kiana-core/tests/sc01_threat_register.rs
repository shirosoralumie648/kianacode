#[test]
fn threat_register_is_explicit_and_deny_first_without_claiming_enforcement() {
    let register = include_str!("../../docs/roadmap/security-threat-register.md");
    let baseline = include_str!("../../docs/roadmap/security-compliance-baseline.md");
    let constitution = include_str!("../../docs/company-os-security-constitution.md");
    let roadmap = include_str!("../../docs/roadmap/security-compliance.md");
    let crosswalk = include_str!("../../docs/roadmap/security-control-crosswalk.md");
    for marker in [
        "SC01:T01",
        "SC01:T01–SC01:T12",
        "SC01:T02",
        "SC01:T03",
        "SC01:T04",
        "SC01:T05",
        "SC01:T06",
        "SC01:T07",
        "SC01:T08",
        "SC01:T09",
        "SC01:T10",
        "SC01:T11",
        "SC01:T12",
        "COMPLIANCE-3.4:T01",
        "COMPLIANCE-3.4:T12",
        "not identifier aliases",
        "Crosswalk to `security-compliance.md` section 3.4",
        "Impact if successful",
        "Owner area / roadmap steps",
        "indirect prompt injection",
        "supply-chain substitution",
        "retention/deletion boundary violation",
        "CI-only security fixture catalog",
        "project_prompt_injection_is_untrusted_data",
        "unverified_extension_dependency_is_quarantined",
        "retention_and_tombstone_scope_violation_is_denied",
        "partial",
        "planned",
        "kiana-core/tests/sc37_deny_matrix.rs::sc37_privilege_escalation_forged_role_is_denied_with_zero_dispatch",
        "kiana-core/tests/sc37_deny_matrix.rs::sc37_toctou_fence_scope_drift_is_denied_with_zero_dispatch",
        "kiana-core/tests/sc37_deny_matrix.rs::sc37_leakage_secret_sentinel_in_receipt_text_is_denied_with_zero_dispatch",
        "kiana-core/tests/sc37_deny_matrix.rs::sc37_replay_unbound_permit_is_denied_with_zero_dispatch",
        "kiana-core/tests/sc37_deny_matrix.rs::sc37_deletion_legal_hold_is_denied_with_zero_dispatch",
        "kiana-core/tests/sc37_deny_matrix.rs::sc37_deletion_unknown_retention_is_denied_with_zero_dispatch",
        "kiana-core/tests/sc39_red_team_corpus.rs::sc39_prompt_injection_cannot_become_product_authority",
        "kiana-core/tests/sc39_red_team_corpus.rs::sc39_indirect_injection_from_repository_text_cannot_widen_a_grant",
        "kiana-core/tests/sc39_red_team_corpus.rs::sc39_malicious_plugin_manifest_cannot_self_authorize",
        "kiana-core/tests/sc39_red_team_corpus.rs::sc39_secret_exfiltration_cannot_leave_the_broker",
        "EventLog facts are authoritative",
        "model/UI",
        "external/physical",
        "source snapshot",
        "limitations",
    ] {
        assert!(
            register.contains(marker),
            "threat register marker missing: {marker}"
        );
    }
    let threat_rows: Vec<_> = register
        .lines()
        .filter(|line| line.starts_with("| SC01:T"))
        .collect();
    assert_eq!(threat_rows.len(), 12, "SC-01 threat rows must be complete");
    for number in 1..=12 {
        let id = format!("| SC01:T{number:02} | ");
        assert_eq!(
            threat_rows.iter().filter(|row| row.starts_with(&id)).count(),
            1,
            "SC-01 threat ID must occur exactly once: {id}"
        );
    }
    for row in threat_rows {
        let columns: Vec<_> = row.split('|').map(str::trim).collect();
        assert_eq!(columns.len(), 9, "SC-01 threat row has an invalid shape");
        assert!(!columns[3].is_empty(), "SC-01 threat impact is required");
        assert!(!columns[7].is_empty(), "SC-01 owner area is required");
    }
    let legacy_crosswalk_rows: Vec<_> = register
        .lines()
        .filter(|line| line.starts_with("| COMPLIANCE-3.4:T"))
        .collect();
    assert_eq!(
        legacy_crosswalk_rows.len(),
        12,
        "all section 3.4 identifiers need an explicit crosswalk row"
    );
    for number in 1..=12 {
        let id = format!("| COMPLIANCE-3.4:T{number:02} | ");
        assert_eq!(
            legacy_crosswalk_rows
                .iter()
                .filter(|row| row.starts_with(&id))
                .count(),
            1,
            "section 3.4 ID must have exactly one crosswalk row: {id}"
        );
    }
    assert!(register.contains(
        "COMPLIANCE-3.4:T02 | confused deputy | SC01:T01, SC01:T03, SC01:T12"
    ));
    for marker in ["SC01:T01", "SC01:T02", "SC01:T10", "not aliases"] {
        assert!(crosswalk.contains(marker), "SC-34 namespace marker missing: {marker}");
    }
    for marker in ["T02 confused deputy", "T07 SSRF"] {
        assert!(
            roadmap.contains(marker),
            "normative section 3.4 threat definition missing: {marker}"
        );
    }
    let fixture_rows: Vec<_> = register
        .lines()
        .filter(|line| line.starts_with("| `"))
        .collect();
    assert_eq!(fixture_rows.len(), 14, "fixture catalog rows must be complete");
    for row in fixture_rows {
        let columns: Vec<_> = row.split('|').map(str::trim).collect();
        assert_eq!(columns.len(), 8, "fixture catalog row has an invalid shape");
        assert!(
            columns[5].contains("planned") || columns[5].contains("partial"),
            "fixture coverage must be marked planned or partial: {row}"
        );
        assert!(!columns[6].is_empty(), "fixture owner steps are required");
    }
    assert!(baseline.contains("SC-00"));
    assert!(baseline.contains("SC01:T01–SC01:T12"));
    assert!(constitution.contains("SEC-01"));
    assert!(roadmap.contains("step-sc-01"));
    assert!(!register.contains("security certification"));
    assert!(!register.contains("feature_status=live"));
}
