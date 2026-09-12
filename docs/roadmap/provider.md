# Roadmap 专项：Provider 专项

> 返回 [Kiana 执行路线图](../roadmap.md) 的总图与当前窗口。本文保留原专项编号、状态、依赖、验收口径和证据限制；专项步骤完成不会自动改变 P 阶段状态。

<a id="provider-plan"></a>

## 16. Provider 专项：设计与详细执行步骤（2026-09-12 追加）

> 来源：用户要求完善 [module-map.md](../module-map.md) 的 Provider 模块，并在现有 roadmap 后追加可执行步骤。
> 完整调研、源码缺口、协议矩阵、模块布局和调用流程见 [provider-design-research.md](../provider-design-research.md)。已盘点 reference 的 72 个项目目录，并重点核对 Provider 实现及官方 API 文档；各项目的阅读深度见该文档附录。
> 本节为新增计划，28 张卡均为 **⏳ / feature_status=target / proof_level=source**。测试名除标注“已有”者均为拟新增，不是通过回执。本次不改变任何产品完成态。
> 按本次用户指示，冲突的旧冻结、旧默认开关和旧文档交接限制不阻塞 Provider 设计；本节安排新增协议、推理、结构化输出和图片输入。执行时仍经唯一 DaemonHost/ControlPlane/Harness 路径，并保留参数拒绝、授权、数据和证据校验。

### 16.1 如何接到当前执行队列

当前 agent 先按 §2 收口正在处理的切片。以下编号延续既有 J7：保留 `P0-J7-01`、`P4-J7-02/03`，新增 `P4-J7-04` 至 `P4-J7-31`，不重编号前面的任务。本节表是追加任务的索引；开始执行或提升状态时同步 §1、§3 和状态账本，不能只在一处勾完成。

P4 是能力归属；本节的纯合同、解析修复和离线 fixture 可以在其具体前置条件满足后前置执行，不必为了编译一个 codec 等待整个 CompanyOS P3 完成。涉及身份、审批、预算和恢复的接线必须满足表中对应旧卡的合同与验证条件。若另一位 agent 已交付同等代码，补验收和迁移即可，不能创建同名第二套实现。

| 已有卡 / 当前 WIP | 本专项如何衔接 |
|---|---|
| `P1-C-03`、`P1-J2-03/04` | WIP 已有 `ProfileRouter`、角色 PromptBundle；`-08/-11/-12` 固定配置、剥离文本路由并验证真实请求 |
| `P1-H-01/02` | 复用工具目录和参数校验；`-05/-12` 补 Provider 解码/编码边界，不另建工具权威 |
| `P1-J2-02`、`P1-K5-01` | WIP 已有 TokenBudget、UsageRecord、CostLedger；`-12/-24` 接最终 wire 预算与分协议 usage，不重复建账本 |
| `P0-J1-01`–`05b` | `-13/-23` 将取消与剩余 wall-time 传到排队、认证、HTTP 和退避，不能只在两次模型调用之间查时限 |
| `P1-J8-01`、`P0-G-03/04`、`P0-F-03` | `-26/-27` 扩展既有 ModelTurn、history 和 resume，补 Provider attempt 与精确续接材料 |
| `P4-J7-02/03`、`P2-M2-01`、`P2-M5-01` | WIP 已有 sequence/epoch/terminal replay；`-28/-30` 复用并验证，不重造 SSE bus |


与 §14 ControlPlane 专项的交界如下。新增任务表中的 `CP-xx` 指其同一合同与验收，已有实现直接复用；这里不再创建第二份预算、许可或恢复系统。

| ControlPlane 所有者 | Provider 对接位置与职责 |
|---|---|
| `CP-02`、`CP-06/07/08` | `-06/-11` 复用身份、事务和 epoch；ModelAttempt 与既有 ExecutionId 一一关联，模型调用不伪装成模型可见工具 |
| `CP-11`、`CP-13/14` | `-11/-24` 请求准入、单次许可消费和预算核销；`ModelCallPermit` 只是 opaque permit 的模型用途视图 |
| `CP-15`、`CP-27` | `-13/-23/-25` 传递取消/单调时限/容量信号；网络健康与队列不能授予执行权 |
| `CP-18/19/20`、`CP-25` | `-20/-22/-27` 受保护 replay、敏感数据目的地、显式恢复和未知用量；共同处理删除/撤销，不另建存储权威 |
| `CP-21/22/26` | `-26/-28` 复用事件/Receipt/配置命令与 UI 投影；原始 Provider 数据不形成另一套事实日志 |

同期 §15 Harness 专项的重叠项合并交付，不建立相互等待的双向依赖。下面是职责分工；已有 H 卡完成相同合同时，以对应证据抵扣本卡，后续只做新增协议/边界。

| Harness 步骤 | Provider 步骤 | 同一交付的归属 |
|---|---|---|
| `H02/H04/H05/H06` | `-06/-14` | 消息/身份/停止分类共享 domain；ModelClient 下沉 ports 并从 runner re-export；wire codec 与唯一 accumulator 归 provider，runner 消费最终语义。H04 的 daemon adapter 最终仅保留装配，相关布局说明同步迁移 |
| `H07/H08` | `-11/-23/-24` | 运行预算与停止条件由 core/runner 输入；Provider 传到每个真实 attempt，不再叠加一个隐藏 retry 层 |
| `H09/H10/H11/H20/H21` | `-12/-20` | Harness 提供目录、配对结果与 ResolvedStepContext；Provider 负责最终 wire、token 估计、映射与私有续接材料 |
| `H22/H23/H24/H25/H27` | `-20/-21/-27` | 压缩、业务纠错/结束、恢复调度归 Harness/Core；Provider 只执行已准入的一次模型请求并校验协议结果 |
| `H32/H34/H35/H36` | `-26/-28/-29/-30/-31` | 共用迁移 fixture、UI cursor、故障注入和真实连接回执，同一证据注明能覆盖的 H/P 卡 |

建议按四组推进，每个卡内的小步骤顺序执行并各自留下聚焦证据：`04–07` 收口与分层 → `08–14` 配置/路由/编译/传输基础 → `15–22` 各协议和内容能力 → `23–31` 可靠性、恢复、界面与验收。组内真实依赖以下表为准。

### 16.2 新增任务索引

| 编号 | 交付 | 依赖 | 退出条件 | 状态 |
|---|---|---|---|---|
| `P4-J7-04` | Provider 基线与兼容性清单 | `P0-J7-01` | 调用链、已有测试、WIP 归属、负向缺口逐项绑定快照 | ⏳ |
| `P4-J7-05` | 非流式工具解析拒绝路径 | `P4-J7-04` | malformed/缺身份/重复 ID 不被默认值修成合法调用；零 dispatch | ⏳ |
| `P4-J7-06` | 中立合同与模型端口 | `P4-J7-05`、`P0-A-01b`、`P0-A-02`、`CP-02` | 有序内容/错误/usage/attempt 合同唯一；旧 cassette 可读，矛盾版本拒绝 | ⏳ |
| `P4-J7-07` | Provider crate 与兼容 facade | `P4-J7-06` | 产品模型分支迁出 services；仅一个模型端口与实现；离线行为可对照 | ⏳ |
| `P4-J7-08` | 连接配置与不可变快照 | `P4-J7-07` | 来源/优先级明确，显式 profile 缺失拒绝，运行中配置不漂移 | ⏳ |
| `P4-J7-09` | 凭据与出站目标 | `P4-J7-08` | secret 不出诊断；跨 origin 重定向/非法 header/未授权地址拒绝 | ⏳ |
| `P4-J7-10` | 模型能力与 discovery | `P4-J7-08` | 未知能力保留未知；模型列表不能授予能力；目录带版本/来源 | ⏳ |
| `P4-J7-11` | 角色路由与调用准入 | `P4-J7-09`、`P4-J7-10`、`P1-C-03`、`P0-K1-01`、`P1-K5-01`、`CP-11`、`CP-13` | 服务端角色决定 route；每真实 attempt 有许可、预算与审计 | ⏳ |
| `P4-J7-12` | 请求、schema 与 history 编译 | `P4-J7-06`、`P4-J7-11`、`P1-H-01`、`P1-J2-02`、`P1-J2-04` | 编译后 wire 与预算/授权 hash 一致；工具映射可逆、历史配对完整 | ⏳ |
| `P4-J7-13` | 有界 HTTP/SSE/NDJSON 传输 | `P4-J7-07`、`P4-J7-09` | 任意切块正确；超时、配额、错误响应体和取消均有界 | ⏳ |
| `P4-J7-14` | 归一化流与完成状态机 | `P4-J7-06`、`P4-J7-13` | 唯一 assembler；完整终态后才输出工具；EOF/长度截断不假成功 | ⏳ |
| `P4-J7-15` | Anthropic Messages 收口 | `P4-J7-12`、`P4-J7-14` | 原生/兼容 dialect 分清；完整文本与工具往返，严格终态 | ⏳ |
| `P4-J7-16` | OpenAI Chat 原生 SSE | `P4-J7-12`、`P4-J7-14` | 原生增量、交错工具、usage-only chunk、结束标记均正确 | ⏳ |
| `P4-J7-17` | OpenAI Responses | `P4-J7-12`、`P4-J7-14` | input/output items、call_id、response status 与 stateless 续接正确 | ⏳ |
| `P4-J7-18` | Ollama 原生 NDJSON | `P4-J7-12`、`P4-J7-14` | 真实增量、done、加载时限与无 wire ID 工具往返正确 | ⏳ |
| `P4-J7-19` | Gemini 原生 Interactions | `P4-J7-12`、`P4-J7-14` | step/status/usage 与 requires_action 正确；不混旧 GenerateContent | ⏳ |
| `P4-J7-20` | 推理与受保护 replay 材料 | `P4-J7-15`、`P4-J7-16`、`P4-J7-17`、`P4-J7-19`、`P2-K7-01`、`CP-18`、`CP-25` | 必须回传的材料按协议保真；未授权/缺失/过期不恢复、不泄露 | ⏳ |
| `P4-J7-21` | 结构化输出 | `P4-J7-15`、`P4-J7-16`、`P4-J7-17`、`P4-J7-18`、`P4-J7-19` | 输出 schema 有独立结果校验；refusal/length/非法 JSON 不伪装合格 | ⏳ |
| `P4-J7-22` | 图片输入与数据准入 | `P4-J7-15`、`P4-J7-17`、`P4-J7-18`、`P4-J7-19`、`P2-K7-01`、`P4-J7-16`、`CP-25` | 已授权 Artifact 才能发送；MIME/大小/hash/模型能力均校验 | ⏳ |
| `P4-J7-23` | 重试、时限与取消 | `P4-J7-11`、`P4-J7-13`、`P4-J7-14`、`P0-J1-04`、`P0-J1-05a`、`P0-J1-05b`、`CP-15` | 唯一重试层；Retry-After/取消/未知响应不造成隐式重复请求 | ⏳ |
| `P4-J7-24` | Usage、成本与预算结算 | `P4-J7-15`、`P4-J7-16`、`P4-J7-17`、`P4-J7-18`、`P4-J7-19`、`P4-J7-23`、`P1-K5-01`、`CP-11`、`CP-14` | 分 attempt 记已知/未知用量，累计不重算，价格钉版，预算不超分配 | ⏳ |
| `P4-J7-25` | 配额、熔断与受控 fallback | `P4-J7-23`、`P4-J7-24` | 有界公平队列，许可释放，fallback 重验能力/数据/预算 | ⏳ |
| `P4-J7-26` | 事件、脱敏与关联链 | `P4-J7-20`、`P4-J7-23`、`P4-J7-24`、`P1-J8-01`、`P0-G-04`、`CP-26` | ModelCall→attempt→provider→Invocation→Receipt 可追溯且不泄密 | ⏳ |
| `P4-J7-27` | 持久 history 与恢复 | `P4-J7-20`、`P4-J7-26`、`P0-G-03`、`P0-F-03`、`P2-K6-01`、`CP-18`、`CP-19`、`CP-20` | 新进程显式恢复不重发已完成工具；缺 replay 材料拒绝 | ⏳ |
| `P4-J7-28` | 三界面配置和诊断 | `P4-J7-10`、`P4-J7-11`、`P4-J7-25`、`P4-J7-26`、`P4-J7-02`、`P4-J7-03`、`P2-M2-01`、`P2-M5-01`、`CP-22` | 同一目录/状态/错误；迟到终态与重连无需重新执行 | ⏳ |
| `P4-J7-29` | 离线协议一致性矩阵 | `P4-J7-20`、`P4-J7-21`、`P4-J7-22`、`P4-J7-25`、`P4-J7-27` | 每个支持的协议/能力通过相同合同及其特有故障用例 | ⏳ |
| `P4-J7-30` | 产品链与四表面回归 | `P4-J7-28`、`P4-J7-29`、`P0-M1-01` | 模型→受控工具→事件→Receipt；CLI/TTY/Web/Desktop 终态一致 | ⏳ |
| `P4-J7-31` | 逐连接 live 验收与发布证据 | `P4-J7-30` | 原生/兼容/本地分别有真实证据；未验证能力如实列出 | ⏳ |

### 16.3 每张卡的具体步骤

<a id="step-p4-j7-04"></a>



#### P4-J7-04 Provider 基线、快照和现有测试　⏳

- **依赖**：`P0-J7-01`。
- **改动位置**：现有 `model_client.rs`、services/api、runner/model、测试与调研清单；先只读核对。
- **步骤**：① 固定 HEAD 与待测 WIP diff/hash，追踪 run/continue/resume、角色调用、`model catalog/smoke` 到实际 Provider；② 对照调研 §2 区分已有能力、已测能力与本次缺口；③ 列出旧配置/cassette/wire 接受集和每个待迁函数的唯一归属；④ 保存当前最小失败样例，交接后续卡。
- **先拒绝**：复跑已有 `native_streaming_without_a_terminal_event_fails_closed`、`native_streaming_invalid_tool_json_fails_closed`、`provider_does_not_retry_auth_errors`；新增缺口先记录 RED，不声称已修复。
- **再成功**：已有 `provider_wrapper_maps_tool_calls_and_final_text`、`anthropic_native_streaming_aggregates_output_without_network`；确认 `provider_standard` 目前主要验证 Fake。
- **退出 / 证据**：每个基线发现有函数、快照、命令、命中测试数和限制；共享 WIP 编译失败单独分类，不归因于未运行的稳定 HEAD。

<a id="step-p4-j7-05"></a>



#### P4-J7-05 非流式工具响应必须严格解析　⏳

- **依赖**：`P4-J7-04`。
- **改动位置**：`kiana-services/src/api/provider.rs` 的 OpenAI/Ollama 响应转换；`kiana-daemon/src/model_client.rs` 的 `output_from_response/map_tools`；相应 tests。
- **步骤**：① 为坏 JSON、缺 name、原生必需 ID 缺失、重复 ID/不同 payload 写失败用例；② 将 `{}`/`shell`/固定 ID 等“修复”改为 Result 错误，禁止静默丢工具；③ 保留协议确实无 ID 时的单独规则，不能误伤 Ollama；④ 在 daemon 断言解析失败后没有 capability dispatch。
- **先拒绝**：`malformed_provider_tool_arguments_never_dispatch`、`missing_native_tool_identity_is_rejected`、`duplicate_tool_id_with_different_payload_is_rejected`。
- **再成功**：`valid_empty_object_arguments_are_preserved`、`provider_tool_result_round_trip_keeps_identity`，保留合法空对象和原有工具调用。
- **退出 / 证据**：流式/非流式对同一恶意响应一致拒绝；原始错误经脱敏；本步可独立于新 crate 交付。

<a id="step-p4-j7-06"></a>



#### P4-J7-06 中立内容、调用身份、错误与模型端口　⏳

- **依赖**：`P4-J7-05`、`P0-A-01b`、`P0-A-02`、`CP-02`。
- **改动位置**：拟新增 domain/model、domain/model_route、ports/model；现有 runner/model、runner-protocol、schema 注册和消费者。
- **步骤**：① 登记 ModelCall/Attempt 与既有 ExecutionId 的关联、有序 ContentBlock、PreparedModelCall、ModelFinish、ModelError、Usage；② 将当前名为 ModelProfile 的能力元数据迁为 ModelCapabilities，角色用的 ModelProfile/RouteDecision 引用既有规范；③ 下沉 ModelClient 并在旧 runner 路径 re-export；④ 提供旧 cassette/upcaster 和兼容 helper，拒绝新旧内容字段矛盾；⑤ 增补原有 Text 回调的兼容桥，只有一个终态聚合来源。
- **先拒绝**：`unknown_model_contract_major_fails_closed`、`conflicting_legacy_and_block_content_is_rejected`、`provider_errors_preserve_structured_classification`。
- **再成功**：`legacy_cassettes_round_trip_through_model_contract`、`ordered_text_tool_and_result_blocks_round_trip`。
- **退出 / 证据**：domain/ports 不依赖 provider/runner 实现；序列化升级有测试，错误码只加不改旧语义；本卡不把所有 DTO 直接暴露到公网 wire。

<a id="step-p4-j7-07"></a>



#### P4-J7-07 提取 kiana-provider 并迁移装配　⏳

- **依赖**：`P4-J7-06`。
- **改动位置**：拟新增 `kiana-provider/`；daemon/model_client；services/api 兼容 facade；workspace manifests/lock 由集成写者统一修改。
- **步骤**：① 建最小 crate 与 adapter/gateway 边界，迁现有编解码和 Fake；② daemon 注入新 ModelClient，Runner 不引用新实现；③ services 原公开路径委托新实现或做显式 legacy DTO 转换；④ 用同一离线 fixture 对照新旧结果，逐协议移除旧生产实现，记录迁移差异。
- **先拒绝**：`provider_dependency_boundary_rejects_legacy_runtime_edges`、`missing_model_still_fails_closed_after_extraction`。
- **再成功**：`daemon_uses_one_provider_gateway_after_migration`、`legacy_provider_facade_matches_recorded_contract`。
- **退出 / 证据**：不出现 provider→services/core/entrypoints 依赖或另一套 Agent loop；不双发真实请求；保留现有 CLI 参数和 cassette 行为的迁移说明。

<a id="step-p4-j7-08"></a>



#### P4-J7-08 连接、profile 和配置快照　⏳

- **依赖**：`P4-J7-07`。
- **改动位置**：provider/config、daemon 的 LocalModelConfig/ProfileRouter 构造、既有用户配置解析与协议设置 DTO。
- **步骤**：① 分开 provider_id、protocol、connection_id、model_id、credential_ref 和 profile_version；② 将 CLI/用户配置/env 解析为一次性快照，记录每个字段来源；③ 明确 legacy cassette 优先与新显式 mode 冲突规则；④ 将无 profile 的历史默认、显式 default 继承、未配置 profile 拒绝分别实现；⑤ 拒绝无效数字/streaming 值，配置热更新生成新 revision。
- **先拒绝**：`explicit_unknown_profile_does_not_fall_back`、`live_and_cassette_selection_conflict_is_rejected`、`invalid_streaming_policy_is_rejected`。
- **再成功**：`legacy_cassette_precedence_is_preserved`、`active_runs_keep_their_config_snapshot`、`connection_change_does_not_inherit_another_connections_key`。
- **退出 / 证据**：角色包/项目文本不能提供 secret 或扩大连接白名单；在同 host 运行两个不同 profile 时互不串配置。

<a id="step-p4-j7-09"></a>



#### P4-J7-09 凭据管理与 HTTP 目标校验　⏳

- **依赖**：`P4-J7-08`。
- **改动位置**：provider/credentials、config、transport/http；daemon 操作者级凭据适配和诊断投影。
- **步骤**：① 使用 SecretRef/env reference 和受保护存储，不把 key 放入可序列化配置；② 认证字段构造全部返回结构化错误，移除相关 unwrap/expect；③ 校验 URL scheme、origin、路径/query、代理/TLS 配置，拒绝 URL userinfo 和跨 origin 认证转发；④ 本地 HTTP 连接显式登记 loopback/允许目标并校验实际连接与 redirect，覆盖 DNS 地址变化；⑤ 连接/credential revision 更新后失效旧池或缓存；需要 OAuth 的后续连接使用单次刷新和并发刷新合并。
- **先拒绝**：`invalid_auth_header_never_panics`、`authenticated_redirect_cannot_change_origin`、`untrusted_project_cannot_override_provider_endpoint`、`provider_secret_never_appears_in_diagnostics`。
- **再成功**：`configured_loopback_provider_works_without_api_key`、`credential_revision_isolated_between_connections`。
- **退出 / 证据**：TLS 证书错误不重试、不关闭证书校验；重定向/代理/IPv4/IPv6/域名解析用本地 fixture 验证，不读取用户真实 key。

<a id="step-p4-j7-10"></a>



#### P4-J7-10 能力目录、未知能力和显式 discovery　⏳

- **依赖**：`P4-J7-08`。
- **改动位置**：provider/catalog、compatibility；现有 model catalog 查询与 DTO。
- **步骤**：① 把任意 model 通用 true/固定窗口改成带来源的 supported/unsupported/unknown；② 分开 model 原生能力、codec 支持和 policy 允许；③ 为内置、用户配置、缓存和 live discovery 定义合并规则、过期和 revision；④ 模型列表只更新存在性，capability 需要声明与验证；⑤ 同名不同连接消歧，完整模型名中的 `/` 不拆错。
- **先拒绝**：`unknown_tool_capability_fails_before_network`、`discovered_model_does_not_grant_capabilities`、`ambiguous_model_name_requires_connection`。
- **再成功**：`pinned_catalog_is_usable_offline`、`catalog_refresh_does_not_mutate_active_route`、`model_ids_with_slashes_are_preserved`。
- **退出 / 证据**：目录明确 native/synthetic/none，支持矩阵不能将 discovery 成功写成 tools/live 成功；默认不探测、不下载模型。

<a id="step-p4-j7-11"></a>



#### P4-J7-11 类型化角色路由与每 attempt 准入　⏳

- **依赖**：`P4-J7-09`、`P4-J7-10`、`P1-C-03`、`P0-K1-01`、`P1-K5-01`、`CP-11`、`CP-13`。
- **改动位置**：domain/model_route、ports/model、provider/route/gateway、daemon/model_admission、core 生命周期/预算/事件、runner 请求构造。
- **步骤**：① 通过服务端 role assignment 传 profile，替换从 System 文本 decode 的路由；② 纯 resolver 过滤能力、数据、预算并输出 RouteDecision；③ 网关提交 PreparedModelCall 摘要，经 ModelAdmissionPort 复用 CP-11/13 的预算与 opaque permit，绑定 request hash/route/epoch/expiry；④ before-send 在共同验证端口原子消费许可，拒绝与预留使用同一事件事务；⑤ 用窄控制句柄避免 Arc 强引用环、持锁 await 和递归 drive_run。
- **先拒绝**：`prompt_text_cannot_select_a_provider`、`expired_model_permit_prevents_network_send`、`model_budget_denial_records_zero_requests`。
- **再成功**：`planning_and_execution_roles_can_use_different_models`（原卡验收复用并加强）、`concurrent_run_routes_are_isolated`。
- **退出 / 证据**：断言实际 HTTP body、模型请求次数和 Receipt route，而非只测 resolver；测试许可撤销与发送竞争，不为 Provider 新增模型可见工具。先使用 -06 的 PreparedModelCall 合同与固定编译 fixture 验收准入，-12 再补全各协议编译，避免相互等待。

<a id="step-p4-j7-12"></a>



#### P4-J7-12 请求编译、工具映射与上下文完整性　⏳

- **依赖**：`P4-J7-06`、`P4-J7-11`、`P1-H-01`、`P1-J2-02`、`P1-J2-04`。
- **改动位置**：provider/request、各协议请求编码、既有 PromptBundle/TokenBudget/ToolSpec；runner history 输入边界。
- **步骤**：① 编译已确定的角色 prompt、context、历史和工具快照；② 生成双向无碰撞 ToolNameMap，保留内部五工具目录，wire hash 入元数据；③ 校验 call/result 配对、顺序与重复身份，不自动插假结果；④ 根据协议转换 system/developer、tool schema 和生成参数；⑤ strict/非 strict 明确，内部 schema 接受集不被 wire 转换改变；⑥ 在最终请求形状上计预算并冻结 payload hash，发送不得重新读取环境改变内容。
- **先拒绝**：`wire_tool_name_collision_is_rejected`、`orphan_tool_result_fails_before_send`、`compiled_request_cannot_exceed_context_budget`、`provider_options_cannot_override_tools_or_auth`。
- **再成功**：`five_tools_round_trip_through_each_wire_schema`、`nullable_wire_schema_preserves_optional_argument_semantics`、`compiled_payload_matches_admitted_digest`。
- **退出 / 证据**：max output/effort/temperature 按所选模型能力生效；预算含 system/schema/输出预留；编译日志只有 hash 和来源，无完整 prompt。

<a id="step-p4-j7-13"></a>



#### P4-J7-13 HTTP、SSE、NDJSON 的有界传输　⏳

- **依赖**：`P4-J7-07`、`P4-J7-09`。
- **改动位置**：provider/transport；共用 Tokio/reqwest client 与 FakeTransport/clock；loopback HTTP fixtures。
- **步骤**：① 分离 connect、header、first-semantic、read-idle、attempt-total 和 run 剩余时限；② 编写受限 SSE/NDJSON framing，覆盖 UTF-8 跨块、CRLF、多行 data、heartbeat、尾行；③ 限制解压后 body/frame/line 和错误正文，校验 Content-Type/status；④ 可取消的有界队列与连接池按连接快照隔离；⑤ reader 退出/drop 释放资源，底层自动重试不隐藏尝试次数。
- **先拒绝**：`oversized_provider_frame_is_rejected_before_allocation_growth`、`heartbeat_cannot_extend_total_deadline`、`html_success_body_is_not_an_empty_model_success`。
- **再成功**：`sse_and_ndjson_survive_arbitrary_utf8_chunking`、`healthy_long_stream_survives_read_idle_policy`、`cancel_interrupts_waiting_for_headers`。
- **退出 / 证据**：不使用一次 reqwest total timeout 同时表达所有流式时限；通过小限额与现有大 fixture 双向测试确定默认值。

<a id="step-p4-j7-14"></a>



#### P4-J7-14 唯一 accumulator 与协议终态　⏳

- **依赖**：`P4-J7-06`、`P4-J7-13`。
- **改动位置**：provider/stream/event、accumulator；ModelClient 兼容收集路径；runner 处理 ModelFinish。
- **步骤**：① 实现 message 和 block 转移表；② 验证 start/delta/stop、工具 JSON、事件身份与终态；③ 区分端口 EOF、模型结束和 Run 结束；④ end_turn/tool_use/length/refusal/pause/incomplete 分开映射；⑤ terminal 到达即退出，不等连接 EOF；⑥ 非流式响应走同一语义校验，未知纯 metadata 与未知关键 block 分开处理。
- **先拒绝**：`closed_tool_block_cannot_receive_more_arguments`、`terminal_with_open_blocks_is_rejected`、`length_finish_never_authorizes_partial_tool_arguments`、`stream_eof_is_not_completion`。
- **再成功**：`terminal_finishes_without_socket_eof`、`identical_text_deltas_are_not_deduplicated`、`stream_and_nonstream_share_one_final_output`。
- **退出 / 证据**：每 attempt 一个终态；只在完整响应校验后交付可派发工具；协议未提供 sequence 时不编造上游重放保证。

<a id="step-p4-j7-15"></a>



#### P4-J7-15 Anthropic Messages 完整收口　⏳

- **依赖**：`P4-J7-12`、`P4-J7-14`。
- **改动位置**：provider/protocols/anthropic、compatibility；旧 Anthropic facade 与 fixtures。
- **步骤**：① 迁独立 system/content/tool_result 编码，包含错误工具结果；② 解码 block 生命周期、message_delta 累计 usage、流内 error/ping；③ 原生终态要求 message_stop 和一致的 blocks；④ DeepSeek Anthropic 兼容连接单独钉 dialect，只有有证据的差异才允许映射；⑤ 记录 requested/reported model、request id，提供 thinking/replay 的完整字段挂点给 `-20`。
- **先拒绝**：`anthropic_stop_reason_without_message_stop_is_incomplete`、`anthropic_stream_error_after_text_never_dispatches_tools`、`anthropic_unknown_required_block_fails_closed`。
- **再成功**：`anthropic_text_tool_result_and_final_answer_round_trip`、`anthropic_usage_updates_are_cumulative`，保留已有 DeepSeek fixture 的工具名映射。
- **退出 / 证据**：原生与兼容测试分桶；本卡只验收不需推理续接的能力，要求 thinking 的 profile 必须等 `-20`，不能丢字段后标支持。

<a id="step-p4-j7-16"></a>



#### P4-J7-16 OpenAI Chat Completions 原生流式　⏳

- **依赖**：`P4-J7-12`、`P4-J7-14`。
- **改动位置**：provider/protocols/openai_chat、compatibility；现有 OpenAI-compatible wrapper；Chat SSE fixtures。
- **步骤**：① 实现 stream=true 的实际 SSE 请求；② 按 choices/index 与 tool_calls/index 聚合交错参数，n 固定为 1；③ 允许 usage-only 的空 choices，处理 finish_reason/[DONE] 的完整性；④ 版本化 max_tokens/max_completion_tokens、system/developer、stream_options 等差异；⑤ 为 DeepSeek/OpenRouter/vLLM 的已支持配置建 dialect fixture，完成后只将该支持范围改为 Native。
- **先拒绝**：`chat_done_without_valid_choice_finish_is_incomplete`、`chat_interleaved_tools_cannot_swap_ids`、`chat_malformed_arguments_never_become_empty_object`。
- **再成功**：`chat_emits_delta_before_terminal_response`、`chat_usage_only_chunk_is_retained`、`chat_tools_round_trip_with_original_call_ids`。
- **退出 / 证据**：mock 记录首 delta 发生在 terminal 之前，断言非 synthetic；兼容服务不支持某参数时显式省略/拒绝，不失败后随机重试变体。

<a id="step-p4-j7-17"></a>



#### P4-J7-17 OpenAI Responses 原生适配　⏳

- **依赖**：`P4-J7-12`、`P4-J7-14`。
- **改动位置**：provider/protocols/openai_responses；protocol 注册；Responses request/item/event fixtures。
- **步骤**：① 单列 protocol/route，不将既有 Chat base_url 自动改为 Responses；② 编译 instructions/input/function_call_output 和平铺 function tools；③ 跟踪 output_index/item_id/call_id，处理 text/arguments/refusal 和 output_item.done；④ completed/failed/incomplete 校验独立；⑤ 默认 store=false、保留回传 item 的字段，下一轮重新发送所需 instructions/tools；⑥ 收集 response ID 和 reported model，`-20` 接精确 reasoning replay。
- **先拒绝**：`responses_item_id_cannot_replace_call_id`、`responses_incomplete_never_completes_the_run`、`responses_unrequested_hosted_tool_is_rejected`。
- **再成功**：`responses_stateless_tool_round_trip`、`responses_final_items_match_streamed_items`。
- **退出 / 证据**：模型只声明本地 function 工具；不启用服务商托管 shell/MCP/web 工具；无需 previous_response_id 才能完成基本受控对话。

<a id="step-p4-j7-18"></a>



#### P4-J7-18 Ollama 原生 NDJSON 与本地模型体验　⏳

- **依赖**：`P4-J7-12`、`P4-J7-14`。
- **改动位置**：provider/protocols/ollama、catalog、compatibility；Ollama NDJSON fixtures。
- **步骤**：① `/api/chat` 实际 stream=true，逐行解码；② tool arguments 对象原样校验，无 native ID 时由 ModelCallId/ordinal 确定性派生，保留回传 tool_name；③ done=true 才完成，usage 映射 prompt_eval_count/eval_count；④ load_duration 与生成时间分别记录；⑤ 本地模型能力由显式配置/已验证元数据决定，按连接设置加载时限。
- **先拒绝**：`ollama_eof_without_done_is_incomplete`、`ollama_same_name_tools_keep_distinct_invocations`、`ollama_unknown_tools_support_is_not_assumed`。
- **再成功**：`ollama_streams_before_model_completion`、`ollama_tool_result_continuation_uses_stable_local_ids`、`ollama_load_latency_is_distinct_from_generation_latency`。
- **退出 / 证据**：不自动拉取、创建或删除本地模型；synthetic→native 仅针对通过实际 NDJSON 测试的能力范围。

<a id="step-p4-j7-19"></a>



#### P4-J7-19 Gemini Interactions 原生协议　⏳

- **依赖**：`P4-J7-12`、`P4-J7-14`。
- **改动位置**：拟新增 provider/protocols/gemini_interactions；连接/catalog 注册；官方版本绑定 fixtures。
- **步骤**：① 核对实施时采用的 API 版本，当前计划采用 Interactions 并显式 store=false；② 编码有序 input、system_instruction/tools/generation_config，每次都提交必需配置；③ 聚合 step.start/delta/stop 的函数参数与输出；④ interaction.completed 的 requires_action 映射 tool_use，completed 才是本轮正常结束；⑤ 用完整本地历史做函数结果回传，保留 thought signature 接入点。
- **先拒绝**：`gemini_requires_action_is_not_run_completion`、`gemini_incomplete_function_arguments_never_dispatch`、`gemini_generate_content_events_are_not_accepted_as_interactions`。
- **再成功**：`gemini_stateless_function_result_round_trip`、`gemini_interaction_parameters_are_resubmitted_each_turn`。
- **退出 / 证据**：旧 GenerateContent 若仍有用户需求，另加 `gemini_generate_content` 协议子卡及独立 fixture；不混两套帧格式，也不调用 SDK 自动工具循环。

<a id="step-p4-j7-20"></a>



#### P4-J7-20 推理签名、续接资料与短期保护存储　⏳

- **依赖**：`P4-J7-15`、`P4-J7-16`、`P4-J7-17`、`P4-J7-19`、`P2-K7-01`、`CP-18`、`CP-25`。
- **改动位置**：各 codec、domain replay 引用、provider replay 编解码、daemon 受保护 Artifact 适配、history/compaction 边界。
- **步骤**：① 分开可公开 summary 与协议要求的私有续接材料；② 按 API 原样关联 Anthropic signature、Responses items、DeepSeek reasoning_content、Gemini signature；③ 进程内有界缓冲，持久恢复使用加密短期 artifact，EventLog 仅记 ref/hash；④ 绑定 connection/protocol/model/effort/prompt/tool/data revision 和 TTL；⑤ 过期/撤销/模型切换时明确失效，压缩只处理允许压缩的内容。
- **先拒绝**：`replay_material_cannot_cross_connection_or_model_scope`、`missing_reasoning_material_blocks_resume`、`reasoning_payload_never_enters_receipt_memory_or_logs`。
- **再成功**：`signed_reasoning_survives_tool_continuation_byte_for_byte`、`replay_payload_deletion_invalidates_resume`。
- **退出 / 证据**：没有受保护存储的部署仅声明进程内支持；不把脱敏后的内容当作无损恢复；不在 repo/.env/事件里存加密密钥。

<a id="step-p4-j7-21"></a>



#### P4-J7-21 结构化输出的请求与验收　⏳

- **依赖**：`P4-J7-15`、`P4-J7-16`、`P4-J7-17`、`P4-J7-18`、`P4-J7-19`。
- **改动位置**：domain response format、provider/request、各 codec；runner 消费结构化结果的边界。
- **步骤**：① 让 text、JSON object、JSON schema 成为独立响应选项；② 按模型/协议校验支持子集与参数，和 tool choice 分开；③ 完整输出后做 schema 校验并返回结构化结果；④ refusal/length/空/非法 JSON 各自返回；⑤ 需要纠错时由 Harness 显式发起新模型调用并计预算。
- **先拒绝**：`unsupported_response_schema_fails_before_send`、`refusal_is_not_an_empty_structured_success`、`truncated_json_is_never_repaired_silently`。
- **再成功**：`structured_output_validates_against_requested_schema`、`structured_output_and_tools_have_distinct_contracts`。
- **退出 / 证据**：Provider 无隐藏 JSON 修复循环；不修改既有业务 Artifact 验收规则；每种支持形式独立记矩阵。

<a id="step-p4-j7-22"></a>



#### P4-J7-22 图片输入与敏感数据出站准入　⏳

- **依赖**：`P4-J7-15`、`P4-J7-17`、`P4-J7-18`、`P4-J7-19`、`P2-K7-01`、`P4-J7-16`、`CP-25`。
- **改动位置**：domain ContentBlock/Artifact 引用、daemon 数据装配、provider modality 编码/预算、用户附件 DTO。
- **步骤**：① 从用户已选定或已授权的 Artifact 取得数据，复用路径和 ProcessingGrant；② 校验 MIME、尺寸、字节数、hash、保留期与所选连接数据范围；③ 按协议编码图片，计算真实编码后体积和上下文预算；④ tool 返回图片复用同一通道；⑤ 对未实现的文档上传/URL抓取/音视频给出准确的 unsupported。
- **先拒绝**：`untrusted_image_path_never_reaches_provider`、`revoked_artifact_grant_blocks_send`、`image_payload_limit_applies_after_encoding`、`remote_image_url_is_not_fetched_implicitly`。
- **再成功**：`authorized_image_input_reaches_vision_capable_provider`、`image_hash_matches_admitted_payload`。
- **退出 / 证据**：Provider 不持有任意文件读取能力；图片输入验证不扩张为生成、上传或远端文件生命周期已实现。

<a id="step-p4-j7-23"></a>



#### P4-J7-23 单层重试、绝对时限和取消传递　⏳

- **依赖**：`P4-J7-11`、`P4-J7-13`、`P4-J7-14`、`P0-J1-04`、`P0-J1-05a`、`P0-J1-05b`、`CP-15`。
- **改动位置**：provider/retry/gateway、ModelError、daemon admission、core/runner 取消及预算衔接。
- **步骤**：① 根据 typed status/error 和发送进度分类，不用消息子串；② 只有一个执行重试的层，默认沿授权上限有限尝试；③ Retry-After 秒数/日期、指数退避和 jitter 都受剩余预算约束，过长时退出而非提前重试；④ queue/auth/send/read/backoff 任意阶段可取消；⑤ 每 retry 新 attempt 重新准入，已见内容或发送结果未知默认不自动重发；⑥ 验证模型结束与取消竞争的单终态。
- **先拒绝**：`cancel_during_retry_backoff_prevents_next_attempt`、`post_send_unknown_is_not_retried_automatically`、`tls_failure_is_not_transient`、`oversized_retry_after_does_not_retry_early`。
- **再成功**：`retryable_429_then_success_records_two_attempts`、`retry_after_http_date_uses_injected_clock`；保留已有 `cancelling_mid_stream_never_completes_or_emits_a_late_delta`。
- **退出 / 证据**：测试断言真实请求数、dispatch 数、终态数与资源释放；SDK 隐式重试为零；模型未知用量与工具 result_unknown 分开。

<a id="step-p4-j7-24"></a>



#### P4-J7-24 Provider 用量、价格快照与预算结算　⏳

- **依赖**：`P4-J7-15`、`P4-J7-16`、`P4-J7-17`、`P4-J7-18`、`P4-J7-19`、`P4-J7-23`、`P1-K5-01`、`CP-11`、`CP-14`。
- **改动位置**：现有 domain/usage、core 预算/receipt、provider usage 归一化与 RateCard 加载。
- **步骤**：① 建每协议的 input/output/cache/reasoning 字段包含关系表；② 区分缺失与真实零、累计与 delta，使用 checked 运算；③ 按 attempt/ExecutionId 向 CP-11/14 同一账本提交失败用量，预留/消费/释放分别落账；④ 价格表带来源、币种、有效时间和版本，estimated 与 measured 分开；⑤ 同 attempt 重放不能重复结算，迟到用量走 correction；⑥ 并发请求原子预留，未知消费有明确保守预算处理。
- **先拒绝**：`missing_usage_and_price_remain_unknown`、`usage_overflow_is_rejected`、`concurrent_model_calls_cannot_overspend_reserved_budget`。
- **再成功**：`cumulative_cache_and_reasoning_usage_is_not_double_counted`、`failed_attempt_usage_is_retained`、`replayed_usage_is_settled_once`。
- **退出 / 证据**：重试增加成本可解释；Receipt 显示未知项；不把本机 GPU 调用的外部费用默认为零，也不宣称模型 token 统计等于账单。

<a id="step-p4-j7-25"></a>



#### P4-J7-25 容量、熔断与白名单 fallback　⏳

- **依赖**：`P4-J7-23`、`P4-J7-24`。
- **改动位置**：provider/gateway/route、connection health、daemon 容量服务、core 预算准入；复用既有 Quota。
- **步骤**：① connection/credential quota group/model 维度的 semaphore、RPM/TPM、有界等待队列；② session 公平性、取消出队和 RAII 释放；③ 明确 closed/open/half-open 及配置更新重置语义；④ fallback 仅取配置白名单，对能力、上下文、数据、预算和许可重验；⑤ 在新 attempt 记录路由原因，不拼接两个模型的部分输出。
- **先拒绝**：`fallback_cannot_send_private_context_to_new_provider`、`fallback_cannot_reduce_required_capabilities`、`cancelled_queue_entry_never_sends`。
- **再成功**：`rate_limit_queue_is_bounded_and_fair`、`failed_attempt_releases_provider_permit`、`allowed_fallback_has_its_own_route_and_usage_receipt`。
- **退出 / 证据**：退避不占网络槽；同一 API key 的多个别名不绕过配额；断路器状态是派生健康信息，不能授权执行。

<a id="step-p4-j7-26"></a>



#### P4-J7-26 模型事件、脱敏和完整关联链　⏳

- **依赖**：`P4-J7-20`、`P4-J7-23`、`P4-J7-24`、`P1-J8-01`、`P0-G-04`、`CP-26`。
- **改动位置**：既有 RunnerEvent::ModelTurn、RuntimeEvent、schema registry、core events/receipts、daemon projections/redactor。
- **步骤**：① 登记 prepared/denied/attempt/retry/finished/usage correction 的版本化载荷；② 贯通 Session/Turn/Run/step/ModelCall/attempt/ExecutionId/provider request/response/tool/Invocation，采用 CP-02 的 legacy 迁移；③ 记录 prompt/schema/route/config hash 与实际模型，不持久化完整请求；④ header/query/error/body/delta/replay 全链脱敏；⑤ 持久日志维持轮次聚合，增量显示不产生逐 token 写放大；⑥ 注入 append 失败，证明缺事实时不会派发工具或标完成。
- **先拒绝**：`provider_secrets_split_across_deltas_are_redacted`、`provider_error_body_cannot_leak_authorization`、`model_fact_persistence_failure_prevents_tool_dispatch`。
- **再成功**：`model_attempt_to_invocation_receipt_chain_is_complete`、`provider_trace_records_actual_model_and_config_version`、`provider_delta_ledger_granularity_remains_bounded`。
- **退出 / 证据**：元数据缺 provider request id 时明确 unknown；UI sequence、上游 sequence 和 durable sequence 不混用；不打印完整 prompt/key。

<a id="step-p4-j7-27"></a>



#### P4-J7-27 完整轮次恢复与 in-flight 对账　⏳

- **依赖**：`P4-J7-20`、`P4-J7-26`、`P0-G-03`、`P0-F-03`、`P2-K6-01`、`CP-18`、`CP-19`、`CP-20`。
- **改动位置**：core/history/recovery/invocation_projection、daemon replay artifact 读取、runner 原有 resume 路径、Provider continuation 编码。
- **步骤**：① 从 committed 模型轮次与工具事实重建有序历史；② 恢复 route/profile/dialect/replay refs，验证 hash/TTL/数据授权；③ 显式 resume 重新领许可，复用原 drive_run；④ 覆盖发送前、发送后、模型完成未落账、工具完成未展示的 crash 点；⑤ in-flight 模型未知与 capability unknown 进入各自 incident/用量处理；⑥ 远端续接 ID 失效时仅使用允许且完整的本地材料，否则拒绝。
- **先拒绝**：`restart_with_missing_replay_material_fails_closed`、`stale_route_authority_cannot_resume`、`inflight_model_attempt_is_not_resubmitted_on_restart`。
- **再成功**：`fresh_process_resumes_completed_model_turn_with_tool_history`、`resume_never_reexecutes_completed_invocation`。
- **退出 / 证据**：独立进程 crash/reopen 证据才支持 durable；UI transcript 不作恢复来源，partial response 不伪装完整 assistant turn。

<a id="step-p4-j7-28"></a>



#### P4-J7-28 模型选择、诊断与事件投影　⏳

- **依赖**：`P4-J7-10`、`P4-J7-11`、`P4-J7-25`、`P4-J7-26`、`P4-J7-02`、`P4-J7-03`、`P2-M2-01`、`P2-M5-01`、`CP-22`。
- **改动位置**：protocol/client 的配置/目录/诊断 DTO、daemon 投影；CLI/Workbench/Web，Desktop 复用 Web。
- **步骤**：① 共用连接/模型能力目录和配置校验命令，密钥值不回读；② 展示 native/synthetic/buffered、排队、重试时间、取消、错误和未知用量；③ 显式连接测试通过同一 gateway/admission，打开设置页不触发请求；④ 保留配置保存/静态校验/live 验证三种状态；⑤ 接 terminal replay、snapshot+cursor，权限或 config epoch 改变时丢弃旧投影；⑥ `model catalog/smoke` 的现有调用链同样迁回准入路径。
- **先拒绝**：`model_settings_view_does_not_trigger_inference`、`stale_provider_ui_action_cannot_mutate_active_run`、`web_reconnect_never_retries_model_request`。
- **再成功**：`three_surfaces_show_same_provider_diagnostics`、`late_subscriber_observes_terminal_from_receipt`、`no_stream_collects_without_second_model_request`。
- **退出 / 证据**：用户看到可行动错误，不暴露协议调试细节；保持现有默认流式行为，额外上游设置有明确来源与实效。

<a id="step-p4-j7-29"></a>



#### P4-J7-29 离线合同矩阵、属性测试与故障语料　⏳

- **依赖**：`P4-J7-20`、`P4-J7-21`、`P4-J7-22`、`P4-J7-25`、`P4-J7-27`。
- **改动位置**：拟新增 `kiana-provider/tests/provider_contract.rs`、`provider_streaming.rs`、`provider_retry.rs`、`fixtures/provider/`；既有 Fake tests 迁移保留。
- **步骤**：① 每协议登记 text/tools/stream/usage/refusal/length/reasoning/structured/image 支持矩阵；② 使用同一中立用例验证所有声称支持的 adapter；③ 加任意分块、乱序/重复 ID、截断、超限和取消的 property/故障测试；④ cassette 保存协议版本、来源、hash 和脱敏规则；⑤ 录制与回放分离，默认全离线，Fake 不可悄悄降到 live；⑥ 每项 unsupported 也有出站前拒绝断言。
- **先拒绝**：`every_claimed_protocol_rejects_incomplete_tool_calls`、`fixture_replay_never_opens_external_connections`、`unsupported_capability_matrix_matches_preflight_errors`。
- **再成功**：`all_supported_adapters_satisfy_model_contract`、`arbitrary_chunk_boundaries_preserve_final_output`。
- **退出 / 证据**：只编译测试不算通过；记录每矩阵格命中测试/fixture，避免空过滤结果；fuzz 只证明其运行范围，不声称无漏洞。

<a id="step-p4-j7-30"></a>



#### P4-J7-30 产品链和四表面回归　⏳

- **依赖**：`P4-J7-28`、`P4-J7-29`、`P0-M1-01`。
- **改动位置**：daemon/entrypoints/client 的集成 tests、四表面 smoke、既有 release harness 脚本。
- **步骤**：① 用 loopback provider 跑模型→shell/apply_patch→工具结果→下一轮→Receipt；② 两角色/两连接、run/continue/resume、受控审批、取消和预算各一条；③ 在 CLI/TTY/Web/Desktop 观测相同 run 终态、文件效果和用量；④ 强制慢订阅、断线、迟到、终态后增量，证明不影响真实执行次数；⑤ 运行必需静态/全量回归，基线失败按快照分类。
- **先拒绝**：`provider_output_cannot_bypass_control_plane`、`cancel_between_model_finish_and_dispatch_runs_no_tool`、`slow_subscriber_does_not_drop_durable_terminal`。
- **再成功**：`provider_backed_coding_run_produces_correlated_receipt`、`four_surfaces_agree_on_provider_run_outcome`。
- **退出 / 证据**：至少包含真实沙箱内的文件/进程观测；只生成模型自然语言不算 coding 闭环；mock 仍只支持 local_behavior。

<a id="step-p4-j7-31"></a>



#### P4-J7-31 逐协议、逐连接 live 验收与迁移收口　⏳

- **依赖**：`P4-J7-30`。
- **改动位置**：`scripts/provider-live-smoke.sh`、USER/README/CURRENT_STATUS/module-map 与支持矩阵；新 smoke 结果使用既有 Receipt。
- **步骤**：① 准备已通过离线回归的制品、固定连接/协议/模型/profile、费用与调用上限、脱敏采集脚本；② 用户已授权真实请求的连接分别做文本、工具两轮、原生增量、取消和各自声明的进阶能力；③ CLI/TTY/Web 使用真实连接分别采集首块与终态时间，Desktop 复用链路也记录实际检查；④ 将本地推理服务、原生服务商、兼容网关分开登记；⑤ 输出 requested/reported model、版本、请求次数、usage/未知项、Receipt、artifact hash；⑥ 修正文档旧 live/默认开关描述，提交/发布按当次授权，保留旧配置迁移和回滚方法。
- **先拒绝**：`required_live_smoke_fails_when_selected_connection_is_unconfigured`、`live_smoke_rejects_synthetic_streaming_claim`、`live_probe_uses_authorized_gateway_and_budget`。
- **再成功**：`selected_provider_live_coding_round_trip`、`selected_provider_live_delta_arrives_before_terminal`；实际命令和观察文件随每个连接记录。
- **退出 / 证据**：每个连接单独验收；缺凭据或未实跑的模型保留 unverified/对应局部状态，不伪造通过、不阻塞已具备条件的连接取证。`--skip-if-unconfigured` 的 exit 0 是跳过，不是 live 成功；一次 live 不提升跨重启证明。

### 16.4 执行时的验证命令与证据要求

以下是实施计划，**本次文档调研未运行这些产品测试**。先在稳定实现快照上复现对应卡的拒绝样例，再改代码；每次命令检查命中测试数量，不能把 0 tests 当 GREEN。

现有基线命令：

```bash
cargo test -p kiana-services --test provider_standard --locked --offline -- --test-threads=1
cargo test -p kiana-daemon --lib model_client::tests --locked --offline -- --test-threads=1
cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
```

`kiana-provider` 和测试 target 创建后，按卡执行所涉及的合同，不在创建前假装已有命令可用：

```bash
cargo test -p kiana-provider --test provider_contract --locked --offline -- --test-threads=1
cargo test -p kiana-provider --test provider_streaming --locked --offline -- --test-threads=1
cargo test -p kiana-provider --test provider_retry --locked --offline -- --test-threads=1
```

每个集成窗口运行以下回归；daemon/control-plane 全程串行，现有脚本内部的并发规则也要核对：

```bash
cargo check --workspace --locked --offline
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --offline
cargo test --workspace --locked --offline --no-fail-fast -- --test-threads=1
bash scripts/harness-golden-smoke.sh
bash scripts/v10-workbench-smoke.sh
bash scripts/v10-p0-closeout-smoke.sh
bash scripts/release-smoke.sh
```

真实连接验收在授权与配置齐备后才运行 `bash scripts/provider-live-smoke.sh --required`。先完成 `-28/-31` 对该脚本所调 catalog/smoke 路径与选择范围的修订；不因发现环境变量存在就把所有已配置账户一起实跑。

每张卡的证据至少包括：`source_snapshot / worktree_status / command_argv / cwd·environment / fixture·cassette / exit_code / status change / proof-level change / limitations / reviewer`；增加 protocol/dialect、connection 的非敏感标识、requested/reported model、config/profile hash、实际请求/派发次数和产物 SHA-256。

合同测试证明其覆盖的本地函数；完整 daemon mock 链才记 local_behavior；独立进程 crash/reopen 证明 durable；真实服务连接才记 live。原有历史 CI 不能套到新 Provider 工作树。发现语义冲突时补显式迁移和回归用例，不删除断言或把失败标 ignore。

### 16.5 延后能力的接入位置

后续确有需求时继续追加 J7 编号：Azure/Bedrock/Vertex 认证与部署适配；官方支持的 OAuth 登录与刷新；远端 Files 的上传/引用/删除；Batch 的 submit/poll/cancel 与结果对账；Realtime 的双向音视频；服务端托管工具。每项都复用本节的 connection、admission、预算、replay 与 Receipt，并提供独立协议和数据生命周期验收，不能通过任意 extra_body 或借用外部 Agent SDK 直接开放。

---

返回：[路线图总图与当前窗口](../roadmap.md#appendix-navigation) · [文档总入口](../README.md)
