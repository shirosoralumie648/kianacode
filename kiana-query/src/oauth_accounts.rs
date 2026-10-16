//! 将 Domain OAuth 账号记录转换为脱敏的只读查询页。
//!
//! 数据流：调用方提供 `OAuthAccountRecord` 列表和 adapter 声明的源游标 → 本文件调用 Domain
//! `OAuthAccountProjection::from_record()` 校验记录并移除 SecretRef/token 原文 → 按账号 ID 去重并排序
//! → 返回带 schema、游标和限制说明的 `OAuthAccountQueryPage`。
//!
//! 上游是账号存储/投影 adapter，下游是 Query crate 导出的查询 DTO。当前仓内只发现 fixture 直接调用
//! `project_oauth_accounts()`，尚未发现生产查询入口接线。它不读取 OAuth store/EventLog、不执行 token
//! refresh、不发 provider 请求，也不为 connector 执行签发权限。原始 access/refresh token 留在 provider
//! 的 SecretStore 边界内；本页仍包含账号身份、scope、generation/status 与 digest 等非密钥 metadata。

use kiana_domain::{OAuthAccountProjection, OAuthAccountRecord, ProviderAccountId};
use std::collections::BTreeMap;

/// 此只读查询页的 wire/schema 标记；升级字段兼容规则时需要同步消费者。
pub const OAUTH_ACCOUNT_QUERY_SCHEMA: &str = "kiana.oauth-account-query.v1";

/// 一次账号查询的展示/传输快照；不是存储记录、执行许可或外部 provider 回执。
///
/// Serde 的 `deny_unknown_fields` 会在反序列化时拒绝未声明字段，但它本身不会验证字段之间的业务关系；
/// 当前构造路径由 `project_oauth_accounts()` 先校验每条 Domain projection，再创建本 DTO。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OAuthAccountQueryPage {
    /// 页面格式版本；目前固定为 `OAUTH_ACCOUNT_QUERY_SCHEMA`。
    pub schema: String,
    /// 按 ProviderAccountId 升序排列、经 Domain 校验的脱敏账号视图。
    pub accounts: Vec<OAuthAccountProjection>,
    /// adapter 声明的源位置；非零检查只证明调用方给了一个数，不证明游标对应完整/最新数据。
    pub source_cursor: u64,
    /// 本 projection 算法/格式版本；当前与 schema 字符串相同。
    pub projection_version: String,
    /// 新鲜度标记；当前构造函数固定写 false，没有与源版本或当前时间比较。
    pub stale: bool,
    /// 本结果不具备的能力/证据说明，供读取方避免把 metadata 当作执行或 live 证明。
    pub limitations: Vec<String>,
}

/// 将 Domain 账号记录转成确定顺序的只读 Query 页面。
///
/// 【调用者】由 Query crate 根模块重导出；当前静态调用点是 INT-12 projection fixture，未找到产品入口。
/// 【输入】`records` 是可消费的账号记录序列；`now_unix_ms` 用于 Domain expiry/status 校验；
/// `source_cursor` 由 adapter 提供，必须非零。
/// 【输出】成功时返回按 ProviderAccountId 排序的页面。相同账号 ID 且 digest 相同的重复记录合并为一项；
/// 同 ID 出现不同 digest 时返回冲突错误。
/// 【失败情况】游标为 0 时返回 `oauth_account_source_cursor_invalid`；Domain 记录校验错误原样向上传播；
/// ID/digest 冲突返回 `oauth_account_projection_conflict`。这里没有重试或存储副作用。
/// 【为什么这样设计】BTreeMap 同时提供稳定排序和按 ID 聚合，让相同记录输入顺序不同也能得到同序结果；
/// 先复用 Domain projection 校验则可避免 Query 层复制 OAuth 生命周期规则。
/// ⚠ source_cursor 只是调用者声明，不是 EventLog 读取回执；`stale=false` 也不证明数据新鲜。
pub fn project_oauth_accounts(
    records: impl IntoIterator<Item = OAuthAccountRecord>,
    now_unix_ms: u64,
    source_cursor: u64,
) -> Result<OAuthAccountQueryPage, String> {
    if source_cursor == 0 {
        return Err("oauth_account_source_cursor_invalid".to_owned());
    }
    // 这里的 key 是服务商账号 ID，不是用户/主体 ID；有序 map 使最终页面顺序不依赖输入迭代顺序。
    let mut by_id = BTreeMap::<ProviderAccountId, OAuthAccountProjection>::new();
    for record in records {
        // Domain 会验证 schema、状态/到期时间、SecretRef 摘要和 account digest，并构造不含原始 token 的视图。
        let projection = OAuthAccountProjection::from_record(&record, now_unix_ms)?;
        if let Some(previous) = by_id.insert(projection.account_id, projection.clone()) {
            // 相同 ID 的重复快照可以收敛为一项；如果 digest 不同，不能擅自决定哪一版才是权威记录。
            if previous.account_digest != projection.account_digest {
                return Err("oauth_account_projection_conflict".to_owned());
            }
        }
    }
    // stale=false 是 DTO 当前固定的默认声明；真正的游标一致性/新鲜度校验需要由未来接入的读取 adapter 提供。
    Ok(OAuthAccountQueryPage {
        schema: OAUTH_ACCOUNT_QUERY_SCHEMA.to_owned(),
        accounts: by_id.into_values().collect(),
        source_cursor,
        projection_version: OAUTH_ACCOUNT_QUERY_SCHEMA.to_owned(),
        stale: false,
        limitations: vec![
            "metadata_only_no_provider_request".to_owned(),
            "source_cursor_is_adapter_supplied".to_owned(),
        ],
    })
}
