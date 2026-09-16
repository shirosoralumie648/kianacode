# CO-02 organization/project/workspace scope baseline

> 快照日期：2026-09-16。本文记录 CO-02 的稳定 Company scope bindings 与 workspace root 边界；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CO-02`](companyos.md#step-co-02) |
| source snapshot | `9b75d21`（UI-03 instance 提交后的干净基线） |
| feature_status | `implemented`（domain scope registry + core workspace root guard source） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | authenticated actor/membership → OrganizationBinding → WorkspaceBinding(root digest) + ProjectBinding → CompanyScope resolve → existing Company EventStore command path |
| this step does | 新增 WorkspaceId 和 OrganizationBinding/WorkspaceBinding/ProjectBinding/CompanyScope/CompanyScopeRegistry；canonical root 只作为 workspace digest，组织/项目稳定 ID 分离；同 workspace 可绑定多个 project 但 resolve scope 各自隔离；actor membership、组织/工作区/项目一致性、非 canonical root、foreign project 和 legacy stream ambiguity fail-closed；core Company context 拒绝 relative/`..` workspace root |
| this step does not | 不把 caller 的 organization_id/project_id/path 变成授权、不改写现有 CompanyEvent stream、不持久化 registry、不接 assignment/expiry/revocation/authority epoch；CO-03+ 负责服务端身份与持久绑定 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain scope bindings | `kiana-domain/src/company_scope.rs` | `a7fa3632ffff33e63177aea4bfdd4ac943939856e4aad3f4c64301f186cd2f40` |
| Stable WorkspaceId/registry export | `kiana-domain/src/ids.rs`, `kiana-domain/src/contracts.rs`, `kiana-domain/src/lib.rs` | `abee22ff0f31ec4885b2a82cbf5510ac916d99dc568ae7009446b618c6c45cda`, `6a6dda049fd003640f1fb7b572bf5a4a2c5562b17dca6e52d49ab0e3b8716578`, `0c2580bb13e08c7c7bee06b89417803509682eafa6353779ef20131e76179461` |
| Core workspace guard | `kiana-core/src/company_scope.rs`, `kiana-core/src/company.rs` | `045c037e1ce794a11ed06f07a6ddf275ed1052818b541e316397da903a72a399`, `10975259c63f6e5a41683f1efcbd687fd64876b12c142ee1e1d33db5a507d8b0` |
| Protocol surface | `kiana-protocol/src/lib.rs` | `2d16e6591b64e7a785c14e7c2a376e11b7fdaad54dcb84c0eca8b74d59560a8e` |
| Fixtures | `kiana-domain/tests/co02_scope.rs`, `.github/workflows/co02-company-scope.yml` | `57dd8b331358bc7f2773022f0a8ff345e7c5b34267ec348d4b104ee11dbe2f83`, `9086462aca876ef8c421347d8ac936ed253fd0d55846e146a70bd7ce6ba41232` |

hash 只用于 CO-02 源码漂移复核，不构成 durable Company state、authenticated assignment、EventStore recovery 或业务结果证明。

## 2. Binding contract

`OrganizationBinding` 保存稳定 OrganizationId、owner/member 集合和 revision；owner 自动成为 member，空/越界/owner 缺失拒绝。`WorkspaceBinding` 从 canonical absolute root 计算稳定 WorkspaceId 与 root digest，另存 trust revision；根路径不是 organization/project ID。`ProjectBinding` 绑定独立 ProjectId、OrganizationId、WorkspaceId 和 owner，并要求 owner 是 organization member。`CompanyScope` 只输出 principal、organization/project/workspace IDs、root/trust digest 和 scope digest，不携带可直接授权的路径。

`CompanyScopeRegistry::resolve` 必须同时命中 actor membership、组织/工作区/项目归属和 workspace 绑定；同一 workspace 下的两个 project 生成不同 scope digest，不能因为共享目录而共享业务 authority。`resolve_for_root` 只把服务端已核对的 canonical root 映射到稳定 WorkspaceId，caller 自报 organization/project 不能跨绑定扩权。

Legacy principal+canonical-root stream 只能通过 `import_legacy_stream` 显式映射；同一 key 重入返回原 scope，若尝试指向不同 project/organization 返回 `company_legacy_stream_ambiguous`，不创建第二历史事实。

## 3. Core path guard

`kiana-core::company_scope::validate_workspace_root` 在现有 Company command/read context 前拒绝 relative、`..` 和非绝对 workspace spelling；canonical project root 仍由既有 DaemonHost/ControlPlane 路径解析，CompanyEvent 的事实 owner/stream/CAS 语义保持不变。此 guard 不从 path 推导 OrganizationId/ProjectId，也不另建 EventStore。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `two_business_projects_share_a_workspace_without_sharing_authority` | 同一 WorkspaceId 可绑定两个 ProjectId，但 resolve scope/project digest 各自隔离 |
| `company_scope_rejects_foreign_project_and_ambiguous_legacy_root` | foreign organization/project 与 legacy root remap 直接拒绝 |
| `company_scope_rejects_noncanonical_root_and_tampered_digest` | relative/`..` root、scope digest 篡改 fail-closed |

`.github/workflows/co02-company-scope.yml` 在 GitHub runner 执行 domain scope fixtures、core compile/source guard 和 fmt；本地只做格式、workspace test-target 静态编译和 diff 检查。

## 5. 限制与交接

- 当前 registry 为进程内纯 authority snapshot，尚无 durable Organization/Workspace/Project store、membership assignment expiry/revocation、authority epoch/fence 或 EventLog migration；CO-03/05/07/08 负责。
- Existing CompanyState/CompanyEvent 仍使用兼容 String IDs 与 canonical project_root stream；本步 typed bindings 尚未把历史业务事件批量 upcast 为 CompanyScope，避免双事实源。
- root digest 只证明 canonical spelling 的绑定输入，不证明目录存在、inode/设备未变化或 ProjectTrust；DaemonHost/SC path/trust guard 仍必须独立验证。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
