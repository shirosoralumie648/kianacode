use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
        #[serde(transparent)]
        /// 由 UUID 支撑的稳定领域标识。
        pub struct $name(Uuid);

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
        }

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
uuid_id!(RunId);
uuid_id!(TurnId);
uuid_id!(CellId);
uuid_id!(EventId);
uuid_id!(ExecutionId);
uuid_id!(InvocationId);
uuid_id!(ApprovalId);
uuid_id!(ArtifactId);
uuid_id!(ReceiptId);
uuid_id!(OrganizationId);
uuid_id!(ProjectId);
uuid_id!(TemplateId);
uuid_id!(SpawnPlanId);
uuid_id!(BudgetLeaseId);
uuid_id!(CapabilityGrantId);
uuid_id!(SupervisionLeaseId);
uuid_id!(DelegationId);

impl RunId {
    /// 从字符串解析 run UUID；首尾空白会被忽略，格式错误返回 `None`。
    pub fn parse_str(value: &str) -> Option<Self> {
        Uuid::parse_str(value.trim()).ok().map(Self::from_uuid)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
/// 用户或入口分配的会话标识。
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
