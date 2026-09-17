use kiana_domain::*;

#[test]
fn expired_or_unowned_dataset_is_not_admitted() {
    let dataset = EvalDataset::new(
        "regression",
        "quality-owner",
        vec!["fixture:source".to_owned()],
        "internal",
        EvalSplit::Regression,
        vec![EvalCaseId::new()],
        100,
        Some(200),
    )
    .unwrap()
    .with_sampling_policy(3, vec!["red-team".to_owned(), "offline".to_owned()])
    .unwrap();
    assert_eq!(dataset.minimum_sample, 3);
    assert_eq!(dataset.workload_tags, vec!["offline", "red-team"]);
    dataset.validate_for_admission(199).unwrap();
    assert_eq!(
        dataset.validate_for_admission(200).unwrap_err(),
        "eval_dataset_expired"
    );

    let mut unowned = dataset.clone();
    unowned.owner_id.clear();
    unowned.dataset_digest = unowned.digest();
    assert_eq!(
        unowned.validate_for_admission(199).unwrap_err(),
        "eval_dataset_owner_invalid"
    );
}

#[test]
fn case_admission_binds_owner_expiry_sample_and_workload_tags() {
    let suite = EvalSuiteId::new();
    let case = EvalCase::new(
        suite,
        "fixture:case",
        None,
        serde_json::json!({"mode":"replay"}),
        vec!["run.completed".to_owned()],
        "completed",
        Vec::new(),
        Vec::new(),
        vec!["network".to_owned()],
        vec!["terminal".to_owned()],
        "restricted",
    )
    .unwrap()
    .with_admission_policy(
        "case-owner",
        2,
        vec!["nightly".to_owned(), "curated".to_owned()],
        Some(300),
    )
    .unwrap();
    assert_eq!(case.owner_id, "case-owner");
    assert_eq!(case.minimum_sample, 2);
    assert_eq!(case.workload_tags, vec!["curated", "nightly"]);
    case.validate_for_admission(299).unwrap();
    assert_eq!(
        case.validate_for_admission(300).unwrap_err(),
        "eval_case_expired"
    );
}

#[test]
fn privacy_and_tag_contracts_reject_unknown_or_noncanonical_values() {
    let case_id = EvalCaseId::new();
    assert_eq!(
        EvalDataset::new(
            "regression",
            "owner",
            vec!["fixture:source".to_owned()],
            "secret",
            EvalSplit::Regression,
            vec![case_id],
            1,
            None,
        )
        .unwrap_err(),
        "eval_dataset_privacy_invalid"
    );
    let mut dataset = EvalDataset::new(
        "regression",
        "owner",
        vec!["fixture:source".to_owned()],
        "internal",
        EvalSplit::Regression,
        vec![case_id],
        1,
        None,
    )
    .unwrap();
    dataset.workload_tags = vec!["z".to_owned(), "a".to_owned()];
    dataset.dataset_digest = dataset.digest();
    assert_eq!(
        dataset.validate().unwrap_err(),
        "eval_dataset_workload_tags_noncanonical"
    );
}
