//! Pure ProjectTrust root resolution for Skills, Plugins, Hooks and MCP resources.
//!
//! The policy layer does not inspect paths or load resources. An adapter supplies canonical root
//! and project digests, and this module selects a same-scope trust record before any loader may
//! read a project-local resource. Unknown, conflicting and untrusted roots are deny decisions.
//!
//! # ⚠ 当前状态：判定已进入生产信任闸门，但资源加载器尚未接线
//!
//! 截至 2026-10-17（SC-25 完成切片），实际状态是三段拼起来的：
//!
//! - `kiana-daemon` 的默认信任权威 `StoredProjectTrustAuthority` 已把持久化的项目信任记录
//!   映射成 [`ProjectTrustResolution`]；daemon 的布尔信任闸门（`project_trusted`）与
//!   `DaemonHost::project_trust_load_audit` / `authorize_project_trust_load` 都经它判定。
//! - [`ProjectTrustPlan`] 与加载前审计 [`ProjectTrustLoadAudit`] 目前只被本文件的单元测试和
//!   `kiana-policy/tests/` 的夹具直接调用，尚无生产调用者把它写进事件账本。
//! - skill/plugin/hook/MCP **加载器**仍走 `kiana-skills` 自己的信任类型（`SourceTrust`），
//!   尚未改为在读取资源前调用 `authorize_loading` —— 这段接线属于 SC-27。
//!
//! 所以本文件是"已实现、判定层已进入 daemon 信任闸门、但资源加载前调用面尚未扩展"的策略值类型。
//! 源码和测试能说明判定逻辑存在且被 daemon 使用；不能说明当前每个加载器都走这里。
//! 这不改变下面那些设计说明的价值 —— 读懂它有助于理解信任模型的整体设计。
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
//! **② DTO 保存摘要字段，而不保存真实路径**
//! 所有的根都用 `project_root_digest`（项目根摘要）和 `root_digest`（资源根摘要）
//! 表示，**不是真实路径字符串**。
//!
//! 摘要字段避免把路径明文直接放进这个 DTO；但 SHA-256 不是加密或匿名化，
//! 路径候选空间较小时仍可能被字典猜测。适配器负责规范化后再求摘要，
//! 本模块只检查摘要格式，不会验证适配器是否真的完成规范化。
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
use std::collections::{BTreeMap, BTreeSet};

/// 信任根记录的 schema 标识符。
pub const PROJECT_TRUST_ROOT_SCHEMA: &str = "kiana.project-trust-root.v1";
/// 信任判定结果的 schema 标识符。
pub const PROJECT_TRUST_RESOLUTION_SCHEMA: &str = "kiana.project-trust-resolution.v1";
/// 跨 scope 信任计划的 schema 标识符。
pub const PROJECT_TRUST_PLAN_SCHEMA: &str = "kiana.project-trust-plan.v1";
/// 加载前审计记录的 schema 标识符。
pub const PROJECT_TRUST_LOAD_AUDIT_SCHEMA: &str = "kiana.project-trust-load-audit.v1";
/// 信任策略版本。
pub const PROJECT_TRUST_POLICY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 一次判定最多能带多少个候选信任根。
///
/// 本源码只能确认它是单次判定接受候选列表的硬上限：超过后返回
/// `project_trust_root_limit`。这个上限也界定后续校验、过滤和排序要处理的输入规模；
/// 精确取 64 的业务依据无法仅从本文件确认。
pub const MAX_PROJECT_TRUST_ROOTS: usize = 64;

/// 检查字段不能为空、不能超过字节上限，且不能包含 NUL 字节。
///
/// 上限按 `str::len()` 的 UTF-8 字节数计算，不是字符数。NUL 检查可防止该字段随后
/// 被交给按 C 字符串处理的边界时发生截断；本函数自身不访问路径、文件系统或 FFI。
/// 任一条件失败都返回带调用方字段名的 `"{field}_invalid"`。
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
/// 对应内容的重算比较由调用方校验逻辑执行。摘要一致只能说明字段间自洽，不能证明记录
/// 由可信方签发；能同时改内容和摘要的一方仍可构造另一份自洽记录。
///
/// 【为什么固定 64 个字符】
/// SHA-256 输出 32 字节，十六进制表示正好 64 个字符。
/// 长度不符就说明这不是一个 SHA-256 摘要。
///
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
/// 区分"信任声明来自哪里"，而不是比较三种来源的权限大小，
/// 详见 [`ProjectTrustScope::priority`]。
///
/// 【三个层级的实际含义】
///
/// - `User` —— 用户级来源，例如用户配置的全局资源目录。
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
    /// 返回该来源层级的固定优先级数值。
    ///
    /// 数值越大层级越具体：`Project`(30) → `KianaHome`(20) → `User`(10)。
    ///
    /// 【它用在哪里】
    /// - `resolve()` 内作为确定性排序键的一部分；
    /// - [`ProjectTrustPlan::resolve`] 用它给出稳定的逐层检查/报告顺序
    ///   （见 [`ProjectTrustScope::precedence_order`]）。
    ///
    /// 【不要把它误读为跨层覆盖规则】
    /// 优先级只决定报告顺序，**不决定谁能放行谁**。一次加载的结论只由匹配资源根的
    /// 那一层给出：低层（用户级）的 Trusted **不能**放行高层（项目级）资源，
    /// 高层的 Untrusted 也不会连带否决另一层自己的资源。
    /// 这就是"路径伪造 trust"要拒绝的形状。
    pub const fn priority(self) -> u8 {
        match self {
            Self::User => 10,
            Self::KianaHome => 20,
            Self::Project => 30,
        }
    }

    /// 按优先级降序返回三个 scope 的固定检查顺序：`Project` → `KianaHome` → `User`。
    ///
    /// [`ProjectTrustPlan::resolve`] 按这个顺序逐层求值，使"哪一层给出的结论"在计划与
    /// 审计记录里的位置稳定可复现；顺序本身不授予任何跨层权限。
    pub const fn precedence_order() -> [ProjectTrustScope; 3] {
        [Self::Project, Self::KianaHome, Self::User]
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
    /// 判定允许该 scope 下的资源继续进入加载器。
    Allow,
    /// 判定拒绝；具体原因由 `ProjectTrustResolution::reason` 提供。
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
/// 【`audit_ref` 的边界】
/// 按字段契约它应是稳定、脱敏的审计引用，不能放真实路径或信任文件原文。
/// 但本类型只检查它非空、长度受限且不含 NUL；校验不会证明内容已脱敏，
/// 也不会解析出操作者、时间或来源。写入方必须负责提供合规引用。
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
    /// 有内容、不超 256 字节且不含 NUL。其他控制字符和真实路径内容不在此 helper 的检查范围内。
    ///
    /// **④ 摘要自洽**
    /// 重算摘要并与存储值比对，可发现内容变化后没有同步更新摘要的情况。
    /// 这不是签名或 MAC：若修改者能同时重算并替换摘要，本方法不能证明记录来源可信。
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
/// 还携带候选、选择结果、结论和原因，供调用方检查或记录。
/// 生产调用者是 daemon 的默认信任权威（`StoredProjectTrustAuthority` 把它映射自持久化记录）；
/// 但它尚未写进事件账本，因此不能把它的存在当作"可复盘的运行记录"。
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
    /// 本次判定针对的项目根摘要，由调用方提供。
    pub project_root_digest: String,
    /// 要求匹配的来源层级；`resolve()` 只保留同层候选。
    pub requested_scope: ProjectTrustScope,
    /// 通过项目摘要和 scope 过滤后的候选，并按 `resolve()` 的规则排序。
    pub candidates: Vec<ProjectTrustRoot>,
    /// 命中时的来源层级；没有可选记录时为 `None`。
    pub selected_scope: Option<ProjectTrustScope>,
    /// 命中记录的 root_digest；没有可选记录时为 `None`。
    pub selected_root_digest: Option<String>,
    /// 信任结果；只有 Trusted 状态映射为 Allow。
    pub decision: ProjectTrustDecision,
    /// 稳定原因码，供调用方区分缺失、冲突、未知与明确拒绝。
    pub reason: String,
    /// 本结果字段的 JSON 摘要；它不等同于签名或来源认证。
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
    /// ⚠ 本方法只读 `decision`，不会自动调用 `validate()`。
    /// 由 `resolve()` 返回的对象在返回前已校验；但 DTO 同时支持反序列化且字段公开，
    /// 因此外部读入或手工构造的值必须先显式调用 `validate()`，再把 `true` 当作可信判定。
    pub fn allows_loading(&self) -> bool {
        self.decision == ProjectTrustDecision::Allow
    }

    /// 校验这个判定结果结构是否自洽。
    ///
    /// 【核心检查 —— 七组】
    ///
    /// **① 头部**：schema 匹配、版本兼容。
    ///
    /// **② 项目摘要格式**合法。
    ///
    /// **③ 候选数量受限，且 scope 必须等于 requested_scope**。
    ///
    /// **④ 每条候选各自校验**
    ///
    /// **⑤ 有限的结论/理由约束**：Allow 必须带 selected 字段；Deny 不能使用
    /// `project_trust_allowed` 这个原因码。
    ///
    /// **⑥ 选中的摘要必须在候选里存在**
    /// 如果 `selected_root_digest` 指向一条不存在的记录 → `project_trust_selected_root_missing`。
    /// 这防的是"引用了一个不存在的对象"这种悬空引用。
    ///
    /// **⑦ 自身摘要自洽**
    /// 重算摘要与存储值比对。
    ///
    /// ⚠ 这只是结构和摘要一致性检查，不是授权来源认证，也不重跑完整决策。
    /// 它不会确认候选的 `project_root_digest` 都等于结果头部的项目摘要，
    /// 不会确认 `selected_scope` 与所选候选对应，也不会验证 decision 与所选 state 一致。
    /// `resolve()` 自己会按项目摘要筛选并从状态计算 decision；直接反序列化或手工构造 DTO 时，
    /// 不能把 `validate()` 当作这些语义关系的替代检查。
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

    /// 产出一条**加载前审计**记录，供加载器在读取资源之前落账。
    ///
    /// 【作用】
    /// 把这次判定的结论（scope、拒绝原因、选中摘要、判定摘要）与本次要加载的资源根摘要
    /// 绑成一条自校验的记录。Allow 和 Deny 都会产出审计——"拒绝"同样是需要留下证据的决定。
    ///
    /// 【⚠ 审计不是许可】
    /// 记录存在不等于允许加载。加载器必须用 [`ProjectTrustResolution::authorize_loading`]
    /// 拿许可，或者自己检查 `decision`；不要拿审计记录的存在当放行依据。
    ///
    /// 【失败情况】
    /// - 这个判定对象自身不自洽 → 返回其校验错误；
    /// - `resource_root_digest` 不是合法摘要格式（例如直接把路径填进来）→
    ///   `project_trust_resource_digest_invalid`；
    /// - `audit_ref` 为空/超长/含 NUL → `project_trust_audit_ref_invalid`。
    pub fn load_audit(
        &self,
        resource_root_digest: impl Into<String>,
        audit_ref: impl Into<String>,
    ) -> Result<ProjectTrustLoadAudit, String> {
        self.validate()?;
        let resource_root_digest = resource_root_digest.into();
        valid_digest(&resource_root_digest, "project_trust_resource_digest")?;
        let audit_ref = audit_ref.into();
        nonempty(&audit_ref, "project_trust_audit_ref", 256)?;
        let mut audit = ProjectTrustLoadAudit {
            schema: PROJECT_TRUST_LOAD_AUDIT_SCHEMA.to_owned(),
            version: PROJECT_TRUST_POLICY_VERSION,
            project_root_digest: self.project_root_digest.clone(),
            resource_root_digest,
            requested_scope: self.requested_scope,
            selected_scope: self.selected_scope,
            selected_root_digest: self.selected_root_digest.clone(),
            decision: self.decision,
            reason: self.reason.clone(),
            resolution_digest: self.resolution_digest.clone(),
            audit_ref,
            audit_digest: String::new(),
        };
        audit.audit_digest = audit.digest();
        audit.validate()?;
        Ok(audit)
    }

    /// 为一次资源加载**发放许可**：允许时返回审计记录，拒绝时返回稳定原因码。
    ///
    /// 【调用时机】
    /// 在任何加载器打开项目本地的 skill、plugin、hook 或 MCP 配置之前。
    /// 拒绝时返回值就是策略原因（如 `project_trust_root_missing`），调用方不要把它改写成
    /// 通用 I/O 错误，也不要在拒绝后继续读取资源。
    pub fn authorize_loading(
        &self,
        resource_root_digest: impl Into<String>,
        audit_ref: impl Into<String>,
    ) -> Result<ProjectTrustLoadAudit, String> {
        let audit = self.load_audit(resource_root_digest, audit_ref)?;
        if !self.allows_loading() {
            return Err(self.reason.clone());
        }
        Ok(audit)
    }
}

/// 单个 scope 的判定摘要，属于 [`ProjectTrustPlan`]。
///
/// 【字段】
/// - `scope` / `priority`：这是哪一层、它在本计划里的固定排序值；
/// - `decision` / `reason`：该层自己的结论与稳定原因码；
/// - `selected_root_digest`：该层命中记录的根摘要（没有命中时为 `None`）；
/// - `resolution_digest`：对应 [`ProjectTrustResolution::resolution_digest`]。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTrustScopeDecision {
    pub scope: ProjectTrustScope,
    pub priority: u8,
    pub decision: ProjectTrustDecision,
    pub reason: String,
    pub selected_root_digest: Option<String>,
    pub resolution_digest: String,
}

impl ProjectTrustScopeDecision {
    pub fn validate(&self) -> Result<(), String> {
        if self.priority != self.scope.priority() {
            return Err("project_trust_scope_priority_invalid".to_owned());
        }
        nonempty(&self.reason, "project_trust_reason", 128)?;
        if self.decision == ProjectTrustDecision::Allow && self.selected_root_digest.is_none() {
            return Err("project_trust_allow_selection_missing".to_owned());
        }
        if self.decision == ProjectTrustDecision::Deny && self.reason == "project_trust_allowed" {
            return Err("project_trust_deny_reason_invalid".to_owned());
        }
        if let Some(root_digest) = &self.selected_root_digest {
            valid_digest(root_digest, "project_trust_selected_digest")?;
        }
        valid_digest(&self.resolution_digest, "project_trust_resolution_digest")?;
        Ok(())
    }
}

/// 一次跨 scope 的信任计划。
///
/// 【作用】
/// 对同一个项目分别求值 user、KIANA_HOME、project 三层的信任记录，并按 scope 优先级
/// （`Project` → `KianaHome` → `User`）给出稳定的逐层结论，供审计与排查使用。
///
/// 【⚠ 只有请求层能放行】
/// `decision` / `reason` 恒等于 `requested_scope` 那一层的结论。其它层哪怕是 Trusted，
/// 也不会让请求层从 Deny 变成 Allow——三层是三个命名空间：
/// 用户级信任放行用户级资源，项目级信任放行项目本地资源。
/// 如果把三层合并成"任一层可信即放行"，一个被克隆的仓库只要让用户级目录里留下一条记录，
/// 就能借道项目级加载——这正是"路径伪造 trust"要拒绝的形状。
///
/// 【为什么不是三份 `resolve()` 的简单集合】
/// 计划把"整体结论来自哪一层"写成不变量并在 `validate()` 里复核（见
/// `project_trust_plan_requested_scope_mismatch`），所以反序列化或手工拼出来的计划
/// 不能把某层的 Allow 冒充成整体 Allow。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTrustPlan {
    pub schema: String,
    pub version: SchemaVersion,
    pub project_root_digest: String,
    /// 本次要加载的资源根所属 scope；只有这一层的 Allow 能放行加载。
    pub requested_scope: ProjectTrustScope,
    /// 逐层判定，恒按 `Project` → `KianaHome` → `User` 顺序。
    pub scope_decisions: Vec<ProjectTrustScopeDecision>,
    pub decision: ProjectTrustDecision,
    pub reason: String,
    pub plan_digest: String,
}

impl ProjectTrustPlan {
    /// 对同一项目逐层求值三个 scope，产出跨层计划。
    ///
    /// 【核心流程】
    /// 1. 校验项目摘要、根数量上限，并按 scope 把候选记录分组；
    /// 2. 按 [`ProjectTrustScope::precedence_order`] 逐层调用
    ///    [`ProjectTrustResolution::resolve`]——每层只看到自己的记录，因此
    ///    "跨层越权"和"同层缺失/未知/冲突"都由同一套规则 fail-closed；
    /// 3. 整体结论取 `requested_scope` 那一层；任何一层判定不自洽都返回错误。
    ///
    /// 【失败情况】
    /// 项目摘要格式非法、候选超过 [`MAX_PROJECT_TRUST_ROOTS`]、任一记录或任一层的
    /// 判定不自洽时返回对应的稳定错误码，不会产出部分计划。
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
        let mut per_scope: BTreeMap<ProjectTrustScope, Vec<ProjectTrustRoot>> = BTreeMap::new();
        for root in roots {
            per_scope.entry(root.scope).or_default().push(root);
        }
        let mut scope_decisions = Vec::with_capacity(ProjectTrustScope::precedence_order().len());
        let mut requested: Option<ProjectTrustScopeDecision> = None;
        for scope in ProjectTrustScope::precedence_order() {
            let scope_roots = per_scope.remove(&scope).unwrap_or_default();
            let resolution =
                ProjectTrustResolution::resolve(project_root_digest.clone(), scope, scope_roots)?;
            let decision = ProjectTrustScopeDecision {
                scope,
                priority: scope.priority(),
                decision: resolution.decision,
                reason: resolution.reason.clone(),
                selected_root_digest: resolution.selected_root_digest.clone(),
                resolution_digest: resolution.resolution_digest.clone(),
            };
            decision.validate()?;
            if scope == requested_scope {
                requested = Some(decision.clone());
            }
            scope_decisions.push(decision);
        }
        let requested = requested.ok_or_else(|| "project_trust_scope_invalid".to_owned())?;
        let mut plan = Self {
            schema: PROJECT_TRUST_PLAN_SCHEMA.to_owned(),
            version: PROJECT_TRUST_POLICY_VERSION,
            project_root_digest,
            requested_scope,
            scope_decisions,
            decision: requested.decision,
            reason: requested.reason,
            plan_digest: String::new(),
        };
        plan.plan_digest = plan.digest();
        plan.validate()?;
        Ok(plan)
    }

    /// 这次计划是否允许加载请求层的资源。
    ///
    /// 【作用】
    /// 等价于 `decision == Allow`，且只在整体结论来自 `requested_scope` 时成立。
    pub fn allows_loading(&self) -> bool {
        self.decision == ProjectTrustDecision::Allow
    }

    /// 校验计划的结构、逐层顺序与"整体结论来自请求层"这一不变量。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROJECT_TRUST_PLAN_SCHEMA
            || !self
                .version
                .is_compatible_with(&PROJECT_TRUST_POLICY_VERSION)
        {
            return Err("project_trust_plan_header_invalid".to_owned());
        }
        valid_digest(&self.project_root_digest, "project_trust_project_digest")?;
        let expected = ProjectTrustScope::precedence_order();
        if self.scope_decisions.len() != expected.len() {
            return Err("project_trust_plan_scope_invalid".to_owned());
        }
        for (index, scope) in expected.iter().enumerate() {
            let decision = &self.scope_decisions[index];
            if decision.scope != *scope {
                return Err("project_trust_plan_scope_invalid".to_owned());
            }
            decision.validate()?;
        }
        let requested = self
            .scope_decisions
            .iter()
            .find(|decision| decision.scope == self.requested_scope)
            .ok_or_else(|| "project_trust_plan_scope_invalid".to_owned())?;
        if requested.decision != self.decision || requested.reason != self.reason {
            return Err("project_trust_plan_requested_scope_mismatch".to_owned());
        }
        valid_digest(&self.plan_digest, "project_trust_plan_digest")?;
        if self.plan_digest != self.digest() {
            return Err("project_trust_plan_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算计划的摘要；逐层判定与整体结论全部参与，任何字段变化都会改变摘要。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "project_root_digest": self.project_root_digest,
            "requested_scope": self.requested_scope,
            "scope_decisions": self.scope_decisions,
            "decision": self.decision,
            "reason": self.reason,
        }))
    }
}

/// 一次资源加载的**加载前审计**记录。
///
/// 【作用】
/// 在加载器读取项目本地资源之前，把这次信任判定的完整依据留成一条自校验、可序列化的记录：
/// 项目摘要、资源根摘要、请求/选中 scope、结论与稳定原因码、判定摘要，以及脱敏的审计引用。
///
/// 【⚠ Allow 与 Deny 都会产出记录】
/// 审计记录的存在不代表允许加载；它只是"决定已作出"的证据。
/// 放行必须来自 [`ProjectTrustResolution::authorize_loading`] 或显式检查 `decision`。
/// Allow 记录还要求 `selected_scope == Some(requested_scope)`——只有请求层自己的命中
/// 才能对应一次放行。
///
/// 【⚠ 它不证明的事】
/// 这不是签名或来源认证：能同时重算并替换摘要的一方仍可构造另一份自洽记录。
/// 它也不重新执行判定，只描述调用方交出的那一次已校验判定。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTrustLoadAudit {
    pub schema: String,
    pub version: SchemaVersion,
    pub project_root_digest: String,
    pub resource_root_digest: String,
    pub requested_scope: ProjectTrustScope,
    pub selected_scope: Option<ProjectTrustScope>,
    pub selected_root_digest: Option<String>,
    pub decision: ProjectTrustDecision,
    pub reason: String,
    pub resolution_digest: String,
    /// Stable redacted audit reference; never a raw path or trust-file payload.
    pub audit_ref: String,
    pub audit_digest: String,
}

impl ProjectTrustLoadAudit {
    /// 校验审计记录的结构、作用域与摘要自洽。
    ///
    /// 【核心检查】
    /// 头部与摘要格式；Allow 必须有选中 scope/root 且选中 scope 恒等于请求 scope；
    /// Deny 不得使用 `project_trust_allowed`；审计摘要与重算值一致。
    ///
    /// 【⚠ 边界】
    /// 这是结构与自洽性检查，不是授权来源认证，也不重跑完整决策；
    /// 反序列化得到或手工构造的记录必须先显式通过本方法，才能当作"决定已记录"的凭据。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROJECT_TRUST_LOAD_AUDIT_SCHEMA
            || !self
                .version
                .is_compatible_with(&PROJECT_TRUST_POLICY_VERSION)
        {
            return Err("project_trust_load_audit_header_invalid".to_owned());
        }
        valid_digest(&self.project_root_digest, "project_trust_project_digest")?;
        valid_digest(&self.resource_root_digest, "project_trust_resource_digest")?;
        nonempty(&self.reason, "project_trust_reason", 128)?;
        nonempty(&self.audit_ref, "project_trust_audit_ref", 256)?;
        if self.decision == ProjectTrustDecision::Allow {
            if self.selected_scope != Some(self.requested_scope)
                || self.selected_root_digest.is_none()
            {
                return Err("project_trust_allow_selection_missing".to_owned());
            }
        }
        if self.decision == ProjectTrustDecision::Deny && self.reason == "project_trust_allowed" {
            return Err("project_trust_deny_reason_invalid".to_owned());
        }
        if let Some(root_digest) = &self.selected_root_digest {
            valid_digest(root_digest, "project_trust_selected_digest")?;
        }
        valid_digest(&self.resolution_digest, "project_trust_resolution_digest")?;
        valid_digest(&self.audit_digest, "project_trust_audit_digest")?;
        if self.audit_digest != self.digest() {
            return Err("project_trust_audit_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算审计记录的摘要；`audit_digest` 自身不参与计算。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "project_root_digest": self.project_root_digest,
            "resource_root_digest": self.resource_root_digest,
            "requested_scope": self.requested_scope,
            "selected_scope": self.selected_scope,
            "selected_root_digest": self.selected_root_digest,
            "decision": self.decision,
            "reason": self.reason,
            "resolution_digest": self.resolution_digest,
            "audit_ref": self.audit_ref,
        }))
    }
}
