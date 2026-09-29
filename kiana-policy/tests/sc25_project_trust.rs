use kiana_policy::{ProjectTrustDecision, ProjectTrustRoot, ProjectTrustScope, ProjectTrustState};

/// 由 seed 的码位按十六进制推导摘要本体。
///
/// 【为什么不能直接重复 seed】
/// `ProjectTrustRoot` 的 `project_root_digest` 要经 `valid_digest` 校验：
/// 必须是 `sha256:` 加 64 位 hex。把 seed 原样重复只有在 seed 本身是十六进制位时才成立，
/// 而这里用的 `'p'` / `'r'` / `'z'` 都不是——于是生成出来的「摘要」被校验器正确拒绝，
/// `project_trust_project_digest_invalid`，三个用例全灭。
/// 码位按十六进制展开后对任意 ASCII seed 都恰好 64 位 hex，且保持单射，
/// 不同 seed 仍得到不同摘要——这些用例依赖的正是这种差异。
fn digest(byte: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (byte as u32) & 0xffff).repeat(16)
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
