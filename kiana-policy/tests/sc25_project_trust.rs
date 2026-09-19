use kiana_policy::{ProjectTrustDecision, ProjectTrustRoot, ProjectTrustScope, ProjectTrustState};

fn digest(byte: char) -> String {
    format!(
        "sha256:{}",
        std::iter::repeat(byte).take(64).collect::<String>()
    )
}

fn root(
    scope: ProjectTrustScope,
    state: ProjectTrustState,
    revision: u64,
    root_byte: char,
) -> ProjectTrustRoot {
    ProjectTrustRoot::new(
        scope,
        digest('p'),
        digest(root_byte),
        revision,
        state,
        format!("trust-audit:{scope:?}:{revision}"),
    )
    .unwrap()
}

#[test]
fn trusted_same_scope_root_allows_loading_and_emits_audit_digest() {
    let resolution = kiana_policy::ProjectTrustResolution::resolve(
        digest('p'),
        ProjectTrustScope::Project,
        vec![root(
            ProjectTrustScope::Project,
            ProjectTrustState::Trusted,
            2,
            'r',
        )],
    )
    .unwrap();
    assert_eq!(resolution.decision, ProjectTrustDecision::Allow);
    assert!(resolution.allows_loading());
    assert_eq!(resolution.selected_scope, Some(ProjectTrustScope::Project));
    resolution.validate().unwrap();
}

#[test]
fn missing_untrusted_unknown_and_scope_mismatch_are_denied() {
    let missing = kiana_policy::ProjectTrustResolution::resolve(
        digest('p'),
        ProjectTrustScope::Project,
        Vec::new(),
    )
    .unwrap();
    assert_eq!(missing.reason, "project_trust_root_missing");

    for state in [ProjectTrustState::Untrusted, ProjectTrustState::Unknown] {
        let denied = kiana_policy::ProjectTrustResolution::resolve(
            digest('p'),
            ProjectTrustScope::Project,
            vec![root(ProjectTrustScope::Project, state, 1, 'r')],
        )
        .unwrap();
        assert_eq!(denied.decision, ProjectTrustDecision::Deny);
        assert!(!denied.allows_loading());
    }

    let mismatch = kiana_policy::ProjectTrustResolution::resolve(
        digest('p'),
        ProjectTrustScope::Project,
        vec![root(
            ProjectTrustScope::KianaHome,
            ProjectTrustState::Trusted,
            1,
            'r',
        )],
    )
    .unwrap();
    assert_eq!(mismatch.reason, "project_trust_root_missing");
}

#[test]
fn same_revision_conflict_fails_closed_and_tampered_digest_is_rejected() {
    let conflict = kiana_policy::ProjectTrustResolution::resolve(
        digest('p'),
        ProjectTrustScope::Project,
        vec![
            root(
                ProjectTrustScope::Project,
                ProjectTrustState::Trusted,
                3,
                'a',
            ),
            root(
                ProjectTrustScope::Project,
                ProjectTrustState::Untrusted,
                3,
                'b',
            ),
        ],
    )
    .unwrap();
    assert_eq!(conflict.reason, "project_trust_conflict");
    assert_eq!(conflict.decision, ProjectTrustDecision::Deny);

    let mut forged = root(
        ProjectTrustScope::Project,
        ProjectTrustState::Trusted,
        1,
        'r',
    );
    forged.root_digest = digest('z');
    assert_eq!(
        forged.validate().unwrap_err(),
        "project_trust_root_digest_mismatch"
    );
}
