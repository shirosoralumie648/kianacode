use kiana_domain::*;

#[test]
fn every_event_receipt_recovery_boundary_is_unknown_or_rejected_and_replayable() {
    let matrix = Er31FaultMatrix::new(31, 8, vec!["event:1".to_owned(), "event:2".to_owned()])
        .expect("ER-31 matrix");
    let replay = Er31FaultMatrix::new(31, 8, vec!["event:2".to_owned(), "event:1".to_owned()])
        .expect("deterministic replay");
    assert_eq!(matrix, replay);
    assert_eq!(matrix.cases.len(), ER31_FAULT_MAX_CASES);
    assert!(matrix.cases.iter().all(|case| {
        !case.duplicate_effect
            && !case.false_success
            && (case.disposition != Er31Disposition::Unknown || case.resources_fenced)
    }));
    matrix.validate().expect("matrix validates");
}

#[test]
fn forged_success_duplicate_effect_and_missing_unknown_limitation_are_rejected() {
    let mut matrix = Er31FaultMatrix::new(32, 2, vec!["event:1".to_owned()]).expect("matrix");
    matrix.cases[0].false_success = true;
    matrix.cases[0].case_digest = matrix.cases[0].digest();
    matrix.matrix_digest = matrix.digest();
    assert_eq!(
        matrix.validate().unwrap_err(),
        "er31_fault_case_header_or_safety_invalid"
    );

    let mut matrix = Er31FaultMatrix::new(33, 2, vec!["event:1".to_owned()]).expect("matrix");
    let unknown = matrix
        .cases
        .iter_mut()
        .find(|case| case.disposition == Er31Disposition::Unknown)
        .expect("unknown case");
    unknown.resources_fenced = false;
    unknown.case_digest = unknown.digest();
    matrix.matrix_digest = matrix.digest();
    assert_eq!(
        matrix.validate().unwrap_err(),
        "er31_unknown_case_fence_or_limit_missing"
    );

    let mut matrix = Er31FaultMatrix::new(34, 2, vec!["event:1".to_owned()]).expect("matrix");
    let unknown = matrix
        .cases
        .iter_mut()
        .find(|case| case.disposition == Er31Disposition::Unknown)
        .expect("unknown case");
    unknown.receipt_limitations.clear();
    unknown.case_digest = unknown.digest();
    matrix.matrix_digest = matrix.digest();
    assert_eq!(
        matrix.validate().unwrap_err(),
        "er31_unknown_case_fence_or_limit_missing"
    );
}

#[test]
fn duplicate_source_refs_and_matrix_drift_fail_closed() {
    assert_eq!(
        Er31FaultMatrix::new(35, 2, vec!["event:1".to_owned(), "event:1".to_owned()]).unwrap_err(),
        "er31_fault_source_duplicate"
    );
    let mut matrix = Er31FaultMatrix::new(36, 2, vec!["event:1".to_owned()]).expect("matrix");
    matrix.cases.pop();
    matrix.matrix_digest = matrix.digest();
    assert_eq!(
        matrix.validate().unwrap_err(),
        "er31_fault_matrix_header_invalid"
    );
}
