//! 项目授权领域值对象，以及把 authority 事件折叠成只读快照的 reducer。
//!
//! 本文件属于 `kiana-domain`：它定义策略配置（PolicyProfile）、数据边界
//! （DataBoundary）、跨项目共享授权（SharingGrant）和 `AuthorityLedger`，并校验事件重放
//! 后得到的领域状态。它只处理传入的 Rust 值和 `RuntimeEvent`，不访问文件、数据库或网络，
//! 不执行工具，也不代替 Core 判断当前请求是否获准。
//!
//! 数据流和实际边界：
//!
//! ```text
//! authority EventStore stream
//!     -> kiana-core::ControlPlane::authority_ledger()
//!     -> AuthorityLedger::rebuild(events)
//!     -> apply_event() 按 stream version 折叠事实
//!     -> Core 上层读取 epoch / membership / grant 等快照
//! ```
//!
//! 领域 reducer 支持多种 membership、assignment、policy、boundary 和 sharing 事件；但当前
//! 生产 writer 只确认会写入 `authority.revised`。因此此文件中的类型/分支说明的是可解析的
//! 领域契约，不等于每类事件已有生产写入者、已进入授权强制路径或已被运行时使用。
//! 生产入口和事实来源应继续阅读 `kiana-core/src/authority.rs` 与相应调用方。
use crate::{
    json_digest, DataBoundaryId, Membership, PolicyProfileId, ProjectAssignment, ProjectId,
    RoleAssignment, RuntimeEvent, SharingGrantId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const POLICY_PROFILE_SCHEMA: &str = "kiana.policy-profile.v1";
pub const DATA_BOUNDARY_SCHEMA: &str = "kiana.data-boundary.v1";
pub const SHARING_GRANT_SCHEMA: &str = "kiana.sharing-grant.v1";
pub const AUTHORITY_LEDGER_SCHEMA: &str = "kiana.authority-ledger.v1";

/// 校验一个必填文本值的通用边界；`max` 按 UTF-8 字节数而不是字符数计算。
///
/// trim 后为空、超长或含 NUL（字符串终止字符）时返回字段专属错误；此 helper 不会
/// trim/改写成功值，也不验证该字段是否真的是路径、ID 或协议名。
fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 只检查摘要文本的外形：要求 `sha256:` 前缀和 64 位十六进制字符。
///
/// 这是格式验证，不重新读取原始数据，也不证明摘要由可信方签发；摘要相等只说明本对象
/// 与同一套 JSON 摘要算法算出的内容一致，不是数字签名或权限凭证。
fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 校验字符串集合的数量和单项文本边界，不负责排序或去重。
///
/// 调用者需要在具体对象中再检查 canonical 顺序；每项最多 512 个 UTF-8 字节，集合最多
/// `max` 项。空字符串、全空白字符串和 NUL 会被拒绝。
fn list(values: &[String], field: &str, max: usize) -> Result<(), String> {
    if values.len() > max
        || values
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 512 || value.contains('\0'))
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 策略配置当前在账本中的生命周期状态。
pub enum PolicyProfileStatus {
    /// 策略可供 `PolicyProfile::allows()` 返回允许结果；是否用于请求仍由调用方决定。
    Active,
    /// 暂停期间不允许操作，但保留配置供账本读取。
    Suspended,
    /// 已撤销的配置不再允许操作。
    Revoked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 某一 authority epoch 下，一个策略 profile 对操作名的声明性 allowlist。
///
/// 它通过 `data_boundary_id` 关联数据边界；`profile_digest` 覆盖 schema、profile 内容、
/// epoch 和状态。Serde 反序列化拒绝未知字段，防止旧 reader 静默忽略新契约字段。
pub struct PolicyProfile {
    /// 固定版本化数据格式名，必须等于 `POLICY_PROFILE_SCHEMA`。
    pub schema: String,
    /// 该策略 profile 的稳定领域 ID。
    pub profile_id: PolicyProfileId,
    /// 同一 profile 的修订号；当前构造器从 1 开始，必须非零。
    pub version: u64,
    /// 该 profile 明确列出的操作名；构造器会排序并去重。
    pub allowed_operations: Vec<String>,
    /// 该 profile 关联的数据边界 ID；数据边界本身在别处独立存储/引用。
    pub data_boundary_id: DataBoundaryId,
    /// 创建/修订该配置时绑定的授权代次，0 无效。
    pub authority_epoch: u64,
    /// Active/Suspended/Revoked 状态，影响 `allows()` 的结果。
    pub status: PolicyProfileStatus,
    /// 对本对象内容计算的 SHA-256 风格 JSON digest，不是签名。
    pub profile_digest: String,
}

impl PolicyProfile {
    /// 创建首个 Active 修订，规范化操作列表并计算内容摘要。
    ///
    /// 输入的 `allowed_operations` 顺序不影响最终列表：先排序、去重，再算摘要；无效 schema
    /// 关联值或空/超界操作名会在 `validate()` 时返回稳定字符串错误。该方法只构造内存值。
    pub fn new(
        profile_id: PolicyProfileId,
        allowed_operations: Vec<String>,
        data_boundary_id: DataBoundaryId,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        let mut profile = Self {
            schema: POLICY_PROFILE_SCHEMA.to_owned(),
            profile_id,
            version: 1,
            allowed_operations,
            data_boundary_id,
            authority_epoch,
            status: PolicyProfileStatus::Active,
            profile_digest: String::new(),
        };
        profile.allowed_operations.sort();
        profile.allowed_operations.dedup();
        profile.profile_digest = profile.digest();
        profile.validate()?;
        Ok(profile)
    }

    /// 检查格式版本、非零版本/epoch、规范化操作列表及其摘要。
    ///
    /// 手工反序列化或直接构造的值也必须调用此方法；它不会查找关联 DataBoundary 是否存在，
    /// 也不会把 Active 状态直接变成 ControlPlane 的授权决定。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != POLICY_PROFILE_SCHEMA || self.version == 0 || self.authority_epoch == 0 {
            return Err("policy_profile_header_invalid".to_owned());
        }
        list(&self.allowed_operations, "policy_profile_operations", 256)?;
        if self
            .allowed_operations
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err("policy_profile_operations_noncanonical".to_owned());
        }
        digest(&self.profile_digest, "policy_profile_digest")?;
        if self.profile_digest != self.digest() {
            return Err("policy_profile_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 判断本 profile 自身是否声明允许某个操作。
    ///
    /// 仅当状态为 Active 且字符串精确出现在 allowlist 中才返回 true；此结果只是该单项领域
    /// 条件，调用方仍需结合 principal、项目、审批和其他策略共同决定最终权限。
    pub fn allows(&self, operation: &str) -> bool {
        self.status == PolicyProfileStatus::Active
            && self
                .allowed_operations
                .iter()
                .any(|candidate| candidate == operation)
    }

    /// 计算不包含 `profile_digest` 字段本身的确定性内容摘要。
    ///
    /// 摘要覆盖字段若有任何变化都会变化；算法不使用秘密密钥，因此它用于一致性检测，
    /// 不是防伪签名。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "profile_id": self.profile_id,
            "version": self.version,
            "allowed_operations": self.allowed_operations,
            "data_boundary_id": self.data_boundary_id,
            "authority_epoch": self.authority_epoch,
            "status": self.status,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 将一组项目及数据类别绑定到单一数据边界，并声明是否允许对外流转。
///
/// 该对象提供查询和摘要能力，不会自行授予文件/网络访问权限；执行层仍需将其与当前授权
/// 上下文及具体资源检查组合起来。
pub struct DataBoundary {
    /// 固定 schema 标识，必须等于 `DATA_BOUNDARY_SCHEMA`。
    pub schema: String,
    /// 数据边界的稳定领域 ID。
    pub boundary_id: DataBoundaryId,
    /// 此边界涵盖的项目 ID；查询使用 UUID 排序后的二分搜索。
    pub project_ids: Vec<ProjectId>,
    /// 边界涉及的数据分类字符串，构造器排序并去重。
    pub data_classes: Vec<String>,
    /// true 表示该边界声明可向外部流转；false 表示此声明不允许外部流转。
    pub allow_external: bool,
    /// 此边界所绑定的 authority epoch，必须非零。
    pub authority_epoch: u64,
    /// 对边界内容的摘要，不是签名或 capability。
    pub boundary_digest: String,
}

impl DataBoundary {
    /// 构造边界并把项目、数据类别规范化为摘要稳定的顺序。
    ///
    /// 项目列表不会由构造器自动去重；重复项目会在 `validate()` 的严格排序检查中被拒绝。
    /// 空项目集合也无效，成功只返回内存中的已校验值，不写入 authority stream。
    pub fn new(
        boundary_id: DataBoundaryId,
        project_ids: Vec<ProjectId>,
        data_classes: Vec<String>,
        allow_external: bool,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        let mut boundary = Self {
            schema: DATA_BOUNDARY_SCHEMA.to_owned(),
            boundary_id,
            project_ids,
            data_classes,
            allow_external,
            authority_epoch,
            boundary_digest: String::new(),
        };
        boundary.project_ids.sort_by_key(|id| id.as_uuid());
        boundary.data_classes.sort();
        boundary.data_classes.dedup();
        boundary.boundary_digest = boundary.digest();
        boundary.validate()?;
        Ok(boundary)
    }

    /// 校验 schema、epoch、非空且严格递增的项目 ID、数据类别和摘要。
    ///
    /// 数据类别列表允许为空，但如果非空必须严格递增且没有重复项。此校验不查询项目是否
    /// 存在，也不执行 allow_external 的策略。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DATA_BOUNDARY_SCHEMA || self.authority_epoch == 0 {
            return Err("data_boundary_header_invalid".to_owned());
        }
        if self.project_ids.is_empty()
            || self
                .project_ids
                .windows(2)
                .any(|pair| pair[0].as_uuid() >= pair[1].as_uuid())
        {
            return Err("data_boundary_projects_invalid".to_owned());
        }
        list(&self.data_classes, "data_boundary_classes", 128)?;
        if self.data_classes.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err("data_boundary_classes_noncanonical".to_owned());
        }
        digest(&self.boundary_digest, "data_boundary_digest")?;
        if self.boundary_digest != self.digest() {
            return Err("data_boundary_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 使用 UUID 二分搜索判断边界是否包含指定项目。
    ///
    /// 因为二分查找依赖排序，直接反序列化的对象应先通过 `validate()`；本方法自身不会自动
    /// 检查列表顺序，也不做数据库查询。
    pub fn contains_project(&self, project_id: ProjectId) -> bool {
        self.project_ids
            .binary_search_by_key(&project_id.as_uuid(), |id| id.as_uuid())
            .is_ok()
    }

    /// 计算覆盖 schema、ID、项目/类别列表、外部流转标记和 epoch 的内容摘要。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "boundary_id": self.boundary_id,
            "project_ids": self.project_ids,
            "data_classes": self.data_classes,
            "allow_external": self.allow_external,
            "authority_epoch": self.authority_epoch,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一次从 source project 向 target project 分享指定 scope/operation 的限时声明。
///
/// grant 还绑定 purpose 与 authority epoch；只有未撤销、未过期且 epoch 与当前账本一致时，
/// `active_at()` 才把它列为有效。此 DTO 不发送数据，也不单独执行跨项目调用。
pub struct SharingGrant {
    /// 固定 schema 标识，必须等于 `SHARING_GRANT_SCHEMA`。
    pub schema: String,
    /// 此次分享授权的稳定 ID；revoke 事件通过它定位 grant。
    pub grant_id: SharingGrantId,
    /// 允许被分享的数据来源项目。
    pub source_project: ProjectId,
    /// 获得分享能力的目标项目；不能与来源项目相同。
    pub target_project: ProjectId,
    /// 额外限定的资源 scope 名称列表；构造器排序并去重。
    pub scope: Vec<String>,
    /// 说明分享用途的必填文本，最大 512 个 UTF-8 字节。
    pub purpose: String,
    /// 此 grant 允许的精确操作名列表；构造器排序并去重。
    pub operations: Vec<String>,
    /// Unix epoch 毫秒绝对过期时刻；当前时刻必须严格小于该值才未过期。
    pub expires_at_unix_ms: u64,
    /// 该 grant 绑定的授权代次；仅匹配账本当前 epoch 时仍新鲜。
    pub authority_epoch: u64,
    /// revoke 事件处理后的撤销标记；撤销后不会再由 `active_at()` 选中。
    pub revoked: bool,
    /// 含 revoked 状态等内容的 SHA-256 风格摘要，不是签名。
    pub grant_digest: String,
}

impl SharingGrant {
    /// 创建未撤销的新 grant，先对 scope 和 operation 排序去重，再计算摘要并验证。
    ///
    /// 本构造器不会检查当前时间或账本是否已含有相同 grant ID，也不会把授权写入 EventStore。
    pub fn new(
        grant_id: SharingGrantId,
        source_project: ProjectId,
        target_project: ProjectId,
        scope: Vec<String>,
        purpose: impl Into<String>,
        operations: Vec<String>,
        expires_at_unix_ms: u64,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        let mut grant = Self {
            schema: SHARING_GRANT_SCHEMA.to_owned(),
            grant_id,
            source_project,
            target_project,
            scope,
            purpose: purpose.into(),
            operations,
            expires_at_unix_ms,
            authority_epoch,
            revoked: false,
            grant_digest: String::new(),
        };
        grant.scope.sort();
        grant.scope.dedup();
        grant.operations.sort();
        grant.operations.dedup();
        grant.grant_digest = grant.digest();
        grant.validate()?;
        Ok(grant)
    }

    /// 检查 schema、跨项目关系、期限/epoch、purpose、两个列表和摘要。
    ///
    /// 列表允许为空；非空列表必须严格递增、无重复。这里不根据当前时钟判定是否已过期，
    /// 因为过期判断由 `active_at()` 在读取时完成。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SHARING_GRANT_SCHEMA
            || self.source_project == self.target_project
            || self.expires_at_unix_ms == 0
            || self.authority_epoch == 0
        {
            return Err("sharing_grant_header_invalid".to_owned());
        }
        required(&self.purpose, "sharing_grant_purpose", 512)?;
        list(&self.scope, "sharing_grant_scope", 256)?;
        list(&self.operations, "sharing_grant_operations", 256)?;
        if self.scope.windows(2).any(|pair| pair[0] >= pair[1])
            || self.operations.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err("sharing_grant_lists_noncanonical".to_owned());
        }
        digest(&self.grant_digest, "sharing_grant_digest")?;
        if self.grant_digest != self.digest() {
            return Err("sharing_grant_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 在给定墙钟毫秒与当前 epoch 下判断 grant 是否可进入候选分享操作集合。
    ///
    /// 撤销、epoch 不匹配或 `now_unix_ms >= expires_at_unix_ms` 任一成立都会返回 false；
    /// `true` 仍只是账本层候选条件，调用方需继续执行完整的请求授权检查。
    pub fn active_at(&self, now_unix_ms: u64, authority_epoch: u64) -> bool {
        !self.revoked
            && self.authority_epoch == authority_epoch
            && now_unix_ms < self.expires_at_unix_ms
    }

    /// 对授权内容和撤销状态计算摘要；状态变化后必须用新状态重算摘要。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "grant_id": self.grant_id,
            "source_project": self.source_project,
            "target_project": self.target_project,
            "scope": self.scope,
            "purpose": self.purpose,
            "operations": self.operations,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "authority_epoch": self.authority_epoch,
            "revoked": self.revoked,
        }))
    }
}

/// 将按序提交的 authority 事件折叠为内存中的确定性状态快照。
///
/// 【调用者】Core `ControlPlane::authority_ledger()` 在读取项目 authority stream 后调用
/// `rebuild()`；也可由领域测试直接调用。`rebuild()` 出错时本地临时 ledger 会随错误丢弃。
/// 【数据】`stream_version` 跟踪已接受事件的 EventStore 顺序；`authority_epoch` 是业务授权
/// 代次；其余 BTreeMap 按领域 ID 保存当前 membership、assignment、profile、boundary 和 grant。
/// 【边界】此结构是纯 Rust 值，没有 EventStore/数据库连接；拥有一个 ledger 不代表事件已由
/// 当前生产路径写入，也不代表某个请求已获准。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AuthorityLedger {
    /// 最近一次成功折叠事件后的授权代次；初始空账本为 0。
    pub authority_epoch: u64,
    /// 最近一次成功折叠事件的 stream version；它与 epoch 在一般事件序列中可能不同。
    pub stream_version: u64,
    /// 以 membership ID 为键的当前成员事实。
    memberships: BTreeMap<String, Membership>,
    /// 以 assignment ID 为键的角色分配事实。
    role_assignments: BTreeMap<String, RoleAssignment>,
    /// 以 assignment ID 为键的项目分配事实。
    project_assignments: BTreeMap<String, ProjectAssignment>,
    /// 以 profile ID 为键的策略配置。
    policy_profiles: BTreeMap<String, PolicyProfile>,
    /// 以 boundary ID 为键的数据边界。
    data_boundaries: BTreeMap<String, DataBoundary>,
    /// 以 grant ID 为键的跨项目分享授权。
    sharing_grants: BTreeMap<String, SharingGrant>,
}

impl AuthorityLedger {
    /// 创建计数为 0、各事实 map 为空的新账本。
    pub fn new() -> Self {
        Self::default()
    }

    /// 从完整有序事件切片创建新账本；任何一条事件无效时整体返回错误。
    ///
    /// 事件必须按 stream version 从 1 开始连续排列。因为 ledger 是本函数内部新建的局部值，
    /// 中途失败不会把部分构建结果返回给调用者；这与对既有 ledger 调用 `apply_event()` 后
    /// 再忽略错误继续使用不同。
    pub fn rebuild(events: &[RuntimeEvent]) -> Result<Self, String> {
        let mut ledger = Self::new();
        for event in events {
            ledger.apply_event(event)?;
        }
        Ok(ledger)
    }

    /// 把一条新事实折叠进当前账本，要求它恰好是下一条 stream version。
    ///
    /// 【输入/输出】事件来自调用方提供的 EventStore stream；成功后更新对应 map/epoch，并把
    /// `stream_version` 推进到该事件版本。未知事件种类、缺版本、版本跳跃、字段解码或领域校验
    /// 失败均返回 String 错误，不产生 I/O。
    /// 【容易误用】这是增量可变 API：某些分支可能先更新 epoch/map/grant，再在后续校验时失败；
    /// 出错后 `stream_version` 不会推进，但 `self` 可能已被部分修改。`rebuild()` 遇错会丢弃
    /// 临时 ledger；直接调用者不能捕获 Err 后假设原对象仍未变化。
    /// 【设计原因】事件次序本身是权限历史的一部分，拒绝缺口和重排，避免把不完整事件流伪装成
    /// 当前状态。
    pub fn apply_event(&mut self, event: &RuntimeEvent) -> Result<(), String> {
        let version = event
            .stream_version
            .ok_or_else(|| "authority_event_version_required".to_owned())?;
        // 要求严格连续。saturating_add 避免整数回绕；在 u64::MAX 边界，下一版仍会饱和为
        // u64::MAX，因此极端极限下重复最大版本可能通过这个连续性比较。
        if version != self.stream_version.saturating_add(1) {
            return Err("authority_event_version_gap_or_regression".to_owned());
        }
        // 按事件 kind 选择领域 DTO，再执行该 kind 专属的前置依赖和 epoch 检查。
        // 没有默认兼容分支：新增事件类型必须明确登记，否则旧 reducer 会拒绝重建。
        match event.kind.as_str() {
            "authority.revised" => {
                // 当前 Core writer 写的配置修订事件把 EventStore stream version 同时用作 epoch。
                // 其他领域事件则可携带自己的 epoch，因此不要假设两种计数普遍等价。
                let epoch = version;
                if epoch < self.authority_epoch {
                    return Err("authority_epoch_rollback".to_owned());
                }
                self.authority_epoch = epoch;
            }
            "membership.bound" => {
                // 绑定事件的 payload 在固定的 `membership` key 下；每个 ID 只允许首次插入。
                let membership: Membership = decode(&event.data, "membership")?;
                self.advance_epoch(membership.authority_epoch)?;
                Self::insert_unique(
                    &mut self.memberships,
                    membership.membership_id.to_string(),
                    membership,
                    "membership_duplicate",
                )?;
            }
            "role_assignment.bound" => {
                let assignment: RoleAssignment = decode(&event.data, "assignment")?;
                self.advance_epoch(assignment.authority_epoch)?;
                Self::insert_unique(
                    &mut self.role_assignments,
                    assignment.assignment_id.to_string(),
                    assignment,
                    "role_assignment_duplicate",
                )?;
            }
            "project_assignment.bound" => {
                let assignment: ProjectAssignment = decode(&event.data, "assignment")?;
                self.advance_epoch(assignment.authority_epoch)?;
                // 项目分配必须引用本账本中已经出现的角色分配；此处没有再验证 membership，
                // 也不访问身份目录或项目数据库。
                if !self
                    .role_assignments
                    .contains_key(&assignment.role_assignment_id.to_string())
                {
                    return Err("authority_role_assignment_missing".to_owned());
                }
                Self::insert_unique(
                    &mut self.project_assignments,
                    assignment.project_assignment_id.to_string(),
                    assignment,
                    "project_assignment_duplicate",
                )?;
            }
            "policy_profile.bound" => {
                let profile: PolicyProfile = decode(&event.data, "profile")?;
                self.advance_epoch(profile.authority_epoch)?;
                Self::insert_unique(
                    &mut self.policy_profiles,
                    profile.profile_id.to_string(),
                    profile,
                    "policy_profile_duplicate",
                )?;
            }
            "data_boundary.bound" => {
                let boundary: DataBoundary = decode(&event.data, "boundary")?;
                self.advance_epoch(boundary.authority_epoch)?;
                Self::insert_unique(
                    &mut self.data_boundaries,
                    boundary.boundary_id.to_string(),
                    boundary,
                    "data_boundary_duplicate",
                )?;
            }
            "sharing_grant.bound" => {
                let grant: SharingGrant = decode(&event.data, "grant")?;
                self.advance_epoch(grant.authority_epoch)?;
                Self::insert_unique(
                    &mut self.sharing_grants,
                    grant.grant_id.to_string(),
                    grant,
                    "sharing_grant_duplicate",
                )?;
            }
            "sharing_grant.revoked" => {
                // 撤销事件只需 grant ID 与新 epoch，不反序列化完整新 grant；下面原地更新当前值。
                let id = event.data["grant_id"]
                    .as_str()
                    .ok_or_else(|| "sharing_grant_id_required".to_owned())?;
                let grant = self
                    .sharing_grants
                    .get_mut(id)
                    .ok_or_else(|| "sharing_grant_missing".to_owned())?;
                let epoch = event.data["authority_epoch"]
                    .as_u64()
                    .ok_or_else(|| "authority_epoch_required".to_owned())?;
                if epoch < self.authority_epoch {
                    return Err("authority_epoch_rollback".to_owned());
                }
                self.authority_epoch = epoch;
                grant.revoked = true;
                grant.authority_epoch = epoch;
                grant.grant_digest = grant.digest();
                // ⚠ 以上字段已在 validate() 前修改；若新对象验证失败，本次 apply_event 返回 Err，
                // 但既有 ledger 已部分变化且 stream_version 尚未推进。
                grant.validate()?;
            }
            _ => return Err("authority_event_kind_unknown".to_owned()),
        }
        // 只有整个分支成功后才推进消费游标；因此前面任何提前 Err 都保留旧版本号。
        self.stream_version = version;
        Ok(())
    }

    /// 将账本代次推进到一个非零且不早于当前值的 epoch。
    ///
    /// 较新的 epoch 可以一次跳过多个数字，并不要求它等于当前事件的 stream version；这个方法
    /// 只做单调性校验，不读取事件流。发现旧 epoch 或 0 时拒绝，防止陈旧事实覆盖新授权状态。
    fn advance_epoch(&mut self, epoch: u64) -> Result<(), String> {
        if epoch == 0 {
            return Err("authority_epoch_invalid".to_owned());
        }
        if epoch < self.authority_epoch {
            return Err("authority_epoch_stale".to_owned());
        }
        self.authority_epoch = epoch;
        Ok(())
    }

    /// 向 ID map 插入唯一记录；重复 key 返回调用点提供的错误码。
    ///
    /// ⚠ 当前实现调用 `insert()` 后才检查是否已有旧值，因此重复 key 会先以新值覆盖旧值，
    /// 然后返回 Err。`apply_event()` 直接调用者若忽略错误会留下改变后的 map；`rebuild()` 遇错
    /// 会丢弃整个局部账本。这里标注现状，不要误认为失败是无副作用回滚。
    fn insert_unique<T>(
        map: &mut BTreeMap<String, T>,
        key: String,
        value: T,
        duplicate: &str,
    ) -> Result<(), String> {
        if map.insert(key, value).is_some() {
            return Err(duplicate.to_owned());
        }
        Ok(())
    }

    /// 按 membership ID 的 BTreeMap 顺序借用迭代当前成员记录，不复制数据。
    pub fn memberships(&self) -> impl Iterator<Item = &Membership> {
        self.memberships.values()
    }

    /// 按 assignment ID 顺序借用迭代当前角色分配。
    pub fn role_assignments(&self) -> impl Iterator<Item = &RoleAssignment> {
        self.role_assignments.values()
    }

    /// 按 assignment ID 顺序借用迭代当前项目分配。
    pub fn project_assignments(&self) -> impl Iterator<Item = &ProjectAssignment> {
        self.project_assignments.values()
    }

    /// 按 profile ID 顺序借用迭代当前策略 profile。
    pub fn policy_profiles(&self) -> impl Iterator<Item = &PolicyProfile> {
        self.policy_profiles.values()
    }

    /// 按 boundary ID 顺序借用迭代当前数据边界。
    pub fn data_boundaries(&self) -> impl Iterator<Item = &DataBoundary> {
        self.data_boundaries.values()
    }

    /// 按 grant ID 顺序借用迭代当前 sharing grant，包括已撤销或已过期记录。
    pub fn sharing_grants(&self) -> impl Iterator<Item = &SharingGrant> {
        self.sharing_grants.values()
    }

    /// 汇总 source→target 方向上当前仍有效 grant 允许的 operation 名称。
    ///
    /// 只保留来源项目、目标项目同时精确匹配，且未撤销、未过期、epoch 等于本账本当前值的
    /// grants；多个 grant 的操作取并集并由 BTreeSet 排序去重。该结果是候选集合，不包含
    /// scope/purpose 的运行时资源检查，也不会自行授权或执行操作。
    pub fn sharing_operations(
        &self,
        source_project: ProjectId,
        target_project: ProjectId,
        now_unix_ms: u64,
    ) -> BTreeSet<String> {
        self.sharing_grants
            .values()
            .filter(|grant| {
                grant.source_project == source_project
                    && grant.target_project == target_project
                    && grant.active_at(now_unix_ms, self.authority_epoch)
            })
            .flat_map(|grant| grant.operations.iter().cloned())
            .collect()
    }

    /// 对 epoch、stream version 和所有当前 map 计算确定性快照摘要。
    ///
    /// BTreeMap 为相同内容提供稳定遍历顺序。digest 用于比较快照内容，不是签名；知道 JSON
    /// 内容的任何调用方都能重新计算它。
    pub fn snapshot_digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": AUTHORITY_LEDGER_SCHEMA,
            "authority_epoch": self.authority_epoch,
            "stream_version": self.stream_version,
            "memberships": self.memberships,
            "role_assignments": self.role_assignments,
            "project_assignments": self.project_assignments,
            "policy_profiles": self.policy_profiles,
            "data_boundaries": self.data_boundaries,
            "sharing_grants": self.sharing_grants,
        }))
    }
}

/// 从事件 payload 指定 key 取出 JSON 子值并反序列化为领域 DTO。
///
/// key 缺失与格式错误映射成稳定的 `authority_{key}_required/invalid` 字符串，避免把 serde
/// 内部细节当成公开错误契约。DTO 自身的 `deny_unknown_fields` 会继续拒绝未认识字段；本函数
/// 不检查事件 kind 与 key 的对应关系，映射由 `apply_event()` 中的 match 分支负责。
fn decode<T: for<'de> Deserialize<'de>>(data: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(
        data.get(key)
            .cloned()
            .ok_or_else(|| format!("authority_{key}_required"))?,
    )
    .map_err(|_| format!("authority_{key}_invalid"))
}
