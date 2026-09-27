//! Pure ProjectTrust root resolution for Skills, Plugins, Hooks and MCP resources.
//!
//! The policy layer does not inspect paths or load resources. An adapter supplies canonical root
//! and project digests, and this module selects a same-scope trust record before any loader may
//! read a project-local resource. Unknown, conflicting and untrusted roots are deny decisions.
//!
//! # ⚠ 当前状态：尚未接入生产路径
//!
//! 这一点必须先说清楚，否则下面的说明会给人错误印象。
//!
//! 本文件定义的 [`ProjectTrustResolution`] / [`ProjectTrustRoot`] 目前
//! **只出现在本文件自己的单元测试和 `kiana-policy/tests/` 下的两个测试文件里**，
//! 没有任何生产代码路径调用它。
//!
//! 实际生效的信任判定走的是另一条路：
//! `kiana-daemon` 调用 `kiana-skills` 的 `load_all_skills_with_trust`，
//! 使用 `kiana_skills::SourceTrust` 这个类型。
//!
//! 所以本文件现在是一份**已实现、已测试、但未接线**的策略值类型。
//! 它的规则是对的、测试是过的，但还没有接到真正的加载路径上。
//!
//! 这不改变下面那些设计说明的价值 —— 读懂它有助于理解信任模型的整体设计，
//! 也为将来接线提供参考。但不要以为"现在加载 skill 会走这里"。
//!
//! # 这个文件在系统里的位置（设计意图）
//!
//! 本文件是**项目本地资源的守门人**。Kiana 允许项目自带 skill、plugin、hook、
//! MCP 配置 —— 它们放在 `.claude/skills`、`.kiana/plugins` 之类的目录里。
//!
//! 为什么要管这么严？因为**这些文件等同于代码**：
//! 一个 skill 能写任意指令，一个 plugin 能注册工具，一个 hook 能在命令前后自动执行代码。
//! 克隆一个陌生仓库，就等于接受了它作者写下的任意逻辑。
//! 所以在**任何加载器读取这些资源之前**，必须先过信任检查。
//!
//! ```text
//! 某处决定"要加载项目里的 skills"
//!        ↓ 适配器提供：规范化的根路径摘要 + 项目根摘要
//! 【本文件：ProjectTrustResolution::resolve】   ← 纯判定，不碰文件系统
//!        ↓ 返回 Allow / Deny + 原因
//! 加载器：只有拿到 Allow 才敢读 .claude/skills 里的文件
//! ```
//!
//! # 三个关键设计
//!
//! **① 纯函数：只做判定，不做 I/O**
//! 文件头说 "The policy layer does not inspect paths or load resources" ——
//! 本文件**从不访问文件系统**，也从不加载任何资源。
//! 路径检查和资源加载是适配器的事；本文件只接收"已经算好的摘要"，
//! 然后做纯逻辑判定。
//!
//! 为什么这样分？因为信任判定必须**可以被独立测试**。
//! 如果它自己去读路径，测试就需要真的建目录、造文件；
//! 而且文件系统状态难以复现，出问题时无法判断是判定错了还是环境变了。
//! 纯函数版本只需要构造输入，就能覆盖所有边界情况。
//!
//! **② 摘要代替真实路径**
//! 所有的根都用 `project_root_digest`（项目根摘要）和 `root_digest`（资源根摘要）
//! 表示，**不是真实路径字符串**。
//!
//! 这有两个好处：一是路径本身可能含敏感信息，进摘要后不泄漏到事件日志；
//! 二是路径可能被软链接、同形字符等方式欺骗，
//! 而"先规范化再摘要"的流程由适配器保证，判定层拿到的已经是唯一的身份。
//!
//! **③ 三种"不确定"都变成 Deny**
//! 文件头最后一句 "Unknown, conflicting and untrusted roots are deny decisions"。
//!
//! | 情况 | 原因码 | 含义 |
//! |---|---|---|
//! | 找不到信任记录 | `project_trust_root_missing` | 没人声明过这个项目可不可信 |
//! | 同一修订号下状态冲突 | `project_trust_conflict` | 有人同时说"可信"和"不可信" |
//! | 状态是 Untrusted | `project_trust_untrusted` | 明确标记为不可信 |
//! | 状态是 Unknown | `project_trust_unknown` | 状态无法确定 |
//!
//! ⚠ 特别注意 `Unknown` 也被拒绝。直觉上"不知道"可能是"没关系吧"，
//! 但在信任边界上，不知道就意味着不能执行 —— 这就是 fail-closed。
//!
//! # 术语
//!
//! - **scope（信任域）**：信任记录的来源层级，见 [`ProjectTrustScope`]。
//! - **root（根）**：一个可被信任判定覆盖的资源根目录。
//! - **revision（修订号）**：同一条信任记录的版本，每次变更递增。
//! - **audit_ref（审计引用）**：脱敏后的来源标记，**不是**真实路径。
//! - **fail-closed**：不确定就拒绝。
//!
//! # 上游契约
//!
//! The policy layer does not inspect paths or load resources. An adapter supplies canonical root
//! and project digests, and this module selects a same-scope trust record before any loader may
//! read a project-local resource. Unknown, conflicting and untrusted roots are deny decisions.

use kiana_domain::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

/// 信任根记录的 schema 标识符。
/// 信任根记录的 schema 标识符。
pub const PROJECT_TRUST_ROOT_SCHEMA: &str = "kiana.project-trust-root.v1";
/// 信任判定结果的 schema 标识符。
/// 信任判定结果的 schema 标识符。
pub const PROJECT_TRUST_RESOLUTION_SCHEMA: &str = "kiana.project-trust-resolution.v1";
/// 信任策略版本。
/// 信任策略版本。
pub const PROJECT_TRUST_POLICY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 一次判定最多能带多少个候选信任根。
///
/// 【为什么是 64】
/// 正常情况下候选只有 0–2 条。上限 64 是给"配置写错导致堆积"留的余量，
/// 同时防止有人构造几万个候选来做暴力匹配 —— 那会变成一个放大攻击面。
/// 一次判定最多能带多少个候选信任根。
///
/// 【为什么是 64】
/// 正常情况下候选只有 0–2 条。上限 64 是给"配置写错导致堆积"留的余量，
/// 同时防止有人构造几万个候选来做暴力匹配 —— 那会变成一个放大攻击面。
pub const MAX_PROJECT_TRUST_ROOTS: usize = 64;

/// 校验一个字符串字段：非空、不超长、不含空字节。
///
/// 【作用】
/// 三合一的基础字段检查。三个条件各防一类问题：
/// - 空（含纯空白）—— 必填字段没填；
/// - 超长 —— 防止超长字符串撑爆存储或日志；
/// - 含空字节（`\0`）—— **防止路径截断攻击**。
///
/// 【⚠ 为什么要专门拦空字节】
/// 空字节是 C 字符串的终止符。如果路径被传给某个底层 C 库，
/// `"safe/path\0/../../etc/passwd"` 会被截断成 `"safe/path"` 通过检查，
/// 而实际访问的却是后面的内容。
/// 虽然 Rust 字符串内部没有这个问题，但只要数据可能流向 FFI 或外部工具，
/// 就应该在数据入口拦住。
///
/// 【⚠ 为什么用 `value.len()` 而不是字符数】
/// `len()` 返回**字节数**。这对"防止超长"是更严格的检查 ——
/// 一个多字节字符算 2–4 个字节。
/// 用字节计数不会切坏合法文本，因为 UTF-8 的字符边界不会落在多字节序列中间。
///
/// 【失败情况】
/// 返回 `Err("{field}_invalid")`，字段名由调用方传入，便于定位是哪个字段出问题。
/// 校验一个字符串字段：非空、不超长、不含空字节。
///
/// 【作用】
/// 三个条件各防一类问题：
/// - 空（含纯空白）—— 必填字段没填；
/// - 超长 —— 防止超长字符串撑爆存储或日志；
/// - 含空字节（`\0`）—— **防止路径截断攻击**。
///
/// 【⚠ 为什么要专门拦空字节】
/// 空字节是 C 字符串的终止符。如果路径被传给某个底层 C 库，
/// `"safe/path\0/../../etc/passwd"` 会被截断成 `"safe/path"` 通过检查，
/// 而实际访问的却是后面的内容。
/// 虽然 Rust 字符串内部没有这个问题，但只要数据可能流向 FFI 或外部工具，
/// 就应该在数据入口拦住。
///
/// 【⚠ 为什么用 `value.len()` 而不是字符数】
/// `len()` 返回**字节数**。这对"防止超长"是更严格的检查 ——
/// 一个多字节字符算 2–4 个字节。
/// 用字节计数不会切坏合法文本，因为 UTF-8 的字符边界不会落在多字节序列中间。
///
/// 【失败情况】
/// 返回 `Err("{field}_invalid")`，字段名由调用方传入，
/// 便于定位到底是哪个字段出了问题。
fn nonempty(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 校验一个摘要字符串的**格式**。
///
/// 【作用】
/// 只查格式（`sha256:` 前缀 + 64 位十六进制），不查内容。
/// 内容比对由调用方重算摘要后进行。
///
/// 【为什么固定 64 个字符】
/// SHA-256 输出 32 字节，十六进制表示正好 64 个字符。
/// 长度不符就说明这不是一个 SHA-256 摘要。
///
/// 【与 `grant_scope.rs` 里的同名函数的关系】
/// 两个文件各有一份 `valid_digest`，逻辑相同。
/// 这不是疏忽 —— `kiana-policy` 内部不共用工具函数，
/// 每个模块自带校验器是这里的既有风格。修改时两处都要看。
/// 校验一个摘要字符串的**格式**。
///
/// 【作用】
/// 只查格式（`sha256:` 前缀 + 64 位十六进制），不查内容。
/// 内容比对由调用方重算摘要之后进行。
///
/// 【为什么固定 64 个字符】
/// SHA-256 输出 32 字节，十六进制表示正好 64 个字符。
/// 长度不符就说明这不是一个 SHA-256 摘要。
///
/// 【与 `grant_scope.rs` 里的同名函数的关系】
/// 两个文件各有一份 `valid_digest`，逻辑完全相同。
/// 这不是疏忽 —— `kiana-policy` 内部不共用工具函数，
/// 每个模块自带校验器是这里的既有风格。修改时两处都要看。
fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 信任的三个来源层级。
///
/// 【作用】
/// 区分"信任声明来自哪里"。三个层级的权限大小不同，
/// 详见 [`ProjectTrustScope::priority`]。
///
/// 【三个层级的实际含义】
///
/// - `User` —— 用户级。指用户自己配置的目录（比如全局 `~/.claude/skills`）。
///   这层的内容由用户亲自放置，信任度最高。
/// - `KianaHome` —— Kiana 自身目录。指 `KIANA_HOME` 环境变量指向的目录。
/// - `Project` —— 项目级。指当前工作目录里的 `.claude/skills`、`.kiana/plugins`。
///   ⚠ **这层最危险**，因为它随仓库一起被克隆下来 ——
///   克隆一个仓库就等于接受了它自带的指令和能力。
///
/// 【⚠ 为什么这三层不能互相替代】
/// 用户级的信任**不能**用来放行项目级的资源。
/// 否则攻击者只要让某个项目落在用户级目录下，就能借用户级的信任获得执行权。
/// 每层必须有自己的信任记录 —— 见 [`ProjectTrustResolution::resolve`]
/// 里 `root.scope == requested_scope` 这个过滤条件。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTrustScope {
    User,
    KianaHome,
    Project,
}

impl ProjectTrustScope {
    /// 各级别的优先级数值，**数值越大优先级越高**。
    ///
    /// 【为什么项目级优先级最高（30 > 20 > 10）】
    /// 这里的优先级只在**同一次判定的多条候选之间**用于排序
    /// （见 [`ProjectTrustResolution::resolve`] 的排序逻辑）。
    /// 由于候选已经被过滤成同一个 scope，这个字段实际上极少起决定作用。
    ///
    /// ⚠ 但它的**方向**很重要：它表达的是"更贴近当前上下文的层级优先"。
    /// 项目级资源离用户当前操作最近，所以给它最高数值。
    /// 如果将来放宽了 scope 过滤（允许跨 scope 比较），这个方向就是对的。
    ///
    /// 【为什么是 10/20/30 而不是 0/1/2】
    /// 留出间隔是为了将来插入新层级时不必重排。
    /// 用 0 起步会让"最小值"和"未设置"难以区分。
    ///
    /// 【`const fn` 的含义】
    /// `const fn` 可以在编译期求值，调用它不产生任何运行时代码。
    /// 这类纯查表函数标注 `const` 是恰当的。
    /// 各级别的优先级数值，**数值越大优先级越高**。
    ///
    /// 【为什么项目级优先级最高（30 > 20 > 10）】
    /// 这里的优先级只在**同一次判定的多条候选之间**用于排序
    /// （见 [`ProjectTrustResolution::resolve`] 的排序逻辑）。
    /// 由于候选已经被过滤成同一个 scope，这个字段实际上极少起决定作用。
    ///
    /// ⚠ 但它的**方向**很重要：它表达的是"更贴近当前上下文的层级优先"。
    /// 项目级资源离用户当前操作最近，所以给它最高数值。
    /// 如果将来放宽了 scope 过滤（允许跨 scope 比较），这个方向就是对的。
    ///
    /// 【为什么是 10/20/30 而不是 0/1/2】
    /// 留出间隔是为了将来插入新层级时不必重排。
    /// 用 0 起步会让"最小值"和"未设置"难以区分。
    ///
    /// 【`const fn` 的含义】
    /// `const fn` 可以在编译期求值，调用它不产生任何运行时代码。
    /// 这类纯查表函数标注 `const` 是恰当的。
    pub const fn priority(self) -> u8 {
        match self {
            Self::User => 10,
            Self::KianaHome => 20,
            Self::Project => 30,
        }
    }
}

/// 一条信任根的当前状态。
///
/// 【作用】
/// 描述"这个根现在算不算可信"。三个状态里**只有 `Trusted` 会放行**。
///
/// 【⚠ `Unknown` 和 `Untrusted` 的区别】
/// 结果一样（都拒绝），但语义不同，回给人看的提示也不同：
/// - `Untrusted`：有人明确说过"这个不可信"—— 是**一个决定**；
/// - `Unknown`：没人做过决定 —— 是**没有决定**。
///
/// 区分它们的意义在于：出现 `Unknown` 时，正确的反应是去**建立信任**
/// （比如用户执行 `kiana trust .`）；而 `Untrusted` 意味着有人明确拒绝过，
/// 不该反复重试。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTrustState {
    /// 明确可信 —— 只有这一种状态会放行。
    Trusted,
    /// 明确不可信。
    Untrusted,
    /// 状态未知 —— 仍然拒绝，但含义是"还没人做过决定"。
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTrustDecision {
    Allow,
    Deny,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一条信任根记录 —— "某个根，在这个项目里，当前是什么状态"。
///
/// 【字段分组】
/// - **身份**：`scope`（哪一层）、`project_root_digest`（哪个项目）、`root_digest`（哪个根）。
/// - **状态**：`state`（可信/不可信/未知）、`revision`（第几次修改）。
/// - **审计**：`audit_ref`、`root_record_digest`。
///
/// 【⚠ `audit_ref` 绝对不能是真实路径】
/// 字段注释里写得很明确："never a raw path or trust-file payload"
/// （绝不是原始路径，也不是信任文件的原始内容）。
/// 它只是一个**脱敏后的引用标记**，
/// 让人能追溯"这条信任是谁、在什么时候、通过什么方式建立的"，
/// 但不会把文件系统布局泄漏进事件日志。
/// 如果这里填了真实路径，一个项目的目录结构就可能被记录并传播。
pub struct ProjectTrustRoot {
    pub schema: String,
    pub version: SchemaVersion,
    pub scope: ProjectTrustScope,
    pub project_root_digest: String,
    pub root_digest: String,
    pub revision: u64,
    pub state: ProjectTrustState,
    /// Stable redacted source/audit reference; never a raw path or trust-file payload.
    pub audit_ref: String,
    pub root_record_digest: String,
}

impl ProjectTrustRoot {
    /// 构造一条信任根记录并校验。
    ///
    /// 【⚠ 顺序：先算摘要，再校验】
    /// 摘要赋值必须在 `validate()` 之前，
    /// 因为校验里有一项是"存储的摘要 vs 重算的摘要"的比对。
    ///
    /// 【失败情况】
    /// 校验不通过返回对应的 `Err`，字段级问题会指明具体字段。
    pub fn new(
        scope: ProjectTrustScope,
        project_root_digest: impl Into<String>,
        root_digest: impl Into<String>,
        revision: u64,
        state: ProjectTrustState,
        audit_ref: impl Into<String>,
    ) -> Result<Self, String> {
        let mut root = Self {
            schema: PROJECT_TRUST_ROOT_SCHEMA.to_owned(),
            version: PROJECT_TRUST_POLICY_VERSION,
            scope,
            project_root_digest: project_root_digest.into(),
            root_digest: root_digest.into(),
            revision,
            state,
            audit_ref: audit_ref.into(),
            root_record_digest: String::new(),
        };
        root.root_record_digest = root.digest();
        root.validate()?;
        Ok(root)
    }

    /// 校验这条信任根记录自身是否合法。
    ///
    /// 【核心检查 —— 四组】
    /// **① 头部**：schema 精确匹配、版本兼容、`revision` 非 0。
    /// （`revision` 为 0 意味着"这条记录从未被正式写入过"，
    /// 不是一条有效的信任声明。）
    ///
    /// **② 两个摘要的格式**
    /// `project_root_digest` 和 `root_digest` 都必须是合法的 SHA-256 格式。
    ///
    /// **③ `audit_ref`**
    /// 有内容、不超 256 字节、无控制字符。
    ///
    /// **④ 摘要自洽**
    /// 重算摘要与存储值比对。防的是**内容被改**：
    /// 有人拿到一条合法的 Trusted 记录，把 `state` 改成 Untrusted 再改回去，
    /// 或者反过来 —— 摘要对不上，改动被发现。
    ///
    /// 【⚠ 不要给这里的检查"精简"掉】
    /// 这个函数看起来分支很多，容易让人想合并。
    /// 但每组检查都对应一类真实问题。删掉任何一组都会开出一个静默的漏洞。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROJECT_TRUST_ROOT_SCHEMA
            || !self
                .version
                .is_compatible_with(&PROJECT_TRUST_POLICY_VERSION)
            || self.revision == 0
        {
            return Err("project_trust_root_header_invalid".to_owned());
        }
        valid_digest(&self.project_root_digest, "project_trust_project_digest")?;
        valid_digest(&self.root_digest, "project_trust_root_digest")?;
        nonempty(&self.audit_ref, "project_trust_audit_ref", 256)?;
        valid_digest(&self.root_record_digest, "project_trust_record_digest")?;
        if self.root_record_digest != self.digest() {
            return Err("project_trust_root_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算这条信任根记录的内容摘要。
    ///
    /// 【⚠ 字段清单就是这条记录的身份】
    /// 把字段加进或移出这个 `json!`，
    /// 会让所有已持久化的信任记录校验失败。
    /// 摘要字段自身不参与计算（否则要算自己，无解）。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "scope": self.scope,
            "project_root_digest": self.project_root_digest,
            "root_digest": self.root_digest,
            "revision": self.revision,
            "state": self.state,
            "audit_ref": self.audit_ref,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一次信任判定的完整结果。
///
/// 【作用】
/// 不只是给出一个 Allow/Deny，
/// 还记录了**判定过程本身**：有哪些候选、选了哪条、结论是什么、原因是什么。
/// 这份记录会进事件账本，事后可以完整复盘"当时为什么放行/拒绝"。
///
/// 【为什么 `selected_scope` 和 `selected_root_digest` 是 `Option`】
/// 因为**拒绝时可能根本没有选中任何一条**
/// （比如找不到任何候选记录，或候选之间状态冲突）。
/// 用 `Option` 诚实地表达"没有选中"，
/// 而不是拿一个空字符串或零值假装选过 ——
/// 后者会让审计时误以为"选中了某条记录"。
pub struct ProjectTrustResolution {
    pub schema: String,
    pub version: SchemaVersion,
    pub project_root_digest: String,
    pub requested_scope: ProjectTrustScope,
    pub candidates: Vec<ProjectTrustRoot>,
    pub selected_scope: Option<ProjectTrustScope>,
    pub selected_root_digest: Option<String>,
    pub decision: ProjectTrustDecision,
    pub reason: String,
    pub resolution_digest: String,
}

impl ProjectTrustResolution {
    /// 执行信任判定 —— 本文件的核心。
    ///
    /// 【核心流程 —— 四步】
    ///
    /// **① 校验输入与候选**
    /// - 项目摘要格式非法 → 报错；
    /// - 候选数超过 64 → 报错（`project_trust_root_limit`）；
    /// - 每条候选各自校验。
    ///
    /// **② 按 scope 和项目过滤**
    /// 只保留 `root.scope == requested_scope` **且**
    /// `root.project_root_digest == project_root_digest` 的记录。
    ///
    /// ⚠ 这两个条件缺一不可。
    /// 只过滤 scope 而不过滤项目，意味着 A 项目的信任能放行 B 项目的资源；
    /// 只过滤项目而不过滤 scope，意味着用户级的信任能放行项目级资源。
    /// **两者都要匹配** —— 这就是文件头说的 "selects a same-scope trust record"。
    ///
    /// **③ 排序**
    /// 按 `(revision 降序, scope 优先级降序, root_digest 升序)` 排。
    ///
    /// revision 排第一 means **最新修订的那条优先**。
    /// root_digest 升序是为了**确定性**：
    /// 当 revision 和 scope 都相同时，按摘要字典序取一个固定结果，
    /// 避免同样的输入在不同运行里给出不同选择。
    /// 这种确定性对可复现的审计很重要。
    ///
    /// **④ 分情况产出结论**
    ///
    /// 情况 A：**候选为空** → 拒绝，原因 `project_trust_root_missing`。
    /// 没人声明过这个项目能不能信。
    ///
    /// 情况 B：**同一 revision 下状态不唯一** → 拒绝，原因 `project_trust_conflict`。
    ///
    /// 代码用 `BTreeSet` 收集所有同 revision 候选的状态，
    /// `states.len() > 1` 就说明有人同时说了"可信"和"不可信"。
    /// ⚠ 这在现实中意味着：两个配置文件都声明了信任，但结论相反。
    /// **此时必须拒绝** —— 无法判断该听谁的。
    /// 注意这里只比较**同一 revision** 的候选；不同 revision 之间不算冲突，
    /// 因为新修订本来就该覆盖旧修订。
    ///
    /// 情况 C：**状态明确** → 按状态映射：
    /// - `Trusted` → 允许，原因 `project_trust_allowed`；
    /// - `Untrusted` → 拒绝，原因 `project_trust_untrusted`；
    /// - `Unknown` → 拒绝，原因 `project_trust_unknown`。
    ///
    /// ⚠ 注意 `Unknown` 也被拒绝。这就是文件头说的 fail-closed：
    /// "不知道能不能信" 和 "明确不可信" 一样，结论都是拒绝。
    /// 区别只在原因码上 —— 前者提示用户"你可以去建立信任"，
    /// 后者提示"有人明确拒绝过，别再试了"。
    pub fn resolve(
        project_root_digest: impl Into<String>,
        requested_scope: ProjectTrustScope,
        roots: Vec<ProjectTrustRoot>,
    ) -> Result<Self, String> {
        let project_root_digest = project_root_digest.into();
        valid_digest(&project_root_digest, "project_trust_project_digest")?;
        if roots.len() > MAX_PROJECT_TRUST_ROOTS {
            return Err("project_trust_root_limit".to_owned());
        }
        for root in &roots {
            root.validate()?;
        }
        let mut candidates = roots
            .into_iter()
            .filter(|root| {
                root.scope == requested_scope && root.project_root_digest == project_root_digest
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            right
                .revision
                .cmp(&left.revision)
                .then_with(|| right.scope.priority().cmp(&left.scope.priority()))
                .then_with(|| left.root_digest.cmp(&right.root_digest))
        });

        let (selected_scope, selected_root_digest, decision, reason) = if candidates.is_empty() {
            (
                None,
                None,
                ProjectTrustDecision::Deny,
                "project_trust_root_missing".to_owned(),
            )
        } else {
            let selected = &candidates[0];
            let states = candidates
                .iter()
                .filter(|root| root.revision == selected.revision)
                .map(|root| root.state)
                .collect::<BTreeSet<_>>();
            if states.len() > 1 {
                (
                    None,
                    None,
                    ProjectTrustDecision::Deny,
                    "project_trust_conflict".to_owned(),
                )
            } else {
                let decision = match selected.state {
                    ProjectTrustState::Trusted => ProjectTrustDecision::Allow,
                    ProjectTrustState::Untrusted => ProjectTrustDecision::Deny,
                    ProjectTrustState::Unknown => ProjectTrustDecision::Deny,
                };
                let reason = match selected.state {
                    ProjectTrustState::Trusted => "project_trust_allowed",
                    ProjectTrustState::Untrusted => "project_trust_untrusted",
                    ProjectTrustState::Unknown => "project_trust_unknown",
                };
                (
                    Some(selected.scope),
                    Some(selected.root_digest.clone()),
                    decision,
                    reason.to_owned(),
                )
            }
        };
        let mut resolution = Self {
            schema: PROJECT_TRUST_RESOLUTION_SCHEMA.to_owned(),
            version: PROJECT_TRUST_POLICY_VERSION,
            project_root_digest,
            requested_scope,
            candidates,
            selected_scope,
            selected_root_digest,
            decision,
            reason,
            resolution_digest: String::new(),
        };
        resolution.resolution_digest = resolution.digest();
        resolution.validate()?;
        Ok(resolution)
    }

    /// 这次判定是否允许加载资源。
    ///
    /// 【作用】
    /// 给加载器用的便捷方法。等价于 `decision == Allow`。
    ///
    /// 【⚠ 它只看结论，不看其他字段】
    /// 这里**没有**再检查 `selected_scope` 是否存在。
    /// 这看起来是个疏漏，其实不是 ——
    /// 完整性由 [`ProjectTrustResolution::validate`] 保证：
    /// 如果结论是 Allow 却没选中任何记录，
    /// 校验会以 `project_trust_allow_selection_missing` 失败。
    /// 而 `resolve` 在返回前一定会调用 `validate()`，
    /// 所以能走到调用方手里的对象，结论和选中记录必然是自洽的。
    pub fn allows_loading(&self) -> bool {
        self.decision == ProjectTrustDecision::Allow
    }

    /// 校验这个判定结果结构是否自洽。
    ///
    /// 【核心检查 —— 六组】
    ///
    /// **① 头部**：schema 匹配、版本兼容。
    ///
    /// **② 项目摘要格式**合法。
    ///
    /// **③ 候选的 scope 必须全部等于 requested_scope**
    /// 这条检查是 `resolve` 里那个过滤条件的**事后验证**。
    /// 如果有人手工构造了一个混了别的 scope 候选的判定结果，这里会拒绝。
    /// 换句话说：过滤规则不只写在生成端，还写在**校验端**。
    /// 两端都写，才能防止有人绕过 `resolve` 直接造一个对象出来。
    ///
    /// 候选数量同样不能超过 64。
    ///
    /// **④ 每条候选各自校验**
    ///
    /// **⑤ 结论与理由必须一致**
    /// 两条交叉检查：
    /// - 结论是 Allow 但没选中任何记录 → `project_trust_allow_selection_missing`；
    /// - 结论是 Deny 但原因却是 `project_trust_allowed` → `project_trust_deny_reason_invalid`。
    ///
    /// 第二条防的是"结论和理由自相矛盾"这种数据损坏。
    /// 这种数据正常流程产生不出来，只能来自手工构造或存储损坏 ——
    /// 正因为它不该出现，所以一旦出现就说明有问题，必须拒绝。
    ///
    /// **⑥ 选中的摘要必须在候选里存在**
    /// 如果 `selected_root_digest` 指向一条不存在的记录 → `project_trust_selected_root_missing`。
    /// 这防的是"引用了一个不存在的对象"这种悬空引用。
    ///
    /// **⑦ 自身摘要自洽**
    /// 重算摘要与存储值比对。
    ///
    /// 【⚠ 不要精简这里的检查】
    /// 这个函数是判定结果的**最后一道关卡**。
    /// 前面所有检查都可以被绕过（只要有人能构造对象），
    /// 只有这个函数保证"拿到的判定结果本身是可信的"。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROJECT_TRUST_RESOLUTION_SCHEMA
            || !self
                .version
                .is_compatible_with(&PROJECT_TRUST_POLICY_VERSION)
        {
            return Err("project_trust_resolution_header_invalid".to_owned());
        }
        valid_digest(&self.project_root_digest, "project_trust_project_digest")?;
        if self.candidates.len() > MAX_PROJECT_TRUST_ROOTS
            || self
                .candidates
                .iter()
                .any(|root| root.scope != self.requested_scope)
        {
            return Err("project_trust_resolution_scope_invalid".to_owned());
        }
        for root in &self.candidates {
            root.validate()?;
        }
        nonempty(&self.reason, "project_trust_reason", 128)?;
        if self.decision == ProjectTrustDecision::Allow
            && (self.selected_scope.is_none() || self.selected_root_digest.is_none())
        {
            return Err("project_trust_allow_selection_missing".to_owned());
        }
        if self.decision == ProjectTrustDecision::Deny && self.reason == "project_trust_allowed" {
            return Err("project_trust_deny_reason_invalid".to_owned());
        }
        if let Some(root_digest) = &self.selected_root_digest {
            valid_digest(root_digest, "project_trust_selected_digest")?;
            if !self
                .candidates
                .iter()
                .any(|root| &root.root_digest == root_digest)
            {
                return Err("project_trust_selected_root_missing".to_owned());
            }
        }
        valid_digest(&self.resolution_digest, "project_trust_resolution_digest")?;
        if self.resolution_digest != self.digest() {
            return Err("project_trust_resolution_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算这次判定结果的摘要。
    ///
    /// 【⚠ 包含全部候选】
    /// `candidates` 数组整体参与计算。
    /// 这意味着候选列表的任何变化都会改变摘要 ——
    /// 包括顺序变化。所以 `resolve` 里的排序必须是**确定性**的，
    /// 这正是它用 `root_digest` 升序做最终 tiebreak 的原因。
    ///
    /// 【⚠ 摘要字段自身不参与计算】
    /// 它没有出现在 `json!` 里，否则要算自己，无解。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "project_root_digest": self.project_root_digest,
            "requested_scope": self.requested_scope,
            "candidates": self.candidates,
            "selected_scope": self.selected_scope,
            "selected_root_digest": self.selected_root_digest,
            "decision": self.decision,
            "reason": self.reason,
        }))
    }
}
