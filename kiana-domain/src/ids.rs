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
//! | UUID 支撑的稳定标识 `struct XId(Uuid)` | `RunId` `EventId` `InvocationId` | 程序按 Rust 类型区分；线上的字符串不带类型名 |
//! | 人可读的会话标识 `struct SessionId(String)` | 会话名 | 人也能看 |
//! | 工作指纹 `struct WorkFingerprint(String)` | 内容摘要 | 用于判重，不是身份 |
//!
//! 最后两个**不是 UUID**，这是有意的：会话标识要能在界面上显示，
//! 工作指纹要能由内容算出来，两者都不是「随机生成的身份」。
//! UUID 的外形也不等于保密或授权：有些值随机生成，有些从已有身份/路径确定性派生；
//! 这些类型都不能单独当作访问凭证。
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
/// 宏同时派生 Serde 的 `Serialize`/`Deserialize`：前者把 Rust 值写到 JSON 等外部格式，
/// 后者从外部格式读回 Rust 值。`#[serde(transparent)]` 表示序列化时只写底层 UUID，
/// 不额外包一层对象，也不把 `RunId` 这个 Rust 类型名写进数据；因此线格式兼容的同时，
/// 编译器内仍能区分这些 newtype。越过 JSON 等边界后，类型标签不再由这个包装器提供。
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
            ///
            /// 包装动作也不会拒绝 nil UUID；调用方若要求非 nil，必须在所属领域对象处另行验证。
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            /// 取出底层 UUID 值。
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }

            /// 从字符串解析 UUID；首尾空白会被忽略，格式错误返回 `None`。
            ///
            /// 解析成功只证明它是 UUID 语法，不证明它是非 nil、随机生成或有权访问对应资源。
            pub fn parse_str(value: &str) -> Option<Self> {
                Uuid::parse_str(value.trim()).ok().map(Self::from_uuid)
            }
        }

        /// ⚠ **`Default` 不是「空」，是「一个新的随机 id」。**
        ///
        /// 这是本文件最容易踩的一个坑：某个结构体派生（derive）了 `Default`，
        /// 某个身份字段忘了显式赋值，仍会拿到一个**看起来完全正常**的新 UUID。
        /// 它不会报错、不会为空，序列化出来也是合法 id；真正的风险是这个新身份与调用方
        /// 想关联的 Run、事件或其他对象无关，错误可能延迟到查找/绑定时才显现。
        ///
        /// 换句话说：`Default` 在这里是**危险的默认值**。
        /// 它只是让通用默认构造仍能产生一个新 ID，不是「构造不出有效值时的空值出口」；
        /// 也绝不能被当成「先占个位子回头再填」。UUID 偶然碰撞虽非数学上不可能，但不是这里的主要误用风险。
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

// 主执行链里最容易混淆的身份分别表示：
// - RequestId：一次入口/命令请求及其关联信息；同一请求的其他对象可以从它派生关联 ID。
// - RunId 与 TurnId：一个长期执行实例，以及该实例中的一次交互轮次；新 turn 不会创建新的 Run。
// - StepId 与 ModelAttemptId：Runner 的一次模型/工具驱动步骤，以及该步骤中的某一次 provider 尝试。
// - InvocationId 与 ExecutionId：一次能力调用的关联身份，以及 Core 为其准备的执行身份；两者不是同一字段。
// - EventId：单条事实事件的身份；事件还会另带 RequestId 关联来源请求。
// 这些名字说明值的业务角色，不会自动验证 ID 之间的关系；绑定仍由 Core/对应 DTO 的校验负责。
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
/// `Serialize`/`Deserialize` 与 `#[serde(transparent)]` 使外部表示直接是字符串；Rust 类型边界不会替字符串内容校验格式。
///
/// 【为什么它不一样】
/// 因为会话标识要能被人看见：界面上写着「会话 sc01」，而不是一串
/// `9f3a7c2e-…`。UUID 支撑的类型主要用于让程序按语义区分字段；原始 UUID 仍可能可见，
/// 有些还是由现有值确定性派生，不能一概称为不可猜测或保密。会话标识恰好相反——
/// 它需要可读、可复制、可在对话里提到。
///
/// 这也是为什么它没有「生成随机值」的构造函数：生成一个 UUID 出来，
/// 正好抹掉了它存在的理由。
///
/// 【⚠ 由此带来的一条纪律】
/// 因为它是自由文本，所以**不能假定它的格式**。
/// `is_empty` 是去空白之后判断的——一个全是空格的 session id 同样没有意义。
pub struct SessionId(String);

impl SessionId {
    /// 创建 session ID；该函数保留调用方的原文，不自动生成 UUID、不裁剪空白，也不拒绝空字符串。
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
    /// 生成。排序与过滤不会去重重复引用。当前算法是 FNV-1a64 文字指纹，不应当作密码学哈希
    /// 或跨系统防碰撞证明。
    /// ⚠ 规范字符串用 U+001F 连接引用，但输入中若本身含该字符就不会转义或拒绝；
    /// 例如 `"alpha\u{1f}beta"` 一个引用与 `"alpha"`、`"beta"` 两个引用会编码成相同字节，
    /// 因而得到相同指纹。这是哈希前的编码歧义，不需要发生 FNV 碰撞；本注释只记录边界。
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
/// 计算 FNV-1a 64 位非密码学哈希，供 `WorkFingerprint` 生成紧凑的确定性文本指纹。
///
/// `0xcbf29ce484222325` 是该算法的 64 位 offset basis（初始状态），`0x100000001b3` 是 FNV prime；
/// `wrapping_mul` 让乘法按 64 位模数回绕，得到算法定义的无符号 64 位结果。
/// 这个 helper 不提供抗恶意碰撞能力，不可用于签名、认证或机密校验。
pub(crate) fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
