//! HTTPS connector transport port.
//!
//! The port is below the ControlPlane and receives only a validated connector permit, opaque
//! credential lease, canonical payload and the domain-owned pinned-origin/DNS observation.  It
//! has no EventStore, approval or filesystem access and its default implementation is unsupported.
//!
//! # 这个文件在系统里的位置
//!
//! 这是 Kiana 允许**触达外部网络**的唯一一条合法通道 —— 而且它在控制面之下，
//! 只能被动接受已经过审的请求。
//!
//! ```text
//! 用户 / 模型 想访问某个外部 HTTPS 端点
//!        ↓
//!    kiana-core::ControlPlane     授权、审批、绑定端点、解析 DNS
//!        ↓  签发一张 ConnectorPreparedPermit（连接器许可）
//!        ↓  附带凭据租约（CredentialLease，不含密钥原文）、绑定快照、DNS 观测结果
//! 【本文件：ConnectorHttpsTransport】   ← 默认实现一律拒绝
//!        ↓  send_checked 重新校验一遍
//!    具体的 HTTPS 适配器（实现方）
//!        ↓
//!    外部服务商 API
//! ```
//!
//! # 三条不可让步的设计原则
//!
//! 本文件里几乎每一行代码都在服务这三条原则，理解它们就理解了整个文件：
//!
//! **① 默认实现是「拒绝」，不是「允许」。**
//! [`ConnectorHttpsTransport::send`] 的默认实现返回
//! `connector_https_transport_unsupported`；[`ConnectorHttpsTransportCapabilities`]
//! 的默认值四项全是 `false`。这意味着：**一个新增的适配器，哪怕还没写完，
//! 也无法被误用来发起真实网络请求。** 想让它工作，必须主动实现并显式声明能力。
//!
//! **② 适配器只能"声称"能力，声称本身要接受审查。**
//! 能力声明不是自证。`send_checked` 会先调 `capabilities.validate()`，
//! 三项安全能力（TLS 校验、DNS 固定、来源固定）缺一不可，
//! 否则整个请求被拒。适配器无法绕过这一步。
//!
//! **③ 校验在"效果发生时"重做一遍，不信任上游的结论。**
//! 即使控制面已经校验过，`send_checked` 仍然重新执行 `request.validate()`。
//! 原因是端口必须假设：**自己拿到的数据可能来自任何调用方**，
//! 只有自己在最后一刻确认过，才能保证"发出去的请求是合法的"。
//! 这是"纵深防御"（defense in depth）在接口层面的体现。
//!
//! # 术语
//!
//! - **pinned origin（来源固定）**：把连接锁死到某个确定的域名。
//!   浏览器用 HSTS 做这件事，这里用它防止 DNS 劫持和重定向到恶意主机。
//! - **DNS pinning（DNS 固定）**：预先解析并把 IP 固定下来，
//!   防止"检查时解析到安全 IP、实际连接时解析到恶意 IP"这种 TOCTOU 攻击。
//! - **TOCTOU（检查时间 / 使用时间不一致）**：先检查再用，中间状态可能已被改变。
//!   这是所有安全检查里最经典的漏洞形态。
//! - **Credential lease（凭据租约）**：一份**限时、限范围**的凭据使用许可。
//!   它本身不含密钥原文，只含"你可以用哪个凭据、到什么时候为止"。密钥在真正
//!   使用的那一刻才被解析出来。
//! - **redirect chain（重定向链）**：HTTP 3xx 响应带来的后续跳转地址序列。
//! - **fail-closed**：证据不足时拒绝，而非降级放行。
//!
//! # 上游契约
//!
//! The port is below the ControlPlane and receives only a validated connector permit, opaque
//! credential lease, canonical payload and the domain-owned pinned-origin/DNS observation.  It
//! has no EventStore, approval or filesystem access and its default implementation is unsupported.

use crate::{CanonicalConnectorPayload, ConnectorPreparedPermit, PortError};
use async_trait::async_trait;
use kiana_domain::{
    ConnectorBindingSnapshot, ConnectorHttpsPolicy, ConnectorHttpsResolution, CredentialLease,
    ProviderReceipt,
};
use serde::{Deserialize, Serialize};

/// 端口契约的 schema 标识符（端口本身）。
pub const CONNECTOR_HTTPS_PORT_SCHEMA: &str = "kiana.connector-https-port.v1";
/// 请求结构的 schema 标识符（每次请求自带，用于版本自检）。
pub const CONNECTOR_HTTPS_REQUEST_SCHEMA: &str = "kiana.connector-https-request.v1";

/// Capabilities describe transport guarantees; they do not grant an endpoint or account scope.
/// A concrete adapter must explicitly claim TLS verification, DNS pinning and origin pinning
/// before the checked wrapper will call it.
///
/// 【作用】
/// 描述一个具体 HTTPS 适配器**实际能保证什么**。这是适配器对端口的自我声明。
///
/// 【四个字段的语义】
///
/// - `tls_verified` —— 是否真的校验了服务端证书。填 `false` 意味着连接可能连到
///   中间人机器。
/// - `dns_pinned` —— 是否把 DNS 解析结果固定下来。填 `false` 意味着攻击者可以用
///   DNS 劫持把你的请求导向别处。
/// - `origin_pinned` —— 是否锁定来源域名。填 `false` 意味着一个合法证书
///   （比如给 example.com 签的）可能被用来把你引到 evil.com。
/// - `explicit_proxy` —— 是否**显式**支持代理。填 `false` 意味着这个适配器
///   无法表达"我确实要通过代理访问"这个需求。
///
/// 【为什么前三项是硬性要求】
/// 因为它们是 HTTPS 适配器存在的**最低门槛**。一个不校验 TLS、不固定 DNS、
/// 不固定来源的 HTTPS 实现，在安全上和明文 HTTP 差距不大。
/// 既然叫 HTTPS 端口，就不能接受一个把这些都关掉的实现。
///
/// 【重要澄清】
/// 这个结构体只描述**传输保证**，**不授予任何端点访问权或账号权限**。
/// 换句话说：一个适配器声称自己"TLS 校验做得很好"，不代表它有权访问某个特定
/// URL —— 那是 [`ConnectorPreparedPermit`] 的事。这两者是分开的：
/// 前者是"我做事安不安全"，后者是"我准不准做这件事"。
///
/// ⚠ 不要把这个结构体的 `true` 理解为授权。它只是能力声明，不是通行证。
///
/// 【serde 注解】
/// - `#[serde(deny_unknown_fields)]` —— 遇到未识别的字段直接报错。
///   为什么不宽容？因为一个适配器如果往这个结构里塞了端口不认识的字段，
///   说明双方版本错配了。**默默忽略**会让适配器以为自己的某项声明生效了，
///   而端口根本没看见 —— 这在安全声明上尤其危险。
/// - `Serialize` / `Deserialize` 来自 serde，用于 JSON 等格式的转换。
///   `Serialize` 是 Rust → 外部格式；`Deserialize` 是外部格式 → Rust。
///   这里两者都需要，因为这个结构体会被持久化，也会在适配器间传递。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorHttpsTransportCapabilities {
    pub tls_verified: bool,
    pub dns_pinned: bool,
    pub origin_pinned: bool,
    pub explicit_proxy: bool,
}

/// 【默认值为什么是全 `false`】
///
/// 这是本文件最重要的一行代码。它意味着**一个还没声明任何能力的适配器，
/// 默认会被 [`ConnectorHttpsTransportCapabilities::validate`] 拒绝**。
///
/// 换句话说：新增适配器时，忘记填能力字段的结果是"用不了"，
/// 而不是"悄悄在没安全保证的情况下跑起来"。**默认值站在安全那一侧。**
impl Default for ConnectorHttpsTransportCapabilities {
    fn default() -> Self {
        Self {
            tls_verified: false,
            dns_pinned: false,
            origin_pinned: false,
            explicit_proxy: false,
        }
    }
}

impl ConnectorHttpsTransportCapabilities {
    /// 校验能力声明是否达到端口要求的最低门槛。
    ///
    /// 【核心流程】
    /// 三个安全能力必须**同时**为真：TLS 校验、DNS 固定、来源固定。
    /// `explicit_proxy` 不在此列 —— 它是**可选能力**，在
    /// [`ConnectorHttpsTransport::send_checked`] 里按需单独检查。
    ///
    /// 【为什么用 `||` 连接三个否定条件，而不是 `&&` 连接三个肯定条件】
    /// `!a || !b || !c` 与 `a && b && c` 逻辑等价，但前者更短，且在代码审查时
    /// 更不容易看错 —— 后者容易被误读成"三个都要满足"，而这个仓库的不变量
    /// 恰恰就是"三个都必须满足"，所以两种写法都对，风格上这里更简洁。
    ///
    /// 【失败情况】
    /// 任一为假 → 返回 `PortError::Unavailable`，原因码
    /// `connector_https_transport_boundary_unsupported`。
    ///
    /// 注意返回的是 `Unavailable`（"不可用"）而不是 `Failed`（"失败"）：
    /// 这个适配器不是坏了，而是**它声明的能力不满足端口的契约**。
    /// 两者在上层可能触发不同的处理逻辑。
    pub fn validate(&self) -> Result<(), PortError> {
        if !self.tls_verified || !self.dns_pinned || !self.origin_pinned {
            return Err(PortError::Unavailable(
                "connector_https_transport_boundary_unsupported".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Effect-time request assembled after the ordinary connector admission path.  `proxy_origin`
/// is explicit and optional; an ambient proxy is never represented and therefore cannot be
/// silently selected by an implementation.
///
/// 【作用】
/// 一次**准备就绪、可以发出去**的 HTTPS 请求。它在控制面完成授权之后才被组装出来。
///
/// 【为什么请求要自己带 `schema` 字段】
/// 因为这个结构体会被序列化、存储、跨进程传递。带着版本号，
/// 接收方才能在解析时确认"我拿到的是哪一版格式"。
/// 如果格式不匹配，[`ConnectorHttpsRequest::validate`] 第一件事就是拒绝。
/// 这比"解析时尽力而为"安全得多 —— 后者在字段增删后会静默产生错误语义。
///
/// 【字段的来源】
/// 全部来自控制面，没有一个是适配器自己填的：
/// - `permit`：控制面签发的许可（证明"这次访问被授权过"）
/// - `binding`：端点绑定快照（证明"访问的是哪个端点"）
/// - `lease`：凭据租约（不含密钥原文）
/// - `payload`：规范化后的请求体
/// - `policy`：该端点的 HTTPS 策略（重定向上限、固定来源等）
/// - `resolution`：DNS 解析观测结果
///
/// 【`proxy_origin` 为什么是 `Option` 且有特殊默认值】
///
/// `#[serde(default, skip_serializing_if = "Option::is_none")]`：
/// - `default` —— 反序列化时字段缺失则填 `None`，所以老版本产生的数据能读；
/// - `skip_serializing_if` —— 序列化为 `None` 时**整个字段都不写出来**，
///   保持报文精简。
///
/// 更重要的是那句注释："an ambient proxy is never represented and therefore
/// cannot be silently selected by an implementation"（环境代理从不被表达，
/// 因此实现方不可能静默地选中它）。
///
/// 含义是：这里**没有"跟随系统环境变量"这种选项**。要么调用方显式给一个
/// 代理地址，要么没有代理。如果允许适配器去读 `HTTP_PROXY` 之类的环境变量，
/// 一个恶意的环境配置就能把所有 HTTPS 流量导向攻击者服务器，而请求报文里
/// 看不到任何异常。显式化把这条路彻底堵死了。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorHttpsRequest {
    pub schema: String,
    pub permit: ConnectorPreparedPermit,
    pub binding: ConnectorBindingSnapshot,
    pub lease: CredentialLease,
    pub payload: CanonicalConnectorPayload,
    pub policy: ConnectorHttpsPolicy,
    pub endpoint: kiana_domain::ConnectorHttpsEndpoint,
    pub resolution: ConnectorHttpsResolution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_origin: Option<String>,
    /// 已经发生过的重定向地址序列。
    ///
    /// `#[serde(default)]` 让字段缺失时得到空列表，兼容不记录重定向的旧格式。
    #[serde(default)]
    pub redirects: Vec<kiana_domain::ConnectorHttpsEndpoint>,
}

impl ConnectorHttpsRequest {
    /// 在**真正发出请求之前**把这份请求从头校验一遍。
    ///
    /// 【为什么控制面已经校验过了，这里还要再校验】
    /// 因为端口不能假设调用方一定是控制面。控制面的准入（admission）发生在更早的时点，
    /// 而网络请求发出前可能已经过了很久，中间状态可能已变（端点被改策略、
    /// 凭据租约到期、重定向链被追加）。**在最后使用点重新确认**，
    /// 才能保证发出去的一定是当前仍然合法的请求。
    ///
    /// 【核心流程】七道检查，顺序有讲究：
    ///
    /// 1. **schema 版本匹配** —— 格式不对立即拒绝。
    /// 2. **策略自身合法性** —— `policy.validate()`，先确认策略本身没被篡改。
    /// 3. **许可与本次请求匹配** —— `permit.validate_for_binding()`。
    ///    关键点：许可必须绑定到**这一个**端点、**这一份** payload、**这一张**凭据。
    ///    防止"用 A 端点的许可去访问 B 端点"这种盗用。
    /// 4. **端点在策略允许范围内** —— `policy.validate_endpoint()`。
    /// 5. **DNS 解析结果与策略一致** —— `policy.validate_resolution()`。
    ///    这一步防的是 DNS 劫持：如果你固定了 IP，这里确认解析结果没被改。
    /// 6. **代理在允许列表内** —— `policy.validate_proxy()`。
    /// 7. **重定向链合法** —— 逐跳检查，见下。
    ///
    /// 【第 7 步为什么单独讲】
    /// 重定向是最容易被利用来绕过检查的地方。攻击者的思路是：
    /// 你校验了 `https://api.example.com`（安全），
    /// 它返回 302 指向 `https://evil.com`（不安全），而有些 HTTP 客户端
    /// 会**自动跟随**重定向，你的校验就白做了。
    ///
    /// 所以这里做了三重检查：
    /// - **数量上限**：`redirects.len() > policy.max_redirects` 就拒绝，
    ///   防止无限重定向（也防止用重定向做放大攻击）。
    /// - **每一跳都校验端点**：`policy.validate_endpoint(redirect)`，
    ///   每一跳都要单独过策略。
    /// - **来源必须保持不变**：`redirect.origin != previous.origin ||
    ///   redirect.origin != self.policy.pinned_origin` —— 两跳比较：
    ///   ① 相对上一跳，来源没变（不能中途换主机）；
    ///   ② 等于固定的来源（不能跳出你最初允许的那个域）。
    ///
    /// ⚠ 不要因为"第一跳检查过了"就跳过后续跳。重定向是可以多跳的，
    /// 只检查第一跳等于给攻击者留了一扇门。
    ///
    /// 【失败情况】
    /// 任何一项不满足都返回 `Err`。
    /// 注意 `map_err` 把策略层的错误统一转成 `PortError::Conflict`，
    /// 保留了原始原因码（`error.code()`）—— 这样上层能区分
    /// "端点不对"和"DNS 不对"这类不同原因。
    ///
    /// 【副作用】
    /// 无。这是一个纯校验函数，不修改自身也不碰外部状态。
    pub fn validate(&self) -> Result<(), PortError> {
        if self.schema != CONNECTOR_HTTPS_REQUEST_SCHEMA {
            return Err(PortError::Failed(
                "connector_https_request_schema_invalid".to_owned(),
            ));
        }
        self.policy
            .validate()
            .map_err(|error| PortError::Conflict(error.code().to_owned()))?;
        self.permit
            .validate_for_binding(&self.binding, &self.payload, &self.lease)?;
        self.policy
            .validate_endpoint(&self.endpoint)
            .map_err(|error| PortError::Conflict(error.code().to_owned()))?;
        self.policy
            .validate_resolution(&self.endpoint, &self.resolution)
            .map_err(|error| PortError::Conflict(error.code().to_owned()))?;
        self.policy
            .validate_proxy(self.proxy_origin.as_deref())
            .map_err(|error| PortError::Conflict(error.code().to_owned()))?;
        if self.redirects.len() > self.policy.max_redirects as usize {
            return Err(PortError::Conflict(
                "connector_https_redirect_limit_exceeded".to_owned(),
            ));
        }
        // 逐跳校验重定向链。`previous` 初始为主端点，
        // 于是第一跳就是"与主端点比较"，之后每跳与前一跳比较。
        let mut previous = self.endpoint.clone();
        for redirect in &self.redirects {
            self.policy
                .validate_endpoint(redirect)
                .map_err(|error| PortError::Conflict(error.code().to_owned()))?;
            // 两个条件用 || 连接：只要"不等于上一跳"或"不等于固定来源"任一成立就拒绝。
            // 注意 `!= previous.origin` 堵死了中途换主机的可能，
            // `!= self.policy.pinned_origin` 堵死了跳出初始域名的可能。
            if redirect.origin != previous.origin || redirect.origin != self.policy.pinned_origin {
                return Err(PortError::Conflict(
                    "connector_https_redirect_origin_denied".to_owned(),
                ));
            }
            previous = redirect.clone();
        }
        Ok(())
    }
}

/// A concrete HTTPS adapter may implement this port, but it cannot add an authorization path or
/// dispatch before `send_checked` has revalidated the permit, lease, endpoint, DNS observation,
/// proxy allowlist and redirect chain.
///
/// 【作用】
/// 具体的 HTTPS 适配器需要实现的接口。
///
/// 【最重要的设计：这是一个"只读不写"的 trait】
/// 实现方能提供的能力只有两个：声明自己会什么（[`capabilities`]）、
/// 以及在通过检查后发请求（[`send`]）。它**不能**：
///
/// - 走一条自己的授权路径（授权只由控制面做）；
/// - 在检查之前就派发请求（必须经由 [`send_checked`]）；
/// - 改写许可、租约、端点或校验结果。
///
/// 换句话说，**实现方的权力被限制在"如何安全地发一个已经获批的请求"，
/// 它无权决定"这个请求该不该发"。**
#[async_trait]
pub trait ConnectorHttpsTransport: Send + Sync {
    /// 声明本适配器的传输能力。
    ///
    /// 【默认实现】
    /// 返回 [`ConnectorHttpsTransportCapabilities::default()`，即**全部为假**。
    ///
    /// 这意味着一个实现了本 trait 但没覆盖此方法的适配器，
    /// 会在 [`send_checked`] 的第一道 `capabilities.validate()` 就被拒。
    /// 这就是"忘记声明 = 用不了"，而不是"忘记声明 = 默认不安全地运行"。
    fn capabilities(&self) -> ConnectorHttpsTransportCapabilities {
        ConnectorHttpsTransportCapabilities::default()
    }

    /// 真正发出 HTTPS 请求。
    ///
    /// 【默认实现】
    /// 返回 `connector_https_transport_unsupported`。
    ///
    /// ⚠ 这个方法**不校验任何东西**，它假定请求已经被 [`send_checked`] 校验过了。
    ///
    /// 所以：**绝不要直接调用 `send`。** 唯一合法的调用路径是 `send_checked`
    /// 在校验完成后调用它。绕过 `send_checked` 就等于跳过了本文件里
    /// 全部七道安全检查。
    ///
    /// 【为什么保留一个公开的 `send`】
    /// 因为这是 trait 的一部分，适配器必须能覆盖它。
    /// 风险控制手段不是"不暴露它"，而是"默认实现拒绝 +
    /// [`send_checked`] 只在通过检查后才调它"。
    /// 如果把 `send` 设计成私有方法，trait 就无法被外部实现。
    async fn send(&self, _request: ConnectorHttpsRequest) -> Result<ProviderReceipt, PortError> {
        Err(PortError::Unavailable(
            "connector_https_transport_unsupported".to_owned(),
        ))
    }

    /// 经过完整校验后发送请求 —— **这是适配器对外的唯一合法入口**。
    ///
    /// 【核心流程】五道关卡，全部通过才会真正发出请求：
    ///
    /// ```text
    /// 1. capabilities.validate()          适配器声明了必需的能力吗？
    /// 2. request.validate()                请求本身通过全部七项校验吗？
    /// 3. 代理一致性检查                     请求要代理，但适配器不支持代理？
    /// 4. self.send(request)                真正发送
    /// 5. validate_receipt_for_permit()     回执与许可对得上吗？
    /// ```
    ///
    /// 【第 3 步为什么单独存在】
    /// `explicit_proxy` 不在 `capabilities.validate()` 的必检三项里，
    /// 因为不是所有请求都需要代理。但**如果这一次请求指定了代理**
    /// （`request.proxy_origin.is_some()`），而适配器又没声明支持代理
    /// （`!capabilities.explicit_proxy`），那就必须拒绝。
    ///
    /// 为什么不直接忽略代理设置？因为适配器会按自己的方式（比如读环境变量）
    /// 建立连接，调用方以为走了指定代理、实际可能走了别的路 —— 或者根本没走成。
    /// **显式不匹配时宁可失败，也不静默降级。**
    ///
    /// 【第 5 步：为什么发完还要校验回执】
    /// 因为请求发出去不等于拿到的是对的东西。适配器返回的
    /// [`ProviderReceipt`]（服务商回执）必须和这次的许可对得上：
    /// 是不是同一个端点、是不是同一次授权的产物。
    ///
    /// 防的是这种情况：适配器内部有个缓存或连接复用，
    /// 把上一次请求的响应当成这次的返回了。没有这一步，
    /// 上层会把一个不匹配的响应当成有效证据记进事件账本。
    ///
    /// 【为什么先 clone 再 send】
    /// `let binding = request.binding.clone(); let permit = request.permit.clone();`
    /// —— 因为 `send` 会**消耗掉** `request`（按值传入），
    /// 而后面第 5 步还需要 binding 和 permit 来做比对。
    /// 在所有权被移走之前先把需要的部分取出来。这两行看着多余，
    /// 去掉会编译不过 —— 编译器在这里帮了忙。
    ///
    /// 【失败情况】
    /// 任一关卡不通过即返回 `Err`，**请求不会发出**。
    /// 注意第 4 步的 `?` —— 如果 `send` 失败，函数在此返回，
    /// 不会执行第 5 步的校验（本来也没有回执可校验）。
    async fn send_checked(
        &self,
        request: ConnectorHttpsRequest,
    ) -> Result<ProviderReceipt, PortError> {
        // 关卡 1：适配器是否声明了必需的安全能力。
        let capabilities = self.capabilities();
        capabilities.validate()?;
        // 关卡 2：请求本身的七项校验（schema / 策略 / 许可 / 端点 / DNS / 代理 / 重定向）。
        request.validate()?;
        // 关卡 3：本次请求需要代理，但适配器没声明支持 —— 显式失败而非静默忽略。
        if request.proxy_origin.is_some() && !capabilities.explicit_proxy {
            return Err(PortError::Unavailable(
                "connector_https_proxy_transport_unsupported".to_owned(),
            ));
        }
        // 在 request 被移走之前，先取出校验回执所需的两个部分。
        let binding = request.binding.clone();
        let permit = request.permit.clone();
        // 关卡 4：真正发送。只有前三关都过了才走到这里。
        let receipt = self.send(request).await?;
        // 关卡 5：回执必须与本次许可匹配，防止响应被张冠李戴。
        crate::connector::validate_receipt_for_permit(&receipt, &binding, &permit)
            .map_err(|error| PortError::Conflict(error.to_string()))?;
        Ok(receipt)
    }
}
