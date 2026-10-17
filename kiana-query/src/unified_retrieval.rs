//! 将 Domain 统一检索算法适配为 Query crate 的 `RetrievalPort` 实现。
//!
//! Domain 的 `rank_retrieval()` 只依赖已准备好的输入，不知道 Query 的端口错误类型；本文件持有固定
//! `RetrievalProfile`，把上层调用转交给该纯算法，并把其字符串错误包装为 `PortError`：
//!
//! ```text
//! 上游准备 RetrievalRequest + RetrievalItem[] + limit
//!     -> UnifiedRetrievalPort::retrieve()
//!     -> kiana-domain::rank_retrieval()
//!     -> RetrievalResult 或 PortError::Failed
//! ```
//!
//! 本文件不读取索引、不创建候选、不做身份/ACL 决策，也不生成 embedding。2026-10-17（CM-12）：Daemon
//! memory tool 经本 adapter 调用 Domain 算法，CLI memory search 直接调用 `kiana_domain::rank_retrieval`；
//! 两个适配器共享同一排名实现，但候选授权与降级输入仍各自准备。

use kiana_domain::{
    rank_retrieval, RetrievalItem, RetrievalProfile, RetrievalRequest, RetrievalResult,
};
use kiana_ports::{PortError, RetrievalPort};

/// 为一组检索调用固定保存算法 profile 的端口 adapter。
///
/// clone 只复制 profile 值，不持有存储连接或共享可变状态；同一 profile 让 adapter 调用共享相同的排名参数。
#[derive(Clone, Debug)]
pub struct UnifiedRetrievalPort {
    /// 每次 retrieve 都交给 Domain validator 检查并写入结果摘要的配置快照。
    profile: RetrievalProfile,
}

impl UnifiedRetrievalPort {
    /// 使用调用方提供的算法 profile 创建 adapter。
    ///
    /// 【输入/输出】接收 profile，验证后保存在 adapter 中。
    /// 【失败】profile schema、参数范围或摘要不合法时传播 Domain 的稳定错误文本。
    /// 【副作用】只保存内存值，不连接索引或初始化模型。
    pub fn new(profile: RetrievalProfile) -> Result<Self, String> {
        profile.validate()?;
        Ok(Self { profile })
    }

    /// 返回当前 adapter 持有的只读 profile，供调用方检查其参数/摘要。
    pub fn profile(&self) -> &RetrievalProfile {
        &self.profile
    }
}

impl Default for UnifiedRetrievalPort {
    /// 默认采用 Domain 定义的固定 `unified()` profile。
    fn default() -> Self {
        Self {
            profile: RetrievalProfile::unified(),
        }
    }
}

impl RetrievalPort for UnifiedRetrievalPort {
    /// 调用 Domain 排序合同，并把 Domain 字符串错误转换为端口错误。
    ///
    /// request、候选和 limit 原样传递，不在 adapter 中补做 ACL 或读取数据；调用者必须在进入端口前准备
    /// 好可信权限范围和候选集合。故障以 `retrieval:` 前缀包装，便于上层识别错误来源。
    fn retrieve(
        &self,
        request: &RetrievalRequest,
        items: &[RetrievalItem],
        limit: usize,
    ) -> Result<RetrievalResult, PortError> {
        rank_retrieval(&self.profile, request, items, limit)
            .map_err(|error| PortError::Failed(format!("retrieval:{error}")))
    }
}

/// 使用默认 profile 的便捷入口，与 `UnifiedRetrievalPort::default().retrieve(...)` 共用同一算法。
///
/// 【调用者】轻量调用者、fixtures 或不需要长期持有端口对象的内部路径。
/// 【失败】直接传播 `rank_retrieval()` 的稳定错误字符串。
/// 【边界】此 helper 不额外做身份验证、索引读取或候选过滤。
pub fn rank_unified(
    request: &RetrievalRequest,
    items: &[RetrievalItem],
    limit: usize,
) -> Result<RetrievalResult, String> {
    rank_retrieval(&RetrievalProfile::unified(), request, items, limit)
}
