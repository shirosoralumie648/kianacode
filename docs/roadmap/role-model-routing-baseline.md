# P1-C-03 role catalog and model profile routing baseline

> 快照日期：2026-09-16。本页记录五部门角色目录如何把服务端 RoleSpec 的 `model_profile` 传到 ProviderGateway；本地不运行测试，运行时/guard 夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`P1-C-03`](../roadmap.md#step-p1-c-03) |
| feature_status | `implemented`（domain catalog → ControlPlane assignment → provider route source） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | RoleCatalog/DepartmentCatalog → RoleSpec.model_profile → ControlPlane ModelAssignment → ProviderGateway connection/profile |
| this step does | 固定五部门九岗位 role dataset，校验 role/department/profile 一致性，并在每次 run 安装 server-owned ModelAssignment；ProviderGateway 只按 assignment profile 选择连接 |
| this step does not | 不让 wire/model text/role pack 改写 profile，不新增模型可见工具，不声称 provider 真实多模型 live 可达或 catalog durable hot reload |

## 2. Routing rules

- RoleSpec 的 `model_profile` 是内置角色目录事实：planning 角色（PM/Architect）、executing Builder、monitoring QA/Reviewer 等 profile 固定并经 `RoleSpec::validate` 校验。
- ControlPlane 在 run admission 解析 RoleSpec，记录 role/catalog/prompt/I/O metadata，并将同一 profile 写入 `ModelAssignment`；assignment 缺失、role/profile drift 或 authority revision drift 时拒绝。
- ProviderGateway `connection(&ModelCallSpec)` 要求已验证的 ModelAssignment，按 `assignment.profile` 查 configured connection；显式 profile 未配置时 fail-closed，不能回退到用户/模型文本指定的 profile。
- Provider profile 配置仍是非秘密受信 operator 输入；不同 profile 可以落到不同 provider/model/endpoint，配置快照和 route revision 由 P4-J7-08+ 继续收口。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `planning_and_execution_roles_can_use_different_models` | planning/executing/quality profile 在同一 ProviderGateway 上选择不同 configured model/connection |
| `role_model_profile_is_server_selected_and_reaches_provider_route` | role catalog、ControlPlane ModelAssignment 和 ProviderGateway assignment.profile 路由均为服务端来源 |
| `role_catalog_and_provenance_stay_on_the_existing_harness_path` | CO-04 既有 catalog/provenance source guard 继续由 CI 执行 |

`.github/workflows/p1-c03-model-routing.yml` 在 GitHub runner 执行 provider route fixture、core source guard、fmt 和 domain/provider/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 当前 role/department/model profile catalog 是 built-in snapshot；组织级 assignment、durable catalog revision、模型能力/配额/每-attempt permit 和真实外部 provider transport 仍由 CO/CI/P4-J7-11+ 负责。
- Provider route fixture 使用 loopback Ollama-style configured endpoints，证明的是确定性选择和边界，不证明网络可达、凭据有效、模型质量或现实业务结果。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
