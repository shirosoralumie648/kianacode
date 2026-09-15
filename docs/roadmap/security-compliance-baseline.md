# SC-00 安全与合规现状基线

> 快照日期：2026-09-16。本文是 SC-00 的 source-only inventory 和迁移护栏，不是合规认证或
> 安全完成声明。运行时夹具只在 GitHub Actions 执行；本轮本地不运行测试，也不等待远端 CI。

## 1. 范围、事实权威与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`SC-00`](security-compliance.md#step-sc-00) |
| source snapshot | `7a009ea`（CP-02 执行身份切片后的干净基线） |
| feature_status | `implemented`（现状清单、资产/入口/控制矩阵和迁移护栏）；安全能力总体仍为 `partial`/`target`/`not_supported` |
| proof_level | `source`；历史局部行为证据仍绑定各自 CURRENT_STATUS 快照，不能迁移成当前全局证明 |
| security authority | `company-os-security-constitution.md` 定义 SEC-01..12；实际状态只认源码、精确命令回执和 `CURRENT_STATUS.md` |
| execution spine | `entrypoints → client/protocol → DaemonHost → ControlPlane → policy/gates/approval → Broker/Runner → handlers → EventLog → Receipt/projections` |
| this step does | 固定资产、入口、信任边界、SEC 状态、已有证据、缺口、SC-01..43 交接和 CI source guard |
| this step does not | 不新增认证/SecretStore/安全服务，不把类型/单测/CI 文件存在写成 enforcement，不改变唯一执行脊柱 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Security constitution/design | `docs/company-os-security-constitution.md`, `docs/roadmap/security-compliance.md` | `96cc261da1dbc48252c823fe68381eb559a56077179789d12f56fad13e8cc597`, `25c86491cfb541dde8e5ad35027d08c1cbb7b75c8da9d79a7bf2574876cd694d` |
| Product status/map/roadmap | `CURRENT_STATUS.md`, `docs/module-map.md`, `docs/roadmap.md` | `a074ef61424f913a86b90c1169ea88f9972764d601022bae99fc4807e5d5d1fd`, `45a2a2ec771af9c2b1f27296b536a0b551c8bbc972e8065235be54716d0129c3`, `17e053a924da2189f4f8e5e7d204f6d84e9b595e0199da62fe4fd3a6602e7ca1` |
| Identity/redaction/schema | `kiana-domain/src/identity.rs`, `kiana-domain/src/execution_identity.rs`, `kiana-domain/src/redaction.rs`, `kiana-domain/src/contracts.rs` | `689bcb7cc16951e82b7689c5132c3dfeba12a52a622acfcd509725f1ba0e36b3`, `76ab2e53b86adaf8833fbf91180927b8a35c4731e85a69271c56216709142518`, `90938d7b6aab71634661bb53c15a674008d4820e60c0e2853b788afd17b3d481`, `ae3831e84e6a5f2d2e1c3cd4ec7233d30cf5799c5ac51a79b6e38ed4ab2ae4e9` |
| Core authority/effect/recovery | `kiana-core/src/lib.rs`, `kiana-core/src/events.rs`, `kiana-core/src/dispatch.rs`, `kiana-core/src/recovery.rs`, `kiana-core/src/projection.rs`, `kiana-core/src/data_governance.rs` | `ebdc8f38a9f85d9e579834627a89982b361ea0b2e70c1f281c7805065f94915e`, `5a0348f5e1940363119d920244724428af1e1373f692f6424ccfd3b8a6dfe26e`, `464b68ec143743580a201b01d26e7ca8d4f1dc542acb4116edb228a885b65cd0`, `7282ad4c1c689b8971283f15e4f7a3234a402e66a026c8fdc3d34d4afca370ea`, `20d84eb8fa0ac77ac85b48acb10e2fcc78214cc1c3c10be339b540230b1ff68a`, `8ec01f91afc04392dde99cb205d09b68f1d8b035e4d8e06fa4d8c8516bca715f` |
| Policy/entry/runtime/storage | `kiana-policy/src/lib.rs`, `kiana-gates/src/lib.rs`, `kiana-daemon/src/lib.rs`, `kiana-entrypoints/src/lib.rs`, `kiana-runner/src/harness.rs`, `kiana-eventlog/src/lib.rs` | `aad5391917fdab78dd60fdeea3f70733c19b076c60b3d2154eda8f65abd9e389`, `b240b7e74dedb7f7b8eb05ca0fda2f7fc0cd5df2925c8fb61b0c147c7d18425e`, `fabcc0eefb41852dc7dfd4ad7cf511bcfae02c13f2ec92ceed8f95fdfae04839`, `de50c785bf11adc7b81eb29342e72ca21abd1db49e616724b8ad5089b233db4c`, `a9e976cab4fb26e60a9cb62d881202b4dc63149a5879c579a0341df1095b3565`, `7af0aba67a44e4575ffd7920ab07866a28c4dbbfcf0c7a3a96221986dfc190bb` |
| SC-00 guard/workflow | `kiana-core/tests/security_baseline.rs`, `.github/workflows/sc00-baseline.yml` | `4069f765935eed4a65ac3e8719d55f01ce663b94fa1fe956f6ba4636c63358e8`, `0e70d840e1bc74c89f9f4c2d227a231bf5e289f0ed93ac9ac29cd11b5362f651` |

上述 hash 是本步静态审计锚点；后续修改任一边界必须在对应证据块刷新，不代表安全保证或合规认证。

## 2. 当前资产与信任边界

| 资产/边界 | 当前 owner | 当前状态与证明 | 未完成或不可推断 |
|---|---|---|---|
| Principal、Session、Role、ProjectTrust、Assignment | DaemonHost + ControlPlane + domain | CP-01 已有 server-owned local principal、canonical project identity、typed assignment/CAS；`source` | OS credential/OAuth/tenant、撤销 epoch、跨进程 authenticated principal 未完成 |
| Run/Turn/Invocation/Execution | ControlPlane/EventLog/Broker | CP-02 已有 typed Turn/Invocation identity、new-turn/legacy/resume 标记；`source` | 历史 upcast、完整 InvocationLedger/RunSnapshot、跨进程 Resume、重试/对账仍开放 |
| Policy、Gate、Approval、Grant、Budget、Lease | core/policy/gates/approval/cell registry | 现有拒绝、审批、资源和预算边界为 `partial`；局部历史证据不等价当前全局行为 | 完整交集、authority epoch、持久 grant/lease、跨入口零 effect 矩阵仍需 SC/CP/CAP |
| Capability、Permit、Handler、Sandbox、PathLock | ControlPlane → Broker → Daemon handlers | permit/CAS、sandbox/path/redaction 有源码及局部行为证据；`partial` | 全量 TOCTOU、bind-mount、网络 egress、进程树停止和 physical effect 未证明 |
| Secret / credential / provider route | Provider/Daemon compatibility boundaries | Provider 局部 redaction 和 route catalog；`partial`/`source` | SecretRef/SecretStore/lease/rotation、raw credential 隔离和真实 provider 账号未完成 |
| EventLog、Receipt、Audit、Projection、Trace、Metric | EventStore + core projections | EventLog 是事实源；Receipt/Projection/Telemetry 皆为派生；`partial` | 跨进程 durable、backup/retention/delete、audit query/export sink、production telemetry 未完成 |
| Memory、Index、Artifact、Notification、UI | 各自 adapter/projector/entrypoint | 只能读取或投影已提交事实；多项 source/partial | purpose/retention/delete 传播、durable inbox/index/artifact、四入口 UAT 未完成 |
| 外部 API、Webhook、支付、设备、物理动作 | 无受支持 product adapter | commerce/travel/IoT/physical 为 `not_supported`；connector 当前限 `local_fixture`/stdio | 真实账户、签名/nonce/receipt/reconcile、独立安全控制器和人工物理确认不存在 |
| 依赖、插件、技能、MCP manifest、发布物 | Cargo/npm/project trust/CI | 设计和局部 ProjectTrust/redaction；`source`/`partial` | SBOM、license/advisory、artifact signing/provenance、插件生命周期未完成 |

## 3. 入口与安全控制矩阵

| 入口 | 规范路径 | 当前可核对控制 | 当前缺口 |
|---|---|---|---|
| CLI | CLI → client/protocol → DaemonHost | actor/role/trust 进入 server context；命令经 ControlPlane；Receipt/状态为投影 | legacy CLI 覆盖、跨进程身份、完整 parity/UAT 仍开放 |
| Workbench/TUI | Workbench → 同一 client/daemon | approval/cancel/receipt/stream 复用 core；UI 不能成为事实源 | 断线 cursor、真实 delivery、视觉/协议全量 parity 未完成 |
| Web/SSE | loopback Web → DaemonHost | exact Host/Origin、bearer/session 局部边界；SSE 只展示 committed facts | token/session 进程内、跨进程 auth、reconnect/cursor 和真实健康探针未完成 |
| Desktop | Electron Web 壳 → Web/DaemonHost | 不另起模型循环，复用 Web 协议 | 安装/升级/签名/崩溃恢复和供应链证据未完成 |
| Scheduler/Workflow/Swarm/Connector | ControlPlane commands/automation/collaboration | planner/connector 只提交受控 intent；local_fixture/stdio 明确标记非外部 effect | durable queue/claim/retry、外部账号、Webhook/A2A、独立 parity fixture 未完成 |
| Provider/Model | Daemon provider boundary → Runner | route/config/usage 只作为输入；不拥有 principal/permission | SecretRef、原生账号、streaming quota、provider receipt 和 live 多 provider 未完成 |

任何入口都不能直接调用 handler、修改 grant、把 transcript/UI/cache 当状态权威或新建模型循环；所有有后果的动作必须回到已有 DaemonHost → ControlPlane → Broker 链。

## 4. SEC-01..SEC-12 当前状态

状态是能力维度，证明是证据维度；本表不提升历史证据的范围。

| 条款 | 当前 feature_status | 当前 proof_level | 现状与下一步 |
|---|---|---|---|
| SEC-01 真实身份绑定 | `partial` | `source` | CP-01 固定 local-user、ProjectIdentity、Assignment；SC-04/06 仍需真实 ingress、认证和撤销 |
| SEC-02 权限单调缩减 | `partial` | `local_behavior` | policy/gate/role/path 已有局部拒绝证据；SC-07..10 需完整交集/审批/epoch |
| SEC-03 Cell 资源上限 | `partial` | `local_behavior` | Cell/预算/并发/步数已有局部边界；SC-09/16/40 需持久配额、泄漏和故障证明 |
| SEC-04 外部/物理能力 | `not_supported`（外部/物理）；`partial`（本地 capability） | `source` | local Broker/fixture 不等于外部效果；SC-12..17/30 保持 deny-first |
| SEC-05 Secret 不出 Broker | `partial` | `local_behavior` | 局部 sentinel/redaction 已有历史证据；SecretRef/全出口/进程内存仍未完成 |
| SEC-06 Loopback 不是认证 | `partial` | `local_behavior` | Web exact listener/token/session 局部保护；主体仍非 durable OS/tenant auth |
| SEC-07 路径与 TOCTOU | `partial` | `local_behavior` | descriptor-relative patch/artifact 和 symlink/hardlink 局部证据；bind-mount/全 handler 未完成 |
| SEC-08 Cancel fencing | `partial` | `local_behavior` | runner cancel/terminal scope 局部证据；跨进程 cancel epoch/late effect 未完成 |
| SEC-09 Unknown 一等状态 | `partial` | `local_behavior` | result_unknown、fence、reconcile projection 已有局部证据；外部 receipt/自动对账未完成 |
| SEC-10 审计与事实源 | `partial` | `source` | EventLog/Receipt/Audit 投影边界已固定；durable audit query/export/retention 尚未完成 |
| SEC-11 不可信输入 | `partial` | `source` | redaction、ProjectTrust、MCP schema、connector local_fixture 有局部控制；全量 injection corpus 未完成 |
| SEC-12 资源耗尽 | `partial` | `source` | 多处 bounded limit/queue/step/wall-time；统一 provider/index/retention/capacity gate 未完成 |

## 5. 现有证据与证明边界

| 证据层 | 可支持的结论 | 不可支持的结论 |
|---|---|---|
| 源码/类型/guard | owner、调用路径、拒绝分支、schema/字段边界存在 | 运行时一定走到分支、跨进程持久、外部效果安全 |
| GitHub CI fixture | 指定快照上的局部行为和负向断言 | 生产 SLO、全部入口、真实密钥/账户、物理效果 |
| EventLog/Receipt | 已提交事实及可重建投影 | Receipt 内容自动正确、外部业务结果已发生 |
| local_behavior 历史块 | 绑定快照/环境的局部真实观察 | 推广到当前全局、durable/live/physical |
| live/physical 专项 | 仅对应目标/账户/环境的独立证据 | 传播到其他 provider、connector、OS 或 tenant |

已存在的 P1/P2、CP-01/02、OA、PD、INT、UI、CI 等证据块继续留在 `CURRENT_STATUS.md`，每块以自身 `source_snapshot` 为界；SC-00 不覆盖或改写历史回执。

## 6. SC-01..SC-43 交接矩阵

| 波次 | 步骤 | SC-00 固定的前置 | 完成条件（后续步骤负责） |
|---|---|---|---|
| A 合同/决定 | SC-01..05 | T01-T12 威胁、稳定 reason/schema、server context、policy revision | threat register、错误码、SecurityContext、可重放 PolicyBundle |
| B 身份/审批 | SC-06..11 | local principal/ProjectIdentity/assignment 和四入口路径已盘点 | authn/revoke/epoch、交集 Grant、精确 Approval、入口 parity |
| C effect/隔离 | SC-12..17 | permit/CAS/Unknown、sandbox/path、MCP/connector 边界已盘点 | effect-time fencing、egress、stop/Unknown、bounded quotas |
| D Secret/数据 | SC-18..24 | redaction、DataGovernance、Memory/Index/Artifact/Receipt 事实边界已盘点 | SecretRef/lease/rotation、purpose/retention/delete 传播 |
| E 供应链 | SC-25..30 | ProjectTrust 和 provider/extension/MCP boundary 已盘点 | manifest/catalog intersection、SBOM/license/advisory、签名/provenance/attestation |
| F 审计/事故 | SC-31..36 | EventLog/Receipt/Audit/Trace/Metric/Incident 派生关系已盘点 | durable projector/cursor、incident/reconcile、cross-entry parity |
| G 验证/发布 | SC-37..43 | deny-first、fixture、证据等级和限制模板已固定 | negative/property/red-team/capacity/release/recovery/状态回填 |

SC-00 的安全拒绝条件：缺少 source snapshot、事实 owner、proof ceiling、限制或下一步 owner 时，基线本身不能标为 complete；任何“类型存在”“CI 绿色”“mock receipt”“单机文件存在”都不能把状态提升为 durable/live/physical。

## 7. CI-only fixture catalog

| Fixture | 目的 | 运行位置 |
|---|---|---|
| `security_baseline_covers_constitution_assets_and_spine` | 校验 SEC-01..12、T01..T12、SC-00..43、唯一执行脊柱和 feature/proof 双维度 | `kiana-core/tests/security_baseline.rs`，GitHub Actions |
| `security_baseline_preserves_partial_and_unsupported_boundaries` | 防止把 SecretStore、tenant、external/physical、durable recovery 写成已完成 | 同上 |

`.github/workflows/sc00-baseline.yml` 只执行 source guard、`cargo fmt --all --check`、`cargo fetch --locked` 和该测试；本地不执行测试，不连接 provider/connector，不读取生产密钥。
