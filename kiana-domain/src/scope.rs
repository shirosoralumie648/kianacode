//! # 文件职责
//!
//! 本文件定义策略和 Core 共同使用的多维权限范围 `ScopeSet`，并提供构造、规范化、求交集、
//! 包含关系和简单匹配函数。它位于 `kiana-domain`，只表达“哪些 operation/path/namespace/
//! network/预算/深度值可以参与后续授权判断”；它不读取授权数据库、不识别用户身份，也不
//! 直接拦截文件、网络或其他副作用。
//!
//! # 数据流与调用关系
//!
//! ```text
//! Core 从请求参数投影 action scope
//!   -> 从服务端 RequestContext 投影 context scope
//!   -> ScopeSet::intersect() 逐维度收窄
//!   -> Policy 将多个 GrantScope 继续求交，并用 allows_operation/allows_path 检查请求
//!   -> Broker discovery 对操作声明做目录过滤
//! ```
//!
//! `kiana-core/src/capabilities.rs::effective_action_scope()` 当前把动作目标和上下文允许路径
//! 合成有效 ScopeSet；`kiana-policy/src/grant_scope.rs` 用它表示 grant 并叠加授权层；
//! `kiana-capability-broker/src/discovery.rs` 消费 operation 维度。`ExecutionScope` 会把结果
//! 快照带到 Broker，但二者都不是已提交 permit 的替代品。
//!
//! # 先理解两个维度状态
//!
//! `NotApplicable` 的意思是“这一层没有额外限制”，在求交时像单位元素一样保留另一侧；它
//! 不是拒绝，也不是缺失数据自动获得授权。`Restricted(values)` 表示这一层明确给出允许值；
//! 空列表表示没有任何允许值。`ScopeLimit::NotApplicable` 同样表示这一层没有数值上限，
//! `Restricted(n)` 表示有上限。把 `NotApplicable` 和空的 `Restricted` 混为一谈，会把拒绝
//! 错算成不受限。
//!
//! 多维约束的安全方向是交集或更小上限：输入由服务端 grant/上下文提供，Core 再把单个动作
//! 与上下文约束相交。值对象只处理规范化文本和声明值；路径是否 canonical、grant 是否可信、
//! 每次请求是否获批仍由调用它的 Core/Policy/Broker 边界负责。
//!
//! Serde 是 Rust 的序列化库：此处的 tag/content 属性把 enum 编码为带 `mode` 的 JSON 数据，
//! 让持久化或协议消费者能区分“不适用”和“受限”。`Serialize` 负责 Rust 值到外部格式，
//! `Deserialize` 负责反向读取。

use crate::{canonical_journal_bytes, json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};

/// 多维权限范围 JSON 合同的稳定名称。
pub const SCOPE_SET_SCHEMA: &str = "kiana.scope-set.v1";
/// 当前兼容的 ScopeSet schema 版本。
pub const SCOPE_SET_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 单个维度最多允许 256 个字符串值，用于限制输入体积和比较成本。
pub const MAX_SCOPE_VALUES: usize = 256;
/// 单个值最多 4,096 个 UTF-8 字节；这是字符串边界，不是文件路径长度的 OS 规则。
pub const MAX_SCOPE_VALUE_BYTES: usize = 4_096;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "values", rename_all = "snake_case")]
/// 一个以字符串集合表达的权限维度。
///
/// JSON 表示通过 `mode` 区分 `not_applicable` 与 `restricted`；受限列表放在 `values`。
/// `Restricted([])` 是空允许集。它与 `NotApplicable` 的权限含义相反：空集不允许任何值，
/// 而 NotApplicable 表示当前这份范围没有为该维度增加限制。
pub enum ScopeDimension {
    /// 当前权限层不对该维度施加额外条件；求交时采用另一层的值。
    NotApplicable,
    /// 该维度明确允许的字符串集合；构造时会 trim、排序并拒绝重复项。
    Restricted(Vec<String>),
}

impl Default for ScopeDimension {
    fn default() -> Self {
        // 结构体字段缺省表示本层不声明该维度；最终范围仍要与父级/请求范围求交。
        Self::NotApplicable
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "value", rename_all = "snake_case")]
/// 一个数值上限维度，例如预算或递归深度。
///
/// JSON 中通过 `mode` 区分状态，受限数值放在 `value`；求交时两个上限取较小值。
pub enum ScopeLimit {
    /// 当前层没有提供该数值维度的限制。
    NotApplicable,
    /// 当前层允许的最大数值；0 也属于合法存储值，本类型不解释其业务单位。
    Restricted(u64),
}

impl Default for ScopeLimit {
    fn default() -> Self {
        // 缺省表示当前层没有声明数值上限，不代表其他授权层也没有上限。
        Self::NotApplicable
    }
}

/// 把字符串维度规范成唯一可比较的集合表示。
///
/// `NotApplicable` 原样保留；受限值先去两端空白、检查 256 项与单值字节/NUL 边界，再排序。
/// 排序之后如果相邻值相同就报重复错误，而不是静默去重；这样调用者可以发现输入配置中的
/// 歧义。空列表本身可通过规范化，表示空允许集。
fn normalize_dimension(dimension: &ScopeDimension, field: &str) -> Result<ScopeDimension, String> {
    let ScopeDimension::Restricted(values) = dimension else {
        return Ok(ScopeDimension::NotApplicable);
    };
    if values.len() > MAX_SCOPE_VALUES {
        return Err(format!("{field}_limit"));
    }
    let mut normalized = Vec::with_capacity(values.len());
    for value in values {
        let value = value.trim();
        if value.is_empty() || value.len() > MAX_SCOPE_VALUE_BYTES || value.contains('\0') {
            return Err(format!("{field}_value_invalid"));
        }
        normalized.push(value.to_owned());
    }
    normalized.sort_unstable();
    if normalized.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(format!("{field}_duplicate"));
    }
    Ok(ScopeDimension::Restricted(normalized))
}

/// 验证 ScopeLimit 当前 enum 表示是否有效。
///
/// 目前两个变体的任意 u64 值都被接受，包括 0；这里没有设置数值上限或单位检查。保留为
/// 独立 helper 是为了让构造、验证和求交入口使用同一合同位置。
fn validate_limit(limit: ScopeLimit) -> Result<(), String> {
    match limit {
        ScopeLimit::NotApplicable | ScopeLimit::Restricted(_) => Ok(()),
    }
}

/// 对非路径字符串维度求精确交集。
///
/// `NotApplicable` 是交集单位元素，另一侧原样成为结果；两边受限时只保留完全相同的字符串。
/// 结果即使为空也先返回 `Restricted([])`，由公开 `ScopeSet::intersect()` 决定是否拒绝空权限。
fn intersect_dimension(left: &ScopeDimension, right: &ScopeDimension) -> ScopeDimension {
    match (left, right) {
        (ScopeDimension::NotApplicable, value) | (value, ScopeDimension::NotApplicable) => {
            value.clone()
        }
        (ScopeDimension::Restricted(left), ScopeDimension::Restricted(right)) => {
            ScopeDimension::Restricted(
                left.iter()
                    .filter(|value| right.binary_search(value).is_ok())
                    .cloned()
                    .collect(),
            )
        }
    }
}

/// 对路径维度按词法目录前缀求交集，而非做文件系统解析。
///
/// 两边都是受限路径时：完全相同的路径保留；若一侧是 `.` 或 `*`，把另一侧视作更具体的
/// 范围；若一侧是另一侧的目录后代，则保留后代。最后排序并去重，因为不同路径对组合可能
/// 得到同一个结果。这里的前缀以 `/` 分隔，不能据此推断 symlink、大小写或 OS canonical path。
/// ⚠ 注意：下面的 `path_subset()` 也把 `.`/`*` 当根范围标记，但 `allows_path()` 并没有这项
/// 特判；维护路径语义时需要一起核对这几个 helper。
fn intersect_path_dimension(left: &ScopeDimension, right: &ScopeDimension) -> ScopeDimension {
    match (left, right) {
        (ScopeDimension::NotApplicable, value) | (value, ScopeDimension::NotApplicable) => {
            value.clone()
        }
        (ScopeDimension::Restricted(left), ScopeDimension::Restricted(right)) => {
            let mut values = Vec::new();
            for left in left {
                for right in right {
                    if left == right {
                        values.push(left.clone());
                    } else if left == "." || left == "*" || right.starts_with(&format!("{left}/")) {
                        values.push(right.clone());
                    } else if right == "." || right == "*" || left.starts_with(&format!("{right}/"))
                    {
                        values.push(left.clone());
                    }
                }
            }
            values.sort_unstable();
            values.dedup();
            ScopeDimension::Restricted(values)
        }
    }
}

/// 对两个数值上限求交集：无上限的一侧让位于另一侧，双侧受限时取较小值。
///
/// 取小值保证交集不会因为合并权限而增大可用预算或深度。
fn intersect_limit(left: ScopeLimit, right: ScopeLimit) -> ScopeLimit {
    match (left, right) {
        (ScopeLimit::NotApplicable, value) | (value, ScopeLimit::NotApplicable) => value,
        (ScopeLimit::Restricted(left), ScopeLimit::Restricted(right)) => {
            ScopeLimit::Restricted(left.min(right))
        }
    }
}

/// 判断普通枚举型维度的 child 是否包含在 parent 之内。
///
/// parent 为 NotApplicable 时没有额外限制，因此任意 child 都是其子集；child 为
/// NotApplicable 而 parent 受限时不成立；两边受限时 child 每个值都必须出现在 parent 中。
/// 输入排序由 ScopeSet 的 validate 保证，所以可以使用二分查找。
fn dimension_subset(child: &ScopeDimension, parent: &ScopeDimension) -> bool {
    match (child, parent) {
        (_, ScopeDimension::NotApplicable) => true,
        (ScopeDimension::NotApplicable, ScopeDimension::Restricted(_)) => false,
        (ScopeDimension::Restricted(child), ScopeDimension::Restricted(parent)) => child
            .iter()
            .all(|value| parent.binary_search(value).is_ok()),
    }
}

/// 判断路径 child 是否都落在 parent 的字面目录范围或根范围标记内。
///
/// 普通 parent 路径可覆盖自身和以 `parent/` 开头的后代；`.` 和 `*` 在本 helper 中匹配任意
/// child 路径。child=NotApplicable 不会落入一个明确受限的 parent。与 `allows_path()` 的
/// 处理差异需参考 `intersect_path_dimension()` 上的提示。
fn path_subset(child: &ScopeDimension, parent: &ScopeDimension) -> bool {
    match (child, parent) {
        (_, ScopeDimension::NotApplicable) => true,
        (ScopeDimension::NotApplicable, ScopeDimension::Restricted(_)) => false,
        (ScopeDimension::Restricted(child), ScopeDimension::Restricted(parent)) => {
            child.iter().all(|child| {
                parent.iter().any(|parent| {
                    parent == "."
                        || parent == "*"
                        || child == parent
                        || child.starts_with(&format!("{parent}/"))
                })
            })
        }
    }
}

/// 判断一个数值上限是否不宽于 parent；数值较小或相等才是更窄范围。
///
/// parent 为 NotApplicable 时任意 child 都满足；child 为 NotApplicable 而 parent 有上限时不满足。
fn limit_subset(child: ScopeLimit, parent: ScopeLimit) -> bool {
    match (child, parent) {
        (_, ScopeLimit::NotApplicable) => true,
        (ScopeLimit::NotApplicable, ScopeLimit::Restricted(_)) => false,
        (ScopeLimit::Restricted(child), ScopeLimit::Restricted(parent)) => child <= parent,
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 同时表达操作、路径、命名空间、网络和数值上限的权限集合。
///
/// `deny_unknown_fields` 使反序列化时遇到未知字段直接失败，避免不同版本对同一 JSON 得出
/// 不同权限含义。`scope_digest` 是其余字段的无密钥内容摘要，不是签名或授权凭证。
pub struct ScopeSet {
    /// 合同名称，必须等于当前 ScopeSet schema。
    pub schema: String,
    /// 合同版本，必须与当前版本兼容。
    pub version: SchemaVersion,
    /// 可使用的 Domain operation 名称集合。
    pub operations: ScopeDimension,
    /// 可访问的词法路径集合；本类型不解析真实文件系统路径。
    pub paths: ScopeDimension,
    /// 可访问的 memory collection 或其他命名空间集合。
    pub namespaces: ScopeDimension,
    /// 可访问的网络目标/策略名称集合；地址解析与观测在其他边界进行。
    pub network: ScopeDimension,
    /// 预算上限声明；具体预算单位由上层合同解释。
    pub budget: ScopeLimit,
    /// 执行深度上限声明；具体深度含义由上层调用流程解释。
    pub depth: ScopeLimit,
    /// 对除自身之外的 ScopeSet 字段计算的无密钥内容摘要。
    pub scope_digest: String,
}

impl ScopeSet {
    /// 构造一个规范化并带自摘要的 ScopeSet。
    ///
    /// 【调用者】Core 将动作/context 权限投影为各维度时使用；Policy 从旧 grant 或合并层生成
    /// GrantScope 时也通过它统一整理字段。
    ///
    /// 【输入/输出】输入四个字符串维度和 budget/depth 两个数值维度；成功返回 schema/version
    /// 固定、字符串值已 trim/排序、digest 已计算的对象。
    ///
    /// 【失败情况】维度超过 256 项、单值为空/超过 4,096 字节/含 NUL、规范化后有重复值，或
    /// 最终自校验失败时返回错误。空 `Restricted` 列表可以作为空允许集创建；公开交集函数
    /// 会单独拒绝其结果为空的情况。
    ///
    /// 【副作用/设计原因】纯内存构造，不访问磁盘或策略服务。统一规范序使相同集合得到稳定
    /// JSON 和 digest，避免单纯输入顺序差异改变合同摘要。
    pub fn new(
        operations: ScopeDimension,
        paths: ScopeDimension,
        namespaces: ScopeDimension,
        network: ScopeDimension,
        budget: ScopeLimit,
        depth: ScopeLimit,
    ) -> Result<Self, String> {
        let mut scope = Self {
            schema: SCOPE_SET_SCHEMA.to_owned(),
            version: SCOPE_SET_SCHEMA_VERSION,
            operations: normalize_dimension(&operations, "scope_operations")?,
            paths: normalize_dimension(&paths, "scope_paths")?,
            namespaces: normalize_dimension(&namespaces, "scope_namespaces")?,
            network: normalize_dimension(&network, "scope_network")?,
            budget,
            depth,
            scope_digest: String::new(),
        };
        validate_limit(scope.budget)?;
        validate_limit(scope.depth)?;
        scope.scope_digest = scope.digest();
        scope.validate()?;
        Ok(scope)
    }

    /// 构造六个维度均为 NotApplicable 的值对象。
    ///
    /// 名称表示本对象自己没有加限制，不应直接当成最终有效授权；调用方必须将它与父层、
    /// grant、请求上下文等已知限制求交。内部 `expect` 依赖固定 schema 和空集合必定有效。
    pub fn unrestricted() -> Self {
        Self::new(
            ScopeDimension::NotApplicable,
            ScopeDimension::NotApplicable,
            ScopeDimension::NotApplicable,
            ScopeDimension::NotApplicable,
            ScopeLimit::NotApplicable,
            ScopeLimit::NotApplicable,
        )
        .expect("unrestricted scope is valid")
    }

    /// 校验合同版本、维度规范序、限制值表示、digest 格式及自摘要。
    ///
    /// 【调用者】`new()` 在完成摘要后自检；grant、Core 与 Broker 消费 ScopeSet 前也会通过
    /// 所属合同验证它。
    ///
    /// 【失败情况】schema/version 不兼容、维度不是 trim 后排序的唯一形式、digest 格式错误或
    /// 内容摘要不匹配都会返回错误。这里允许空 Restricted 列表，因为它表示无值，不是缺省
    /// 无限制；ExecutionScope 的外层合同可以额外拒绝这种空维度。
    ///
    /// 【边界】只验证字符串和摘要内部自洽，不验证 grant 的签发人、路径真实存在、网络规则
    /// 当前可达，或该范围是否已通过审批。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SCOPE_SET_SCHEMA
            || !self.version.is_compatible_with(&SCOPE_SET_SCHEMA_VERSION)
        {
            return Err("scope_set_header_invalid".to_owned());
        }
        for (dimension, field) in [
            (&self.operations, "scope_operations"),
            (&self.paths, "scope_paths"),
            (&self.namespaces, "scope_namespaces"),
            (&self.network, "scope_network"),
        ] {
            if normalize_dimension(dimension, field)? != dimension.clone() {
                return Err(format!("{field}_noncanonical"));
            }
        }
        validate_limit(self.budget)?;
        validate_limit(self.depth)?;
        let Some(hex) = self.scope_digest.strip_prefix("sha256:") else {
            return Err("scope_set_digest_invalid".to_owned());
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("scope_set_digest_invalid".to_owned());
        }
        if self.scope_digest != self.digest() {
            return Err("scope_set_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 返回排序 JSON 对象键后的确定性字节表示，供 journal/digest 周边使用。
    ///
    /// 数组顺序仍有意义；编码深度和序列化错误由 `canonical_journal_bytes()` 返回。稳定字节
    /// 便于比较和计算摘要，但不是原始输入文本的签名。
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    /// 把两个权限层逐维度相交，生成一份新的、不会比任一输入更宽的范围。
    ///
    /// 【调用者】Core 将“动作想访问的资源”与“当前上下文允许的范围”合并；Policy/GrantScope
    /// 将模板、部门、WorkPacket、approval 等多个授权层逐层收窄。
    ///
    /// 【核心流程】先验证双方 -> operation/namespace/network 做精确集合交集 -> paths 做词法
    /// 目录交集 -> budget/depth 取较小上限 -> `new()` 重新规范化并计算摘要。
    ///
    /// 【失败情况】输入无效时原样返回校验错误；若任一字符串维度求交后变成空 Restricted 集合，
    /// 则返回 `scope_intersection_empty`。空集合表示没有可用值，报错可让授权流程拒绝继续，
    /// 避免把空列表错误解释成 NotApplicable。
    ///
    /// 【副作用】不修改 `self` 或 `other`，也不写授权状态；它只是算出待后续校验的值。
    pub fn intersect(&self, other: &Self) -> Result<Self, String> {
        self.validate()?;
        other.validate()?;
        // 每一维独立合并，不能把某维缺省误当作其他维度也不受限。
        let result = Self::new(
            intersect_dimension(&self.operations, &other.operations),
            intersect_path_dimension(&self.paths, &other.paths),
            intersect_dimension(&self.namespaces, &other.namespaces),
            intersect_dimension(&self.network, &other.network),
            intersect_limit(self.budget, other.budget),
            intersect_limit(self.depth, other.depth),
        )?;
        if [
            &result.operations,
            &result.paths,
            &result.namespaces,
            &result.network,
        ]
        .iter()
        .any(|dimension| matches!(dimension, ScopeDimension::Restricted(values) if values.is_empty()))
        {
            return Err("scope_intersection_empty".to_owned());
        }
        Ok(result)
    }

    /// 对一串必须同时成立的权限层从左到右逐层求交。
    ///
    /// 空输入返回 `scope_layers_required`，不会返回 unrestricted：调用方漏传授权层时拒绝
    /// 比静默扩大到全集更安全。任一中间交集为空/任一层无效，`try_fold` 都会短路返回错误。
    /// 返回的是新值，输入 slice 不会被修改。当前仓内未找到此 ScopeSet helper 的直接调用点。
    pub fn intersect_all(scopes: &[Self]) -> Result<Self, String> {
        let Some((first, rest)) = scopes.split_first() else {
            return Err("scope_layers_required".to_owned());
        };
        first.validate()?;
        rest.iter()
            .try_fold(first.clone(), |current, next| current.intersect(next))
    }

    /// 判断当前范围是否完全落在父范围之内。
    ///
    /// 先验证双方，再逐维比较：普通名称要求 child 值属于 parent；路径允许 child 为 parent
    /// 目录本身或其后代；数值限制要求 child 上限不大于 parent。若 parent 是 NotApplicable，
    /// 该维不限制 child；child 是 NotApplicable 而 parent 有明确限制则不成立。
    /// 这是纯判定，不生成新授权，也不表示 child 已获准执行。
    pub fn is_subset_of(&self, parent: &Self) -> Result<bool, String> {
        self.validate()?;
        parent.validate()?;
        Ok(dimension_subset(&self.operations, &parent.operations)
            && path_subset(&self.paths, &parent.paths)
            && dimension_subset(&self.namespaces, &parent.namespaces)
            && dimension_subset(&self.network, &parent.network)
            && limit_subset(self.budget, parent.budget)
            && limit_subset(self.depth, parent.depth))
    }

    /// 判断当前 operation 是否命中操作维度。
    ///
    /// NotApplicable 返回 true；Restricted 只接受完全相等的名称，不做 alias、前缀或大小写
    /// 归一化。Policy grant 和 Broker discovery 会消费相关操作声明；调用方应传规范 operation。
    /// 此 helper 不先调用 `validate()`，上层必须在信任输入前完成合同校验。
    pub fn allows_operation(&self, operation: &str) -> bool {
        matches!(&self.operations, ScopeDimension::NotApplicable)
            || matches!(&self.operations, ScopeDimension::Restricted(values) if values.iter().any(|value| value == operation))
    }

    /// 判断一条路径文本是否命中 paths 维度。
    ///
    /// NotApplicable 返回 true；Restricted 只接受完全相等或以已声明值加 `/` 开头的文本。
    /// 本函数不 canonicalize 路径、不解析 symlink，也不特判 `.` / `*`；策略 grant 会将请求
    /// 参数中名为 `path` 的字符串交给它检查。上游必须先验证路径字段形状和 trust 边界。
    /// ⚠ `intersect_path_dimension()` / `path_subset()` 对 `.` 和 `*` 有根范围特判，而本函数
    /// 没有；当前源码存在语义差异，修改或依赖通配路径前应先确认这是预期合同。
    pub fn allows_path(&self, path: &str) -> bool {
        matches!(&self.paths, ScopeDimension::NotApplicable)
            || matches!(&self.paths, ScopeDimension::Restricted(values) if values.iter().any(|value| value == path || path.starts_with(&format!("{value}/"))))
    }

    /// 计算 ScopeSet 自身的无密钥内容摘要，并把 `scope_digest` 字段先置空。
    ///
    /// 这样摘要不会递归包含自身；任何其他字段变化都要求重新计算。它能检测载荷修改或摘要
    /// 不匹配，但不能证明是谁创建了 ScopeSet，也不能替代 grant/permit 校验。
    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "scope_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}
