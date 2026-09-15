# OA-00 Observability / Audit 现状 inventory

> 快照日期：2026-09-15。本文是 `OA-00` 的 source-only inventory，不是
> `ObservabilityPort`、Audit、Metric、Trace 或 Health 管线的交付声明。它记录事实、
> 派生视图和尚未存在的合同，避免把显示、统计或评测快照误当成授权或业务结果。
> 本轮不在本地运行测试；`observability_baseline` 仅由 GitHub Actions 执行。
> OA-01/OA-02 后续引入的 domain contract 与 correlation 模块会使导出/注册表源码发生
> 预期漂移；本表继续锁定 OA-00 所覆盖的旧边界文件，并把后续 overlay hash 单独列明，
> 扩展时必须在同一提交中更新 hash 和迁移说明。

## 1. 快照、范围与证明上限

| 项目 | 记录 |
|---|---|
| roadmap 卡 | [`OA-00`](../roadmap.md#step-oa-00) |
| source snapshot | `0fb757588a232333ecb0e8304215e8c247ad0d8`（SW-00 已推送的干净基线） |
| feature_status | `partial`：事实/收据/展示的现状已盘点；正式 observability/audit 目标仍未实现 |
| proof ceiling | `source`；静态检查与源码索引不提升 `local_behavior`、`durable`、`live` 或 `physical` |
| 事实源 | `ControlPlane → EventStore → RuntimeEvent/CommandReceipt`；Receipt 和所有信号均须标为派生或观察 |
| 范围 | `docs/module-map.md`、`CURRENT_STATUS.md`、domain event/journal/contracts、core event/receipt/projection/recovery/versioning、EventLog adapters、daemon RunStream/Host、现有 `P1-J8-01`/`ER-30` |
| 本轮不做 | 不新增 schema、sink、exporter、projector、health probe、audit query、第二事实源或第二执行循环；这些属于 OA-01+ |

源码快照 hash（后续 OA 步骤改动这些文件时，必须重新盘点并更新护栏）：

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Runtime event shape | `kiana-domain/src/states.rs` | `dca28dc71ef75e5dd92a25bed399349ca286bc7c5c0e514e11de20d6d1491ae3` |
| Transition contract | `kiana-domain/src/journal.rs` | `4dbd656d03ec12e7821ffac254307227419423cbaf74ccb99a2b819c5eeedd48` |
| Schema/ID registry | `kiana-domain/src/contracts.rs` | `0b6c395976b521cf189af498fcda6a7baf7ce9fcd6a3cd649ae72c1143299a40`（OA-05 health schema 扩展） |
| Domain exports | `kiana-domain/src/lib.rs` | `ac35afaebdb152eddb0898f7e9a28a886d17abdc35643d71cdfb2267e3f731a4`（OA-04 audit reducer 导出） |
| Audit taxonomy/reducer（OA-04） | `kiana-domain/src/audit.rs` | `b17e84def5825be9c2fbdfa704ffe12f9502821325e0c2f66e977b1d72aed1e1` |
| Core audit facade（OA-04） | `kiana-core/src/audit.rs` | `9c444ff6a125a90ac6ba25c1756802e826c40e681ce24f21645f1f08eff9ee22` |
| Health snapshot（OA-05） | `kiana-domain/src/observability.rs` | `03b6f822d5bac02c4717e2cf32796f6e8bfe53e853670b25ae88c0e98d5c7ea2` |
| Span lifecycle contract（OA-07 overlay） | `kiana-domain/src/observability.rs` | `2079d5fcc4850d50e9e73a1f386144d81c03c1d4d8ccb7936062ce4e739e0d5f` |
| Model attempt contract（OA-08 overlay） | `kiana-domain/src/observability.rs` | `5e53b7a30fc87243e4c997f2dd9a9f2fb5108e7cfe6eb67ddda0aa40d7deceeb` |
| Capability attempt contract（OA-09 overlay） | `kiana-domain/src/observability.rs` | `80d4c60dfd20b9508b39c82c2ad39fdb9b7fbd16a45b3854ec27e5ad8903d708` |
| Signal ports/fakes/observer contract（OA-05/OA-06） | `kiana-ports/src/lib.rs` | `36fca0363cd1aab7ca00d3a75375be99ab6aeb13de28f38e42fcf425a96b16f0` |
| Correlation links（OA-02 overlay） | `kiana-domain/src/correlation.rs` | `6fabe5e7cb8d2c86604738108eebcea6274d36306afac197a6115d13b1bfa951` |
| Event construction/redaction | `kiana-core/src/events.rs` | `7d1852ad4a9d93792256288a01a25e06c677e0f6641274b2575b718c87020679` |
| Receipt projection | `kiana-core/src/receipts.rs` | `dda33c346b1ffd94389093e2856f99433ea083a3c61b35cf562484f9f4bc7e1e` |
| Run/invocation projection | `kiana-core/src/projection.rs` | `20d84eb8fa0ac77ac85b48acb10e2fcc78214cc1c3c10be339b540230b1ff68` |
| Recovery/checkpoint | `kiana-core/src/recovery.rs` | `f964c69bb32e940782ef52b399afd9ad6902778b4900f6b7164cb79f3dcc7d26` |
| Lifecycle receipt write | `kiana-core/src/lifecycle.rs` | `346ecb8ea2879b24a53a2ee238bb46c3f7793ffa6211c126ec86ad49c47bc237` |
| Golden trace/replay | `kiana-core/src/versioning.rs` | `679c88757469189f93b20163bbfccb9d65a161ab24ec9f6d9e73a483586108ab` |
| Review/Outcome boundary | `kiana-core/src/collaboration.rs` | `49e61cf89add855ca4fc3687104d666388477848d63397b9ea50b0404021dabf` |
| EventStore facade | `kiana-eventlog/src/lib.rs` | `7af0aba67a44e4575ffd7920ab07866a28c4dbbfcf0c7a3a96221986dfc190bb`（OA-06 observer export） |
| Commit observer wrapper（OA-06） | `kiana-eventlog/src/stream.rs` | `bc955e9c583466a97c1fb02fda4bff8af076dd04ee8df342e21777345b9c033a` |
| Span lifecycle projection（OA-07 overlay） | `kiana-core/src/span_projection.rs` | `e28d545faeb51c8ff1c6410aac0f8b96687a8e283641cb6ac6887db15b9fdfec` |
| Model attempt projection（OA-08 overlay） | `kiana-core/src/model_attempt_projection.rs` | `ce723db86729bbce0c9391157dbf8140bb09470a8d81ba05ebf4ba613a168de0` |
| Capability attempt projection（OA-09 overlay） | `kiana-core/src/capability_attempt_projection.rs` | `e2de85b4baaabdf0afd9b7b60713e96b950a771f6354008b9daec4791924986a` |
| Core capability-attempt exports（OA-09 overlay） | `kiana-core/src/lib.rs` | `ab12f84c7cfa46ab7744801364608a7d8e98994d0a70d78b12544f40e67bdd87` |
| Operational metrics projection（OA-10 overlay） | `kiana-core/src/metrics.rs` | `cb1a53c3efd0232136d43aadf67b667e7155e4a330cc7faedf2f929d1886514b` |
| Metric snapshot/catalog contracts（OA-10 overlay） | `kiana-domain/src/observability.rs` | `e7aa1777a5e4cb3fbea7bbbf2ca20ac86c49ff020a8f74680da99c297bfeeb41` |
| Health probe/component contracts（OA-11 overlay） | `kiana-domain/src/observability.rs` | `dff3e21fb86902a6a9575d9fd108c0b5240236e9968169431fe9bcd752066757` |
| Metric quality/cardinality contracts（OA-12 overlay） | `kiana-domain/src/observability.rs` | `bb901d2eb92336f6848466767e518bf1a302c42ce2fa2b0d000fd9dc37a343ba` |
| Schema registry metric snapshot（OA-10 overlay） | `kiana-domain/src/contracts.rs` | `15f79bf7d1743a22069f88d926fc2819cdf72ee170c99a5485e1cc40e12617a8` |
| Core metrics exports（OA-10 overlay） | `kiana-core/src/lib.rs` | `fcd574fa2e53f2c4d1b4902ab7027197cfbb1cfb4c79ad481205c8a41c2d8571` |
| Core health aggregation（OA-11 overlay） | `kiana-core/src/health.rs` | `92eac9ddc14aee095ab3fbc492b738acfe85452839b6afe28111632090743cdf` |
| Core health exports（OA-11 overlay） | `kiana-core/src/lib.rs` | `3a2577d0c6659fcb5d1b455787a08fbba30f24f247576e5fcbf1a5cac4e40083` |
| Metric reducer/cardinality guard（OA-12 overlay） | `kiana-core/src/metrics.rs` | `5ca67e24913dbb43f4b90e9e6c4054d42da072e767069a33fcefdb58fdb622ea` |
| Metric reducer exports（OA-12 overlay） | `kiana-core/src/lib.rs` | `db071a3de3d0c97e97027eed5c453d7e9a1650f4fa5e10f71466035c364512b5` |
| Trace export/context contract（OA-14 overlay） | `kiana-domain/src/observability.rs` | `101acc38e9429242da3ae52f23beb1d466f09ee583def9e4b7b17e862e274b81` |
| Trace export/context adapter（OA-14 overlay） | `kiana-core/src/trace_export.rs` | `6045461f648248005321426def7090388f134855d24fa0fe93b7b3bcac568191` |
| Trace export schema registry（OA-14 overlay） | `kiana-domain/src/contracts.rs` | `77d15cfcf55353128bb8f872698cc20efa5087ea76af910f6b8c41dbcd11e628` |
| Trace exporter exports（OA-14 overlay） | `kiana-core/src/lib.rs` | `a739e3cee27d9e004a70cc2cf9c937b5da32e6d5c25a26aeeacedf1fa4d681a3` |
| Audit projection/checkpoint contracts（OA-15 overlay） | `kiana-domain/src/observability.rs` | `60748f9393fc8e93d70a8d9f910a38827597d11eb00e0e8216ca97e6a796f84e` |
| Audit projection schema registry（OA-15 overlay） | `kiana-domain/src/contracts.rs` | `07217343e1c430d8393abe1ecead4ed61c426bc9441f73b5260d0abb84f81738` |
| Audit projection reducer（OA-15 overlay） | `kiana-core/src/audit_projection.rs` | `45e6ed8775495ed0ecc4de129eb6484a8b4b82796e9f90c004054d6fe761c52f` |
| Audit projection exports（OA-15 overlay） | `kiana-core/src/lib.rs` | `3b0366662dd10466ef00b286de5615a2901ba8195481ae4ce5834ff89af06405` |
| Audit query/core filter（OA-16 overlay） | `kiana-core/src/audit_projection.rs` | `0bf6f397417c2d76d43ebab79e0b2b59e664901dbd8231a364cfc3539d45c5c9` |
| Audit query schema registry（OA-16 overlay） | `kiana-domain/src/contracts.rs` | `83fb1a4a025385b1d291be4908e7f3301a9bdda8da0e110662ce49b39ae7a524` |
| Audit query protocol DTO（OA-16 overlay） | `kiana-protocol/src/lib.rs` | `ec545dd0390d7492bf455d5824af8370afd7ca6d47969f01695856204a5fb407` |
| Audit query client facade（OA-16 overlay） | `kiana-client/src/lib.rs` | `c9749b96fb2077ed574216390a935195d9c7a966cad7a8753abd4bc1df516136` |
| Audit query DaemonHost route（OA-16 overlay） | `kiana-daemon/src/lib.rs` | `e7ed9167cd00c39a6c292661a7909d091d2418be3acd18494c2ef5c5458745c3` |
| Audit query cursor contract（OA-17 overlay） | `kiana-domain/src/observability.rs` | `9e2f417c4cc88636c5049a74cb87d863678c6bf9d8f7309a28eb25331a106537` |
| Audit query cursor schema registry（OA-17 overlay） | `kiana-domain/src/contracts.rs` | `8739613390c463a5d1d45eb066c6f26a52cdbf9a205ae12ea7f7d9ff794f0a49` |
| Audit query cursor reducer（OA-17 overlay） | `kiana-core/src/audit_projection.rs` | `5ddf6d04d6ce42d9f657a65d00f40a391af69f80a4f76aa785d57e4268fcd5fd` |
| Audit query cursor protocol（OA-17 overlay） | `kiana-protocol/src/lib.rs` | `2c5506dd9bdb9e998898fe8cb367442b7d3b71298bf6569a48ee321bf1cdaf1e` |
| Audit query cursor DaemonHost route（OA-17 overlay） | `kiana-daemon/src/lib.rs` | `5bdb0e6a02eef8b52d636237f1c148b44a35d2772c78aed9c03a51eb802ce7e4` |
| Audit export/delivery contracts（OA-18 overlay） | `kiana-domain/src/observability.rs` | `305164150f974706c6398f7aa7c73897850e4540c6a7893d3a524fac9b236d3e` |
| Audit export schema registry（OA-18 overlay） | `kiana-domain/src/contracts.rs` | `479cb9668acb51b29a9629ae60c9bd47b47cf8115020ffe9a5754a6dadd08cf5` |
| Audit export materializer（OA-18 overlay） | `kiana-core/src/audit_export.rs` | `7294a6434b5bbf3b8f679db10b6d7e896a041a2d09693b209d4d4e07bfc6b563` |
| Audit export core exports（OA-18 overlay） | `kiana-core/src/lib.rs` | `76edce8eff56cbd019b0925993c3620fc0001f81f9fda8f29ebebeab767540f5` |
| Audit export protocol/client/daemon（OA-18 overlay） | `kiana-protocol/src/lib.rs`, `kiana-client/src/lib.rs`, `kiana-daemon/src/lib.rs` | `76efc57ef967fd81a2e7ad4e98acfd273cb904fcfc8012e05ed8a34105ac9b56`, `314f28d69494f53c8ece0d552a2d086ffbdf96ef332fafeb69c985882b069e78`, `2353b3f4fa00b795f03ec848081d40e265690b0cc7174c6e3f01ebc092aeece4` |
| Observability incident contracts（OA-19 overlay） | `kiana-domain/src/observability.rs` | `19d89e13abd551e6052f0522e9a220a717cfac7d060012ca607e67543b915096` |
| Observability incident schema registry（OA-19 overlay） | `kiana-domain/src/contracts.rs` | `da24e4a042672a464ad5b429519898770cc9b7ca30e3ed508b19988db2c22394` |
| Observability incident reducer（OA-19 overlay） | `kiana-core/src/incident_projection.rs` | `78de0bdb32cf1a978bfe9629a12cb5863fc28038f71ba6247b6ade579027a08b` |
| Observability incident exports（OA-19 overlay） | `kiana-core/src/lib.rs` | `941de8ac868890b0fd5ce3e177b3ef880160163a884cb33f371d37d772aef793` |
| Data governance policy/snapshot contracts（OA-20 overlay） | `kiana-domain/src/governance.rs` | `8d64488c287fe7fe8e2a67f9db2cafa685c50d0012600a6ac23fe4d372e1a777` |
| Data governance schema registry（OA-20 overlay） | `kiana-domain/src/contracts.rs` | `3d8f7da4c5dda097ecda67d4ca652e143596ec876e87e9b028f1999ee67c9a6a` |
| Data governance projection/invalidation（OA-20 overlay） | `kiana-core/src/data_governance.rs` | `8ec01f91afc04392dde99cb205d09b68f1d8b035e4d8e06fa4d8c8516bca715f` |
| Data governance core exports（OA-20 overlay） | `kiana-core/src/lib.rs` | `9a4fd415a1daa78733d94ddc6055bc3eb4daeed975b9211152d97bf8f1044e6e` |
| Daemon policy propagation/integrity（OA-20 overlay） | `kiana-daemon/src/data_governance.rs` | `1baaaa459fa88c6bb197c8c0ef809278c9e262c398681ccc5dc0b063148d9bce` |
| Replay diagnostic contracts（OA-21 overlay） | `kiana-domain/src/observability.rs` | `db8856e67efa6f69cdf6a329cc170b9c34e3727a57081f38315f4da2a030fefc` |
| Replay diagnostic reducer（OA-21 overlay） | `kiana-core/src/replay_diagnostics.rs` | `a368cc524fdb38828184f60a415463250eaf20c33ed301cc9de51a137991bc2c` |
| Replay diagnostic exports（OA-21 overlay） | `kiana-core/src/lib.rs` | `cb3c14fc103a0d18085c975707c96394780478b1aada9661c91078784784cfd5` |
| Fault injection contracts（OA-22 overlay） | `kiana-domain/src/fault.rs` | `0a4a1514099e5877bdc2d71d5be39a47d1022dcf6b8030fdb730c4c4ed0fb89b` |
| Fault injection schema/exports（OA-22 overlay） | `kiana-domain/src/contracts.rs`, `kiana-domain/src/lib.rs` | `e898cd7b6f88a1adeec188658af4fcc7334af137458c4cb189f941146f01e1f1`, `5d44ac73e6e10f978d45471792d539848b202dae5927a07b24da5dfe5e834373` |
| Fault injection simulator（OA-22 overlay） | `kiana-core/src/fault_injection.rs` | `44f6915a880c0fe8991819291b606435c80fd6926721d8bb6484ddd90251d23a` |
| Fault injection core exports（OA-22 overlay） | `kiana-core/src/lib.rs` | `74de04f3cf1c2e53c82bf8184cb35d3f818aacfc7bad5d7b3bd9e2aa0bc219d8` |
| Replay/fault contract exports（OA-22 overlay） | `kiana-domain/src/lib.rs`, `kiana-domain/src/contracts.rs` | `5d44ac73e6e10f978d45471792d539848b202dae5927a07b24da5dfe5e834373`, `e898cd7b6f88a1adeec188658af4fcc7334af137458c4cb189f941146f01e1f1` |
| Provider-independent eval contracts（OA-23 overlay） | `kiana-domain/src/eval.rs` | `abde909e15ff96b8cf4d4020d85da233d667bce1dede12d2c2c58053b803e5f4` |
| Eval schema registry/exports（OA-23 overlay） | `kiana-domain/src/contracts.rs`, `kiana-domain/src/lib.rs` | `f8ce4fd84bca887e12e7ecd0a66b44a1e47b61c77577b1d11fa8f8c2a7b45607`, `4a5992e66ffd5c7c0bd1bba2ecd4876931c9d232b3e0541828af7bba03ee5f76` |
| Provider-independent eval reducer（OA-23 overlay） | `kiana-core/src/eval.rs` | `dc2d6e06758e04343a9ee3a33865e0fdd988a7e86e97949ad536eaebc3d1df45` |
| Eval core exports（OA-23 overlay） | `kiana-core/src/lib.rs` | `f46e73277fd78d82b0b8fdbf4b9c7279a07bf5c7a35b9e6a651c0c3d47a97150` |
| Entrypoint parity contract（OA-24 overlay） | `kiana-domain/src/parity.rs` | `1e0ae3f680b88b78ffd9cae1c8c0a5dff8ff881d2a1ee1f7e44d1b868b3b4f68` |
| Entrypoint parity schema/exports（OA-24 overlay） | `kiana-domain/src/contracts.rs`, `kiana-domain/src/lib.rs` | `9dd25e93e5c07010094ce951f30392c66b72d38e85aa74329fe820be8a61b72d`, `44919ea7760ebd2cda9743b15187e0a746ec2e9d9f850addb5d5547c0195db2d` |
| Entrypoint parity reducer（OA-24 overlay） | `kiana-core/src/parity.rs` | `39f699e1d7b463f796f5ba3873452478b0f129f3e39ceaaeb0e8b8d9b443a80b` |
| Entrypoint parity core exports（OA-24 overlay） | `kiana-core/src/lib.rs` | `b3e8a0d6f43ce7b1d802e1c772638740d1d1e49071cbb55cdbb6f2b9051557fa` |
| Entrypoint parity protocol/client（OA-24 overlay） | `kiana-protocol/src/lib.rs`, `kiana-client/src/lib.rs` | `f67fe65db42cc4eb26056a8ea4e5aac3ec6ce4badaf78dfd8c1aa88486e7f654`, `4d5d5aa16c5dc990110a3299af517873ab30de84028f75253cb10d3bb44754df` |
| Entrypoint parity daemon/entrypoints（OA-24 overlay） | `kiana-daemon/src/lib.rs`, `kiana-entrypoints/src/cli.rs`, `kiana-entrypoints/src/harness_run.rs`, `kiana-entrypoints/src/web.rs`, `kiana-entrypoints/src/web_page.html`, `kiana-entrypoints/src/workbench_chat.rs` | `718021e2be25fd91af85fab304b7ca4fb57526c564b948ef73dc6babe4882423`, `a709fa28d4062d8b915c47b21f36b98972261a6a4eef372cfb9cd44669a1a912`, `469cc92809f38a51d0682b8ed3bc6692d1a311c5a84e988740e67e0777e3e6cc`, `6e3cab51eca20a46fd5ccad8243daa05367e9164bcf91a2fd3223f350ab70870`, `699187f46aa649cb07fe4f608e67916cfd3a4ceb78d5ee6db9b5613fc7334342`, `40e6be895cbf0c3d7e0386b4dda82c9311180c2e9e1d3699013f9272ac2c8d02` |
| Entrypoint parity remote fixtures/workflow（OA-24 overlay） | `kiana-core/tests/oa24_entrypoint_parity.rs`, `kiana-protocol/tests/oa24_parity_wire.rs`, `.github/workflows/oa24-entrypoint-parity.yml` | `1f8db3d08575afbf2a6522c460744640025ca80c03b7255aaa8d6738cde62858`, `c26d1bc7fb727e2d6582b92b689e3855d49d15131f75cc1c2c1e701305c6038d`, `da5ff3b63d165302c41b602cba5d3e56861fcfaa95cbc31987a442417f194e00` |
| Bounded observability queue（OA-13 overlay） | `kiana-ports/src/observability_queue.rs` | `ae2d6c4b9f7b9d9f1bac0fe73e8a2471be5c40bd04145e0f5bc53f0db89345cf` |
| Queue port exports（OA-13 overlay） | `kiana-ports/src/lib.rs` | `a539b96c0813c0f08c043dd89b4f2bcbe9fdb4d6d9f93923443821b54679e6d0` |
| Daemon queue/health bridge（OA-13 overlay） | `kiana-daemon/src/lib.rs` | `e9f3ceb87fdcb7610a91dcbc7cead025fbe5e2f48300e7ce6aaf0c42fd641bde` |
| DaemonHost health bridge（OA-11 overlay） | `kiana-daemon/src/lib.rs` | `d673fd32687057ac9653cd92114182864a477eea2d3736389d2ffd148b127ac4` |
| Capability admission/effect instrumentation（OA-09 overlay） | `kiana-core/src/capabilities.rs` | `34a78eb181ef97253ab41d254ca298e621e9ea540ccec73a0bb94fde7dfbd061` |
| Approval effect metadata（OA-09 overlay） | `kiana-core/src/approvals.rs` | `439a602ee0463d1be33ce144d787e5b7b6cbdb19db7277de2da248aa0cf28b1e` |
| Dispatch permit/execution boundary（OA-09 overlay） | `kiana-core/src/dispatch.rs` | `75ef963c49c89cc3848a542f3dc5a63a01e55d2321b47b1f4bb918addd22f672` |
| Capability event metadata（OA-09 overlay） | `kiana-core/src/events.rs` | `5a0348f5e1940363119d920244724428af1e1373f692f6424ccfd3b8a6dfe26e` |
| Harness effect/stop metadata（OA-09 overlay） | `kiana-daemon/src/harness_capabilities.rs` | `342dcbca27709f41821872c60ab199595f51ebc6fa9dccabbaf0b040ae5c46ce` |
| Provider safe telemetry（OA-08 overlay） | `kiana-provider/src/telemetry.rs` | `1a9ec235b5d0cf8c420d758bde425ff89a0ea8c7d746c38f09daf6a7dfc835d5` |
| Daemon model boundary（OA-08 overlay） | `kiana-daemon/src/model_client.rs` | `02bf42ff2171842679421621d128be26d44298457d758850499eae1694cfa0b9` |
| EventStore append planning | `kiana-eventlog/src/event_store_core.rs` | `506a8222a757a89e6516a12b6260daa7f35af2b3da49c17a23ca7cdbcc9d0b1e` |
| Journal state/replay | `kiana-eventlog/src/journal_core.rs` | `84ef64bc8208b2ead45d1d05feb0265e94129b898cd67d04f5ed189d388bba56` |
| JSONL adapter | `kiana-eventlog/src/jsonl.rs` | `f549e5de5bc4e197f268b865327bd1d0d7a4c10208daa3e7c13ceb68188e5c5f` |
| Memory adapter | `kiana-eventlog/src/memory.rs` | `bcb98daa0a9548384a9dafdd9d5af2aefacca3d01d699056ba86477d3276e9e9` |
| RunStream projection | `kiana-daemon/src/run_stream.rs` | `750bb78e46e5f1c42d3a773d0afcf468cc6713032ef47e30ffa100f3313db5c6` |
| Daemon composition | `kiana-daemon/src/lib.rs` | `f262e808ab239eed0e01c1e28aef75cb5d78b819d809c1d07592ab34d367d281` |
| Port contract | `kiana-ports/src/lib.rs` | `36fca0363cd1aab7ca00d3a75375be99ab6aeb13de28f38e42fcf425a96b16f0`（OA-02 correlation + OA-05 signal + OA-06 observer overlay） |

## 2. Signal matrix

| 信号/对象 | 当前 owner 与接线 | 事实还是派生 | 当前限制 / 迁移 owner |
|---|---|---|---|
| `RuntimeEvent` | `kiana-domain::RuntimeEvent` 是 `kind: String + data: Value`，由 `ControlPlane::append_event`/`record_terminal_event` 构造 | EventLog 中的事实载荷；不是完整 signal schema | 没有 event-kind registry、schema owner、timestamp、correlation/causation 或 DataClass；OA-01/OA-02 |
| `TransitionBatch` / `CommandReceipt` / `CommitOutcome` | domain journal 合同 + `EventStorePort`；JSONL/Memory journal 做 CAS、幂等和 cursor；OA-06 `StreamEventStore` 只在 Committed 后通知 | 命令提交事实 | receipt 证明提交边界，不证明 dispatch/effect/业务 Outcome；observer 是 wake/diagnostic hint，重启仍须按 cursor 扫描；EventStore port 本身不承诺 durable；ER-02/OA-10 |
| `MemoryEventLog` | `kiana-eventlog::MemoryEventLog` | 进程内事实缓存 | `durable_commits=false`，不能作为 durable observability evidence；ER/PD |
| `JsonlEventLog` | JSONL frame、锁、checksum/torn-tail/Unknown 路径 | 本地 EventLog 事实 adapter | `cfg!(unix)` 能力和源码不等于重启/物理 durability 证明；没有 observability projector；ER-31/PD-27 |
| Run / Invocation projection | `kiana-core::projection` 从 run/approval/capability 事件折叠，内存 map 仅缓存 | 派生状态 | 缺正式 projection version、source cursor、lag/checkpoint；缓存不是事实；OA-06/OA-10 |
| `CommandReceipt` → Run receipt | `kiana-core::receipts::receipt_from_events` 从过滤后的 EventLog 生成，并最终再做 redaction | 派生 Receipt | 缺顶层 `source_cursor`/`source_event_ids`/`projection_version`/`limitations` 合同；Receipt ≠ business Outcome；ER-01/ER-30 |
| `run.receipt` event | lifecycle 在成功路径把派生 receipt 再写入 EventLog | 现有事件事实中的 shadow copy | 易使 receipt 看似第二事实源；必须区分原始事实与可重算 projection；OA-06/ER-02 |
| `ResultUnknown` | receipt/projection 对缺 terminal、矛盾 terminal、读取/投影异常保留 `ResultUnknown` | 事实驱动的失败分类 | 没有统一 Incident/Recovery/Audit projection 与 operator query；ER-30/OA-10/OA-19 |
| RunStream / SSE / Workbench | `RunStreamBus` + daemon `StreamEventStore`；只在 committed fresh append 投影，replay 不重复；OA-06 eventlog wrapper 提供通用 committed observer | 进程内、bounded、best-effort 展示 | broadcast send 可丢、epoch/cursor 在内存中；gap/terminal replay 不是 EventLog 事实、授权或 Outcome；OA-13/OA-24 |
| transcript / UI timeline / cache | 入口与 `kiana-screens`/context index 读取 RunStream 或缓存 | 派生视图/加速缓存 | 不能作为状态、审计、成功、健康或恢复依据；OA-16/OA-20/OA-24 |
| model turn / usage | `run.model_turn` 事件携带 provider/model/route/prompt hash、stream/usage/elapsed/finish/retry；OA-08 的 `kiana-core::model_attempt_projection` 从 committed 事件派生 `ModelAttemptRecord`，`UsageRecord` 仍负责成本账本 | 事件中局部事实 + 有界模型 attempt 投影 | prompt/header/raw response 不可进入 projection；malformed/truncated/timeout/retry/missing usage 不得为 `ok`；`run.usage`、`model.usage`、`provider.usage`、`usage.recorded` 的命名收敛与 MetricCatalog/单位/低基数 labels 仍由 OA-10/OA-12 完成 |
| Business Metric / Incident | Company closeout 的 `MetricObservation`、`Incident` 和 platform `FailureIncident` | Company/故障领域事实，不是 OA signal contract | 不能把业务指标或 Incident DTO 当 runtime MetricSnapshot/HealthSnapshot/AuditRecord；OA-04/OA-11/OA-19 |
| `PreparedModelCall::audit()` | provider/model 请求的安全摘要 | 局部诊断摘要 | 不是 `AuditRecord`，无 source event/cursor、decision taxonomy、retention/query boundary；OA-03/OA-04 |
| `golden_trace.captured` / `trace.replay` | `kiana-core::versioning` 复制 run events/receipt，校验 owner/project/hash/revocation，replay 明确 `side_effects=false`/`provider_calls=0` | EventLog 中的评测快照与只读 replay | golden trace ≠ telemetry trace；trace 不得授权、恢复或替代 event order；完整 events/receipt shadow copy 的 retention 需迁移；OA-02/OA-14/OA-21 |
| Operational log | 没有 `ObservabilityPort`/结构化 log sink；OA-03 提供 `RedactionProfile`/bounded encoder | profile/encoder source 已实现；runtime sink 未实现 | exporter、脱敏失败、队列、flush/关闭 ack 未建模；OA-05/OA-13 |
| Metric | OA-01 已注册 `MetricCatalog`/`MetricPoint` domain contract，OA-03 提供 Metric profile boundary；仍没有 reducer/sink | schema/encoder source 已实现；runtime 未实现 | 不得把 usage counter、UI cursor 或 Company metric 当 canonical runtime metrics；OA-10/OA-12 |
| Trace / span | OA-01 已注册 `TraceSummary`，OA-02 已注册 `CorrelationContext`/`TraceRef`/`SpanRef` 与 link，OA-03 提供 Trace bounded encoder；OA-07 的 `kiana-core::span_projection` 派生 Run/Turn/Invocation lifecycle，OA-08 的 `ModelAttemptRecord` 以稳定 model span ID 关联 provider attempt；没有 exporter/`TraceSink` runtime 接线 | schema/correlation/encoder/projection source 已实现；runtime exporter 未实现 | trace 只可关联，不可作为 actor/authority/policy/approval；span/attempt end 不制造 terminal，迟到/重复/旧 attempt 不覆盖事实；OA-09/OA-14 |
| Audit | OA-04 在 `kiana-domain/src/audit.rs` 固定 taxonomy，并由 `kiana-core/src/audit.rs` 从带 cursor/source binding 的 committed `RuntimeEvent` 派生 `AuditRecord`；OA-06 `StreamEventStore` 只在新 Committed 后通知，OA-05 `AuditQueryPort` 仅读投影 | taxonomy/reducer、observer/query port source 已实现；checkpoint projector 未实现 | 模型/UI/plugin/exporter 不能伪造批准/完成/导出；未知 `audit.*`、缺 source/epoch、矛盾 decision、重复 source 记录 fail-closed；observer 丢失须 cursor 扫描；OA-10/OA-15/OA-16 |
| Health / Incident signal | OA-05 新增 versioned `HealthSnapshot` 与 `HealthProbePort`；Health 仍是 probe projection，不是授权或业务 Outcome | snapshot/port/fake source 已实现；runtime probe、lag/incident projector 未实现 | stale、projector gap、unknown exporter、journal corruption 不能返回 healthy；OA-10/OA-11/OA-19 |

## 3. 先拒绝：已确认的风险与证据窗口

1. **显示不是事实。** RunStream、SSE、Workbench transcript、UI cursor、context/index
   cache 和模型自述均是派生视图；只有 EventLog/Transition/Receipt 可定位到事实游标。
   `RunStreamBus` 的 `epoch`、sequence、gap 和 terminal replay 只防止部分 stale/replay
   展示问题，不提供授权或业务成功证明。
2. **Receipt 不是 Outcome。** Receipt 从事件重算并被 redaction；`run.completed` 只说明
   当前运行事实完成。现有 review 路径在 `run.completed` 且 `files_changed` 非空时生成
   `pass`/accepted merge receipt，这不能证明现实交付、业务验收或外部效果，列为
   `receipt/terminal-as-Outcome` issue。
3. **Golden trace 不是 telemetry trace。** `golden_trace.captured` 嵌入完整 events 与
   receipt，是评测/重放快照，不是 W3C trace/span。虽然 replay 不调用 provider 且
   `side_effects=false`，它不能成为授权、恢复或审计事实。
4. **事实先于观察仍需完整收敛。** core 的 `append_event` 在构造事件前做通用 redaction，
   但 EventStore adapter 不负责 classification/redaction，现有 redactor 是有限的 key/marker
   规则且不可失败；所有直接 append writer、future sink/export 都要在 OA-03 统一收敛。
5. **命名和字段仍漂移。** OA-01 已固定 domain signal 的版本、digest、cursor、source event
   IDs 和 attribute 上限，但 RuntimeEvent/usage 仍同时出现多种 event kind，时长有 `elapsed_ms` 与
   `duration_ms`；没有注册表、单位、低基数 label、source cursor、data class、retention
   或 proof ceiling 字段。重复或私自命名的观测字段不得继续扩散。
6. **Unknown 必须保持 Unknown。** JSONL commit 在无法确认写入时返回 `CommitOutcome::Unknown`，
   receipt/projection 对缺失或矛盾 terminal 返回 `ResultUnknown`；EventLog worker queue 满时
   返回 `eventlog_worker_queue_full`，RunStream broadcast 可能丢 best-effort 投影。当前没有
   audit/telemetry 专用队列、flush ack、lag 或 incident recovery contract。
7. **历史证据不重写。** `ER-30` 与 `P1-J8-01` 仍是待实施卡；本 inventory 只记录其与当前
   源码的差距，不把 ER-00 的 EventLog 事实边界或任何既有局部测试升级成统一 observability
   runtime/durable 证明。

## 4. Owner、proof ceiling 与迁移清单

| 后续 owner | 迁移结果 | 现状 proof ceiling |
|---|---|---|
| OA-01 | 注册 observability/audit/metric/trace/health schema、版本和 unknown-field 策略 | `source`；四类 domain contract 已实现，HealthSnapshot/runtime adapter 仍未实现 |
| OA-02 | 服务端构造 CorrelationContext、TraceRef/SpanRef 和 causation/parent link | `source`；domain/port contracts 已实现，尚无 runtime ingress/span bridge |
| OA-03 | 统一 redaction/classification、bounded encoder 和 secret sentinel 全信号扫描 | `source`；profile/encoder 已实现，EventStore/各 runtime sink 尚未统一接线 |
| OA-04 | Audit taxonomy/Record reducer，从 committed security facts 派生 | `source`；domain/core reducer 已实现，尚无 EventLog commit observer、checkpoint、query 或 durable projection |
| OA-05 | Observability/Trace/Metric/Audit/Health ports 与 fake adapters | `source`；ports、Memory/JSONL fake、flush/cancel/capacity/query/probe contracts 已实现，尚无 EventLog observer 或 durable sink |
| OA-06 | commit observer 只通知 Committed，重放不重复通知，建立 projection cursor | `source`；`kiana-ports` 的 `CommittedTransition`/observer contract 与 `kiana-eventlog::StreamEventStore` 已实现；通知是可丢 wake hint，尚无 durable checkpoint |
| OA-07 | Run/Turn/Invocation span 生命周期与 runner/event projection bridge | `source`；`SpanLifecycleRecord`、稳定 trace/span ID、只读 reducer 和 ControlPlane bridge 已实现；尚无 exporter、durable checkpoint 或 live backend |
| OA-08 | provider/model/stream/usage instrumentation | `source`；`ModelAttemptRecord`、provider safe prepared summary、daemon model-port boundary 和 committed-event reducer 已实现；尚无 MetricSink/TraceSink exporter、durable checkpoint 或 live backend |
| OA-08 | provider/model/stream/usage instrumentation | `source`；`ModelAttemptRecord`、safe prepared summary 与 committed-event reducer 已实现；尚无 MetricSink/TraceSink exporter、durable checkpoint 或 live backend |
| OA-09 | broker/approval/effect/stop instrumentation | `source`；`CapabilityAttemptRecord`、handler 前 execution CAS、拒绝/过期/TOCTOU/取消 stop evidence 已实现；尚无 durable attempt checkpoint、外部 effect receipt、reconcile projector 或 live exporter |
| OA-10 | EventLog/projector/Receipt/Artifact/Recovery metrics | `source`；`MetricSnapshot` 与 committed-event reducer 已实现 durable/projector cursor、lag、commit/latency/rebuild/query、orphan/unknown、artifact bytes 和 last-error presence；尚无 durable projector checkpoint、runtime gauge、receipt reconciliation 或 live exporter |
| OA-11 | health snapshot/readiness/liveness/component capability | `source`；`HealthProbeKind`/`ComponentHealth`、core health aggregator 和 DaemonHost read-only bridge 已实现；stale/gap/unknown/unsupported capability 返回 degraded/unavailable；尚无 durable heartbeat、provider/Broker/exporter live probe 或 admission gate 接线 |
| OA-12 | Metric catalog/reducer/cardinality guard | `source`；typed catalog/unit、MetricQuality、catalog digest、allowlist/cardinality guard、overflow、counter reset/cursor regression 和 replay/live reducer 已实现；尚无 durable MetricSink/queue、runtime gauge feed 或 checkpoint/exporter |
| OA-13 | asynchronous queue/backpressure/drop policy | `source`；bounded non-blocking queue、critical preservation、best-effort drop reason/counter、flush/shutdown/reopen/cancel ack 和 DaemonHost bridge 已实现；尚无 durable spool、consumer/exporter、cross-process shutdown 或 queue checkpoint |
| OA-14 | trace exporter and W3C context adapter | `source`；TraceExportSpan contract、foreign parent link、sampling/invalid-parent/capacity guards、local JSONL/no-op exporter、flush/shutdown/reopen 已实现；尚无 OTLP/durable backend、async consumer 或 persisted sampling policy |
| OA-15 | AuditProjection checkpoint/rebuild | `source`；AuditProjectionSnapshot/Checkpoint、rebuild/append/restore、source cursor/event/schema/decision/checksum binding 已实现；尚无 durable checkpoint store、cross-process automatic reload、Artifact ref/query/export/correction/incident wiring |
| OA-16 | Audit query command/wire DTO | `source`；server-scoped AuditQuery request/page、ControlPlane filter、DaemonHost/client route、bounded limit/cursor and raw-event rejection 已实现；尚无 durable query index/filter snapshot, cross-entry parity, export/delivery or external auth provider |
| OA-17 | Query cursor/snapshot/paging and slow-query boundary | `source`；AuditQueryCursor epoch/projection/source/after/filter digest binding、stale/ahead rejection and bounded next cursor 已实现；尚无 durable query index/retention snapshot, slow-query instrumentation, reconnect parity or multi-entry cursor store |
| OA-18 | Audit export/manifest/delivery evidence | `source`；server-scoped redacted JSONL/JSON/CSV materializer、query/source/projection/artifact manifest hashes、purpose/recipient/retention guards 和 Unknown delivery receipt 已实现；尚无 durable ArtifactStore/export file、external delivery connector/confirmation 或 export audit fact |
| OA-19 | Alert/Incident rules, dedupe and Recovery association | `source`；committed metrics/facts 规则 fingerprint 去重、bounded Alert/Incident snapshot、source cursor/event refs、固定 reconciliation-safe recovery plan 已实现；尚无 durable incident checkpoint/event, operator workflow, queue/exporter live state or automatic reconciliation |
| OA-20 | DataClass/Purpose/Retention/Deletion propagation | `source`；versioned DataPolicy/data_epoch/digest、payload-vs-audit metadata observations、committed invalidation projection、derived-store propagation map 和 daemon policy integrity/readout 已实现；尚无 durable policy/snapshot checkpoint、实际 Artifact/Memory/Index/Telemetry purge scheduler、legal hold 或 cross-process deletion proof |
| OA-21 | Replay/reconciliation diagnostics | `source`；read-only deterministic Invocation/Run/Metric/Audit/Health/Span comparison、bounded divergence locator、projection digests、unknown/schema/gap/duplicate guards 已实现；尚无 provider receipt reconciliation、automatic retry/compensation, durable diagnostics checkpoint or fault/capacity gate |
| OA-22 | Crash/fault injection | `source`；deterministic replay-only eight-point FaultMatrix/Case safety contract、seed/source binding、unknown/rejected fencing and duplicate/false-success invariants 已实现；尚无 real crash/process fault hooks, EventStore/Broker/Provider/projector/export/shutdown injection, durable recovery or cross-process resource proof |
| OA-23 | Provider-independent eval suite | `source`；versioned EvalCaseSpec/Result/Suite、normalized event/Audit/Metric/Span/Run/Replay evidence、secret/forbidden-effect/missing-evidence/status/replay/cost guards、promote only all-pass 已实现；尚无 real provider eval, Promptfoo runner, durable eval artifact, external receipt or automatic promotion/rollback |
| OA-24 | 四入口审计/健康/Receipt parity | `source`；`kiana.entrypoint-parity.v1` 与 owner-scoped ControlPlane projection 已实现，CLI/Web/Workbench/Desktop 通过 protocol/DaemonHost 复用 source cursor、status、Receipt/Audit/Health digests、retention/unknown limitations；健康 endpoint 使用 liveness projection，入口不读 EventLog、不自行判定成功或恢复；尚无 durable query index、外部认证/健康探针、跨进程 retention/reconcile 或真实业务 Outcome 证明 |
| OA-10–13 | Receipt/Health/Metric reducer、lag、队列背压与丢弃分类 | `source`；没有 runtime gauges 或 telemetry queue |
| OA-14–18 | trace exporter、Audit checkpoint/query/cursor/export | `source`；golden replay 不是 exporter，不能声称 durable/live |
| OA-19–21 | Incident/Recovery、retention/deletion 和 replay diagnostics | `source`；FailureIncident/Company Incident 不能代替 OA Incident |
| OA-22–28 | fault/eval/入口 parity/容量/local durable/live handoff | `source`；无本地 runtime 或外部 backend 证明 |

## 5. 现有测试边界（仅源码索引）

源码中已有 EventLog 的 Memory/JSONL 序列、幂等、重开和 Unknown 测试，以及 core 的
`event_stream_read_failure_prevents_append_and_runner_dispatch`、
`receipt_replays_result_unknown_without_claiming_success`、
`receipt_read_all_failure_fails_closed`、
`runner_delta_completion_and_receipt_are_redacted`、
`new_process_rebuilds_run_state_from_events_alone` 和
`new_process_rebuilds_invocation_state_from_events_alone` 等局部测试名。

这些测试仍属于各自 Event/Receipt/Runner 边界；它们没有共同的 observability schema、
Audit query、Metric reducer、Trace sink 或 Health projection，因此不能作为 OA-00 的统一
runtime 证据。本步只在 GitHub Actions 运行 source guard；本地未执行测试二进制。

## 6. OA-00 退出条件

OA-00 完成只表示：signal matrix、代码 owner、当前 proof ceiling、迁移清单和明确的拒绝
边界已入库；`source-only` 护栏会在后续源文件漂移或缺少这些边界声明时失败。它不表示
Audit/Metric/Trace/Health runtime 已实现，也不改变 `P1-J8-01`、`ER-30` 或任何历史 evidence block
的状态。OA-01 的 domain schema 叠加已单独记录；下一步按 roadmap 进入 OA-02 correlation/trace
references。

## 7. OA-01 叠加说明

OA-01 在 `kiana-domain` 注册 `observability.v1`、`audit-record.v1`、`metric-catalog.v1` 和
`trace-summary.v1`，并提供 `ObservabilityRecord`、`AuditRecord`、`MetricCatalog`、
`MetricPoint`、`TraceSummary` 及封闭状态/来源枚举。每个 contract 使用 `deny_unknown_fields`、
同 major 的 minor compatibility、非零 `source_cursor`、bounded `source_event_ids`/attributes
和 `sha256:` digest 校验；`MetricPoint::validate_with_catalog` 对未注册名称 fail-closed。

这只是 domain/schema source proof。没有新增 EventStore 写者、projector、sink、授权判断或
外部 exporter；schema 存在不代表 audit/metric/trace/health 的 runtime、durable、live 或
physical 证明。OA-00 的旧边界 hash 已在同一迁移序列中更新 `contracts.rs`/`lib.rs` 两项，
新 `observability.rs` 由 OA-01 的专项编译/CI 护栏负责。

## 8. OA-02 叠加说明

OA-02 在 `kiana-domain/src/correlation.rs` 注册 `kiana.correlation-context.v1`，提供严格
解析的 W3C `traceparent`、`TraceId`/`SpanId`、`TraceRef`/`SpanRef`、`SpanLink`、
`AttemptRef`、`CausationRef` 和服务端派生的 `CorrelationContext`。根上下文从已认证的
`RequestContext`、服务端解析的 scope 与 authority/data epoch 构造；外部 traceparent 只
形成 `ForeignParent` link，不能提供 actor、project、session 或权限。run→turn→invocation→
attempt 绑定和 command/attempt、scope、epoch 校验均 fail-closed；本地 child span 使用
parent ref，异步/recovery child 使用新 span + `FollowsFrom` link。

`kiana-ports` 的 `CorrelationContextPort`/`DomainCorrelationContextPort` 仅委托这些纯
domain 不变量，不访问 EventStore、Broker、Provider 或网络。这是 source/静态编译 proof；
没有新增 runtime ingress、span sink、授权路径或 durable/live/physical 证明。OA-03 继续
处理跨 signal 的统一 redaction/classification 和 bounded encoder。

## 9. OA-03 叠加说明

OA-03 在 `kiana-domain/src/redaction.rs` 增加 versioned `RedactionProfile`，按 log/metric/
trace/audit/export signal 声明 `DataClass`、最大字节数和最大嵌套深度，并绑定 canonical
profile digest。`encode_bounded_value`/`encode_bounded_text` 复用既有 `redact_value`/
`redact_text`，随后检查 NUL、深度、UTF-8/JSON 编码、大小和残余 secret marker；profile、
结构或文本任一校验失败都返回稳定错误，不提供原文 fallback。`secret_ref` 等受控引用
可以保留，secret/token/password/api-key/header 等值必须变为 `[REDACTED]`。

本步只交付 domain/profile/encoder source contract 与远端 sentinel/边界夹具，没有改写
EventStore、Receipt、Provider、Broker 或外部 exporter 的既有事实路径。它不证明所有输出
通道已经接线，也不提升 durable/live/physical 等级；OA-05/OA-06 已分别固定 sink 端口/fake
adapter 与 committed fact observer，后续 OA-07+ 负责 runtime producer/projector 接线。

## 10. OA-04 叠加说明

OA-04 在 `kiana-domain/src/audit.rs` 固定 RuntimeEvent taxonomy，覆盖 command、authorization、
approval、capability、credential、recovery、query 和 export 的稳定 action/decision 映射。未知
非 audit 事件保持 opaque 并跳过；所有 `audit.*` 自报事件（包括 approved/completed/exported）
均拒绝，`audit.correction` 只允许后续以追加事实实现，不能在 reducer 中覆写原记录。

`reduce_audit_records` 要求非零起始 EventCursor、非空 source binding、非零 authority/data epoch、
唯一 source event ID，并为每个来源事件产生 checked cursor、source ID、服务端固定 actor、bounded
Audit redaction 后的 action digest、可选 input/reason/correlation/causation refs 和 record digest。
重复逻辑键、矛盾 decision、伪造 actor/record payload、capability gate 冲突、cursor overflow、
无绑定目标或缺 epoch 均 fail-closed；原始 EventLog 事件不被修改。`kiana-core` 仅暴露同一纯
reducer facade，不访问 Broker、Provider、UI 或 exporter。

这是 domain/core source 与静态编译 proof；OA-04 远端 workflow 承担运行时 taxonomy/reducer 夹具，
本地不执行测试且不声称 durable/live/physical。OA-05/OA-06 已建立端口、fake sink 和 committed
observer；projection checkpoint 留给 OA-10/OA-15。

## 11. OA-05 叠加说明

OA-05 在 `kiana-ports` 增加 `ObservabilityPort`、`TraceSink`、`MetricSink`、`AuditQueryPort` 和
`HealthProbePort`，以 `ObservabilitySignalRecord` 封闭 signal union，统一返回 append/flush ack，
并提供 durable/flush/cancellation/capacity capability negotiation。`AuditQueryRequest`/`Page`
固定 source cursor、bounded page/filter、projection version 和空页/不可用错误边界；query 不暴露
raw RuntimeEvent。`HealthSnapshot` 在 domain 注册 `kiana.health-snapshot.v1`，绑定 cursor/source
IDs、status、observed time、bounded capabilities/limitations 与 digest。

`MemoryObservabilitySink` 与 `JsonlObservabilitySink` 是显式 non-durable fake：可记录 log/metric/
trace/audit/health、注入一次性失败、容量拒绝、取消和 flush ack，且仅返回已验证 projection；
不依赖或调用 Broker，不把 exporter/sink ack 变成授权或效果事实。缺 durable/flush/cancel 或容量
能力的 adapter 由 `require_observability_capabilities` fail-closed。OA-05 远端 workflow 承担运行时
fake/query/probe 夹具，本地不执行测试；OA-06 已补 EventLog commit observer，真实 durable sink、
backpressure 和入口接线留给 OA-13/OA-24。

## 12. OA-06 叠加说明

OA-06 在 `kiana-ports` 固定 `CommittedTransition` 与 `EventStoreCommitObserver`：通知携带原始
`TransitionBatch`、同一 `CommandReceipt`、连续 `first_cursor/cursor` 和严格对应的
`source_event_ids`，伪造 receipt、cursor 回退、事件 ID 错配或重复 ID 均 fail-closed。通知
只描述新提交，不授予权限，也不调用 Broker。

`kiana-eventlog::StreamEventStore` 是现有 EventStore 的装饰器；仅当内层返回
`CommitOutcome::Committed` 时按注册顺序调用 observer，`Replayed`、`Conflict`、`Unknown` 和
底层错误都不发布。observer 失败只进入有界 `CommitObserverFailure` 诊断队列，已提交结果不
被改写成假拒绝，也不会因重试重放再次通知。`read_from` 仍是重启后的事实补偿路径，因此
callback 丢失不等于 projection 已同步。legacy `append*` 兼容路径不伪造 transition receipt，
继续由既有 adapter 语义负责。

OA-06 远端 workflow 覆盖 fresh commit、replay、CAS conflict、Unknown、observer failure 和
receipt/cursor 伪造夹具；本地只执行格式、静态源码检查和 test-target 编译，不执行测试二进制，
不宣称 durable/live/physical。

## 13. OA-07 叠加说明

OA-07 在 `kiana-domain` 注册 `kiana.span-lifecycle.v1`，并以 `SpanLifecycleRecord` 固定
Run、Turn、Invocation 三类 span 的稳定 `TraceId`/`SpanId`、实体关联、attempt、生命周期阶段、
`TraceStatus`、来源 cursor/event、受限错误码和低基数属性。记录要求单一 source event、非零
cursor、严格的实体 ID 关系、bounded attributes 与 digest；它是可重算 projection，不是授权
令牌，也不创建或修改 EventLog 事实。

`kiana-core::span_projection::project_span_lifecycle` 按输入事实顺序折叠 `run.authorized` /
`run.prompt` / `run.started`、approval、invocation dispatch/result、compact、cancel 和 terminal
事件。稳定 trace/span ID 从 run/实体键的 digest 派生，重复 event ID 和重复 terminal 幂等；
terminal 冲突 fail-closed，迟到 delta/approval/result、旧 attempt 不覆盖已结束 span，新 attempt
只由更高 attempt 明确重开。没有可验证 invocation/turn 关联的事件保持 opaque，不猜造 span。
`ControlPlane::span_lifecycle`/`span_state` 只读 EventLog 重建入口，不访问 Runner、Broker 或
Exporter，也不会把 span end 当成 run terminal。

OA-07 远端 workflow 覆盖 deterministic replay、Run/Turn/Invocation start/pause/resume/checkpoint/
end、cancel/unknown、duplicate terminal、late delta、terminal conflict 和 stale attempt 夹具；
本地只执行格式、静态源码检查和 test-target 编译，不执行测试二进制，不宣称 durable/live/physical。

## 14. OA-08 叠加说明

OA-08 在 `kiana-domain` 注册 `kiana.model-attempt.v1`，以 `ModelAttemptRecord` 固定 provider、
model、route digest、prompt version hash、streaming、latency、stop reason、usage、retry class 和
低基数 cache usage。记录带稳定 trace/span ID、model call/request/attempt、source cursor/event 和
canonical digest；`status=ok` 只有在 attempted、完整 stop (`end_turn`/`tool_use`)、完整 usage、无
错误且 retry class 为 `never` 时成立。缺字段、未知 stop、超界 usage、截断/超时/连接失败、重试或
未完成 usage 一律保留 `unknown`/`degraded`，不能伪造成功。

`kiana-provider::safe_prepared_metadata` 和 daemon 的 `InstrumentedModelClient` 共同执行
allow-list：只暴露 route identity 的 digest、request/attempt ID、provider/model、prompt/request hash、
budget 和 streaming；compiled wire body、prompt/messages、工具参数、endpoint、鉴权 header、cache key
和 raw provider response 不可进入该摘要。Harness 仍只通过现有 `run.model_turn` 事件落账，
`kiana-core::model_attempt_projection` 从 committed EventLog 重建记录并提供只读
`ControlPlane::model_attempts`/`provider_attempts`，不创建第二模型循环或 sink。

OA-08 远端 workflow 覆盖正常流、secret sentinel、malformed/truncated/timeout/retry、缺 usage、
cache/stop/retry 分类、重复 attempt 和 contract fail-closed 夹具；provider 测试只构造离线
prepared 请求。此次本地仅执行格式、静态源码检查和 test-target 编译，不执行测试二进制；该投影
仍是 source proof，不等价于 Receipt、账单、Metric/Audit 对账、durable/live telemetry 或外部 provider
结果。

## 15. OA-09 叠加说明

OA-09 在 `kiana-domain` 注册 `kiana.capability-attempt.v1`，以
`CapabilityAttemptRecord` 固定一次能力请求的 admission、approval、permit、dispatch、execution、
effect、stop、fencing 和 zero-effect 证据。记录只保留稳定 ID、operation/action digest、低基数状态、
source cursor/event 与有界错误码；原始参数、shell command、路径、header、secret 和 handler 输出不
可表示。`status=ok` 必须同时满足 committed allowed admission、已知成功 effect、无错误，且不能是
zero-effect；`effect=unknown` 或 stop 未确认时始终保持 `fenced=true`。

`kiana-core::capability_attempt_projection` 只消费已提交 request/decision/approval/permit/
dispatch/execution/result/cancel facts，按 `(request_id, attempt)` 稳定折叠，并通过
`ControlPlane::capability_attempts`/`effect_attempts` 提供只读重建入口。ControlPlane 在调用 Broker
handler 前以 execution-permit CAS 追加 `invocation.executing`，因此 execution boundary 未提交时不
调用 handler；拒绝、hook/policy block、过期 approval、lease/authority/TOCTOU mismatch 均保留
`zero_effect` 或 `unknown`，不会被 telemetry、cancel 请求或 UI 结果覆盖。daemon shell/patch handler
仅补充 bounded effect/stop metadata，仍沿原有 Broker 主链执行。

OA-09 远端 workflow 覆盖成功 admission→permit→dispatch→execution→result、policy/hook deny、过期
审批、TOCTOU/lease unknown、cancel 未确认和 secret sentinel；本地仅执行格式、静态源码检查和
test-target 编译，不执行测试二进制。该记录是 EventLog 派生 source proof，不等价于外部效果 receipt、
durable checkpoint、reconcile 完成或 live stop/telemetry 证明。

## 16. OA-10 叠加说明

OA-10 在 `kiana-domain` 注册 `kiana.metric-snapshot.v1`，以 `MetricSnapshot` 绑定状态、源游标、
projector 游标、受限 `MetricPoint` 集合、限制说明和 canonical digest。`MetricCatalog::builtin`
补齐 EventLog append/flush latency、durable/projector cursor、lag/rebuild、Receipt query、Artifact
bytes/read failure 与 Recovery orphan/unknown/last-error 指标；延迟使用 histogram、游标/bytes/布尔
质量信号使用 gauge，计数使用 counter，均不带 run/session/request/path/prompt/secret 高基数标签。

`kiana-core::metrics::project_operational_metrics` 只折叠去重后的 committed `RuntimeEvent`：空源直接
返回 `metrics_source_empty`，显式 source cursor 或 stream version 缺口进入 bounded limitation，
projector cursor 超前拒绝，未确认 dispatch 计为 orphan，`effect_known=false`/`result_unknown` 保留
unknown，artifact 读取错误不被归零；未提供 durable projector checkpoint 时标记
`projector_cursor_inferred`。`ControlPlane::operational_metrics`/`metrics` 只允许 EventStore 全量读取，
不把单个 run stream 冒充系统级统计，也不调用 Broker、Provider、Receipt writer 或 Recovery action。

OA-10 远端 workflow 覆盖空源拒绝、cursor gap、projector lag、orphan/unknown、artifact failure、延迟
样本、完整 committed snapshot、digest/serde 和 secret sentinel；本地只执行格式、静态源码检查和
test-target 编译，不执行测试二进制。该快照仍是 source projection，不等价于 durable projector
checkpoint、runtime gauge、Receipt/Artifact 事实、外部效果确认或 live exporter。

## 17. OA-11 叠加说明

OA-11 扩展 `HealthSnapshot` 为显式 `HealthProbeKind`（startup/readiness/liveness/drain/maintenance）
和有界 `ComponentHealth` 集合。每个组件只携带固定名称、版本、`healthy|degraded|unavailable|unknown`
状态、可选 last-success cursor 与单条 limitation；快照自身继续绑定 source cursor/event、capability
声明和 digest。`readiness` 表示能否接收新的受控命令，不等价于历史 Run 完成、provider 现实可用或
外部效果成功；旧 JSON 缺失新增字段按兼容默认值读取，未知字段仍拒绝。

`kiana-core::health::project_health_snapshot` 先从 committed EventLog 重建 OA-10 metrics，再结合
`EventStoreCapabilities` 聚合 control-plane、eventlog、projector、receipt、artifact、recovery、
provider、broker、telemetry 和 daemon 组件。空源返回 `health_source_empty`；cursor gap、lag、未知/孤儿
效果、artifact/query failure、atomic/durable/cursor 能力不足或未持久化 projector checkpoint 只产生
degraded/unavailable 和 bounded limitation。provider/Broker/telemetry 没有独立 probe 时保持 unknown，
不会因为 HTTP/模型自报或指标 sink ack 变成 ready。`ControlPlane::readiness`/`liveness`/`startup_health`
与 `DaemonHost` bridge 均为只读，不写 EventLog、不调用 Broker/Provider、不改变 admission。

OA-11 远端 workflow 覆盖空源 fail-closed、readiness 对 Unknown/lag/checkpoint/能力不足的拒绝、liveness
只读语义、组件状态/version/last-success/limitation、serde/digest 和 capability boundedness；本地只执行
格式、静态源码检查和 test-target 编译，不执行测试二进制。该快照仍是 source proof，不等价于 durable
heartbeat、跨进程 lease、provider/Broker/exporter live probe 或 ready admission gate。

## 18. OA-12 叠加说明

OA-12 将指标治理分成三个不可互换的边界：`MetricCatalog` 声明 kind/unit/source/稳定性与 allowlist，
`MetricPoint` 绑定 source cursor/event、digest 和 `MetricQuality`（measured/estimated/unknown），
`MetricCardinalityGuard` 再限制 label key/value、series 数和单 label distinct values。注册表不允许
通过未知名称或单位漂移；敏感/high-cardinality label（run/session/request/user/org/project ID、path、
prompt、tool args、command、header、token、secret、cookie、email、raw response）及 Bearer/secret/
password 等值 fail-closed；达到上限返回显式 overflow，不丢弃为“零”。

`MetricReducer` 的 `apply_point/apply_points/apply_snapshot` 采用事务式候选状态，重复同一 digest 幂等，
旧 cursor 和 counter 回退拒绝，catalog digest 不匹配拒绝；`replay` 先运行 OA-10 committed-event
projection，再走同一 reducer/guard，保证 replay 与增量 live 的 point 集合和类型校验一致。估算 quality
不能进入 measured 指标，counter 只能单调增长；histogram/gauge/counter 的 kind/unit 由目录固定，
不把 MetricPoint、UI cursor、Receipt 或模型自报当事实源。

OA-12 远端 workflow 覆盖目录 digest/kind/unit、未知 metric、敏感 label/value、series/value cardinality
overflow、counter reset/cursor regression、estimated/measured quality、replay/live 一致与 serde 夹具；
本地只执行格式、静态源码检查和 test-target 编译，不执行测试二进制。该治理仍是 source proof，不等价
于 durable metric sink、队列背压、跨进程 checkpoint、runtime gauge 或 live exporter。

## 19. OA-13 叠加说明

OA-13 在 `kiana-ports::ObservabilityQueue` 固定 bounded、non-blocking admission：
`Event|Audit|Approval|Recovery|Terminal` 是 critical，`Log|Trace|Metric` 是 best-effort。队列满载时
critical 只可淘汰一个 best-effort 项；若全是 critical，立即返回 `critical_queue_full` 并记录 reject
计数/原因，不等待慢消费者、不丢事实。best-effort 满载返回 `best_effort_queue_full`，记录 drop 计数和
有界 `last_drop_reason`；这些结果是诊断，不能改写已提交 EventLog/Receipt/Approval/Recovery 状态。

队列提供 `try_enqueue`、`try_dequeue`/`dequeue`、`flush`/`flush_cancellable`、`shutdown` 与 `reopen`，
并返回 capacity/depth/enqueued/dequeued/drop/reject/flush/reopen/closed 统计。flush 只证明项目被消费，
不伪称 exporter/durable sink ack；shutdown 保留尚未消费的 critical 项，reopen 不声称跨进程 spool 恢复。
`DaemonHost` 仅持有并暴露该队列，队列 drop/reject 会让 telemetry component/HealthSnapshot 降级，不创建
第二模型循环、Broker 路径或 EventLog 写路径。

OA-13 远端 workflow 覆盖 critical 保留/驱逐 best-effort、critical 满载立即拒绝、drop/reject reason/counter、
flush/shutdown/reopen/cancellable ack 和 bounded stats；本地只执行格式、静态源码检查和 test-target 编译，
不执行测试二进制。该队列仍是 source-level delivery primitive，不等价于 durable spool、异步 exporter、
跨进程 shutdown/reopen、backpressure persistence 或 live telemetry。

## 20. OA-14 叠加说明

OA-14 在 `kiana-domain` 注册 `kiana.trace-export-span.v1`，以 `TraceExportSpan` 固定 W3C-compatible
32-hex trace ID、16-hex span ID、可选 parent、`kiana.*` span name、TraceStatus、sampled、source
cursor/event、duration、低基数属性和 digest。属性只允许 provider/model/capability/operation/
sandbox/component/outcome/reason/retry/approval/effect/stop 等白名单；原始 parent baggage、prompt、
response、tool args、path、header、token 和 actor/scope 不可表示。

`TraceParent::parse`/`foreign_parent_link` 只验证并建立 `ForeignParent` link，永远不覆盖服务端 trace ID、
span ID、actor、project/session、authority 或 approval。`LocalTraceExporter` 在 `enabled && sampled` 且
摘要/parent/source 合法时写入 bounded validated records；关闭或 sampled=false 返回 `SampledOut`，
无效 parent/summary、capacity、closed 返回结构化错误，不触碰 EventLog/Receipt/ControlPlane。`jsonl()`
只序列化已验证 projection；flush/shutdown/reopen 是本地 exporter 生命周期 ack，不等价于 OTLP/durable
delivery。

OA-14 远端 workflow 覆盖 invalid parent/trace ID、foreign link、sampled=false/no-op、低基数属性与
secret sentinel、capacity/closed、JSONL、flush/shutdown/reopen 夹具；本地只执行格式、静态源码检查和
test-target 编译，不执行测试二进制。该 exporter 仍是 source/local projection，不等价于外部 backend、
live sampling、跨进程 exporter recovery 或 trace 完整性证明。

## 21. OA-15 叠加说明

OA-15 在 `kiana-domain` 注册 `kiana.audit-projection.v1` 与
`kiana.audit-projection-checkpoint.v1`。`AuditProjectionCheckpoint` 绑定 projection version、连续
source cursor/event IDs、按源顺序的 AuditRecord IDs、records digest 和 checkpoint digest；
`AuditProjectionSnapshot` 再绑定完整记录集、限制说明和 projection digest。所有字段均 bounded/closed，
未知 major/字段、重复 ID、cursor overflow、checksum mismatch 直接拒绝。

`kiana-core::rebuild_audit_projection` 先校验 EventLog 页的显式 source cursor（若存在）与连续顺序，再
调用 OA-04 server-derived audit reducer；自报 `audit.*`、decision conflict、缺 actor/epoch/target、
redaction/record 错误不生成行。`AuditProjection::apply_page` 只接受紧邻的下一个 cursor，事务式构造新
snapshot；`from_snapshot`/`restore` 先验证 checkpoint 与 records 完整绑定，不修改或删除任何原始事实。
`ControlPlane::audit_projection` 只读 EventStore 全量事实，不把 run stream 当全局审计，也不调用 Broker、
Provider、UI 或 exporter。

OA-15 远端 workflow 覆盖 deterministic rebuild、新进程等价 restore、增量连续页、cursor gap/regression、
unknown audit schema、decision conflict、duplicate source、坏 checkpoint/serde 和原事实保留夹具；本地
只执行格式、静态源码检查和 test-target 编译，不执行测试二进制。该投影仍是 source-level checkpoint
contract，不等价于 durable checkpoint store、Artifact refs、query/export、correction 或 incident workflow。

## 22. OA-16 叠加说明

OA-16 在 `kiana-protocol` 增加封闭的 `AuditQueryRequest`/`AuditQueryResponse` 与
`RequestBody::AuditQuery`，请求只接受 bounded `limit`、可选当前 source cursor、after cursor、action/
decision/target-kind filter；没有 owner、actor、scope、raw event 或 arbitrary EventLog endpoint 字段。
`KianaClient::audit_query` 只封装 envelope，不在客户端授权或解析事件。

`DaemonHost` 在覆盖 caller actor 前要求 query 请求带 server principal 的 actor，验证 limit/cursor 后把
请求交给 `ControlPlane::query_audit`；该方法只读全量 EventLog，先 rebuild OA-15 projection，再以
`run.authorized` 的 actor/session/canonical project 事实派生 owned run/request/approval lineage，过滤
AuditRecord source event，最后应用 bounded page/filter。客户端提供 owner/scope 覆盖、raw events、伪造 actor、
unlimited limit 或 stale cursor 均拒绝/返回结构化错误，不产生 capability、approval、EventLog 或 exporter 副作用。
无匹配记录与 EventLog/query projection 不可用保持不同错误/空页语义；返回页携带 schema、source cursor、
projection version 和 limitations。

OA-16 远端 workflow 覆盖 wire round-trip、unknown owner/raw event 字段、missing/forged actor、limit/cursor
边界、server-scoped query/filter/page DTO；本地只执行格式、静态源码检查和 test-target 编译，不执行测试
二进制。该切片仍是 source-level query route，不等价于 durable audit index、filter-bound cursor snapshot、
跨入口 parity、外部认证服务、导出或 delivery receipt。

## 23. OA-17 叠加说明

OA-17 注册 `kiana.audit-query-cursor.v1`，`AuditQueryCursor` 固定 epoch、projection version、当前
source cursor、after cursor、filter digest 和 cursor digest；cursor 不携带 owner、scope、raw event 或
权限字段。`AuditQueryRequest` 保留兼容的 source/after 字段但若携带 cursor 必须逐字段一致；未知字段、
空/越界 cursor、坏 digest 和 filter mismatch fail-closed。

`ControlPlane::query_audit` 在重建 OA-15 projection 后以 checkpoint digest 作为当前 epoch，以 canonical
action/decision/target filter digest 绑定查询；cursor 的 source/projection/epoch/filter 任一变化返回
`audit_query_cursor_stale`，after 超过当前 source 返回 `audit_query_cursor_invalid`。分页只在有下一页时
生成新的 `AuditQueryCursor`，返回页携带 source cursor、projection version、epoch、filter digest 和
limitations；空页仍表示匹配为空，EventLog read-all/投影不可用仍返回 unavailable，不混为零工作。

OA-17 远端 workflow 覆盖 cursor serde/digest、字段绑定、stale/ahead/filter mismatch、bounded page 与
next cursor；本地只执行格式、静态源码检查和 test-target 编译，不执行测试二进制。该 cursor 仍是 source
projection token，不等价于 durable query index、retention/epoch store、慢查询 telemetry 或跨入口 reconnect
一致性。

## 24. OA-18 叠加说明

OA-18 注册 `kiana.audit-export.v1` 与 `kiana.audit-delivery-receipt.v1`，由
`AuditExportManifest`/`AuditDeliveryReceipt` 绑定 export/query/source/projection/artifact digest、格式、
record count、purpose、recipient、retention 和 source events。`Delivered` 必须有 server-owned
confirmation digest；`Unknown`/`Failed` 不能带 confirmation，也不能被 content/HTTP ack 猜成 delivered。

`AuditExportRequest` 嵌套 OA-16/OA-17 query，额外要求非空 bounded purpose/recipient/retention；DaemonHost
要求 authenticated local principal，ControlPlane 要求显式非 Safe permission，复用 server-scoped query 后
仅序列化已验证 AuditRecord 的 JSONL/JSON/CSV 安全字段。content 有硬字节上限并再次扫描 Bearer/secret/
password/authorization sentinel；manifest artifact hash、query digest、source cursor、projection version
和 redacted source IDs 必须验证。`deliver=true` 在没有独立 delivery adapter/confirmation 时返回
`AuditDeliveryReceipt{state=unknown}` 与 limitation，不写成功事实；任何参数不生成 raw EventLog endpoint、
capability/Broker/Provider 副作用。

OA-18 远端 workflow 覆盖 manifest/delivery round-trip、missing purpose/recipient/retention、Safe/未认证
deny、scope/query reuse、JSONL/JSON/CSV bounded redaction、hash mismatch、oversize/secret sentinel 和
unknown-vs-delivered receipt；本地只执行格式、静态源码检查和 test-target 编译，不执行测试二进制。该
export 仍是 source-local materialization，不等价于 durable ArtifactStore、外部 delivery、operator approval、
export audit event 或 live delivery proof。

## 25. OA-19 叠加说明

OA-19 注册 `kiana.observability-alert.v1`、`kiana.observability-incident.v1` 与
`kiana.observability-incident-snapshot.v1`。`ObservabilityAlert`/`ObservabilityIncident` 只保留 rule/category
fingerprint、severity/state、source cursor/event IDs、bounded message、incident/alert refs 与
reconciliation-safe recovery plan；没有 approve/retry/close action 字段，未知/需对账的事故在
`Verified|Closed` 状态校验时 fail-closed。

`kiana-core::project_observability_incidents` 仅消费 OA-10 `MetricSnapshot` 和 committed failure facts：
projector lag/cursor gap、audit/artifact loss、redaction failure、queue overflow、journal corruption、
effect unknown、orphan dispatch 触发稳定 rule fingerprint；重复事件聚合为一个 incident，source refs 保留
有界集合。模型/UI 自报或普通文本不会触发规则。每条事故关联固定“检查事实→保持 fencing→显式 reconcile”
计划，`requires_reconciliation` 事故保持 Open，不能自动 approve/retry/resolve/close；ControlPlane 只读
`observability_incidents` bridge 不写 EventLog、不调用 Broker/Provider、不覆盖 Company Incident/FailureIncident。

OA-19 远端 workflow 覆盖 lag/unknown/orphan/artifact/redaction/queue/journal triggers、fingerprint dedupe、
source refs、recovery association、unknown cannot close、model/UI self-report rejection 和 snapshot serde；
本地只执行格式、静态源码检查和 test-target 编译，不执行测试二进制。该投影仍是 source diagnostic，不等价于
durable incident checkpoint/event、operator triage workflow、自动 reconcile、queue/exporter live state 或业务 Incident。

## 26. OA-20 叠加说明

OA-20 将治理策略升级为 `DataPolicy` schema/version/revision/data_epoch/digest，`Purpose`、`Retention` 和
`ProcessingGrant` 均有 bounded path/hash/parent/creator/expiry 校验；撤销会级联父子 grant、增加 data_epoch、
写入新 policy digest，篡改或 schema/retention 不完整的 policy 读取 fail-closed。`DataGovernanceSnapshot`/
`DataRetentionObservation` 将 payload 状态（Available/Expired/Revoked/Unknown）与
`audit_metadata_retained` 分离，source cursor/event 和 digest 绑定，绝不把 payload 删除当成 audit metadata
被删除或把 audit metadata 当作 payload。

`kiana-core::project_data_governance_snapshot` 从 server-owned policy 与 committed
`data.revocation_requested`/`workspace.restore_requested`/governance-result facts 重建状态：pending invalidation
将 receipt/audit/artifact/memory/index/cache/export derived stores 全部置 Unknown/fenced；已提交 revoked/expired
状态传播为 Revoked/Expired，原 EventLog source IDs 保留。`kiana-daemon::data_governance` 在策略读取时验证
integrity，并返回 data_epoch、affected grants、erased sources、audit metadata retained 和每个 derived store
的 propagation 状态；现有内存/索引/cache purge 只能是派生清理，不能删除 EventLog/audit facts。

OA-20 远端 workflow 覆盖 policy/grant/purpose/retention schema/hash/epoch、parent revoke cascade、expired vs
retained audit metadata、pending/committed revocation propagation、cursor gap/duplicate、tampered policy/snapshot
和 raw payload absence；本地只执行格式、静态源码检查和 test-target 编译，不执行测试二进制。该切片仍是
source governance projection，不等价于 durable policy/snapshot store、跨进程 deletion scheduler、legal hold、
Artifact/Memory/Index/Telemetry 实际 purge、external retention 或 live deletion proof。

## 27. OA-21 叠加说明

OA-21 注册 `kiana.replay-diagnostic.v1` 与 `kiana.replay-diagnostic-snapshot.v1`。每条
`ReplayDiagnostic` 绑定可选 invocation/attempt、input digest、expected/observed TraceStatus、受限
divergence kind/error code、source cursor/event IDs 和 digest；snapshot 绑定 source、每个 deterministic
projection digest、diagnostic 列表、status 和 limitations。原始 prompt/tool output/stack/secret 不进入
diagnostic。

`kiana-core::diagnose_replay` 仅重放已有 Invocation、Run、Metric、Audit、Health、Span projection；duplicate
source、cursor gap、unknown audit/schema、terminal/projection error、effect unknown 或 expectation status/input
digest mismatch 产生首个可定位 divergence 并令 snapshot `unknown`，不会调用 Model/Provider/Broker、执行
Recovery action、重试或回写 metric/trace/receipt facts。`ControlPlane::replay_diagnostics` 只读 EventLog 全量
事实；projection digest 只用于一致性诊断，不替代 EventLog/Receipt/Audit authority。

OA-21 远端 workflow 覆盖 deterministic replay、source duplicate/gap、unknown schema/terminal/projection
errors、unknown effect locator、expectation status/input mismatch、safe error code/secret absence、snapshot
serde 和 bounded expectations；本地只执行格式、静态源码检查和 test-target 编译，不执行测试二进制。该
诊断仍是 source projection，不等价于 provider-side reconciliation/receipt、automatic compensate/retry、
durable diagnostic checkpoint、fault injection 或 live health/incident consistency。

## 28. OA-22 叠加说明

OA-22 注册 `kiana.fault-case.v1` 与 `kiana.fault-matrix.v1`。`FaultCase` 固定 injection point
（prepare/commit/dispatch/result/flush/projector/export/shutdown）、seed、`rejected|unknown|observed`、
effect started/known、resource fenced、source cursor/event、safe error code 和 duplicate/false-success flags；
`FaultMatrix` 绑定同一 seed/source/case set 和 matrix digest，拒绝重复点、混合 seed/source、未知 schema 或
不安全状态。Rejected 必须 zero-effect/known，Unknown 必须 fenced，任何 duplicate effect/false success 直接
fail-closed。

`kiana-core::fault_matrix`/`fault_matrix_from_events` 是 replay-only deterministic simulator，固定八个故障
边界的保守分类：prepare reject，commit/dispatch/result/flush/projector/export/shutdown unknown/fenced；seed
只用于重放定位，不调度真实 crash、kill process、EventStore、Broker、Provider、projector、exporter 或 shutdown
副作用。`ControlPlane::fault_matrix` 只接收调用方已提供的 source event IDs，输出测试/诊断 artifact，不写事实、
不释放资源、不自动 retry/compensate/reconcile。

OA-22 远端 workflow 覆盖八点矩阵、seed 重放、source duplicate/limit、unknown/rejected/observed invariants、
false-success/duplicate-effect/unfenced-unknown tamper 和 closed serde；本地只执行格式、静态源码检查和
test-target 编译，不执行测试二进制。该 matrix 仍是 source safety model，不等价于真实 fault hooks、crash
recovery、durable receipt/incident、process/resource fencing 或 live provider/export behavior。

## 29. OA-23 叠加说明

OA-23 注册 `kiana.eval-case.v1`、`kiana.eval-result.v1` 与 `kiana.eval-suite.v1`。`EvalCaseSpec` 固定
case/input digest、expected normalized event kinds/status、是否要求 Audit/Metric/Span/Receipt、是否禁止
effect/secret、cost quality 和 latency bucket；`EvalCaseResult` 只保存 projection digests、effect/secret/
replay flags、source cursor/event、bounded failures、cost kind/latency bucket；`EvalSuiteReport.promote` 必须
由所有 case 的 Pass 重新计算，未知或失败绝不会被“文本解释”抵消。

`kiana-core::evaluate_provider_independent` 只折叠 committed facts：OA-15 Audit、OA-10 Metric、OA-07 Span、
Run/Receipt 和 OA-21 replay diagnostics。缺任何声明的证据、normalized event、measured usage、expected
status/input、latency bucket 或发现 secret/forbidden effect/replay divergence 都生成 Fail/Blocked；禁止 effect
的 case 只允许 zero-effect，measured cost 需要完整 usage，estimated 与 measured quality 不混用。函数和
`ControlPlane` bridge 不调用 Model/Provider/Broker，不写 EventLog、不自动 Promote/Retry/Rollback。

OA-23 远端 workflow 覆盖 provider-independent success、secret/forbidden effect blocking、replay divergence、
missing evidence、normalized event/audit/metric/span/receipt digests、cost/latency bucket、suite promote 和
closed serde；本地只执行格式、静态源码检查和 test-target 编译，不执行测试二进制。该 eval 仍是 source
quality gate，不等价于真实 provider/backend、Promptfoo 外部 runner、durable eval artifact、cost receipt 或
自动发布/回滚。

## 30. OA-24 叠加说明

OA-24 注册 `kiana.entrypoint-parity.v1`，把 CLI、Web、Workbench 和 Desktop 的审计/健康/Receipt
展示收敛为同一个 ControlPlane 只读投影。`EntryPointParitySnapshot` 绑定入口标签、source cursor、完整且
有界的 source event IDs、projection version、运行状态、Receipt/Audit/Health digest 以及 bounded limitations；
snapshot digest 会覆盖所有字段，Completed 必须有 Receipt digest，重复 source、cursor gap、未知字段和坏
digest 均 fail-closed。入口标签只用于显示与对账，不能改变状态或权限。

`ControlPlane::entrypoint_parity` 从 authenticated `RequestContext` 解析 session/run，按
`run.authorized` 的 actor/session/project/role/department 绑定筛选 committed EventLog；缺 owner 证据、run
不存在、请求 run 与 session 不一致或 read-all 不可用时返回 blocked/unavailable，不读取 RunStream，也不接受
客户端 owner/scope。Audit 与 Health digest、Run projection status 和 retention revoke limitation 都由同一
事件切片派生；数据撤销把 parity status 降为 `result_unknown`，保留 `data_revoked` limitation，绝不伪造健康
或成功。协议新增只读 `RequestBody::Parity`/`KianaClient::parity`；DaemonHost 将其标为 Safe/no-execute。

CLI `kiana parity --session-id <id> [--run <run_id>]`、Workbench `/parity`、Web `/api/parity`（允许受控的
`entrypoint=desktop` 供 Electron 壳复用）都经过 `harness_run` 和同一 DaemonHost。Web `/api/health` 保留
旧的兼容字段，但 `ok/status` 由真实 liveness projection 或 bounded unavailable limitation 派生；不再硬编码
成功。四入口没有本地 EventLog 解析、恢复动作或自拼 Receipt/Health 判断。

OA-24 远端 workflow 覆盖四入口 digest/status/limitation/source ref 一致、terminal conflict/result_unknown、
source gap/duplicate/limit、Completed-without-receipt、unknown-field wire rejection 和 parity request round-trip；
本地只执行格式、静态源码检查和 test-target 编译，不执行测试二进制。该切片仍是 source-level、进程内只读
projection，不等价于 durable query index、跨进程 cursor/retention store、外部 authenticated principal、真实
provider/Broker/telemetry liveness、业务 Outcome 或自动 reconcile/恢复证明。
