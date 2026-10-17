# SC-25 ProjectTrust root baseline

## 2026-10-17 completion slice (CI-only)

- `kiana-policy::ProjectTrustPlan` 对同一项目逐层求值 `Project` → `KianaHome` → `User`
  三个 scope（`ProjectTrustScope::precedence_order`），并把它作为不变量写进 `validate()`：
  整体 `decision`/`reason` 恒等于 `requested_scope` 那一层的结论
  （`project_trust_plan_requested_scope_mismatch`）。低层 Trusted 永远不能放行请求层资源，
  高层 Untrusted 也不连带否决别的层自己的资源。
- `ProjectTrustResolution::load_audit` / `authorize_loading` 产出**加载前审计**
  `ProjectTrustLoadAudit`：Allow 和 Deny 都产出；Allow 要求
  `selected_scope == Some(requested_scope)`；资源根摘要必须是规范 `sha256:` 摘要，
  原始路径（如 `.claude/skills`）被 `project_trust_resource_digest_invalid` 拒绝。
- 生产默认权威 `kiana-daemon::StoredProjectTrustAuthority` 现在把 `kiana-types` v2 信任记录
  映射成 `ProjectTrustScope::Project` 的 `ProjectTrustResolution`：缺失记录 → 空候选 →
  `project_trust_root_missing`；明确不可信 → `project_trust_untrusted`；可信 → Allow。
  读记录失败仍是 `Err`（daemon 映射 `project_trust_unavailable`），与"记录缺失"区分。
  旧布尔闸门 `project_trusted` 改为 `allows_loading()` 的结果，语义保持不变。
- `ProjectTrustAuthority` 新增默认方法 `project_trust_resolution`（默认 `Ok(None)`）与
  `project_trust_load_audit`（默认委托前者并绑定 `PROJECT_TRUST_AUDIT_REF`）；
  只实现旧 `project_trusted` 的注入权威（测试/兼容）不受影响。
- `DaemonHost::project_trust_load_audit` / `authorize_project_trust_load` 是新的加载前闸门：
  拒绝时返回审计里的稳定策略原因码（不塌缩成通用 I/O 错误）；权威没有策略判定时
  fail-closed（`project_trust_policy_unavailable`），不退回布尔开关。

### Fixtures

`kiana-policy/tests/sc25_project_trust.rs`（整 target 运行，另有逐夹具 `--exact`）：
- `scope_precedence_order_is_fixed_and_not_a_permission_grant`
- `lower_scope_trust_cannot_unlock_project_resources`
- `project_scope_trust_cannot_unlock_user_scoped_resources`
- `tampered_plan_cannot_relabel_a_scope_decision_as_the_overall_decision`
- `raw_paths_zero_revisions_empty_audits_and_foreign_projects_are_denied`
- `project_trust_root_limit_is_enforced_before_any_resolution`
- `load_audit_records_allow_and_deny_and_authorize_matches_the_decision`
- `load_audit_rejects_raw_resource_paths_and_tampering`
- 既有 `trusted_same_scope_root_allows_loading_and_emits_audit_digest`、
  `missing_untrusted_unknown_and_scope_mismatch_are_denied`、
  `same_revision_conflict_fails_closed_and_tampered_digest_is_rejected` 保留。

`kiana-policy/tests/sc25_project_trust_guard.rs`：`sc25_policy_keeps_trust_resolution_pure_and_deny_first`
把 plan/load-audit/schema/原因码 marker 与"无文件系统访问"钉在源码上。

`kiana-daemon/tests/sc25_project_trust_wiring.rs`：
- `stored_authority_deny_first_reasons_and_bool_compatibility`
- `daemon_gate_denies_before_allow_and_keeps_policy_reasons`
- `daemon_gate_refuses_to_fallback_when_the_authority_has_no_policy_resolution`
- `sc25_daemon_wiring_and_lane_are_pinned`

接入 `.github/workflows/sc25-project-trust.yml`（手动 `workflow_dispatch`；`cargo fmt --all --check`
+ 三个 target + 逐夹具 `--exact --test-threads=1` + 三个 `--no-run`）。统一 `ci.yml` 的
`kiana-policy` / `kiana-daemon` 分片仍是整 crate（`targets: null`），自动涵盖三个 target。

## Delivered source slice (2026-09-19)

- `kiana-policy` now defines versioned `ProjectTrustRoot` and `ProjectTrustResolution` values
  with separate User, KIANA_HOME and Project scopes and explicit source priority.
- Resolution is bound to a canonical project-root digest and same-scope roots only. Missing,
  unknown, untrusted and same-revision conflicting roots return stable deny reasons.
- Root and resolution records carry redacted audit references and self-verifying digests, so a
  loader can record the decision before reading skills, plugins, hooks or MCP configuration.
- Existing filesystem adapters remain responsible for obtaining canonical/root digests and for
  invoking the policy contract; this step does not add a second loader or execution path.

## Boundary and proof ceiling

The policy contract does not inspect the filesystem, load project resources or grant capability
permissions. The daemon adapter now maps the single `kiana-types` project trust store into a
`ProjectTrustScope::Project` resolution, so the existing boolean gate runs through the policy
decision; User/KIANA_HOME scope adapters, the skill/plugin/hook/MCP loaders' use of
`authorize_loading`, signed trust roots and cross-process recovery still require later
integration evidence (SC-26/SC-27). This slice does not claim live signed trust roots or
production authentication. All evidence here is source-level with CI pending; no local test
run and no CI receipt was waited for.
