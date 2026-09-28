//! Provider-side validation for the additive INT-17 connector quota envelope.
//!
//! The provider only rechecks server-issued facts together with the existing BQ-16 capacity lease.
//! It cannot reserve, settle, release or dispatch on its own and never accepts an alias or raw
//! credential override from a model request.
//!
//! ## 这个文件在系统里的位置
//!
//! connector 配额（quota）指的是“某个外部连接器在一段时间内允许消耗多少”。
//! 控制面负责**预留（reserve）与结算（settle）**，provider 只在真正发请求的那一刻
//! 复核“这张预留单是不是我以为的那一张”。
//!
//! ```text
//!   ControlPlane：校验策略 → 预留配额 → 签发 ConnectorQuotaReservation + claim
//!        │
//!        ↓
//!   BQ-16 容量租约（capacity lease）——同一时刻的并发/RPM 闸门
//!        │
//!        ↓
//!   【本文件 validate_connector_quota_lease()】← 三个来源的事实交叉比对
//!        │
//!        ↓
//!   transport::send()
//! ```
//!
//! ## ⚠ 调用者现状（诚实说明）
//!
//! `rg -n "validate_connector_quota_lease"` 只命中本文件与 `lib.rs` 的重新导出，
//! **未在 kiana-daemon / kiana-core 中找到调用点**。当前属于已就位的契约层校验。

use kiana_domain::{
    connector_quota_effect_admission, ConnectorQuotaClaim, ConnectorQuotaPolicy,
    ConnectorQuotaReservation,
};

/// 这条边界的形状标识，供未来把它写进事件/日志时断言用；目前仅重新导出，未落进任何 payload。

pub const CONNECTOR_QUOTA_PROVIDER_BOUNDARY_SCHEMA: &str =
    "kiana.provider-connector-quota-boundary.v1";

/// 交叉校验连接器配额预留单在 provider 侧仍然有效。
///
/// 【作用】
/// 一次外部调用要同时满足三件事，本函数把三处**独立来源**的事实放在一起比对，
/// 任何一处对不上就拒绝发请求：
///
/// 1. **领域层准入**：`connector_quota_effect_admission(...)`（`kiana-domain` 提供）校验
///    预留单本身是否过期、claim 是否与预留单匹配、策略是否允许这次用量。
/// 2. **容量租约一致**：预留单所属的容量租约摘要必须等于当前连接手上那张。
/// 3. **凭据世代一致**：预留单里的 `credential_generation` 必须等于策略里的。
///
/// 【调用者】
/// 未在仓库中找到生产调用点；仅由 `kiana-provider/src/lib.rs` 重新导出。
///
/// 【输入】
/// - `reservation`：控制面签发的配额预留单。
/// - `claim`：本次调用对配额提出的具体用量声明。
/// - `policy`：配额策略（上限、租约摘要、凭据世代）。
/// - `capacity_lease_digest`：当前连接实际持有的容量租约摘要。
/// - `now_unix_ms`：当前时间，用来判过期。由调用方注入，便于测试。
///
/// 【输出】 `Ok(())` 或 `Err(String)`（稳定错误码字符串）。
///
/// 【副作用】 无。纯函数。
///
/// 【失败情况】
/// - 第 1 步的错误由 `kiana-domain` 给出（过期 / claim 不匹配 / 超策略等），本文件原样透传。
/// - `connector_quota_capacity_lease_mismatch`：两张容量租约不是同一张。
/// - `connector_quota_credential_generation_mismatch`：凭据换代后配额单没跟着换。
///
/// 【⚠ 为什么这两项比对不能省】
///
/// ```text
///   只校验 reservation 自身合法性  →  不够
///        攻击/误配场景：凭据刚轮换（key 从只读换成全权），
///        旧的预留单在“数值上”仍然合法，于是凭新凭据用掉了旧单的额度。
///
///        两个 generation 比对      →  挡住
/// ```
///
/// 同理，容量租约摘要不匹配意味着“控制面算好的并发额度”和“provider 实际扣的并发额度”
/// 可能不是同一本账；只比数字不看账本编号，账实就会对不上。
///
/// 【为什么错误类型是 `String` 而不是结构体】
/// 与本 crate 其它边界保持一致：这里只返回一个可被上层记录/断言的稳定错误码字符串，
/// 不试图在 provider 层重建完整的错误分类。
pub fn validate_connector_quota_lease(
    reservation: &ConnectorQuotaReservation,
    claim: &ConnectorQuotaClaim,
    policy: &ConnectorQuotaPolicy,
    capacity_lease_digest: Option<&str>,
    now_unix_ms: u64,
) -> Result<(), String> {
    connector_quota_effect_admission(reservation, claim, policy, now_unix_ms)?;
    if policy.capacity_lease_digest.as_deref() != capacity_lease_digest {
        return Err("connector_quota_capacity_lease_mismatch".to_owned());
    }
    if reservation.key.credential_generation != policy.key.credential_generation {
        return Err("connector_quota_credential_generation_mismatch".to_owned());
    }
    Ok(())
}
