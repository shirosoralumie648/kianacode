//! Provider-side validation for BQ-18 fallback attempts.
//!
//! The provider never chooses a fallback and never mints a permit.  It only checks that the
//! route/credential/price bindings produced by ControlPlane still match the candidate prepared at
//! the existing gateway effect boundary.
//!
//! ## 这个文件在系统里的位置
//!
//! ```text
//!   ControlPlane 决定"这次允许降级到备用路由"
//!        │  产出 FallbackAttemptAdmission（准入凭据：route/credential/permit 的绑定值）
//!        ↓
//!   【本文件 validate_fallback_attempt()】  ← provider 侧的"凭据没被掉包"复核
//!        │
//!        ↓
//!   transport::send() 真正发请求
//! ```
//!
//! ## 上游 / 下游
//!
//! - 上游：谁构造 `FallbackAttemptAdmission`——`rg -n "FallbackAttemptAdmission"` 显示
//!   该类型定义在 `kiana-domain`，provider 只**消费**它。
//! - 下游：本文件只做校验，不调用任何东西。校验通过后由调用方继续走既有发送路径。
//!
//! ## ⚠ 调用者现状（诚实说明）
//!
//! `rg -n "validate_fallback_attempt"` 的结果只有两处：本文件的定义，
//! 和 `kiana-provider/src/lib.rs` 里的 `pub use fallback::{validate_fallback_attempt, ...}` 重新导出。
//! **在 kiana-daemon / kiana-core / kiana-entrypoints 里没有找到实际调用点。**
//! 因此当前它是一个**已就位但尚未接入调用链的边界校验函数**（契约层），不是热路径。
//! 写注释时不能假装它已经在跑。

use kiana_domain::{FallbackAttemptAdmission, ModelError, ModelRoute};

/// 这条边界的形状标识。
///
/// 【作用】 给未来的调用方一个可断言的常量：谁在 provider 侧做降级准入复核，就该登记这个 schema。
/// 目前它只是被重新导出，还没有写进任何 payload——这一点如实说明，不要误以为已有落盘产物。

pub const FALLBACK_PROVIDER_BOUNDARY_SCHEMA: &str = "kiana.fallback-provider-boundary.v1";

/// 复核一次降级（fallback）尝试的准入绑定是否仍然成立。
///
/// 【作用】
/// 降级意味着“原路由用不了，改用备用路由”。这件事一旦发生，
/// **实际上用的可能已经不是用户批准的那条路由/凭据/价格**了。
/// 本函数在真正发请求前，把“准入时记录的绑定值”和“当前准备好的候选值”逐项对比，
/// 只要有一项对不上就拒绝。
///
/// 【调用者】
/// 仓库中未找到生产调用点；目前仅由 `kiana-provider/src/lib.rs` 重新导出（见文件级说明）。
///
/// 【输入】
/// - `admission`：控制面签发的降级准入凭据，里面的 route / credential_revision / permit_digest
///   是**准入当时**的值。
/// - `route`：这一次实际准备使用的路由。
/// - `credential_revision`：当前连接绑定的凭据版本。
/// - `permit_digest`：控制面这次签发的许可摘要。
///
/// 【输出】
/// 全部一致返回 `Ok(())`；任一项漂移返回带稳定错误码的 `Err(ModelError)`。
///
/// 【副作用】
/// 无。纯函数，不改参数、不写事件、不发网络请求。
///
/// 【失败情况】
/// | 错误码 | 含义 |
/// |---|---|
/// | （`admission.validate()` 的错误） | 准入凭据自身不合法 |
/// | `fallback_route_admission_drift` | 准入时批的路由 ≠ 现在要用的路由（换模型/换服务商了） |
/// | `fallback_credential_revision_drift` | 准入之后凭据被换过（可能换成了权限更大的 key） |
/// | `fallback_permit_admission_drift` | 携带的许可摘要对不上这次准入 |
///
/// 【⚠ 为什么路由要同时比 digest 和两个字段】
/// `route.digest()` 是整体摘要，用来防“结构上完全不同的路由”；
///
/// ```text
///   route.digest() != admission.route_digest
///        │
///        ├─ 捕获「整体换了」：连 provider_id 都不同的情况
///        │
///   route.provider_id != admission.route.provider_id
///   route.model_id   != admission.route.model_id
///        │
///        └─ 捕获「只换了一半」：digest 碰撞或被刻意构造成相同，
///           但 provider_id / model_id 已经不同
/// ```
///
/// 只比 digest 理论上够快、够简洁，但多比两个标量字段几乎零成本，
/// 而且能在摘要实现被改动时多一道防线。这是有意的冗余。
///
/// 【为什么 provider 不自己选降级】
/// 因为“要不要降级、降到哪条、贵多少”都是**业务与授权决策**，必须留在 ControlPlane。
/// provider 若能自己换路由，就等于绕过了审批与预算。所以它只做“对账”，不做“决策”。

pub fn validate_fallback_attempt(
    admission: &FallbackAttemptAdmission,
    route: &ModelRoute,
    credential_revision: Option<&str>,
    permit_digest: &str,
) -> Result<(), ModelError> {
    admission.validate().map_err(ModelError::invalid)?;
    if route.digest() != admission.route_digest
        || route.provider_id != admission.route.provider_id
        || route.model_id != admission.route.model_id
    {
        return Err(ModelError::invalid("fallback_route_admission_drift"));
    }
    if credential_revision != Some(admission.credential_revision.as_str()) {
        return Err(ModelError::invalid("fallback_credential_revision_drift"));
    }
    if permit_digest != admission.permit_digest {
        return Err(ModelError::invalid("fallback_permit_admission_drift"));
    }
    Ok(())
}
