use kiana_quality::{
    core_negative_fixture_matrix, validate_core_negative_fixture_matrix, FixtureError,
    FixtureFamily,
};

#[test]
fn core_negative_fixture_matrix_is_complete() {
    let fixtures = core_negative_fixture_matrix();
    validate_core_negative_fixture_matrix(&fixtures).unwrap();
    assert_eq!(fixtures.len(), 6);
    assert_eq!(fixtures[0].family, FixtureFamily::Runtime);
    assert_eq!(fixtures[5].family, FixtureFamily::Swarm);
    assert!(fixtures.iter().all(|fixture| {
        fixture.expected_status == "blocked"
            && fixture.forbidden_effects.contains(&"network".to_owned())
            && fixture.forbidden_effects.contains(&"secret".to_owned())
    }));
}

#[test]
fn matrix_rejects_missing_or_duplicate_family() {
    let mut fixtures = core_negative_fixture_matrix();
    fixtures.pop();
    assert_eq!(
        validate_core_negative_fixture_matrix(&fixtures).unwrap_err(),
        FixtureError::MatrixIncomplete
    );

    let mut duplicate = core_negative_fixture_matrix();
    duplicate[5].family = FixtureFamily::Runtime;
    assert_eq!(
        validate_core_negative_fixture_matrix(&duplicate).unwrap_err(),
        FixtureError::MatrixIncomplete
    );
}

#[test]
fn fixture_digest_and_forbidden_effects_are_bound() {
    let fixtures = core_negative_fixture_matrix();
    for fixture in fixtures {
        fixture.validate().unwrap();
        assert!(fixture.fixture_digest.starts_with("sha256:"));
        assert_eq!(fixture.trace.events.len(), 1);
        assert!(fixture.trace.events[0].value["side_effects"] == false);
    }
}
