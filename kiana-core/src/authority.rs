//! ControlPlane 项目授权版本、授权账本和受保护事件提交。
//!
//! 本模块实现了授权系统的四个核心操作：
//! 1. 查询当前授权修订号（revision）
//! 2. 读取授权 epoch（用于 fencing 和撤销关联）
//! 3. 重建完整的授权账本（从事件流重放）
//! 4. 提交受保护的事件（多聚合 CAS）和配置同步
//!
//! # 在系统中的位置和数据流
//!
//! daemon 在处理可能执行能力的请求前，根据项目身份、角色目录、策略标识、模型配置和工具目录
//! 计算配置指纹，并调用 [`ControlPlane::synchronize_authority`]。该方法把项目授权状态的变化
//! 记入 EventLog 的 `authority` stream；后续 Core 的安全上下文、执行 permit、资源 lease、Run
//! 快照和恢复流程会读取 revision/epoch，用于发现旧权限快照是否已过期。
//!
//! ```text
//! DaemonHost 建立 RequestContext 和配置指纹
//!     ↓ synchronize_authority
//! EventLog: authority stream（按规范化项目根目录计算 aggregate id）
//!     ↓ authority_revision / authority_epoch / authority_ledger
//! SecurityContext、permit/fence、Run checkpoint、审批恢复等 Core 组件
//! ```
//!
//! `commit_protected_event` 是 Company、Platform、Automation、Swarm 等 Core 功能共用的受保护
//! 写入 helper：它把目标聚合（aggregate）版本和当前 authority 版本放进同一个
//! `TransitionBatch`（状态转换批次），由 EventStore 一起比较并提交，防止写入期间授权快照变化。
//! 这里的“批次”是 EventStore 的状态转换接口，不表示本函数直接开启数据库事务。它只负责日志
//! 提交的一致性，不代替调用方做策略或权限判定，也不执行工具。
//!
//! # 授权的两个关键概念
//!
//! **修订摘要（Revision Digest）**：由授权配置内容计算出的指纹。配置内容变化时摘要变化；它用于
//! 比较配置快照，不单独充当授权凭证。
//!
//! **授权代次（Authority Epoch）**：账本中用于判定授权材料是否仍新鲜的代次；安全快照、
//! Grant/审批和资源许可会携带它，授权变化后旧代次会被 fence。它与
//! **事件流版本（Stream Version）**不同：后者是 EventLog 对 `authority` aggregate 的
//! 比较并交换（Compare-and-Swap，CAS）/排序版本，每追加一条流事件就递增；
//! `authority_revision` 返回事件流版本 + 摘要，
//! 而 `authority_epoch` 返回从 AuthorityLedger 重建的语义授权代次，两者不能互换。
//!
//! # 为什么要分开内容摘要、授权代次与 stream version
//!
//! 如果只比较 digest，一个常见的回滚情形是：
//! 1. Alice 获得 grant：digest_A
//! 2. 管理员撤销 Alice 的权限，之后配置又回到同样内容 digest_A；
//! 3. 仅比较 digest 的旧快照看起来会与现在相同。
//!
//! 代次用于发现这类权限历史已经变化；事件流版本则负责 CAS，避免并发追加基于过期的
//! 日志状态。三者描述不同事实，不能仅凭其中一个推断另外两个。

use super::*;
use kiana_domain::{json_digest, AggregateVersion, AuthorityLedger, TransitionBatch};

impl ControlPlane {
    /// 读取项目 `authority` stream 最新事件的 stream version 与配置摘要。
    ///
    /// 返回格式是 `"{stream_version}:{revision_digest}"`，例如 `"5:sha256:<hex>"`；这里的
    /// stream version 是 EventLog aggregate 版本，不等于 [`Self::authority_epoch`]。
    ///
    /// `root` 是项目根目录；先规范化后计算 stream aggregate id，使同一项目的记录落到同一条流。
    ///
    /// 没有授权事件时返回 `None`；否则返回最新事件的 stream version 和 `revision_digest`。
    /// EventStore 读取失败传播为 `CoreError`。此方法只读，供 SecurityContext、Run 快照和恢复
    /// 逻辑比较已观察到的授权配置版本，不会刷新配置或写新事件。
    pub(crate) async fn authority_revision(&self, root: &str) -> Result<Option<String>, CoreError> {
        // 规范化项目根路径后计算确定性 aggregate id；项目路径相同就会读取同一条 authority 流。
        let key = json_digest(&json!({"project_root":Self::canonical_project_root(root)}));

        // read_stream 按 aggregate 身份返回已提交的项目授权事件，供下面读取最新状态。
        Ok(self
            .events
            .read_stream("authority", &key)
            .await?
            // EventStore 按 stream 顺序返回；最后一条是本次查询观察到的最新授权事实。
            .last()
            // 把 stream version 与配置 digest 组合为内部快照标识。
            .map(|event| {
                format!(
                    "{}:{}",
                    // 旧/不完整事件没有版本时以 0 表示；有效 authority 流会由账本重建校验连续性。
                    event.stream_version.unwrap_or(0),
                    // 载荷没有 digest 时保留空串。本函数只拼接快照标识，不在这里验证账本或授权有效性。
                    event.data["revision_digest"].as_str().unwrap_or_default()
                )
            }))
    }

    /// 返回从项目 AuthorityLedger 重建出的授权代次，用于 fencing 和撤销关联。
    ///
    /// 该值由 authority 事件载荷中的授权代次重放得到，不应直接替换成 EventStore `stream_version`。
    /// 账本 epoch 为 0 表示尚未初始化，返回 `None`；大于 0 时返回 `Some(epoch)`。读取/重建
    /// 失败返回 `CoreError`。调用者用它检查旧授权快照、permit 或 lease 是否仍可用。
    pub(crate) async fn authority_epoch(&self, root: &str) -> Result<Option<u64>, CoreError> {
        let ledger = self.authority_ledger(root).await?;
        // 0 是空账本的初始值，不是可供授权材料绑定的有效代次。
        Ok((ledger.authority_epoch > 0).then_some(ledger.authority_epoch))
    }

    /// 从 EventLog 按序重放项目的 authority stream，重建当前授权账本。
    ///
    /// `root` 先被规范化并转换成 authority aggregate id；读取结果交给 domain 的
    /// `AuthorityLedger::rebuild` 按 stream version 验证顺序并折叠。领域 reducer 支持处理
    /// membership、role/project assignment、policy profile、data boundary 和 sharing grant 等事件，
    /// 但当前生产路径中确认的 writer 只写入 `authority.revised`；不能从 reducer 支持某事件就推断
    /// 生产账本已经包含该类事实或这些事实已参与授权强制。Cell、Budget、Lease 运行状态也不属于此账本。
    /// 相同有序事件输入会得到相同账本；事件缺失、版本跳跃或载荷不合法会映射为
    /// `authority_ledger_invalid`，底层读取错误传播为 `CoreError`。
    pub(crate) async fn authority_ledger(&self, root: &str) -> Result<AuthorityLedger, CoreError> {
        let key = json_digest(&json!({"project_root":Self::canonical_project_root(root)}));
        let records = self.events.read_stream("authority", &key).await?;
        AuthorityLedger::rebuild(&records)
            .map_err(|error| PortError::Failed(format!("authority_ledger_invalid:{error}")))
            .map_err(Into::into)
    }

    /// 在目标 aggregate 和项目 authority stream 版本都匹配时提交受保护事件。
    ///
    /// 【作用与调用者】
    /// Platform、Company、Automation、Swarm 等 Core 领域方法在完成自己的输入/策略检查后调用。
    /// 本 helper 把业务事件和当前 authority 版本放进同一个 TransitionBatch，避免授权变更与业务
    /// 事件写入并发交错；它自身**不检查调用者是否有业务权限**，也不执行能力 handler。
    ///
    /// 【输入】
    /// - `context`：已由上层建立的请求上下文；其中 project_root 用来定位 authority stream；
    /// - `event`：带有目标 `aggregate_type` 和 `aggregate_id` 的业务事件；
    /// - `expected`：调用方读取目标 aggregate 时观察到的 stream version。
    ///
    /// 【返回、副作用与失败】
    /// 首次提交成功时返回原事件对象；幂等重放时从目标 stream 读回与幂等键/种类/载荷相符的
    /// 已存事件并返回。缺少 aggregate 身份、authority stream 不存在、版本冲突或 EventStore
    /// 无法确认提交时返回 `CoreError`；冲突不会在本方法内部重试。
    ///
    /// 【关键流程】
    /// 从 event 取得目标聚合身份；使用 event 自带 idempotency key，或以 aggregate 类型/ID 和
    /// request ID 生成稳定命令 key；再读取当前 authority stream version。最后调用
    /// `dispatch::commit_confirmed` 原子比较目标 `expected` 版本与刚读取的 authority 版本。
    /// `commit_confirmed` 返回 `false` 表示首次提交，返回 `true` 表示命令已提交过并被重放。
    ///
    /// ⚠ 多聚合 CAS 只保护 EventLog 中这次状态转换，不覆盖随后发生的外部副作用；调用者必须
    /// 继续遵守各自的 execution permit、handler 和结果回执边界。
    pub(crate) async fn commit_protected_event(
        &self,
        context: &RequestContext,
        event: RuntimeEvent,
        expected: u64,
    ) -> Result<RuntimeEvent, CoreError> {
        // 1. 提取事件的主键（aggregate_type 和 aggregate_id）
        let aggregate_type = event
            .aggregate_type
            .clone()
            .ok_or_else(|| PortError::Failed("protected_event_scope_required".to_owned()))?;
        let aggregate_id = event
            .aggregate_id
            .clone()
            .ok_or_else(|| PortError::Failed("protected_event_scope_required".to_owned()))?;

        // 2. 构造幂等 key
        // 如果事件本身有 idempotency_key，用它；否则从 aggregate 和请求 ID 组合生成
        let key = event
            .idempotency_key
            .as_deref()
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!("{}:{}:{}", aggregate_type, aggregate_id, context.request_id)
            });

        // ⚠ 缺少 event.idempotency_key 时，fallback key 只用于生成 TransitionBatch 的 command ID，
        // 不会写回 event 本身。下面的重放读回仍按原 event key、kind、data 比较；若同一聚合已有
        // 多条 key 为空且 kind/data 完全相同的事件，可能无法区分这次应读回哪一条，后续改匹配条件前
        // 需先确认调用方是否总会提供事件级幂等键。

        // 命令摘要用于识别同一个确定性 command 的重试，不是密码学签名。
        let command_id = kiana_domain::derived_request_id("protected.command", &key);
        let digest = json_digest(&json!({"kind":event.kind,"data":event.data}));

        // 4. 读取项目的授权流
        let authority_key = json_digest(
            &json!({"project_root":Self::canonical_project_root(&context.project_root)}),
        );
        let authority = self.events.read_stream("authority", &authority_key).await?;
        if authority.is_empty() {
            return Err(PortError::Failed("authority_snapshot_missing".to_owned()).into());
        }

        // 5. 构造 TransitionBatch：多聚合 CAS
        // 这个批次同时约束两个东西：主事件的版本 + 授权流的版本
        let batch = TransitionBatch {
            command_id,
            command_digest: digest,
            expected_versions: vec![
                // 主聚合的版本约束：期望是 expected 版本
                AggregateVersion {
                    aggregate_type: aggregate_type.clone(),
                    aggregate_id: aggregate_id.clone(),
                    version: expected,
                },
                // 授权聚合的版本约束：期望是当前最新版本
                // 这保证了即使在并发写入的情况下，授权状态一致
                AggregateVersion {
                    aggregate_type: "authority".to_owned(),
                    aggregate_id: authority_key,
                    version: authority
                        .iter()
                        .filter_map(|e| e.stream_version)
                        .max()
                        .unwrap_or(0),
                },
            ],
            events: vec![event.clone()],
        };

        // `commit_confirmed` 的 true/false 表示“是否重放”，不是提交成功/失败：版本冲突会 Err。
        if super::dispatch::commit_confirmed(self.events.as_ref(), batch).await? {
            // 只有幂等重放分支需要按幂等键、种类和载荷读回原记录；首次提交分支在下方返回输入事件。
            return self
                .events
                .read_stream(&aggregate_type, &aggregate_id)
                .await?
                .into_iter()
                // 寻找与我们提交的事件完全匹配的记录
                // （可能有并发写入，我们需要找到自己的）
                .find(|old| {
                    old.idempotency_key == event.idempotency_key
                        && old.kind == event.kind
                        && old.data == event.data
                })
                .ok_or_else(|| {
                    PortError::Failed("committed_command_result_missing".to_owned()).into()
                });
        }

        // commit_confirmed 返回 false 表示本次是首次成功提交；它已写入 EventStore，
        // 这里返回带调用方事件内容的对象。真正的版本冲突已通过上面的 `?` 作为错误返回。
        Ok(event)
    }

    /// 将 daemon 计算出的授权配置指纹同步到项目 authority stream。
    ///
    /// 【调用者与输入】
    /// 当前 daemon 在通过项目身份/信任解析后，对可能执行能力的请求调用本方法。它传入服务端
    /// `RequestContext` 和配置指纹；该指纹由 daemon 根据 project identity、内建角色/部门目录、
    /// 本地允许角色、模型配置、默认策略标识及工具/动作目录计算，不是由本方法访问 Git 生成。
    ///
    /// 【核心流程与数据流】
    /// 把规范化项目根目录、`project_trusted` 和配置指纹组成 authority revision 数据并计算摘要；
    /// 若最新事件已有相同摘要则直接返回。否则读取当前 stream version，以该版本为 CAS 前置条件
    /// 追加 `authority.revised`。已有授权事件时，追加成功后再要求 ApprovalStore 失效旧审批，并
    /// 调用 `stop_project_runs` 停止该项目的运行；首次初始化没有旧状态需要清理。
    ///
    /// 【副作用、输出与失败】
    /// 配置相同时返回 `Ok(())` 且无写入。配置变化会先写 EventLog，然后可能修改审批存储并向
    /// Runner 发送停止请求。读取/提交/失效/停止任一步失败都返回 `CoreError`；注意后两项发生在
    /// `authority.revised` 已提交之后，因此返回错误不表示授权事件没有写入。
    ///
    /// ⚠ 当前相同摘要的早退发生在失效审批和停止 Run 之前。如果某次调用已提交新摘要、随后
    /// `invalidate_project` 或 `stop_project_runs` 失败，下一次相同配置的调用会直接早退，不会
    /// 重试失败的清理步骤；此函数本身不提供清理续做机制。
    pub async fn synchronize_authority(
        &self,
        context: &RequestContext,
        configuration_revision: &str,
    ) -> Result<(), CoreError> {
        // 配置指纹按规范化项目根目录隔离；不同项目即使配置内容相同，也各有自己的 authority stream。
        let project_root = Self::canonical_project_root(&context.project_root);
        let key = json_digest(&json!({"project_root":project_root}));

        // 摘要覆盖 schema、项目身份范围、信任标志和 daemon 提供的当前配置指纹。
        let value = json!({"schema":"kiana.authority-revision.v1","project_root":project_root,
            "project_trusted":context.project_trusted,"configuration_revision":configuration_revision});
        let revision_digest = json_digest(&value);

        // 之后的幂等比较和 CAS 都基于同一条项目授权流。
        let records = self.events.read_stream("authority", &key).await?;

        // 同一内容已成为最新事实时不再追加重复 authority.revised。
        // 注意：该早退不会检查或续做上次可能失败的审批失效/Run 停止清理。
        if records
            .last()
            .is_some_and(|event| event.data["revision_digest"] == revision_digest)
        {
            return Ok(());
        }

        // 记录当前期望 stream version；新 authority.revised 将使用 version + 1。
        let version = records
            .iter()
            .filter_map(|event| event.stream_version)
            .max()
            .unwrap_or(0);

        // 使用新 command id 记录一次新的授权配置事实，并把 revision digest 放在事件载荷中。
        let command_id = RequestId::new();
        let mut payload = value;
        payload["revision_digest"] = json!(revision_digest);
        let event = RuntimeEvent::new(command_id, 1, "authority.revised", payload)?
            .with_stream_metadata("authority", &key, version + 1);

        // 只有当前 authority stream 仍是刚才读取的 version，配置更新才会被接受。
        let batch = TransitionBatch {
            command_id,
            command_digest: revision_digest,
            expected_versions: vec![AggregateVersion {
                aggregate_type: "authority".to_owned(),
                aggregate_id: key,
                version,
            }],
            events: vec![event],
        };

        // CAS 冲突或提交结果未知会在此返回；成功后 authority 事件已经是持久化事实。
        super::dispatch::commit_confirmed(self.events.as_ref(), batch).await?;

        // 已有授权状态时，变更可能使旧审批/运行持有的授权材料过期，故在新事实提交后触发清理。
        if version > 0 {
            // 使该项目待处理的旧审批不能继续使用。
            self.approvals
                .invalidate_project(&context.project_root, "authority_changed")
                .await?;
            // 再停止该项目已有运行；任一清理错误都会向 daemon 返回，但不会回滚上面的 EventLog 事件。
            self.stop_project_runs(&context.project_root, "authority_changed")
                .await?;
        }
        Ok(())
    }
}
