use kiana_domain::{LegalHold, RetentionPolicy};
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn retention_policy_requires_a_finite_default_window() {
    assert_eq!(
        RetentionPolicy::new("/repo", 1, 1, 0, BTreeMap::new()).unwrap_err(),
        "retention_policy_finite_window_required"
    );
}

#[test]
fn legal_hold_and_policy_contracts_round_trip_without_losing_digest() {
    let policy = RetentionPolicy::new("/repo", 4, 2, 100, BTreeMap::new()).unwrap();
    let hold = LegalHold::new(
        "hold-1",
        "/repo",
        BTreeSet::from(["src/private.txt".to_owned()]),
        "regulatory review",
        "principal:operator",
        10,
        policy.revision,
        true,
    )
    .unwrap();
    let encoded = serde_json::to_string(&hold).unwrap();
    let decoded: LegalHold = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, hold);
    assert!(decoded.covers("/repo", "src/private.txt"));
    assert!(!decoded.covers("/other", "src/private.txt"));
    assert_ne!(decoded.hold_digest, format!("sha256:{}", "0".repeat(64)));
}
