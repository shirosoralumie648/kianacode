# P4-J7-10 model capability catalog baseline

> 快照日期：2026-09-16。本文记录 model existence/capability catalog、来源/expiry/revision、pinned resolution 和同名连接消歧；本地不运行测试，运行时/guard 夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`P4-J7-10`](provider.md#step-p4-j7-10) |
| source snapshot | `af5f448`（P4-J7-09 parent）加本步 model catalog/CI；最终 commit 记录在 git history |
| feature_status | `implemented`（typed catalog + Gateway projection + deterministic resolve source） |
| proof_level | `source`；静态编译与远程 fixtures 不提升为 local_behavior/durable/live/physical |
| authority path | configured ProviderGateway connections → ModelCatalog existence/capability projection → explicit `(model_id, connection_id)` resolve → prepared ModelRoute/policy; catalog never grants tools |
| this step does | 新增 ModelCatalog/ModelCatalogEntry 及 Builtin/Configured/Cache/LiveDiscovery source、expiry、catalog revision、capability Supported/Unsupported/Unknown；Gateway 投影 catalog，model IDs 原样保留，ambiguous same-name entries require connection ID，expired/missing fail-closed |
| this step does not | 不让 model list/discovery 授予工具、网络、预算或 role authority；不自动探测/下载模型，不改变活动 PreparedModelCall route；live discovery cache/expiry 的 durable store 留待后续 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain catalog | `kiana-domain/src/model_catalog.rs`, `kiana-domain/src/model.rs`, `kiana-domain/src/contracts.rs`, `kiana-domain/src/lib.rs` | `4675c51108c92df20ee8516d3377f6c69e3737ef172ebe59d6beb9cead6f57f7`, `95bc42f1a1a01d196bb4d10c701b16b22ddd0e5f44a5c120ea6492b1068729e9`, `5f800197a608600886b36ea7d81555a91e56617d5c5020ab056199d9e9aa8b13`, `a57046ac9d6d80b8ee85a92b0e3508cfb75fabf7d6afb6d543e0c8ecdcfc9abe` |
| Gateway/config/codec | `kiana-provider/src/lib.rs`, `kiana-provider/src/config.rs`, `kiana-provider/src/request.rs` | `36a51ee1784b70f5c2978e30c2f4d7908dc7be39fa6353ac5e43dda3cb7e0aeb`, `8194ee74bd1e7a237a9f2d553b143e7851583f93c773019f05ec1dca3ef1fbae`, `ec5a05bf3fd136bdbfbc4bab8351dc3a807c2f67ecacf072f89e7a01cab75471` |
| Fixtures and CI | `kiana-provider/tests/p4_j7_10_catalog.rs`, `kiana-core/tests/p4_j7_10_catalog_guard.rs`, `.github/workflows/p4-j7-10-model-catalog.yml` | `000b1f4ac7be4e746246b7e960a326796c9232fb458522fd1b9d7bd5a4bee4cd`, `76a9daa6d309d57661ea1260e9177c949ca493da50bb885d1189453b465e9b2f`, `9d7ce1f10dc99f28446e7c4050d8fe083814ed1a3a088282049d1f54fb852a53` |

## 2. Catalog contract

`ModelCatalogEntry` 绑定 provider/connection/model 原始字符串、capability support matrix、source、catalog revision 和 optional expiry。`ModelCatalog` digest 对 entry identity/order/metadata 敏感；同一 model 名在多个连接出现时，`resolve(model_id,None)` 返回 `model_catalog_ambiguous`，显式 connection ID 才能得到 pinned entry。model IDs 含 `/`、`:` 等字符不拆分、不做隐式 alias。

Capability 状态只描述配置或 discovery 声明，不能直接变成 `CapabilityGrant`/tool schema。Provider request compiler 仍按最终 connection capabilities、RoleSpec 和 ControlPlane policy 检查；未知 capability 保留 Unknown 并拒绝需要它的操作。Catalog refresh 生成新 digest，不修改已冻结的 ModelRoute/PreparedModelCall。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `model_catalog_preserves_slashes_and_requires_connection_for_ambiguous_names` | 同名不同连接必须指定 connection，slash model ID 保持原样，Unknown tools 不变成授权 |
| `model_capability_catalog_does_not_authorize_tools_or_mutate_active_routes` | domain/provider/request source 只投影 catalog，codec 仍独立检查 capabilities/route |

`.github/workflows/p4-j7-10-model-catalog.yml` 在 GitHub runner 执行 provider catalog fixtures、core source guard、fmt 和 domain/provider/core test-target compile；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 当前 Gateway catalog 来自进程内 configured connections；Cache/LiveDiscovery entry 尚无 durable expiry/refresh store、签名或跨进程 generation。
- Catalog 能力是声明/存在性信息，不等于 provider 真实支持或网络可达；P4-J7-11/12 负责 role route/permit/codec capability enforcement，P4-J7-13+ 负责真实 transport。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
