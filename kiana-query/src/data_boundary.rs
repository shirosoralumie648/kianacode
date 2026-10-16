//! 本文件把数据撤销、保留策略和导出请求转换成 Query 层派生视图的边界 DTO。
//!
//! 在系统中的位置：数据治理层或调用方提供项目根、scope 摘要、data epoch 与撤销/保留标志；
//! `QueryDataBoundary::derive()` 将这些输入映射为 Memory、Index、Cache、Export 四个方向的
//! Allow/Deny 字段，并计算自摘要。`billing_api` 与 `audit_projector` 会消费这些字段作本地拒绝
//! 判断；仓内目前没有找到它们的生产路由调用，构造器直接调用也仅见测试。
//!
//! ```text
//! 治理状态 + 项目/scope/epoch + export_requested
//!     -> QueryDataBoundary::derive()
//!     -> memory/index/cache/export dispositions + decision_digest
//!     -> billing_api 或 audit_projector 的局部边界检查
//! ```
//!
//! 这个结构是可序列化的判断输入/结果，不是签名、权限授予或可信来源证明。摘要无密钥，公开
//! 调用方能在修改字段后重算；真正的调用入口仍须从可信治理状态构造边界，并结合自己的 authority
//! 和 freshness 校验。`validate()` 检查合同和部分拒绝不变量，不会重新推导治理事实。

use kiana_domain::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const QUERY_DATA_BOUNDARY_SCHEMA: &str = "kiana.query-data-boundary.v1";

/// Query 读取某一类派生数据时得到的本地允许/拒绝结果。
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryDataDisposition {
    /// 依照传入边界字段，允许对应 Query 派生视图继续进入后续检查。
    Allowed,
    /// 拒绝对应视图；调用方应返回拒绝或采取明确恢复流程，不能伪装成“没有数据”。
    Denied,
}

/// 一次治理输入在 Query 派生视图上的允许/拒绝投影。
///
/// 【系统角色】它把同一项目和数据 epoch 下的治理信息同时投影到 memory、index、cache、export，
/// 让 Query API 可以绑定请求与相同 scope，并对撤销/保留限制执行拒绝检查。
/// 【数据流】调用方输入进入 `derive()`，四个 disposition 和元数据被 `digest()` 一起摘要；公开
/// consumer 再读取这些字段，但本结构不会自己控制文件、缓存、Memory 或 EventLog。
/// 【Serde】`Serialize` / `Deserialize` 来自 serde，支持 JSON 写入和读回；`deny_unknown_fields`
/// 让当前版本拒绝未识别的字段，避免静默忽略合同扩展。
/// 【信任边界】`decision_digest` 是无密钥自摘要，不证明构造者身份。只验证 DTO 自洽不能替代可信
/// authority；消费者必须自己确认来源及项目/scope/epoch 是否与本次请求绑定。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryDataBoundary {
    /// 清单合同版本。
    pub schema: String,
    /// 边界适用的项目根文本；此类型只检查非空，不会规范化路径或确认目录身份。
    pub project_root: String,
    /// 调用方提供的 scope 摘要，`derive()` 要求 `sha256:` 加 64 位十六进制文本。
    pub scope_digest: String,
    /// 绑定数据版本的非零 epoch；由调用方提供，本模块不向治理存储查询最新值。
    pub data_epoch: u64,
    /// 来源数据是否已被撤销；为 true 时 `derive()` 拒绝全部四类派生视图。
    pub revoked: bool,
    /// 当前策略是否允许保留/读取派生数据；为 false 时同样拒绝全部视图。
    pub retention_allowed: bool,
    /// Memory 派生视图的 disposition。
    pub memory: QueryDataDisposition,
    /// Index 派生视图的 disposition。
    pub index: QueryDataDisposition,
    /// Cache 派生视图的 disposition。
    pub cache: QueryDataDisposition,
    /// 导出视图的 disposition；只有一般读取条件通过且调用方请求 export 时，构造器才允许。
    pub export: QueryDataDisposition,
    /// 覆盖 schema、项目/scope/epoch、治理标志及四个 disposition 的自摘要；不包含自身。
    pub decision_digest: String,
}

impl QueryDataBoundary {
    /// 按项目治理布尔值生成四类派生视图的 disposition，并校验最终 DTO。
    ///
    /// 【调用者】仓内直接构造点是 SC-24、SC-32 与 BQ-23 fixture；crate 根模块公开 re-export，
    /// 但没有找到生产入口从治理账本调用它。
    /// 【输入】项目根文本、scope 摘要和非零 data epoch；`revoked` 与 `retention_allowed` 是调用方
    /// 已判定的治理事实；`export_requested` 只决定导出字段，不会保存为独立结构字段。
    /// 【输出】一般条件通过时 memory/index/cache 同时 Allowed，否则同时 Denied；export 还要求
    /// `export_requested` 为 true。输出带版本 schema 和对全部结果字段的自摘要。
    /// 【失败】项目根为空或 epoch 为 0 时返回 header 错误；scope 摘要不是 `sha256:` 加 64 位十六进制
    /// 时返回 scope 错误；生成对象再次 `validate()` 失败时返回相应合同错误。
    /// 【副作用】纯 DTO 计算，不读治理存储、不读取数据、不删除缓存、不写审计事件，也不访问网络。
    /// 【为什么集中派生】把 revoke/retention 的主拒绝条件映射到所有派生视图，减少某个 Query
    /// adapter 忘记检查一个数据副本的机会；但调用者仍负责提供可信布尔值并执行实际拒绝。
    pub fn derive(
        project_root: impl Into<String>,
        scope_digest: impl Into<String>,
        data_epoch: u64,
        revoked: bool,
        retention_allowed: bool,
        export_requested: bool,
    ) -> Result<Self, String> {
        let project_root = project_root.into();
        let scope_digest = scope_digest.into();
        if project_root.trim().is_empty() || data_epoch == 0 {
            return Err("query_data_boundary_header_invalid".to_owned());
        }
        let Some(hex) = scope_digest.strip_prefix("sha256:") else {
            return Err("query_data_boundary_scope_digest_invalid".to_owned());
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("query_data_boundary_scope_digest_invalid".to_owned());
        }
        // 同一来源的 memory/index/cache 共享主策略；导出是额外能力，所以还要检查显式请求。
        let allowed = !revoked && retention_allowed;
        let mut boundary = Self {
            schema: QUERY_DATA_BOUNDARY_SCHEMA.to_owned(),
            project_root,
            scope_digest,
            data_epoch,
            revoked,
            retention_allowed,
            memory: if allowed {
                QueryDataDisposition::Allowed
            } else {
                QueryDataDisposition::Denied
            },
            index: if allowed {
                QueryDataDisposition::Allowed
            } else {
                QueryDataDisposition::Denied
            },
            cache: if allowed {
                QueryDataDisposition::Allowed
            } else {
                QueryDataDisposition::Denied
            },
            export: if allowed && export_requested {
                QueryDataDisposition::Allowed
            } else {
                QueryDataDisposition::Denied
            },
            decision_digest: String::new(),
        };
        boundary.decision_digest = boundary.digest();
        boundary.validate()?;
        Ok(boundary)
    }

    /// 校验反序列化或构造后的边界 DTO 的头部、自摘要和撤销拒绝不变量。
    ///
    /// 【调用者】`derive()` 在返回前调用；`billing_api` 和 `audit_projector` 在消费边界时也会调用。
    /// 【成功/失败】成功返回 `Ok(())`；schema/root/epoch 无效、digest 形状或值错误，以及撤销/禁止
    /// 保留时任一视图仍为 Allowed，都会返回稳定错误字符串，消费者再转换为自己的拒绝错误。
    /// 【副作用】只读本对象，不更改状态、重试请求或清理数据。
    /// 【重要差别】scope digest 的格式只在 `derive()` 检查；`validate()` 没有重复此检查。它还只在
    /// revoked 或 retention 被禁止时强制所有视图 Denied，并不会重算正常情况下 memory/index/cache
    /// 应一起允许、export 应受请求开关控制的完整映射。因此自摘要正确不等于字段组合一定由
    /// `derive()` 生成；digest 也不是来源认证。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUERY_DATA_BOUNDARY_SCHEMA
            || self.project_root.trim().is_empty()
            || self.data_epoch == 0
        {
            return Err("query_data_boundary_header_invalid".to_owned());
        }
        let Some(hex) = self.decision_digest.strip_prefix("sha256:") else {
            return Err("query_data_boundary_digest_invalid".to_owned());
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("query_data_boundary_digest_invalid".to_owned());
        }
        if self.revoked || !self.retention_allowed {
            if [self.memory, self.index, self.cache, self.export]
                .iter()
                .any(|value| *value == QueryDataDisposition::Allowed)
            {
                return Err("query_data_boundary_revoked_data_allowed".to_owned());
            }
        }
        if self.decision_digest != self.digest() {
            return Err("query_data_boundary_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算边界决策字段的确定性 JSON 自摘要。
    ///
    /// `decision_digest` 本身被排除，避免摘要递归；项目、scope、epoch、两项治理布尔值和四个
    /// disposition 都纳入。摘要用于发现忘记更新字段的改动，不带密钥，不能防止有意重算。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "project_root": self.project_root,
            "scope_digest": self.scope_digest,
            "data_epoch": self.data_epoch,
            "revoked": self.revoked,
            "retention_allowed": self.retention_allowed,
            "memory": self.memory,
            "index": self.index,
            "cache": self.cache,
            "export": self.export,
        }))
    }
}
