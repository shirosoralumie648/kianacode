//! Provider-side SecretStore adapters.
//!
//! The provider connection keeps only a [`SecretRef`] and an adapter handle.  A raw value is
//! materialized into [`SecretMaterial`] for the final HTTP request, after which its `Drop`
//! implementation clears the owned buffer.  Domain, runner, EventLog and provider snapshots
//! never receive that value.
//!
//! ## 这个文件在系统里的位置
//!
//! ```text
//!   config.rs 组装 Connection 时：只放 SecretRef（不透明引用）+ credential_revision（摘要）
//!        |
//!        |  ... 到了真正要发请求的那一刻 ...
//!        v
//!   transport.rs::send_inner_attempt
//!        |  1. connection.credential_store.current_revision(&ref)  ← 复查版本有没有变
//!        |  2. connection.credential_store.issue(...)             ← 换取一次性租约 + 明文
//!        |  3. material.lease.validate_for(...) / consume(...)     ← 绑定校验 + 一次性消费
//!        |  4. 把 material.value 写进 HTTP 认证头
//!        v
//!   【本文件】SecretStore 的两个可用实现 + 三个 fail-closed 占位实现
//!        |
//!        v
//!   SecretMaterial { lease, credential_revision, value }
//!        |
//!        +-- Drop 时 value.clear()（尽力而为的内存卫生）
//! ```
//!
//! ## 核心概念：SecretRef vs SecretMaterial
//!
//! | | SecretRef（引用） | SecretMaterial（材料） |
//! |---|---|---|
//! | 内容 | `store`("env") + `key`("ANTHROPIC_API_KEY") | **真实的密钥字符串** |
//! | 出现时机 | 组装连接时，全程持有 | 只在组装 HTTP 请求的那一瞬间 |
//! | 能进日志吗 | 能（它不含密钥） | **绝对不能** |
//!
//! 整套设计的目的是：**让“持有引用”这件事本身毫无危险**，危险只集中在最后几行代码。
//!
//! ## 上游 / 下游
//!
//! - 上游：`config.rs` 构造 store；`transport.rs` 调用 `current_revision` 与 `issue`。
//! - 下游：本文件不调用外部任何东西，只读环境变量 / 持有内存里的字符串。
//!
//! ## ⚠ 关于"清零"的诚实说明
//!
//! `SecretMaterial::drop` 里 `self.value.clear()` 只是**尽力而为**：
//! `String::clear()` 只把长度置 0，**不保证覆盖原内存**，编译器/分配器可能保留旧字节；
//! 而且 HTTP 客户端内部还持有认证头的副本。
//! 源码英文注释已经写明：this is not represented as a durability or HSM claim。
//! 请不要把这个当作“密钥已从内存擦除”的证据。

use kiana_domain::{CredentialLease, ModelError, SecretRef, CREDENTIAL_LEASE_DEFAULT_TTL_MS};
use std::sync::Arc;

/// 密钥值的字节上限：64 KiB。
///
/// 现实中的 API key 通常是 40~200 字节。64 KiB 是一个宽松到不可能误伤、
/// 又能挡住“把整个证书或大段文本塞进 api_key”的上限。

const MAX_SECRET_VALUE_BYTES: usize = 64 * 1024;
/// 凭据租约的用途标签，固定为 `provider.request`。
///
/// 【为什么需要 purpose】 一张租约要说清"我凭什么可以用这把密钥"。
/// 用途标签保证这张租约**只能**用于发起 provider 请求，
/// 不会被拿去干别的事（例如写进一条 connector 操作的凭据）。

const PROVIDER_CREDENTIAL_PURPOSE: &str = "provider.request";

/// The narrow backend set understood by the provider adapter.  Only `Env` and `Inline` are
/// usable in this local slice; the protected-storage backends return an explicit unsupported
/// error until their OS-specific implementations are admitted.
/// 密钥后端的种类。
///
/// 【⚠ 当前只有 2 个是真正可用的】
/// `Env` 和 `Inline` 有真实实现；`Keyring` / `File` / `Os` 都是
/// **显式失败的占位实现**（见下方三个 `SecretStore` impl）。
///
/// 【为什么不"先偷偷 fallback 到环境变量"】
/// 源码英文注释：intentionally explicit rather than silently falling back。
/// 如果 `SecretRef { store: "os", ... }` 被悄悄降级成读环境变量，
/// 用户以为自己在用系统钥匙串，实际密钥一直裸在环境里——这正是
/// Kiana 反对的"把状态写成比证据更强"。宁可明确报错。

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SecretBackend {
    Env,
    Inline,
    Keyring,
    File,
    Os,
}

/// A secret store resolves an opaque reference into an effect-scoped lease.  It must never put
/// the value in an event, snapshot, error or public port result.
/// 密钥存储后端端口。
///
/// 【作用】 把"不透明引用变成可用密钥"这件事抽象出来。
/// 换后端（环境变量 -> 钥匙串 -> 文件）不需要改 `transport.rs`。
///
/// 【全局不变量 —— 最重要的一条】
/// **实现绝不能把密钥值放进事件、快照、错误信息或任何公开返回值里。**
///
/// 允许返回的：只有 `SecretMaterial`（受控、一次性、`Drop` 时清零）。
/// 禁止返回的：错误消息、`Debug` 输出、任何 `String` 拼接。

pub(crate) trait SecretStore: Send + Sync {
    #[allow(dead_code)]
    fn backend(&self) -> SecretBackend;

    /// Read only the current value digest for pre-admission revision fencing.  The implementation
    /// may touch its protected source, but it never returns the value itself.
    /// 只读取**当前密钥内容的摘要**，用于准入前后的版本围栏。
    ///
    /// 【作用】 回答"这把密钥从我上次看过之后有没有被换过"。
    ///
    /// 【⚠ 允许触碰受保护的源，但绝不返回值本身】
    /// `EnvSecretStore` 的实现会真的去读一次环境变量（为了算摘要），
    /// 但只把 `json_digest(值)` 返回出去，明文不离开这个函数。
    ///
    /// 【调用者】 `lib.rs::complete_admitted` 在发请求前调用一次，
    /// 与 `Connection` 里缓存的 `credential_revision` 比对，
    /// 不一致就报 `model_credential_revision_changed`。

    fn current_revision(&self, secret_ref: &SecretRef) -> Result<String, ModelError>;

    /// 换取一份带**一次性租约**的密钥材料。
    ///
    /// 【作用】 把 `SecretRef` 变成 `SecretMaterial`，同时签发一张 `CredentialLease`。
    ///
    /// 【租约绑定了什么】 见 `materialize()`：provider 账号、用途、audience（服务商 id）、
    /// 端点摘要、以及失效时间。也就是说：**一张租约只能用于“把密钥发给那个端点”**。
    /// 把密钥发到别处？那张租约校验不过。
    ///
    /// 【一次性】 租约被 `consume()` 用掉之后就作废（测试里验证了二次消费报
    /// `credential_lease_replayed`）。这防的是“同一个租约被复用发两次请求”。
    fn issue(
        &self,
        secret_ref: &SecretRef,
        provider_account: &str,
        audience: &str,
        endpoint_digest: &str,
        now_unix_ms: u64,
    ) -> Result<SecretMaterial, ModelError>;
}

/// Raw material is deliberately private to this crate and exists only while constructing the
/// outbound request.  Clearing the `String` is best-effort memory hygiene; the HTTP client may
/// retain an internal header copy, so this is not represented as a durability or HSM claim.
/// 一次性的密钥材料。**只在组装 HTTP 请求期间存在。**
///
/// 【字段】
/// - `lease`：凭据租约，绑定账号/用途/端点/时效，且只能消费一次。
/// - `credential_revision`：这份密钥的**内容摘要**（不是密钥本身）。
/// - `value`：**明文密钥**。整个 crate 里唯一持有明文的地方。
///
/// 【为什么字段是 `pub(crate)`】 crate 内部（`transport.rs`）要读 `value` 填 HTTP 头，
/// 要读 `lease` 做校验。但 crate 外部拿不到——`SecretMaterial` 本身是 `pub(crate)`。

pub(crate) struct SecretMaterial {
    pub(crate) lease: CredentialLease,
    pub(crate) credential_revision: String,
    pub(crate) value: String,
}

/// 密钥材料离开作用域时清空它持有的字符串。
///
/// 【⚠ 这是“尽力而为”，不是安全擦除，理由有三】
/// 1. `String::clear()` 只改长度，不保证覆写底层字节；分配器可能原样保留旧内存。
/// 2. 编译器与分配器可以做优化，语义上不保证覆写发生。
/// 3. `reqwest` 内部还会持有一份认证头的副本，本文件管不到。
///
/// 所以它只能把"窗口期缩短"，不能把"泄漏"变成"不可能"。
/// 真正把这件事做对的是架构层面的约束：
/// **明文只在 `transport.rs` 的 `send_inner_attempt` 里出现，且不进任何持久化产物。**

impl Drop for SecretMaterial {
    fn drop(&mut self) {
        self.value.clear();
    }
}

/// 校验一个密钥值是否可以安全地放进 HTTP 头。
///
/// 【四道检查，缺一不可】
/// ```text
///   1. trim 后非空        -> 空 key 一定是配错了
///   2. 长度 <= 64 KiB     -> 见 MAX_SECRET_VALUE_BYTES
///   3. 不含 NUL           -> NUL 会截断传给 C 层的字符串
///   4. 不含 CR / LF       -> **最重要的一条**：防止 header injection
///   5. reqwest 能解析成 HeaderValue -> 最终防线
/// ```
///
/// 【⚠ 第 4 条是安全关键】 如果密钥里含换行，攻击者（或一个手滑的复制粘贴）
/// 就能构造出形如 `KEY<CR><LF>X-Injected-Header: evil` 的值，
/// 从而在**发往上游的请求里插入任意头部**。这就是 header injection 攻击。
///
/// 【为什么还要第 5 条】 `HeaderValue::from_str` 会做一遍完整的 HTTP 合法性校验。
/// 自己重复实现的规则不可能和 HTTP 库完全一致，所以以库的结果为准。

fn validate_value(value: &str) -> Result<(), ModelError> {
    if value.trim().is_empty()
        || value.len() > MAX_SECRET_VALUE_BYTES
        || value.contains('\0')
        || value.contains(['\r', '\n'])
    {
        return Err(ModelError::invalid("model_credential_header_invalid"));
    }
    reqwest::header::HeaderValue::from_str(value)
        .map(|_| ())
        .map_err(|_| ModelError::invalid("model_credential_header_invalid"))
}

/// 从环境变量里取出一个密钥值，并做完整校验。
///
/// 【作用】 `EnvSecretStore` 的实际取数逻辑。
///
/// 【检查项】
/// - `store` 必须是 `"env"`（防止把一个 `file` 引用交给环境变量后端去读）。
/// - `key` 只能由 `[A-Z0-9_]` 组成 —— 这条白名单同时防住了路径穿越，
///   因为名字里根本没有 `/` 和 `.`。
/// - 变量必须存在且 trim 后非空。
/// - 取出后走 `validate_value`。
///
/// 【⚠ 这是一次真实的读取】 注意 `current_revision` 为了算摘要也会调到这里，
/// 也就是说**版本围栏的检查会真的去读环境变量**。这不是免费的，但换来的是
/// "环境变量在运行期被改掉"能被立刻发现。

fn env_value(secret_ref: &SecretRef) -> Result<String, ModelError> {
    if secret_ref.store != "env"
        || secret_ref.key.is_empty()
        || !secret_ref
            .key
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(ModelError::invalid("credential_secret_ref_invalid"));
    }
    let value = std::env::var(&secret_ref.key)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ModelError::invalid("model_credential_unavailable"))?;
    validate_value(&value)?;
    Ok(value)
}

/// 把一个校验过的密钥值组装成带租约的 `SecretMaterial`。
///
/// 【核心流程】 四步，顺序固定：
/// ```text
///   1. secret_ref.validate()      引用自身合法
///   2. validate_value(&value)     值能安全进 HTTP 头
///   3. CredentialLease::issue(...) 签发一次性租约，绑定五元组
///   4. 组装 SecretMaterial（顺带算 credential_revision 摘要）
/// ```
///
/// 【租约绑定的五元组，逐项解释】
/// ```text
///   secret_ref         哪把密钥
///   provider_account   哪个账号     -> 防止 A 账号的租约用在 B 账号上
///   PROVIDER_CREDENTIAL_PURPOSE  干什么用 -> 见上面的常量
///   audience           发给谁（服务商 id）
///   endpoint_digest    发到哪个端点
///   CREDENTIAL_LEASE_DEFAULT_TTL_MS  多久后失效
/// ```
/// 前四项加上时效，构成"这张租约只允许把**这把密钥发给这个端点**，且只在短时间内"。

fn materialize(
    secret_ref: &SecretRef,
    provider_account: &str,
    audience: &str,
    endpoint_digest: &str,
    now_unix_ms: u64,
    value: String,
) -> Result<SecretMaterial, ModelError> {
    secret_ref
        .validate()
        .map_err(|_| ModelError::invalid("credential_secret_ref_invalid"))?;
    validate_value(&value)?;
    let lease = CredentialLease::issue(
        secret_ref.clone(),
        provider_account,
        PROVIDER_CREDENTIAL_PURPOSE,
        audience,
        endpoint_digest,
        now_unix_ms,
        CREDENTIAL_LEASE_DEFAULT_TTL_MS,
    )
    .map_err(|_| ModelError::invalid("credential_lease_invalid"))?;
    Ok(SecretMaterial {
        lease,
        credential_revision: kiana_domain::json_digest(&serde_json::json!(&value)),
        value,
    })
}

/// Environment-backed adapter.  The environment name is an opaque `SecretRef.key`; the value
/// is looked up only when a request is about to be sent.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct EnvSecretStore;

impl SecretStore for EnvSecretStore {
    fn backend(&self) -> SecretBackend {
        SecretBackend::Env
    }

    fn current_revision(&self, secret_ref: &SecretRef) -> Result<String, ModelError> {
        let value = env_value(secret_ref)?;
        Ok(kiana_domain::json_digest(&serde_json::json!(&value)))
    }

    fn issue(
        &self,
        secret_ref: &SecretRef,
        provider_account: &str,
        audience: &str,

        endpoint_digest: &str,
        now_unix_ms: u64,
    ) -> Result<SecretMaterial, ModelError> {
        let value = env_value(secret_ref)?;
        materialize(
            secret_ref,
            provider_account,
            audience,
            endpoint_digest,
            now_unix_ms,
            value,
        )
    }
}

/// Explicit API-key compatibility adapter.  New callers should provide an env/keyring/file/OS
/// reference; this adapter keeps the existing `ProviderConfig.api_key` API working while the
/// value remains private to the provider connection and effect boundary.
#[derive(Clone, Debug)]
pub(crate) struct InlineSecretStore {
    value: Arc<String>,
    revision: String,
}

impl InlineSecretStore {
    pub(crate) fn new(value: String) -> Result<Self, ModelError> {
        validate_value(&value)?;
        Ok(Self {
            revision: kiana_domain::json_digest(&serde_json::json!(&value)),
            value: Arc::new(value),
        })
    }
}

impl SecretStore for InlineSecretStore {
    fn backend(&self) -> SecretBackend {
        SecretBackend::Inline
    }

    fn current_revision(&self, secret_ref: &SecretRef) -> Result<String, ModelError> {
        if secret_ref.store != "inline" {
            return Err(ModelError::invalid("credential_secret_ref_invalid"));
        }
        Ok(self.revision.clone())
    }

    fn issue(
        &self,
        secret_ref: &SecretRef,
        provider_account: &str,
        audience: &str,
        endpoint_digest: &str,
        now_unix_ms: u64,
    ) -> Result<SecretMaterial, ModelError> {
        if secret_ref.store != "inline" {
            return Err(ModelError::invalid("credential_secret_ref_invalid"));
        }
        materialize(
            secret_ref,
            provider_account,
            audience,
            endpoint_digest,
            now_unix_ms,
            self.value.as_str().to_owned(),
        )
    }
}

/// Keyring adapter placeholder.  It is intentionally explicit rather than silently falling
/// back to an environment variable or inline value.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct KeyringSecretStore;

impl SecretStore for KeyringSecretStore {
    fn backend(&self) -> SecretBackend {
        SecretBackend::Keyring
    }

    fn current_revision(&self, _secret_ref: &SecretRef) -> Result<String, ModelError> {
        Err(ModelError::invalid(
            "credential_backend_unsupported:keyring",
        ))
    }

    fn issue(
        &self,
        _secret_ref: &SecretRef,
        _provider_account: &str,
        _audience: &str,
        _endpoint_digest: &str,
        _now_unix_ms: u64,
    ) -> Result<SecretMaterial, ModelError> {
        Err(ModelError::invalid(
            "credential_backend_unsupported:keyring",
        ))
    }
}

/// File-backed adapter placeholder.  A future implementation must enforce ownership, mode and
/// symlink checks before it can be enabled.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FileSecretStore;

impl SecretStore for FileSecretStore {
    fn backend(&self) -> SecretBackend {
        SecretBackend::File
    }

    fn current_revision(&self, _secret_ref: &SecretRef) -> Result<String, ModelError> {
        Err(ModelError::invalid("credential_backend_unsupported:file"))
    }

    fn issue(
        &self,
        _secret_ref: &SecretRef,
        _provider_account: &str,
        _audience: &str,
        _endpoint_digest: &str,
        _now_unix_ms: u64,
    ) -> Result<SecretMaterial, ModelError> {
        Err(ModelError::invalid("credential_backend_unsupported:file"))
    }
}

/// OS credential adapter placeholder.  This prevents an unreviewed platform lookup from being
/// inferred from a `SecretRef.store` string.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct OsSecretStore;

impl SecretStore for OsSecretStore {
    fn backend(&self) -> SecretBackend {
        SecretBackend::Os
    }

    fn current_revision(&self, _secret_ref: &SecretRef) -> Result<String, ModelError> {
        Err(ModelError::invalid("credential_backend_unsupported:os"))
    }

    fn issue(
        &self,
        _secret_ref: &SecretRef,
        _provider_account: &str,
        _audience: &str,
        _endpoint_digest: &str,
        _now_unix_ms: u64,
    ) -> Result<SecretMaterial, ModelError> {
        Err(ModelError::invalid("credential_backend_unsupported:os"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_store_issues_and_consumes_a_bound_one_shot_lease() {
        let secret_ref = SecretRef::new(
            "inline",
            "fixture",
            PROVIDER_CREDENTIAL_PURPOSE,
            "fake-provider",
            1,
        )
        .expect("fixture ref");
        let store = InlineSecretStore::new("sentinel-ci07".to_owned()).expect("fixture value");
        assert_eq!(store.backend(), SecretBackend::Inline);
        let endpoint_digest =
            kiana_domain::json_digest(&serde_json::json!("https://provider.invalid/v1"));
        let mut material = store
            .issue(
                &secret_ref,
                "fake-account",
                "fake-provider",
                &endpoint_digest,
                1_000,
            )
            .expect("lease");
        material
            .lease
            .validate_for(
                1_001,
                "fake-account",
                PROVIDER_CREDENTIAL_PURPOSE,
                "fake-provider",
                &endpoint_digest,
            )
            .expect("binding");
        material.lease.consume(1_001).expect("consume");
        assert_eq!(
            material.lease.consume(1_002).unwrap_err(),
            "credential_lease_replayed"
        );
        assert_eq!(material.value, "sentinel-ci07");
    }

    #[test]
    fn protected_backend_placeholders_fail_closed() {
        let reference =
            SecretRef::new("keyring", "fixture", "purpose", "audience", 1).expect("fixture ref");
        let endpoint_digest =
            kiana_domain::json_digest(&serde_json::json!("https://provider.invalid/v1"));
        assert_eq!(
            KeyringSecretStore
                .issue(&reference, "account", "audience", &endpoint_digest, 1_000)
                .err()
                .expect("keyring denial")
                .code,
            "credential_backend_unsupported:keyring"
        );
        assert_eq!(
            FileSecretStore
                .issue(&reference, "account", "audience", &endpoint_digest, 1_000)
                .err()
                .expect("file denial")
                .code,
            "credential_backend_unsupported:file"
        );
        assert_eq!(
            OsSecretStore
                .issue(&reference, "account", "audience", &endpoint_digest, 1_000)
                .err()
                .expect("os denial")
                .code,
            "credential_backend_unsupported:os"
        );
    }
}
