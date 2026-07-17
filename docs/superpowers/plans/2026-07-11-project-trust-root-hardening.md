# Project Trust 信任根加固 Implementation Plan

> **Execution rule:** This project does not use TDD. Implement each approved trust-root contract first, then run focused, adversarial, integration, and release verification. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Project Trust 从项目内 fail-open 模型迁移为项目外 fail-closed 用户信任根，并让 Unknown 状态统一约束项目资源和 mutating Tool。

**Architecture:** `kiana-types::trust` 负责项目身份、外部记录与决策优先级；命令、app-server、resource loaders 和 permission engine 只消费同一 effective trust。旧项目内 trust 文件只作为 ignored legacy 诊断，不参与授权。

**Tech Stack:** Rust、Serde JSON、SHA-256、现有 Command/App Server/Permission/Skill/Hook/MCP contracts。

---

### Task 1: Core Trust Contract Implementation

**Files:**
- Modify: `kiana-types/src/trust.rs`
- Modify: `kiana-types/src/lib.rs`
- Modify: `kiana-types/Cargo.toml`
- Modify: `Cargo.lock`

- [ ] 增加 `ProjectTrust::Unknown`，更新 `as_str/as_bool/allows_project_resources`。
- [ ] 增加 `project_trust_root`、`project_trust_id`、`project_trust_file_path`、`legacy_project_trust_file_path`。
- [ ] 将 `ProjectTrustRecord` 固定为 `kiana.project-trust.v2`，校验 project id/root 并拒绝未知字段。
- [ ] 将 read/write/remove 改为 `$KIANA_HOME/trust/projects/<project_id>.json`；写入使用同目录临时文件和权限收紧。
- [ ] 让 app-state 合法显式决定优先，其次 user store，缺失或无效均为 Unknown。

### Task 2: Core Trust Contract Verification

**Files:**
- Modify: `kiana-types/src/trust.rs`
- Modify: `kiana-types/Cargo.toml`

- [ ] 新增 `missing_decision_is_unknown_and_disallows_project_resources`，断言空 store 返回 `ProjectTrust::Unknown`、`as_bool=false`、`allows_project_resources=false`。
- [ ] 新增 `project_local_trust_file_cannot_authorize_project`，在 fixture 写 `.kiana/trust.json` 的 `trusted:true`，断言仍为 Unknown。
- [ ] 新增 `user_store_trust_record_authorizes_canonical_git_root`，设置隔离 `KIANA_HOME`，从 repo root 写 trust，再从 nested cwd 读取为 Trusted。
- [ ] 新增 `nested_git_repo_does_not_inherit_parent_trust`，断言最近 `.git` 根产生不同 project id。
- [ ] 新增 `tampered_user_store_record_fails_closed`，修改 `project_root` 或 `project_id` 后断言读取报错且 effective trust 为 Unknown。
- [ ] 运行 `cargo test -p kiana-types trust --locked --offline` 和 `cargo test -p kiana-types --locked --offline`，确认实现通过。

### Task 3: Trust CLI And Status Contract

**Files:**
- Modify: `kiana-commands/src/trust.rs`
- Modify: `docs/schemas/kiana-app-server-trust-status.v1.schema.json`
- Modify: `scripts/schema-contract-smoke.sh`

- [ ] 更新 text/JSON status，输出 project id/root、user store、legacy ignored 和读取错误。
- [ ] 更新 schema enum 与 required fields，并给 schema smoke fixture 增加 Unknown 示例。
- [ ] 增加 command focused verification：隔离 `KIANA_HOME`；初始状态为 Unknown；trust/untrust 写项目外记录；reset 回到 Unknown；legacy 文件显示 ignored。
- [ ] 运行 command tests 与 `bash scripts/schema-contract-smoke.sh`。

### Task 4: Permission And Resource Gates

**Files:**
- Modify: `kiana-tools/src/permissions.rs`
- Modify: `kiana-query/src/stop_hooks.rs`
- Modify: `kiana-skills/src/lib.rs`
- Modify: `kiana-tools/src/mcp_tool.rs`
- Modify: `kiana-tools/src/agent.rs`

- [ ] 将 mutating gate 改为 `!project_trust.allows_project_resources()`，错误信息区分 unknown/untrusted。
- [ ] 保持 managed/user allow 规则位于 trust gate 之后，不能绕过。
- [ ] 增加 Unknown focused/adversarial verification：mutating Tool deny/read-only allow；project hooks/skills/MCP/agents 不加载。
- [ ] 运行：
  - `cargo test -p kiana-tools permissions --locked --offline`
  - `cargo test -p kiana-query project_trust_source_boundary --locked --offline`
  - `cargo test -p kiana-skills --lib --locked --offline`
  - `cargo test -p kiana-tools mcp --locked --offline`
  - `cargo test -p kiana-tools agent --locked --offline`

### Task 5: Runtime And App Server Surfaces

**Files:**
- Modify: `kiana-entrypoints/src/cli.rs`
- Modify: `kiana-entrypoints/src/runner.rs` only if focused tests expose a bypass

- [ ] 移除 stream JSON skill summary 的 hardcoded Trusted，统一调用 effective trust。
- [ ] 更新 app trust status payload，使用核心 trust status 而非重复来源判断。
- [ ] 增加 direct-connect/stream JSON focused verification：无记录为 Unknown，外部记录为 Trusted，legacy 项目文件无效，Unknown 项目不显示 project skill。
- [ ] 运行 `cargo test -p kiana-entrypoints trust --locked --offline` 和相关 stream/direct-connect tests。

### Task 6: Product Documentation And Release Gates

**Files:**
- Modify: `USAGE.md`
- Modify: `docs/reference-migration-roadmap.md`
- Modify: `docs/reference-feature-matrix.md`
- Modify: `docs/commercial-release-readiness.md`
- Modify: `scripts/release-smoke.sh`
- Modify: `scripts/package-lifecycle-smoke.sh`
- Modify: `scripts/release-preflight.sh`

- [ ] 文档说明 unknown 默认、外部 store、legacy ignored、显式 trust/reset 行为和迁移步骤。
- [ ] release/installed binary smoke 使用隔离 `KIANA_HOME` 验证 unknown -> trusted -> reset，并确认项目本地 self-claim 无效。
- [ ] preflight 固定 schema、smoke 命令和 fail-closed 关键字。
- [ ] 更新 roadmap/matrix，只声明本地机制证据，不声明平台、客户或线上验收。

### Task 7: Verification And Review

**Files:**
- Review all changed files from Tasks 1-6.

- [ ] 运行 `cargo fmt --all --check`。
- [ ] 运行全部 trust/resource focused tests。
- [ ] 运行 `bash scripts/schema-contract-smoke.sh`。
- [ ] 运行 `KIANA_PREFLIGHT_SKIP_COMPLIANCE=1 bash scripts/release-preflight.sh --local-rc`。
- [ ] 运行 `git diff --check`。
- [ ] 进行规格符合性审查，逐项核对 Unknown、external store、legacy ignored、mutation gate、runtime/app status。
- [ ] 进行代码质量与安全审查，重点检查路径规范化、store record binding、错误 fail-closed、测试环境隔离和现有用户改动保护。
