//! 本仓库**所有稳定标识**的定义处。整个设计只表达一个想法：
//! **把「用错 id」变成编译错误，而不是运行期事故。**
//!
//! # 为什么值得单独一个文件
//!
//! 因为一个系统里的 id 种类很多，而且**互相搞错是有代价的**。
//! 把 `RunId` 当成 `RequestId` 传下去，不会立刻崩——它是一个格式合法的 UUID，
//! 会被接受，然后作用在**错误的对象**上。表现出来就是「用户点了取消 A，
//! 结果 B 被取消了」。
//!
//! 这里用 90 个各自独立的 newtype 结构体（由下面那个宏生成）来根除这类错误。
//!
//! # 三类东西要分清
//!
//! | 形态 | 例子 | 谁能读懂 |
//! |---|---|---|
//! | UUID 支撑的稳定标识 `struct XId(Uuid)` | `RunId` `EventId` `InvocationId` | 只有程序 |
//! | 人可读的会话标识 `struct SessionId(String)` | 会话名 | 人也能看 |
//! | 工作指纹 `struct WorkFingerprint(String)` | 内容摘要 | 用于判重，不是身份 |
//!
//! 最后两个**不是 UUID**，这是有意的：会话标识要能在界面上显示，
//! 工作指纹要能由内容算出来，两者都不是「随机生成的身份」。
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// 生成一个 UUID 支撑的标识类型。下面有 90 次调用，所以这一处的任何疏漏
/// 会同时出现在 90 个类型上——这是把它写成宏而不是写 90 遍的唯一理由。
///
/// 【它生成的是 newtype，不是 type alias】
/// `type RunId = Uuid;` 只能做到一半：序列化和相等性都对，但**类型还是同一个**，
/// 互相传错照样编译通过。写成 `struct RunId(Uuid)` 才多出一层——
/// 编译器会拒绝「一个参数声明为 `RunId` 的函数」收到 `EventId`。
/// 这多出来的一层，就是这个文件存在的全部理由。
///
/// 【代价：读代码时看不出某个类型是什么】
/// newtype 的代价是自解释性差：`x.0` 是什么，得看定义。
/// 所以下面每个 `uuid_id!(XId);` 都值得读一遍——它在告诉你系统里有哪些身份。
macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        /// 由 UUID 支撑的稳定领域标识。
        pub struct $name(Uuid);

        /// 这一组方法对 90 个类型完全一致。
        ///
        /// 【`from_uuid` 是 `const fn`】
        /// 因为它只是包一层，没有分配、没有校验、没有副作用，所以可以放进常量。
        /// 这不是炫技——测试与静态表里经常需要编译期就有的 id。
        impl $name {
            /// 创建新的随机 UUID 标识。
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// 从已有 UUID 构造标识，不进行额外格式校验。
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            /// 取出底层 UUID 值。
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }

            /// 从字符串解析 UUID；首尾空白会被忽略，格式错误返回 `None`。
            pub fn parse_str(value: &str) -> Option<Self> {
                Uuid::parse_str(value.trim()).ok().map(Self::from_uuid)
            }
        }

        /// ⚠ **`Default` 不是「空」，是「一个新的随机 id」。**
        ///
        /// 这是本文件最容易踩的一个坑：某个结构体派生（derive）了 `Default`，
        /// 某个字段忘了赋值，于是拿到了一个**看起来完全正常**的随机 UUID。
        /// 它不会报错、不会为空、序列化出来也是一个合法 id——
        /// 直到某天有人发现两个本该不同的东西拿到了同一个身份。
        ///
        /// 换句话说：`Default` 在这里是**危险的默认值**。
        /// 它之所以存在，是因为需要一个「构造不出有效值时也不能 panic」的出口；
        /// 但它绝不能被当成「先占个位子回头再填」。
        ///
        /// 读到这里之后，你在别处看到 `..Default::default()` 时，
        /// 应该先问一句：这个字段是身份吗？如果是，那它多半已经错了。
        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

uuid_id!(RequestId);
uuid_id!(InputId);
uuid_id!(InteractionId);
uuid_id!(RunId);
uuid_id!(WorkflowInstanceId);
uuid_id!(TurnId);
uuid_id!(StepId);
uuid_id!(CellId);
uuid_id!(EventId);
uuid_id!(ExecutionId);
uuid_id!(InvocationId);
uuid_id!(ModelAttemptId);
uuid_id!(ApprovalId);
uuid_id!(ArtifactId);
uuid_id!(ReceiptId);
uuid_id!(OrganizationId);
uuid_id!(PrincipalId);
uuid_id!(ProviderAccountId);
uuid_id!(ServiceIdentityId);
uuid_id!(PolicyProfileId);
uuid_id!(DataBoundaryId);
uuid_id!(SharingGrantId);
uuid_id!(SwarmPlanId);
uuid_id!(PartitionId);
uuid_id!(ChildCellId);
uuid_id!(AttemptId);
uuid_id!(DispatchIntentId);
uuid_id!(QueueEntryId);
uuid_id!(MergeDecisionId);
uuid_id!(MessageId);
uuid_id!(NotificationId);
uuid_id!(SubscriptionId);
uuid_id!(DeliveryAttemptId);
uuid_id!(UsageId);
uuid_id!(ReservationId);
uuid_id!(LedgerEntryId);
uuid_id!(CostAllocationId);
uuid_id!(RateCardId);
uuid_id!(CostCorrectionId);
uuid_id!(QuotaReservationId);
uuid_id!(ActionRefId);
uuid_id!(DeliveryReceiptId);
uuid_id!(QualityArtifactId);
uuid_id!(QualityTransitionId);
uuid_id!(EvalDatasetId);
uuid_id!(EvalSuiteId);
uuid_id!(EvalCaseId);
uuid_id!(GoldenTraceId);
uuid_id!(EvalExperimentId);
uuid_id!(EvalResultId);
uuid_id!(QualityCandidateId);
uuid_id!(QualityGateId);
uuid_id!(QualityGateDecisionId);
uuid_id!(FeedbackId);
uuid_id!(DriftAlertId);
uuid_id!(StorageRootId);
uuid_id!(InstanceId);
uuid_id!(StoreIdentityId);
uuid_id!(StorageLockId);
uuid_id!(StorageErrorId);
uuid_id!(StorageHealthId);
uuid_id!(StorageIntegrityIncidentId);
uuid_id!(ProjectId);
uuid_id!(TemplateId);
uuid_id!(SpawnPlanId);
uuid_id!(BudgetLeaseId);
uuid_id!(CapabilityGrantId);
uuid_id!(SupervisionLeaseId);
uuid_id!(DelegationId);
uuid_id!(ExtensionId);
uuid_id!(ComponentId);
uuid_id!(SnapshotId);
uuid_id!(HookRunId);
uuid_id!(WorkspaceId);
uuid_id!(MembershipId);
uuid_id!(AssignmentId);
uuid_id!(ProjectAssignmentId);
uuid_id!(EvidenceId);
uuid_id!(CriterionId);
uuid_id!(SecurityRegistryId);
uuid_id!(SecurityContextId);
uuid_id!(SecurityPolicyId);
uuid_id!(SecurityDecisionId);
uuid_id!(SecurityEventId);
uuid_id!(GrantId);
uuid_id!(OperationId);
uuid_id!(AuditId);
uuid_id!(SecretRefId);
uuid_id!(EvidenceRefId);
uuid_id!(FenceTokenId);

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
/// 用户或入口分配的会话标识。
/// 会话标识。**不是 UUID，是调用方给的文字。**
///
/// 【为什么它不一样】
/// 因为会话标识要能被人看见：界面上写着「会话 sc01」，而不是一串
/// `9f3a7c2e-…`。程序内部的 id 越不可读越好（不泄漏结构、不可被猜测），
/// 而会话标识恰好相反——它需要可读、可复制、可在对话里提到。
///
/// 这也是为什么它没有「生成随机值」的构造函数：生成一个 UUID 出来，
/// 正好抹掉了它存在的理由。
///
/// 【⚠ 由此带来的一条纪律】
/// 因为它是自由文本，所以**不能假定它的格式**。
/// `is_empty` 是去空白之后判断的——一个全是空格的 session id 同样没有意义。
pub struct SessionId(String);

impl SessionId {
    /// 创建 session ID；该函数保留调用方的文字，不自动生成 UUID。
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// 返回不拷贝的原始 session ID。
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 判断去除首尾空白后是否为空。
    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
/// 用于幂等和冲突识别的规范化工作指纹。
/// 工作指纹：用来识别「这两次请求是不是同一件事」的摘要。
///
/// 【⚠ 它不是身份】
/// 身份回答「这是谁」，指纹回答「这是不是同一份活」。两者混用会导致
/// 「同一个 run 被当成两个」或者「两个不同的 run 被当成同一个」，
/// 所以它单独成一个类型，字段名也不叫 Id。
///
/// 【它由内容算出来】
/// 构造时把目标、输入引用、分区、输出契约和策略快照一起算进去。
/// 策略快照也在里面，意味着**换了策略就不再是同一份指纹**——
/// 这是有意的：换了策略的活，即便目标相同，也确实不是同一件事。
pub struct WorkFingerprint(String);

impl WorkFingerprint {
    /// 按目标、输入引用、分区、输出契约和策略快照生成指纹。
    ///
    /// 输入引用会排序并去除空项，使同一工作集合不受输入顺序影响；关键字段缺失时拒绝
    /// 生成。当前算法是 FNV-1a64 文字指纹，不应当作密码学哈希或跨系统防碰撞证明。
    pub fn from_parts(
        objective: &str,
        input_refs: &[String],
        partition: &str,
        output_contract: &str,
        policy_snapshot: &str,
    ) -> Result<Self, &'static str> {
        if objective.trim().is_empty()
            || partition.trim().is_empty()
            || output_contract.trim().is_empty()
            || policy_snapshot.trim().is_empty()
        {
            return Err("work_fingerprint_input_required");
        }
        let mut inputs = input_refs
            .iter()
            .map(|input| input.trim())
            .filter(|input| !input.is_empty())
            .collect::<Vec<_>>();
        inputs.sort_unstable();
        let canonical = format!(
            "objective={}\ninputs={}\npartition={}\noutput={}\npolicy={}",
            objective.trim(),
            inputs.join("\u{1f}"),
            partition.trim(),
            output_contract.trim(),
            policy_snapshot.trim(),
        );
        Ok(Self(format!(
            "fnv1a64:{:016x}",
            fnv1a64(canonical.as_bytes())
        )))
    }

    /// 返回指纹文字，例如 `fnv1a64:...`。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorkFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}
pub(crate) fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
