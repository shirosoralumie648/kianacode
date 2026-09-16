# CO-04 versioned role and department catalog baseline

> 快照日期：2026-09-16。本文记录五部门、九个内置岗位、受信 role pack、模型路由和 Run provenance 的 source slice；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CO-04`](companyos.md#step-co-04) |
| source snapshot | `a182830`（CO-03 parent）加本步源码；最终 commit 记录在 git history |
| feature_status | `implemented`（domain role/department catalog + prompt/model provenance source） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| authority path | built-in RoleCatalog/DepartmentCatalog → RoleSpec/PromptBundle metadata → ControlPlane run.authorized + ModelAssignment → existing Harness/ProviderGateway route |
| this step does | 保留五部门职责并扩展 Analyst、QA、Librarian；每个 RoleSpec 固定 schema/version、prompt hash、input/output schema、model profile、工具/路径/知识边界；catalog digest、PromptBundle、ModelAssignment、run/session/receipt provenance 同步记录 |
| this step does not | role pack 文本不授予工具/批准；不自动启动全部角色；不把目录当作 assignment、审批或 EventLog 事实；Provider 配置仍由受信 operator 环境提供 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Role/department contracts and packs | `kiana-domain/src/roles.rs`, `kiana-domain/src/prompts.rs`, `kiana-domain/src/model.rs`, `kiana-domain/src/lib.rs`, `kiana-domain/src/contracts.rs`, `kiana-domain/role-packs/{analyst,qa,librarian}.md` | `5e80307901dc23ca10481beea6fdc32b4a927a5fb773e7eb2aa03be3de218499`, `a133712bf191461cbed9e6654ec73ab16db4d03f544a973fdbe50268fc574699`, `cbdac845e3d2e1bbd23cc1680373be4609c2cb0d43947dd104d585499f66de9c`, `b8dbb36808a843b6cbbfd619d44c586de51e375abd09a2671f8b1e20d6312def`, `7185a9a7fefe68145ec0c1c6ac60af2b0b7360424599d2562e5778826c359950`, `8eb1d75863bc77c73057f508d33665f17609b16234b3f54042ddda48372e3b2f`, `bc9238632c800299e55eb18734d07333a9be57f294afce323f24a2b853d71f54`, `455da60d5d4cd5dfe94b15dd3a779f6d0bd07a74295f748281f7355ec285d9f6` |
| Run/session/receipt provenance | `kiana-core/src/{lifecycle,events,sessions,receipts}.rs` | `58fd4d1b3eae52c838295f8cf9e73a743e543913f47b6445b9ae85d6117fe045`, `76229b49fa885a183bc066bee222e0c18d33f404c0db667104bd464bcdb411f0`, `eee35ba5c6b91f23a9f3e0b6ad3ae44ed4a91087e285b8a7da0ae297245cbb54`, `82bf8dff8b45f19c470bebe4b461420efe5e4f77a5ee346bfdd79bb21b62a837` |
| Provider/daemon path | `kiana-provider/src/config.rs`, `kiana-provider/tests/co04_role_model_routes.rs`, `kiana-daemon/src/{lib,harness_skills}.rs`, `kiana-protocol/src/lib.rs` | `8194ee74bd1e7a237a9f2d553b143e7851583f93c773019f05ec1dca3ef1fbae`, `f88e3203122ad22f029adcffa758e6fbf124a714bd6999466e5cc9307c528d6e`, `8cf74ce42364295a64c1871da8f0290deaf8d34ed80bd8b826e7433f21540483`, `8da70c8b160b8583936ebf15a3c297817c3343518aa08156917c8ba92d4ee783`, `574fa87d43c12e06d0963771977245852e0790295512583aacd009963071f908` |
| Fixtures and CI | `kiana-domain/tests/co04_role_catalog.rs`, `kiana-core/tests/co04_role_catalog_guard.rs`, `.github/workflows/co04-role-catalog.yml` | `201d03ce07c5c70d84093adb0d4b4cabf08a66d4dc027379dc410d89f514232b`, `0a81bd7e4bcc888100c73cfbc7accf91b9b0f7cd3adb24ed58dc7f152d5f20bf`, `203cba5a7addad26c47ee7b8ad916ab66e4b5bff303c5c9dde07adce21aee458` |

## 2. Versioned catalog contract

`RoleSpec` 现在包含 `kiana.role-spec.v1`、`SchemaVersion(1,0)`、prompt hash、按 role ID 固定的 input/output schema 和 model profile。`RoleSpec::validate` 拒绝未知 role、错误 department/model profile、prompt drift、未知模型工具和 schema drift；catalog 中的 `RoleDescriptor` 只保留可审计的元数据，不复制 prompt 正文。

`RoleCatalog::builtin` 固定九个岗位：Sponsor、Analyst、PM、Architect、Builder、Reviewer、QA、Closer、Librarian；`DepartmentSpec`/`DepartmentCatalog` 固定五部门及 mission、roles、artifacts、RAG collection 和 gates，目录 digest 对顺序与元数据变化敏感。专业岗位按需存在，不会因目录创建启动模型进程。

`PromptBundle` 必须携带 catalog/role version、prompt hash、input/output schema 和 model profile，并验证其中的 product role section 与内置 role pack 完全一致。纯文本或篡改 metadata 不会静默降级为更高权限角色。

## 3. Model and evidence path

ControlPlane `run.authorized`、session assignment、run identity/receipt 和 `ModelAssignment` 都记录 role schema/version、catalog version、prompt hash、input/output schema 与 model profile。`ModelAssignment::validate` 要求这些字段与当前 RoleSpec 完全匹配后，Harness 才能创建 admitted model call；ProviderGateway 继续依据 server-installed assignment profile 路由，显式 profile 配置中的未知名称 fail-closed。

Role packs 只是受信 product instructions。工具集合仍固定为五个，skills/extensions 仍由 daemon 的 ProjectTrust 分支控制；role 文本、模型输出和 UI 字段不会扩展 capability grant、sandbox、写集或审批。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `versioned_role_and_department_catalogs_are_deterministic` | 五部门、九岗位、专业岗位 metadata、目录 digest 和 I/O schema 稳定 |
| `role_pack_metadata_cannot_be_forged_into_permissions_or_unknown_role` | 未知工具、错误 model profile、未知岗位和 apply_patch 越权 fail-closed |
| `prompt_bundle_carries_role_and_io_versions_without_permission_from_text` | PromptBundle 重开保留版本/hash，篡改 profile 被拒绝 |
| `role_catalog_and_provenance_stay_on_the_existing_harness_path` | role pack、ProjectTrust、provider profile、ControlPlane/Harness/receipt provenance 边界存在 |
| `planning_execution_and_quality_roles_select_distinct_configured_models` | 同一 ProviderGateway 根据 server profile 为 planning/executing/quality 选择不同模型连接 |

`.github/workflows/co04-role-catalog.yml` 在 GitHub runner 执行 domain catalog/role invariants、core source guard、fmt 和 provider/core/daemon test-target compile；本地只做格式、静态编译和 diff 检查。

## 5. 限制与交接

- 目录和 role pack 是编译进产品的 built-in snapshot；尚无可热更新的 durable catalog、签名包升级、跨进程 catalog generation 或组织级配置存储。
- RoleAssignment/ProjectAssignment 仍由 CO-03 的 in-process adapter 提供；本步没有把目录成员自动当作 assignment，也没有替代 approval/Company command 权限矩阵。
- 现有 provider profile 未配置时仍可使用其默认连接（assignment 的 profile 不会改变身份/权限）；显式配置错误会拒绝，真实多模型请求差异仍由 CI/fake provider 或后续 live 证据确认。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
