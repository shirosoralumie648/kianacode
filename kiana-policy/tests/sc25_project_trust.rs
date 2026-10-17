use kiana_policy::{
    ProjectTrustDecision, ProjectTrustPlan, ProjectTrustResolution, ProjectTrustRoot,
    ProjectTrustScope, ProjectTrustState,
};

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

/// 在指定项目摘要下构造信任根。项目摘要可替换，用于覆盖"跨项目记录"拒绝路径。
fn root_in(
    project_digest: String,
    scope: ProjectTrustScope,
    state: ProjectTrustState,
    revision: u64,
    root_byte: char,
) -> ProjectTrustRoot {
    ProjectTrustRoot::new(
        scope,
        project_digest,
        digest(root_byte),
        revision,
        state,
        format!("trust-audit:{scope:?}:{revision}"),
    )
    .unwrap()
}

fn root(
    scope: ProjectTrustScope,
    state: ProjectTrustState,
    revision: u64,
    root_byte: char,
) -> ProjectTrustRoot {
    root_in(digest('p'), scope, state, revision, root_byte)
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

#[test]
fn scope_precedence_order_is_fixed_and_not_a_permission_grant() {
    assert_eq!(
        ProjectTrustScope::precedence_order(),
        [
            ProjectTrustScope::Project,
            ProjectTrustScope::KianaHome,
            ProjectTrustScope::User,
        ]
    );
    assert!(ProjectTrustScope::Project.priority() > ProjectTrustScope::KianaHome.priority());
    assert!(ProjectTrustScope::KianaHome.priority() > ProjectTrustScope::User.priority());
}

#[test]
fn lower_scope_trust_cannot_unlock_project_resources() {
    // 路径伪造 trust 的形状：User/KianaHome 两层都明确 Trusted，但项目层自己的记录不可信。
    let plan = ProjectTrustPlan::resolve(
        digest('p'),
        ProjectTrustScope::Project,
        vec![
            root(ProjectTrustScope::User, ProjectTrustState::Trusted, 5, 'u'),
            root(
                ProjectTrustScope::KianaHome,
                ProjectTrustState::Trusted,
                5,
                'h',
            ),
            root(
                ProjectTrustScope::Project,
                ProjectTrustState::Untrusted,
                1,
                'r',
            ),
        ],
    )
    .unwrap();

    // 整体结论只取请求层（Project）：低层的 Trusted 不能把项目层抬成 Allow。
    assert_eq!(plan.decision, ProjectTrustDecision::Deny);
    assert_eq!(plan.reason, "project_trust_untrusted");
    assert!(!plan.allows_loading());
    assert_eq!(
        plan.scope_decisions
            .iter()
            .map(|decision| decision.scope)
            .collect::<Vec<_>>(),
        vec![
            ProjectTrustScope::Project,
            ProjectTrustScope::KianaHome,
            ProjectTrustScope::User,
        ],
    );
    assert_eq!(
        plan.scope_decisions[1].decision,
        ProjectTrustDecision::Allow
    );
    assert_eq!(
        plan.scope_decisions[2].decision,
        ProjectTrustDecision::Allow
    );
    plan.validate().unwrap();
}

#[test]
fn project_scope_trust_cannot_unlock_user_scoped_resources() {
    // 反方向同样成立：项目层 Trusted 不能替 User 层的缺失记录兜底。
    let plan = ProjectTrustPlan::resolve(
        digest('p'),
        ProjectTrustScope::User,
        vec![root(
            ProjectTrustScope::Project,
            ProjectTrustState::Trusted,
            1,
            'r',
        )],
    )
    .unwrap();
    assert_eq!(plan.decision, ProjectTrustDecision::Deny);
    assert_eq!(plan.reason, "project_trust_root_missing");
    assert!(!plan.allows_loading());
    plan.validate().unwrap();
}

#[test]
fn tampered_plan_cannot_relabel_a_scope_decision_as_the_overall_decision() {
    let mut plan = ProjectTrustPlan::resolve(
        digest('p'),
        ProjectTrustScope::Project,
        vec![root(
            ProjectTrustScope::Project,
            ProjectTrustState::Trusted,
            1,
            'r',
        )],
    )
    .unwrap();
    // 整体结论被手工改成 Deny；即使摘要重算自洽，"来自请求层"的不变量仍必须拒绝。
    plan.decision = ProjectTrustDecision::Deny;
    plan.reason = "project_trust_untrusted".to_owned();
    plan.plan_digest = plan.digest();
    assert_eq!(
        plan.validate().unwrap_err(),
        "project_trust_plan_requested_scope_mismatch"
    );
}

#[test]
fn raw_paths_zero_revisions_empty_audits_and_foreign_projects_are_denied() {
    // 根摘要位置放真实路径：必须先于任何判定被格式校验拒绝。
    assert_eq!(
        ProjectTrustRoot::new(
            ProjectTrustScope::Project,
            digest('p'),
            ".claude/skills",
            1,
            ProjectTrustState::Trusted,
            "trust-audit:project:1",
        )
        .unwrap_err(),
        "project_trust_root_digest_invalid"
    );
    // revision 0 是"从未正式写入过"的声明。
    assert_eq!(
        ProjectTrustRoot::new(
            ProjectTrustScope::Project,
            digest('p'),
            digest('r'),
            0,
            ProjectTrustState::Trusted,
            "trust-audit:project:0",
        )
        .unwrap_err(),
        "project_trust_root_header_invalid"
    );
    // 没有来源引用的信任声明不成立。
    assert_eq!(
        ProjectTrustRoot::new(
            ProjectTrustScope::Project,
            digest('p'),
            digest('r'),
            1,
            ProjectTrustState::Trusted,
            "   ",
        )
        .unwrap_err(),
        "project_trust_audit_ref_invalid"
    );

    // 另一项目（digest('q')）的 Trusted 记录不能放行本项目（digest('p')）。
    let foreign = ProjectTrustResolution::resolve(
        digest('p'),
        ProjectTrustScope::Project,
        vec![root_in(
            digest('q'),
            ProjectTrustScope::Project,
            ProjectTrustState::Trusted,
            1,
            'r',
        )],
    )
    .unwrap();
    assert_eq!(foreign.decision, ProjectTrustDecision::Deny);
    assert_eq!(foreign.reason, "project_trust_root_missing");
    assert!(foreign.candidates.is_empty());
}

#[test]
fn project_trust_root_limit_is_enforced_before_any_resolution() {
    let roots = (0..kiana_policy::MAX_PROJECT_TRUST_ROOTS)
        .map(|index| {
            root(
                ProjectTrustScope::Project,
                ProjectTrustState::Trusted,
                1,
                (b'a' + index as u8) as char,
            )
        })
        .collect::<Vec<_>>();
    let at_limit =
        ProjectTrustResolution::resolve(digest('p'), ProjectTrustScope::Project, roots.clone())
            .unwrap();
    assert_eq!(
        at_limit.candidates.len(),
        kiana_policy::MAX_PROJECT_TRUST_ROOTS
    );
    assert_eq!(at_limit.decision, ProjectTrustDecision::Allow);

    let mut over_limit = roots;
    over_limit.push(root(
        ProjectTrustScope::Project,
        ProjectTrustState::Trusted,
        1,
        (b'a' + kiana_policy::MAX_PROJECT_TRUST_ROOTS as u8) as char,
    ));
    assert_eq!(
        ProjectTrustResolution::resolve(
            digest('p'),
            ProjectTrustScope::Project,
            over_limit.clone()
        )
        .unwrap_err(),
        "project_trust_root_limit"
    );
    assert_eq!(
        ProjectTrustPlan::resolve(digest('p'), ProjectTrustScope::Project, over_limit).unwrap_err(),
        "project_trust_root_limit"
    );

    assert_eq!(
        ProjectTrustResolution::resolve("not-a-digest", ProjectTrustScope::Project, Vec::new())
            .unwrap_err(),
        "project_trust_project_digest_invalid"
    );
}

#[test]
fn load_audit_records_allow_and_deny_and_authorize_matches_the_decision() {
    let allowed = ProjectTrustResolution::resolve(
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
    let allow_audit = allowed
        .authorize_loading(digest('s'), "load-audit:skills:1")
        .unwrap();
    assert_eq!(allow_audit.decision, ProjectTrustDecision::Allow);
    assert_eq!(allow_audit.selected_scope, Some(ProjectTrustScope::Project));
    assert_eq!(allow_audit.resource_root_digest, digest('s'));
    assert_eq!(allow_audit.audit_ref, "load-audit:skills:1");
    allow_audit.validate().unwrap();
    let encoded = serde_json::to_string(&allow_audit).unwrap();
    let decoded: kiana_policy::ProjectTrustLoadAudit = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, allow_audit);
    decoded.validate().unwrap();

    let missing =
        ProjectTrustResolution::resolve(digest('p'), ProjectTrustScope::Project, Vec::new())
            .unwrap();
    assert_eq!(
        missing
            .authorize_loading(digest('s'), "load-audit:skills:2")
            .unwrap_err(),
        "project_trust_root_missing"
    );
    // 拒绝也必须留下审计："决定已作出"与"允许加载"是两件事。
    let deny_audit = missing
        .load_audit(digest('s'), "load-audit:skills:2")
        .unwrap();
    assert_eq!(deny_audit.decision, ProjectTrustDecision::Deny);
    assert_eq!(deny_audit.reason, "project_trust_root_missing");
    assert!(deny_audit.selected_scope.is_none());
    deny_audit.validate().unwrap();
}

#[test]
fn load_audit_rejects_raw_resource_paths_and_tampering() {
    let allowed = ProjectTrustResolution::resolve(
        digest('p'),
        ProjectTrustScope::Project,
        vec![root(
            ProjectTrustScope::Project,
            ProjectTrustState::Trusted,
            1,
            'r',
        )],
    )
    .unwrap();
    assert_eq!(
        allowed
            .load_audit(".claude/skills", "load-audit:skills:3")
            .unwrap_err(),
        "project_trust_resource_digest_invalid"
    );

    // Allow 记录缺选中 scope：不能把"没有命中"的记录当放行凭据。
    let mut missing_selection = allowed
        .load_audit(digest('s'), "load-audit:skills:3")
        .unwrap();
    missing_selection.selected_scope = None;
    assert_eq!(
        missing_selection.validate().unwrap_err(),
        "project_trust_allow_selection_missing"
    );

    // 改审计引用而不重算摘要：自校验必须发现。
    let mut tampered = allowed
        .load_audit(digest('s'), "load-audit:skills:4")
        .unwrap();
    tampered.audit_ref = "load-audit:forged".to_owned();
    assert_eq!(
        tampered.validate().unwrap_err(),
        "project_trust_audit_digest_mismatch"
    );
}
