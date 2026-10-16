//! # 文件职责
//!
//! 本文件定义两种交给 Core 做一致性检查的快照数据传输对象（Data Transfer Object，DTO）：
//! `ProjectTrustSnapshot` 记录 Daemon 查询到的项目可信结论及其项目范围，`DepartmentSnapshot`
//! 固定某个部门在指定修订和 authority epoch 下允许出现的角色集合。它们不读取磁盘、不查询信任库、
//! 不持久化事实，也不单独授予角色或 capability。
//!
//! # 主要调用链
//!
//! ```text
//! DaemonHost::context_from_assignment()
//!   -> project_identity()：解析项目身份（Daemon 负责文件系统与 ProjectTrust 读取）
//!   -> project_trust_snapshot() -> ProjectTrustSnapshot::from_project()
//!   -> DepartmentSpec::lookup() -> DepartmentSnapshot::from_spec()
//!   -> Core::SecurityAuthoritySnapshot::from_parts()/validate_request()
//!   -> 返回填好 actor / role / department 的 RequestContext
//!   -> 后续请求仍需进入 ControlPlane 的正常授权链
//! ```
//!
//! 仓内源码搜索找到上述快照组合代码，但没有找到 `context_from_assignment()` 的生产调用点；因此
//! 这条路径说明当前实现如何组合 DTO，不证明它已成为 CLI、Workbench 或 Web 的实际入口。项目快照
//! 还被 Domain 的 storage preflight、deployment config 合同引用。读取快照时，结构校验和无密钥摘要
//! 只能发现字段不完整或内容变化，不能证明 `trusted` 真由可信 Daemon 写入，也不能代替当前授权检查。
//!
//! ⚠ `context_from_assignment()` 先解析一次项目身份，随后 `project_trust_snapshot()` 又解析身份并
//! 单独读取信任状态；这不是同一个原子快照。`ProjectTrustSnapshot` 只校验两者各自的字段/摘要，
//! 不证明 `trusted` 与 `trust_revision` 来自同一次观察。若信任状态在读取间变化，组合结果可能
//! 反映不同时间点；需要由未来 adapter 的取值与绑定方式解决，本 Domain DTO 不会自行修复。

// Serde 将 Rust 值与 JSON 等外部格式互转；deny_unknown_fields 让反序列化遇到合同外字段时失败，
// 避免拼错或偷偷添加的属性被静默忽略。它不验证字段事实来源。

use crate::{json_digest, DepartmentSpec, ProjectId, ProjectIdentity, RoleSpec, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

/// 项目可信快照在 JSON 等外部格式中的稳定合同名称。
pub const PROJECT_TRUST_SNAPSHOT_SCHEMA: &str = "kiana.project-trust-snapshot.v1";
/// 部门角色快照在 JSON 等外部格式中的稳定合同名称。
pub const DEPARTMENT_SNAPSHOT_SCHEMA: &str = "kiana.department-snapshot.v1";
/// 两种快照当前共用的 schema 兼容版本。
pub const TRUST_SNAPSHOT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// 项目可信状态及其适用项目的快照。
///
/// 该值由 Daemon 从项目身份和 `ProjectTrust` 查询结果组装，再由 Core 的
/// `SecurityAuthoritySnapshot` 与 assignment、部门角色及请求上下文交叉比较。单独校验本结构只证明
/// 字段格式和自摘要相符；由于字段公开、摘要无密钥，反序列化成功不能证明信任结论来自 Daemon。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTrustSnapshot {
    /// 此 DTO 的稳定合同名称，用来拒绝其他用途或其他版本的数据。
    pub schema: String,
    /// DTO 的兼容版本。
    pub version: SchemaVersion,
    /// 该信任结论适用的稳定项目 ID。
    pub project_id: ProjectId,
    /// 已规范化项目根目录的 SHA-256 摘要；不保存目录路径，也不在这里重新读取目录。
    pub canonical_root_digest: String,
    /// ProjectIdentity 携带的信任版本/状态摘要，用于把上层所见的信任修订带入此快照。
    pub trust_revision: String,
    /// 当前上游报告的信任结论：true 表示可信，false 表示不可信；此 bool 本身不是认证证明。
    pub trusted: bool,
    /// 记录快照由哪个逻辑来源提供，例如 Daemon 的项目 authority；这是普通文本而非签名身份。
    pub source: String,
    /// 由构造方提供的非零修订号；本 DTO 只检查不为 0，不查询持久化版本或单调递增关系。
    pub revision: u64,
    /// 对本结构其余字段计算的无密钥 SHA-256 摘要，用于发现字段改动。
    pub trust_digest: String,
}

impl ProjectTrustSnapshot {
    /// 将已取得的项目身份和信任查询结果收拢成一份可传递的快照。
    ///
    /// 【调用者】Daemon 的 `project_trust_snapshot()` 在解析项目并查询 `ProjectTrust` 后调用；
    /// Domain 合同测试也会构造内存 fixture。
    /// 【输入】`project` 提供 project ID、canonical root 和 trust revision；`trusted`、`source`、
    /// `revision` 由调用方提供，分别表示信任结论、逻辑来源标签和修订号。
    /// 【输出】带当前 schema/version 与 `trust_digest` 的快照。
    /// 【失败情况】构造后 `validate()` 失败会原样返回领域错误，例如 project ID 为空、source 无效、
    /// revision 为 0、摘要外形无效或自摘要不匹配。
    /// 【副作用/设计】只对根路径字符串做摘要并在内存中组装；不调用 `project.validate()`，也不检查
    /// trust_revision 是否确实对应传入的 `trusted`。调用方必须从可信边界提供并绑定这些输入。
    pub fn from_project(
        project: &ProjectIdentity,
        trusted: bool,
        source: impl Into<String>,
        revision: u64,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: PROJECT_TRUST_SNAPSHOT_SCHEMA.to_owned(),
            version: TRUST_SNAPSHOT_VERSION,
            project_id: project.project_id,
            canonical_root_digest: json_digest(&serde_json::json!({
                "canonical_root": project.canonical_root
            })),
            trust_revision: project.trust_revision.clone(),
            trusted,
            source: source.into(),
            revision,
            trust_digest: String::new(),
        };
        snapshot.trust_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// 从 JSON 值恢复项目可信快照，并在交给调用者前校验合同。
    ///
    /// 【调用者】读取持久化或协议中的快照时使用；当前仓内直接调用只见于 Domain 合同测试，
    /// 尚未发现生产存储/协议恢复路径。
    /// 【输入/输出】输入任意 `serde_json::Value`，成功返回字段类型正确且 `validate()` 通过的值。
    /// 【失败情况】JSON 类型/字段不匹配或包含未知字段时返回 `project_trust_snapshot_decode_failed`；
    /// DTO 已解码但合同或摘要无效时返回 `validate()` 的稳定错误。
    /// 【副作用】只克隆 JSON 值并在内存中解码，不访问信任库，也不恢复真实性证明。
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let snapshot: Self = serde_json::from_value(value.clone())
            .map_err(|_| "project_trust_snapshot_decode_failed".to_owned())?;
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// 将当前字段序列化为 JSON 值，供外层协议或存储层承载。
    ///
    /// 本方法只编码，不先调用 `validate()`；由于字段公开，调用者若曾直接修改快照，应先自行校验。
    /// 编码错误映射为 `project_trust_snapshot_encode_failed`；不会写文件或记录事件。
    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "project_trust_snapshot_encode_failed".to_owned())
    }

    /// 校验快照格式、必填范围和自摘要是否互相一致。
    ///
    /// 【调用者】构造器、JSON 解码器及 Core 的 authority snapshot 校验会调用。
    /// 【输入/输出】输入当前快照；所有字段形状及 `trust_digest == digest()` 时返回 `Ok(())`。
    /// 【失败情况】头部字段不合格返回 `project_trust_snapshot_header_invalid`；摘要前缀/长度/字符
    /// 不合格返回对应的 `*_invalid`；字段被改动而摘要未同步更新时返回
    /// `project_trust_snapshot_digest_mismatch`。
    /// 【边界】这里不会检查 canonical root 当前是否仍存在、`project_id` 是否由该 root 派生、
    /// `trust_revision` 是否绑定 `trusted`、source 是否可信，或 revision 是否为最新值。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROJECT_TRUST_SNAPSHOT_SCHEMA
            || !self.version.is_compatible_with(&TRUST_SNAPSHOT_VERSION)
            || self.project_id.as_uuid().is_nil()
            || self.source.trim().is_empty()
            || self.source.len() > 128
            || self.source.contains('\0')
            || self.revision == 0
        {
            return Err("project_trust_snapshot_header_invalid".to_owned());
        }
        validate_digest(
            &self.canonical_root_digest,
            "project_trust_snapshot_root_digest",
        )?;
        validate_digest(&self.trust_revision, "project_trust_snapshot_revision")?;
        validate_digest(&self.trust_digest, "project_trust_snapshot_digest")?;
        if self.trust_digest != self.digest() {
            return Err("project_trust_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算快照内容摘要，排除 `trust_digest` 自身以避免自我引用。
    ///
    /// `json_digest()` 对 schema/version、项目 ID、root 摘要、trust revision、trusted、source 和
    /// revision 生成 SHA-256 内容指纹。它可以检测未同步更新摘要的字段变化，但不是带密钥签名，
    /// 任何能改写 DTO 的一方都能重新计算它。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "project_id": self.project_id,
            "canonical_root_digest": self.canonical_root_digest,
            "trust_revision": self.trust_revision,
            "trusted": self.trusted,
            "source": self.source,
            "revision": self.revision,
        }))
    }
}

/// 某个部门在一个 authority epoch 下允许使用的规范角色列表快照。
///
/// `from_spec()` 将静态 `DepartmentSpec` 中的角色复制并排序，再绑定调用方提供的修订号和
/// authority epoch。Core 将它嵌入 `SecurityAuthoritySnapshot`，验证 assignment 的部门/角色是否
/// 包含在目录范围内；它不独立决定操作者身份，也不授予角色。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DepartmentSnapshot {
    /// 此 DTO 的稳定合同名称。
    pub schema: String,
    /// DTO 的兼容版本。
    pub version: SchemaVersion,
    /// 部门目录中的稳定 ID，例如 `executing`。
    pub department_id: String,
    /// 该部门允许的角色 ID；规范形式为排序且无重复的列表。
    pub role_ids: Vec<String>,
    /// 部门配置的非零修订号；本结构不查询当前目录版本。
    pub revision: u64,
    /// 快照所属的非零授权代次；由调用方绑定，Domain 不查询 Core 当前 epoch。
    pub authority_epoch: u64,
    /// 对其余字段计算的无密钥摘要，用于发现目录内容变化。
    pub department_digest: String,
}

impl DepartmentSnapshot {
    /// 从一个已知部门规格生成带排序角色清单的快照。
    ///
    /// 【调用者】Daemon 在组合公司授权上下文时，从 assignment 的 department ID 查到
    /// `DepartmentSpec` 后调用；Domain 测试也可直接提供规格。
    /// 【输入】静态部门规格、该 assignment 使用的修订号和 authority epoch。
    /// 【输出】角色 ID 复制并排序、带摘要且通过本地校验的快照。
    /// 【失败情况】先传播 `spec.validate()` 错误；revision/epoch 为 0、部门/角色不匹配、重复角色、
    /// 排序或摘要不符合合同，则传播本结构的稳定错误。
    /// 【副作用/设计】只复制值并在内存中排序；不查 assignment、不决定操作者是否可以采用这些角色。
    pub fn from_spec(
        spec: &DepartmentSpec,
        revision: u64,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        spec.validate()?;
        let mut role_ids = spec.roles.clone();
        role_ids.sort();
        let mut snapshot = Self {
            schema: DEPARTMENT_SNAPSHOT_SCHEMA.to_owned(),
            version: TRUST_SNAPSHOT_VERSION,
            department_id: spec.department_id.clone(),
            role_ids,
            revision,
            authority_epoch,
            department_digest: String::new(),
        };
        snapshot.department_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// 从 JSON 值恢复部门快照，并校验角色列表和摘要合同。
    ///
    /// 未知字段或反序列化类型错误统一映射为 `department_snapshot_decode_failed`；合同错误继续返回
    /// `validate()` 的稳定错误。该方法不查询实时 DepartmentSpec 或 authority epoch。
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let snapshot: Self = serde_json::from_value(value.clone())
            .map_err(|_| "department_snapshot_decode_failed".to_owned())?;
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// 按完全相等的角色 ID 查询当前列表是否包含该角色。
    ///
    /// 【调用者】Core 的 `SecurityAuthoritySnapshot::validate()` 用它检查 assignment 角色是否在部门内。
    /// 【输入/输出】角色 ID 字符串；找到同名项返回 true，否则 false。
    /// 【限制】该便捷查询本身不先调用 `validate()`；外部 JSON 应先经过 `from_json()` 或显式校验，
    /// 也不能把“目录包含该角色”单独当成身份认证或授权许可。
    pub fn contains_role(&self, role_id: &str) -> bool {
        self.role_ids.iter().any(|candidate| candidate == role_id)
    }

    /// 检查部门快照的已知部门、角色集合、排序及内容摘要。
    ///
    /// 【调用者】`from_spec()`、`from_json()` 和 Core authority snapshot 校验调用。
    /// 【拒绝顺序】先检查 schema/version、非空修订和角色列表；再确认 department ID 已登记；随后
    /// 每个角色必须属于该部门且不重复，列表必须严格升序，最后验证摘要格式与内容。
    /// 【副作用/边界】纯内存检查；不比较当前时间或实时 authority epoch，不证明输入来自可信注册表，
    /// 也不为某个用户授予目录中的任一角色。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPARTMENT_SNAPSHOT_SCHEMA
            || !self.version.is_compatible_with(&TRUST_SNAPSHOT_VERSION)
            || self.department_id.trim().is_empty()
            || self.revision == 0
            || self.authority_epoch == 0
            || self.role_ids.is_empty()
        {
            return Err("department_snapshot_header_invalid".to_owned());
        }
        if DepartmentSpec::lookup(&self.department_id).is_none() {
            return Err("department_snapshot_department_unknown".to_owned());
        }
        let mut ids = BTreeSet::new();
        for role_id in &self.role_ids {
            if role_id.trim().is_empty()
                || RoleSpec::lookup(role_id)
                    .is_none_or(|role| role.department_id != self.department_id)
                || !ids.insert(role_id.as_str())
            {
                return Err("department_snapshot_roles_invalid".to_owned());
            }
        }
        if self.role_ids.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err("department_snapshot_roles_noncanonical".to_owned());
        }
        validate_digest(&self.department_digest, "department_snapshot_digest")?;
        if self.department_digest != self.digest() {
            return Err("department_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算部门快照摘要；角色列表的次序参与摘要，因此构造器先规范排序、校验器再拒绝非规范顺序。
    ///
    /// 摘要包含 schema/version、部门 ID、角色 ID、修订号和 authority epoch，不包含
    /// `department_digest` 自身；它是无密钥内容指纹，不是注册表签名。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "department_id": self.department_id,
            "role_ids": self.role_ids,
            "revision": self.revision,
            "authority_epoch": self.authority_epoch,
        }))
    }
}

/// 检查文本是否具有 `sha256:` 加 64 个十六进制字符的外形。
///
/// 【调用者】项目 root/trust revision、自摘要及部门摘要校验共用；`field` 是调用点传入的错误码前缀。
/// 【失败情况】缺少前缀、十六进制长度不是 64 或含非十六进制字符时返回 `{field}_invalid`。
/// 大小写都被 `is_ascii_hexdigit()` 接受；这里只检查格式，不重新计算摘要或认证其来源。
/// 【副作用】无，只读取字符串。
fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
