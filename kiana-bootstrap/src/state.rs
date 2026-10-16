//! 旧入口使用的进程内会话状态与统计容器。
//!
//! # 文件职责与数据流
//!
//! `kiana-entrypoints` 和兼容 Runner 在一次进程生命周期内，通过 [`GlobalState`] 记录当前
//! 工作目录、session、模型用量、工具/Hook 耗时、已调用 skill、计划缓存和 UI 模式开关。它
//! 把多个入口共享的临时状态集中到一个 `Arc<Mutex<State>>`，避免每个组件维护一份互相漂移
//! 的副本；但它不是 EventLog，也不是 `kiana-domain` 的 Run/Turn/Request 身份事实。
//!
//! ```text
//! entrypoint / runner 产生运行时观察
//!        -> GlobalState 方法在 Mutex 临界区内更新 State
//!        -> CLI/UI/诊断读取统计和显示辅助值
//!        -> 进程退出或 session 切换时状态可能丢失/被重置
//! ```
//!
//! 这里的计费、token、耗时和行数是本地统计投影；`total_cost_usd` 不能替代 provider 账单，
//! `session_id` 也不能替代服务端生成的 RunId/RequestId。`session_trust_accepted`、
//! `session_bypass_permissions_mode` 等布尔字段只是旧 UI/Runner 的本地开关，不能直接作为
//! ControlPlane 的授权依据。需要跨重启恢复、审计或权限证明时，应沿 EventLog/receipt/领域
//! 快照链路读取，而不是把 `State` 序列化后当成事实源。

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub type SessionId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
/// 允许当前会话使用的插件/服务器 channel 描述。
///
/// 这是入口配置和显示层使用的兼容 DTO；`dev` 只是开发标志，不会自动给 channel 授予
/// 网络、文件或能力权限。真正的资源准入仍由受控扩展/Capability 流程决定。
pub enum ChannelEntry {
    #[serde(rename = "plugin")]
    Plugin {
        /// 插件名称，供显示和匹配使用。
        name: String,
        /// 插件所属 marketplace 标识。
        marketplace: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        /// 可选开发模式标志；缺失时保持兼容旧配置。
        dev: Option<bool>,
    },
    #[serde(rename = "server")]
    Server {
        /// 服务器 channel 名称。
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        /// 是否为开发服务器。
        dev: Option<bool>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 按 provider/model 归属的 token 计量快照。
///
/// 这些计数由客户端侧观察累加，主要用于界面和本地成本估算；它们不是 provider 最终账单，
/// 也不能证明某一次请求已经成功提交。
pub struct ModelUsage {
    /// 输入 token 总数。
    pub input_tokens: u64,
    /// 输出 token 总数。
    pub output_tokens: u64,
    /// 从 provider prompt cache 读取的输入 token 数。
    pub cache_read_input_tokens: u64,
    /// 写入 provider prompt cache 的输入 token 数。
    pub cache_creation_input_tokens: u64,
    /// web search 请求次数。
    pub web_search_requests: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 会话级定时任务的旧入口描述。
pub struct SessionCronTask {
    /// 任务本地标识，不等于 EventLog 事件 ID。
    pub id: String,
    /// cron 表达式文本，由调度器解释。
    pub cron: String,
    /// 触发时提交的 prompt。
    pub prompt: String,
    /// 创建时间（Unix 秒/毫秒的具体单位由调用方约定）。
    pub created_at: u64,
    /// 是否重复执行；缺失表示旧数据没有提供该开关。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurring: Option<bool>,
    /// 可选 agent 标识，用于把任务归属到某个兼容会话。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 一次 skill 调用在本地会话中的缓存记录。
pub struct InvokedSkillInfo {
    /// skill 名称，作为显示和去重键的一部分。
    pub skill_name: String,
    /// skill 文件路径；路径本身不代表该文件已经通过 ProjectTrust。
    pub skill_path: String,
    /// 读取到的 skill 内容，可能包含敏感/项目本地文本，不能直接当作审计事实。
    pub content: String,
    /// 记录时间。
    pub invoked_at: u64,
    /// 调用它的 agent，可为空表示主会话调用。
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 慢操作诊断样本，仅用于本地观测。
pub struct SlowOperation {
    /// 操作名称。
    pub operation: String,
    /// 操作耗时毫秒。
    pub duration_ms: u64,
    /// 样本时间戳。
    pub timestamp: u64,
}

#[derive(Debug)]
/// 兼容入口共享的全部进程内状态。
///
/// 字段可以分成五组：
///
/// 1. 工作区与 session 身份；
/// 2. API/tool/Hook 成本与耗时统计；
/// 3. 旧 UI、自动模式和权限提示开关；
/// 4. skill、计划、cron、channel 等会话缓存；
/// 5. 远程连接、prompt cache 和 compaction 边沿状态。
///
/// `State` 本身没有内部锁，外部通过 [`GlobalState`] 在短临界区内访问；不要把裸 `State`
/// 跨线程共享，也不要在持锁期间执行网络、文件或模型调用。
pub struct State {
    /// 进程启动时的原始工作目录，用于回到入口位置；不会随 `cwd` 自动变化。
    pub original_cwd: PathBuf,
    /// 当前会话认定的项目根目录；它是兼容层提示，不自动完成 project trust/canonical 校验。
    pub project_root: PathBuf,
    /// 按本地成本估算累积的美元数。
    pub total_cost_usd: f64,
    /// 所有 API 调用耗时（包含重试）。
    pub total_api_duration: u64,
    /// 排除重试耗时后的 API 调用耗时。
    pub total_api_duration_without_retries: u64,
    /// 工具执行耗时总量。
    pub total_tool_duration: u64,
    /// 当前 turn 的 Hook 耗时。
    pub turn_hook_duration_ms: u64,
    /// 当前 turn 的工具耗时。
    pub turn_tool_duration_ms: u64,
    /// 当前 turn 的分类器耗时。
    pub turn_classifier_duration_ms: u64,
    /// 当前 turn 已计入的工具调用次数。
    pub turn_tool_count: u32,
    /// 当前 turn 已计入的 Hook 次数。
    pub turn_hook_count: u32,
    /// 当前 turn 已计入的分类器调用次数。
    pub turn_classifier_count: u32,
    /// State 初始化时间。
    pub start_time: u64,
    /// 最近一次交互时间。
    pub last_interaction_time: u64,
    /// 本地统计的新增行数。
    pub total_lines_added: u64,
    /// 本地统计的删除行数。
    pub total_lines_removed: u64,
    /// 某些模型费用无法计算时的标志；true 不表示请求失败，只表示金额不完整。
    pub has_unknown_model_cost: bool,
    /// 当前进程实际使用的工作目录。
    pub cwd: PathBuf,
    /// 按模型名保存的 token 用量。
    pub model_usage: HashMap<String, ModelUsage>,
    /// 是否为交互式终端。
    pub is_interactive: bool,
    /// 旧 Kairos 模式是否启用。
    pub kairos_active: bool,
    /// 是否要求工具结果严格一一配对。
    pub strict_tool_result_pairing: bool,
    /// SDK 是否输出 agent progress summary。
    pub sdk_agent_progress_summaries_enabled: bool,
    /// 用户消息相关功能是否显式 opt-in。
    pub user_msg_opt_in: bool,
    /// 当前客户端类型（通常是 `cli` 或其他入口标识）。
    pub client_type: String,
    /// 可选 session 来源标签。
    pub session_source: Option<String>,
    /// 兼容层 session 标识；它不等于 Domain RunId。
    pub session_id: SessionId,
    /// 重新生成 session 时保存的父 session 标识。
    pub parent_session_id: Option<SessionId>,
    /// 旧模式是否绕过本地权限提示；不能作为服务端授权输入。
    pub session_bypass_permissions_mode: bool,
    /// 是否开启会话级 cron 任务。
    pub scheduled_tasks_enabled: bool,
    /// 当前 session 的定时任务列表。
    pub session_cron_tasks: Vec<SessionCronTask>,
    /// 当前 session 创建过的 team 名称集合。
    pub session_created_teams: HashSet<String>,
    /// 用户是否接受了本地会话 trust 提示；不等于 ProjectTrust authority 的服务端结论。
    pub session_trust_accepted: bool,
    /// 是否禁用兼容层 session 持久化。
    pub session_persistence_disabled: bool,
    /// 是否已经离开 plan mode。
    pub has_exited_plan_mode: bool,
    /// 是否需要在下一次输出附加 plan mode 退出信息。
    pub needs_plan_mode_exit_attachment: bool,
    /// 是否需要在下一次输出附加 auto mode 退出信息。
    pub needs_auto_mode_exit_attachment: bool,
    /// 当前 session 是否已经展示过 LSP 建议。
    pub lsp_recommendation_shown_this_session: bool,
    /// session -> plan slug 的临时缓存。
    pub plan_slug_cache: HashMap<String, String>,
    /// 以 agent/session 组合键保存的 skill 调用缓存。
    pub invoked_skills: HashMap<String, InvokedSkillInfo>,
    /// 慢操作诊断样本。
    pub slow_operations: Vec<SlowOperation>,
    /// 主线程 agent 类型。
    pub main_thread_agent_type: Option<String>,
    /// 是否走远程模式。
    pub is_remote_mode: bool,
    /// Direct Connect 远程服务器地址（如有）。
    pub direct_connect_server_url: Option<String>,
    /// 系统 prompt 分段缓存；None 表示尚未加载或明确没有该段。
    pub system_prompt_section_cache: HashMap<String, Option<String>>,
    /// 最近一次输出所使用的日期标签。
    pub last_emitted_date: Option<String>,
    /// 额外读取 CLAUDE.md 的目录列表。
    pub additional_directories_for_claude_md: Vec<String>,
    /// 当前允许的 plugin/server channels。
    pub allowed_channels: Vec<ChannelEntry>,
    /// 是否存在开发 channel。
    pub has_dev_channels: bool,
    /// 当前 session 的项目目录覆盖值。
    pub session_project_dir: Option<PathBuf>,
    /// 允许使用 1 小时 prompt cache 的模型/规则列表。
    pub prompt_cache_1h_allowlist: Option<Vec<String>>,
    /// 当前模型是否满足 1 小时 prompt cache 条件。
    pub prompt_cache_1h_eligible: Option<bool>,
    /// AFK 模式 header 是否已经在当前请求中写入。
    pub afk_mode_header_latched: Option<bool>,
    /// fast mode header 是否已经写入。
    pub fast_mode_header_latched: Option<bool>,
    /// cache editing header 是否已经写入。
    pub cache_editing_header_latched: Option<bool>,
    /// thinking clear header 是否已经写入。
    pub thinking_clear_latched: Option<bool>,
    /// 当前 prompt 的兼容 ID。
    pub prompt_id: Option<String>,
    /// 最近一次主请求 ID，用于本地关联诊断。
    pub last_main_request_id: Option<String>,
    /// 最近一次 API 完成时间。
    pub last_api_completion_timestamp: Option<u64>,
    /// compaction 后是否需要在下一步消费一次 pending 标记。
    pub pending_post_compaction: bool,
}

impl Default for State {
    /// 创建一个新的进程内状态快照。
    ///
    /// 【调用者】
    /// `GlobalState::new()` 通过这里初始化共享状态；兼容入口也可能直接使用
    /// `State::default()` 创建测试或临时上下文。
    ///
    /// 【初始化内容】
    /// 工作目录取当前进程目录，时间取 Unix 毫秒时间戳，session 使用新的 UUID；计数器、
    /// 集合和可选配置均从空值开始。这样可以把“尚未发生”与“累计值为零”统一成一个可用
    /// 的初始状态，调用方不需要先执行额外的初始化顺序。
    ///
    /// 【为什么不是持久化恢复】
    /// 这个类型服务的是旧入口的短生命周期内存状态。需要跨进程恢复的运行事实必须从
    /// EventLog 或领域快照重建，不能因为这里的默认值看起来完整，就把它当作持久化状态。
    fn default() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        Self {
            original_cwd: cwd.clone(),
            project_root: cwd.clone(),
            total_cost_usd: 0.0,
            total_api_duration: 0,
            total_api_duration_without_retries: 0,
            total_tool_duration: 0,
            turn_hook_duration_ms: 0,
            turn_tool_duration_ms: 0,
            turn_classifier_duration_ms: 0,
            turn_tool_count: 0,
            turn_hook_count: 0,
            turn_classifier_count: 0,
            start_time: now,
            last_interaction_time: now,
            total_lines_added: 0,
            total_lines_removed: 0,
            has_unknown_model_cost: false,
            cwd,
            model_usage: HashMap::new(),
            is_interactive: false,
            kairos_active: false,
            strict_tool_result_pairing: false,
            sdk_agent_progress_summaries_enabled: false,
            user_msg_opt_in: false,
            client_type: "cli".to_string(),
            session_source: None,
            session_id: Uuid::new_v4().to_string(),
            parent_session_id: None,
            session_bypass_permissions_mode: false,
            scheduled_tasks_enabled: false,
            session_cron_tasks: Vec::new(),
            session_created_teams: HashSet::new(),
            session_trust_accepted: false,
            session_persistence_disabled: false,
            has_exited_plan_mode: false,
            needs_plan_mode_exit_attachment: false,
            needs_auto_mode_exit_attachment: false,
            lsp_recommendation_shown_this_session: false,
            plan_slug_cache: HashMap::new(),
            invoked_skills: HashMap::new(),
            slow_operations: Vec::new(),
            main_thread_agent_type: None,
            is_remote_mode: false,
            direct_connect_server_url: None,
            system_prompt_section_cache: HashMap::new(),
            last_emitted_date: None,
            additional_directories_for_claude_md: Vec::new(),
            allowed_channels: Vec::new(),
            has_dev_channels: false,
            session_project_dir: None,
            prompt_cache_1h_allowlist: None,
            prompt_cache_1h_eligible: None,
            afk_mode_header_latched: None,
            fast_mode_header_latched: None,
            cache_editing_header_latched: None,
            thinking_clear_latched: None,
            prompt_id: None,
            last_main_request_id: None,
            last_api_completion_timestamp: None,
            pending_post_compaction: false,
        }
    }
}

/// `State` 的线程安全访问门面。
///
/// 【位置】
/// 它位于兼容入口与具体状态字段之间：入口和 Runner 持有 `GlobalState`，通过方法读取或
/// 修改内部 `State`，而不直接共享裸的 `HashMap`、`Vec` 或会话字段。
///
/// 【并发边界】
/// `Arc` 负责让多个入口持有同一个状态容器，`Mutex` 负责让一次读写在内存上保持互斥。
/// 锁只保护本地字段更新；网络请求、文件扫描、模型调用等长操作不能放在锁内，否则会让
/// 一个慢调用阻塞所有统计和 session 读取。
///
/// 【事实边界】
/// 这里保存的是兼容层观察值和 UI 辅助开关，不是 ControlPlane 的授权状态，也不是 EventLog
/// 的事件事实。方法返回成功只表示内存更新完成，不表示对应的上游动作已经成功。
pub struct GlobalState {
    state: Arc<Mutex<State>>,
}

impl GlobalState {
    /// 创建一个新的共享进程状态容器。
    ///
    /// 【调用者】
    /// 入口初始化和兼容 Runner 在启动阶段调用；之后把返回的 `GlobalState` 传给需要共享
    /// session、统计或 UI 状态的组件。
    ///
    /// 【输出】
    /// 返回指向一个新 [`State`] 的 `Arc<Mutex<_>>` 门面。每次调用都会创建独立状态，不会
    /// 自动连接到其他 `GlobalState` 实例。
    ///
    /// 【失败情况】
    /// 初始化本身不返回 `Result`。当前目录读取失败时由 `State::default()` 退回 `.`；这只
    /// 影响兼容层路径提示，不会替代后续真正的项目根目录解析和 trust 校验。
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
        }
    }

    /// 读取当前兼容 session 标识。
    ///
    /// `session_id` 用于把本地缓存、计划或 UI 记录归到同一会话；它不是领域层 RunId，不能
    /// 用来证明某次请求已经进入 ControlPlane。返回值是克隆出来的字符串，因此调用者可以
    /// 在释放锁后使用它。
    pub fn get_session_id(&self) -> SessionId {
        self.state.lock().unwrap().session_id.clone()
    }

    /// 生成新的 session 标识，并清理旧 session 的计划目录缓存。
    ///
    /// 【调用者】
    /// 会话重置、重新开始或兼容层“fork session”流程调用。
    ///
    /// 【输入】
    /// `set_current_as_parent=true` 时，旧 ID 会先写入 `parent_session_id`，用于保留本地
    /// 重新生成关系；传 `false` 则只切换当前 ID。
    ///
    /// 【副作用】
    /// 旧 ID 对应的 `plan_slug_cache` 项会被删除，`session_project_dir` 会被清空，因为新
    /// session 不应无条件继承旧 session 的项目覆盖值。这里没有删除 EventLog，也没有撤销
    /// 旧运行；持久化生命周期必须由上层 ControlPlane/事件链路处理。
    ///
    /// 【并发】
    /// 所有读取、父 ID 记录、缓存清理和新 ID 写入在同一把锁内完成，避免其他线程观察到
    /// “新 session 已写入但旧缓存尚未清理”的中间状态。
    pub fn regenerate_session_id(&self, set_current_as_parent: bool) -> SessionId {
        let mut state = self.state.lock().unwrap();
        if set_current_as_parent {
            state.parent_session_id = Some(state.session_id.clone());
        }
        let old_session_id = state.session_id.clone();
        state.plan_slug_cache.remove(&old_session_id);
        state.session_id = Uuid::new_v4().to_string();
        state.session_project_dir = None;
        state.session_id.clone()
    }

    /// 切换到调用方提供的 session，并同步设置该 session 的项目目录覆盖值。
    ///
    /// 与 [`regenerate_session_id`](Self::regenerate_session_id) 不同，这个方法不会生成 ID，
    /// 因此常用于从外部恢复或继续一个已知的兼容 session。切换前只清理旧 session 的计划
    /// slug 缓存；它不会验证传入 ID 的真实性，也不会加载 EventLog。`project_dir` 为 `None`
    /// 表示使用默认项目根目录，非空值只作为兼容层上下文，仍需经过真正路径/权限边界。
    pub fn switch_session(&self, session_id: SessionId, project_dir: Option<PathBuf>) {
        let mut state = self.state.lock().unwrap();
        let old_session_id = state.session_id.clone();
        state.plan_slug_cache.remove(&old_session_id);
        state.session_id = session_id;
        state.session_project_dir = project_dir;
    }

    /// 获取进程启动时记录的原始工作目录。
    ///
    /// 原始目录和当前 `cwd` 有意分开：命令执行或 session 切换可能改变当前目录，但某些
    /// 兼容流程仍需要回到启动位置。返回值为克隆路径，不把锁带到调用方。
    pub fn get_original_cwd(&self) -> PathBuf {
        self.state.lock().unwrap().original_cwd.clone()
    }

    /// 获取兼容层记录的项目根目录提示。
    ///
    /// 这是状态读取，不是项目发现或 trust 检查。调用者不能仅凭该值决定可写范围；写操作
    /// 仍必须走 WorkPacket、PathLock 和 sandbox/profile 规定的边界。
    pub fn get_project_root(&self) -> PathBuf {
        self.state.lock().unwrap().project_root.clone()
    }

    /// 更新原始工作目录提示。
    ///
    /// 该方法只修改内存字段，没有切换进程的实际当前目录，也不触发路径规范化或权限检查。
    /// 调用者通常在入口确定工作区后使用；把它误认为 `std::env::set_current_dir` 会造成后续
    /// 相对路径理解不一致。
    pub fn set_original_cwd(&self, cwd: PathBuf) {
        self.state.lock().unwrap().original_cwd = cwd;
    }

    /// 更新兼容层项目根目录提示，不执行磁盘探测或授权。
    pub fn set_project_root(&self, root: PathBuf) {
        self.state.lock().unwrap().project_root = root;
    }

    /// 读取兼容层当前工作目录字段。
    ///
    /// 它表示状态中保存的目录，不保证等于操作系统此刻的进程 cwd；需要执行文件系统
    /// 操作的代码仍应使用明确的路径解析流程。
    pub fn get_cwd(&self) -> PathBuf {
        self.state.lock().unwrap().cwd.clone()
    }

    /// 更新兼容层当前工作目录字段，不直接改变进程 cwd。
    pub fn set_cwd(&self, cwd: PathBuf) {
        self.state.lock().unwrap().cwd = cwd;
    }

    /// 累加一次 API 调用的总耗时和去除重试后的耗时。
    ///
    /// 【输入语义】
    /// `duration` 包含本次尝试及其内部重试占用的时间；`duration_without_retries` 只代表
    /// 不把重试重复计入后的基准耗时。两个值同时保存，是为了让 UI/诊断区分“provider 慢”
    /// 和“客户端反复重试导致等待变长”。调用方必须保证两个值使用相同单位（当前字段约定
    /// 为毫秒），否则统计会失真。
    ///
    /// 【副作用】
    /// 只更新本地累计计数，不触发重试、限流或账号状态变化。
    pub fn add_to_total_duration(&self, duration: u64, duration_without_retries: u64) {
        let mut state = self.state.lock().unwrap();
        state.total_api_duration += duration;
        state.total_api_duration_without_retries += duration_without_retries;
    }

    /// 记录一次模型调用的本地成本估算和 token 用量。
    ///
    /// `model` 是本地聚合键，`usage` 是该模型的最新快照，写入 `HashMap::insert` 会覆盖同
    /// 名模型以前保存的快照；`cost` 则累加到 session 总额。这种组合意味着总金额与按模型
    /// token 表并不是严格可逆的账本，不能拿它替代 provider 账单或 EventLog receipt。
    /// 成本无法计算时，上层还应设置 `has_unknown_model_cost`，本方法本身不会推断该状态。
    pub fn add_to_total_cost(&self, cost: f64, usage: ModelUsage, model: String) {
        let mut state = self.state.lock().unwrap();
        state.model_usage.insert(model, usage);
        state.total_cost_usd += cost;
    }

    /// 读取当前 session 的本地美元成本估算。
    ///
    /// 返回 `f64` 只是展示/诊断值；浮点舍入和未知模型都可能使它与真实账单不同。
    pub fn get_total_cost_usd(&self) -> f64 {
        self.state.lock().unwrap().total_cost_usd
    }

    /// 读取包含重试的 API 累计耗时（毫秒约定）。
    pub fn get_total_api_duration(&self) -> u64 {
        self.state.lock().unwrap().total_api_duration
    }

    /// 计算从当前 `State::start_time` 到现在的进程内经过时间。
    ///
    /// 这个值不是 API 调用耗时之和，而是墙上时钟（wall-clock）差值，因此包含空闲时间、
    /// 本地处理和等待锁的时间。时间戳异常会在 `SystemTime` 转换处触发原有的 panic；这里
    /// 保持现有行为，不把它改成错误返回。
    pub fn get_total_duration(&self) -> u64 {
        let state = self.state.lock().unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        now - state.start_time
    }

    /// 累加一次工具执行的耗时，并增加当前 turn 的工具计数。
    ///
    /// `total_tool_duration` 跨 turn 保留，`turn_tool_duration_ms` 和 `turn_tool_count` 只供
    /// 当前 turn 的摘要使用。该方法只记录调用方传入的观测值，不表示工具的副作用已经被
    /// Capability Broker 接受或成功完成。
    pub fn add_to_tool_duration(&self, duration: u64) {
        let mut state = self.state.lock().unwrap();
        state.total_tool_duration += duration;
        state.turn_tool_duration_ms += duration;
        state.turn_tool_count += 1;
    }

    /// 累加当前 turn 的 Hook 耗时和 Hook 调用次数。
    ///
    /// Hook 是兼容入口的外围扩展观察点；这里的计数不会改变 ControlPlane 的授权结论。
    pub fn add_to_turn_hook_duration(&self, duration: u64) {
        let mut state = self.state.lock().unwrap();
        state.turn_hook_duration_ms += duration;
        state.turn_hook_count += 1;
    }

    /// 在开始新的 turn 或生成摘要后清零 Hook 的 turn 级统计。
    ///
    /// 该操作不影响跨 turn 的 API/tool 总耗时。
    pub fn reset_turn_hook_duration(&self) {
        let mut state = self.state.lock().unwrap();
        state.turn_hook_duration_ms = 0;
        state.turn_hook_count = 0;
    }

    /// 清零工具的 turn 级耗时和调用次数，保留累计工具耗时。
    pub fn reset_turn_tool_duration(&self) {
        let mut state = self.state.lock().unwrap();
        state.turn_tool_duration_ms = 0;
        state.turn_tool_count = 0;
    }

    /// 累加本地统计的新增/删除行数。
    ///
    /// 这些数字来自兼容层观察，可能只是某次工具输出或 diff 的估计，不是 EventLog 中可
    /// 验证的变更证明，也不会自行触发保存、提交或权限判断。
    pub fn add_to_total_lines_changed(&self, added: u64, removed: u64) {
        let mut state = self.state.lock().unwrap();
        state.total_lines_added += added;
        state.total_lines_removed += removed;
    }

    /// 汇总所有已缓存模型快照中的输入 token 数。
    ///
    /// 这是按模型表求和，不是从每一次请求事件重放出来的精确账单；某模型被重复覆盖时，
    /// 只保留当前快照的数值。
    pub fn get_total_input_tokens(&self) -> u64 {
        self.state
            .lock()
            .unwrap()
            .model_usage
            .values()
            .map(|u| u.input_tokens)
            .sum()
    }

    /// 汇总所有已缓存模型快照中的输出 token 数，语义与输入 token 汇总相同。
    pub fn get_total_output_tokens(&self) -> u64 {
        self.state
            .lock()
            .unwrap()
            .model_usage
            .values()
            .map(|u| u.output_tokens)
            .sum()
    }

    /// 设置入口是否处于交互式终端环境。
    ///
    /// 该标志影响兼容层展示/提示策略，不能被解释为用户已经批准某个 capability。
    pub fn set_is_interactive(&self, value: bool) {
        self.state.lock().unwrap().is_interactive = value;
    }

    /// 读取交互式终端标志。
    pub fn get_is_interactive(&self) -> bool {
        self.state.lock().unwrap().is_interactive
    }

    /// 记录本地 session 曾加载过的 skill 内容。
    ///
    /// 【调用者】
    /// 兼容 Runner/skill loader 在读取 skill 后调用，用于 UI 显示、去重或后续清理。
    ///
    /// 【键与覆盖】
    /// 键由 `agent_id` 和 `skill_name` 拼接而成；同一 agent 再次调用同名 skill 会覆盖旧
    /// 记录，而不是追加历史。`agent_id=None` 使用空前缀，代表主会话调用。
    ///
    /// 【信任边界】
    /// 路径和内容只是缓存，写入这里不等于 skill 已通过 ProjectTrust，也不等于内容可以
    /// 改变工具权限。真正的项目资源加载必须走受信检查和受控 capability 链路。
    pub fn add_invoked_skill(
        &self,
        skill_name: String,
        skill_path: String,
        content: String,
        agent_id: Option<String>,
    ) {
        let mut state = self.state.lock().unwrap();
        let key = format!("{}:{}", agent_id.as_deref().unwrap_or(""), skill_name);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        state.invoked_skills.insert(
            key,
            InvokedSkillInfo {
                skill_name,
                skill_path,
                content,
                invoked_at: now,
                agent_id,
            },
        );
    }

    /// 清理 skill 缓存，可选择保留指定 agent 的记录。
    ///
    /// `None` 表示全部清空；传入空集合也表示全部清空；非空集合只保留记录中带有这些
    /// agent ID 的项。这个三态语义服务于 session/agent 生命周期，不能简化成“只要传了
    /// `Some` 就保留全部”，否则旧 agent 的 skill 内容会泄漏到新上下文。
    pub fn clear_invoked_skills(&self, preserved_agent_ids: Option<&HashSet<String>>) {
        let mut state = self.state.lock().unwrap();
        if let Some(preserved) = preserved_agent_ids {
            if preserved.is_empty() {
                state.invoked_skills.clear();
                return;
            }
            state.invoked_skills.retain(|_, skill| {
                skill
                    .agent_id
                    .as_ref()
                    .map_or(false, |id| preserved.contains(id))
            });
        } else {
            state.invoked_skills.clear();
        }
    }

    /// 标记下一次读取时需要处理 compaction 后续逻辑。
    ///
    /// compaction 会压缩上下文展示，但不会改变 EventLog 事实；这里的布尔值只是兼容层的
    /// 一次性提示。多次标记不会累计次数，只保留“至少需要处理一次”的状态。
    pub fn mark_post_compaction(&self) {
        self.state.lock().unwrap().pending_post_compaction = true;
    }

    /// 原子地读取并清除 compaction 后续标记。
    ///
    /// 返回 `true` 表示调用者应在当前流程补一次 compaction 后处理；无论返回值如何，标记
    /// 都会被清零，因此连续两次读取只会消费一次。读写在同一锁内完成，避免两个线程同时
    /// 把同一个一次性提示当成两次事件。
    pub fn consume_post_compaction(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        let was = state.pending_post_compaction;
        state.pending_post_compaction = false;
        was
    }

    /// 重置 session 级成本、耗时、行数和 token 快照。
    ///
    /// 通常在新的计费/统计窗口开始时调用。它还重置 `start_time` 和兼容 `prompt_id`，但
    /// 不会切换 session ID、清理 skill、删除 EventLog 或撤销正在执行的请求；这些生命周期
    /// 操作由各自的上层流程负责。
    pub fn reset_cost_state(&self) {
        let mut state = self.state.lock().unwrap();
        state.total_cost_usd = 0.0;
        state.total_api_duration = 0;
        state.total_api_duration_without_retries = 0;
        state.total_tool_duration = 0;
        state.start_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        state.total_lines_added = 0;
        state.total_lines_removed = 0;
        state.has_unknown_model_cost = false;
        state.model_usage.clear();
        state.prompt_id = None;
    }

    /// 清除一次请求级 beta header 的“已经写入”锁存位。
    ///
    /// 这些字段防止同一个兼容请求重复追加 AFK/fast/cache-editing/thinking-clear header。
    /// 清除它们只影响下一次构造请求时的本地判断，不会撤销已发送的 HTTP 请求，也不会改变
    /// provider 或 ControlPlane 的状态。
    pub fn clear_beta_header_latches(&self) {
        let mut state = self.state.lock().unwrap();
        state.afk_mode_header_latched = None;
        state.fast_mode_header_latched = None;
        state.cache_editing_header_latched = None;
        state.thinking_clear_latched = None;
    }
}

impl Default for GlobalState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_id_generation() {
        let state = GlobalState::new();
        let id1 = state.get_session_id();
        let id2 = state.regenerate_session_id(false);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_cost_tracking() {
        let state = GlobalState::new();
        let usage = ModelUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_input_tokens: 10,
            cache_creation_input_tokens: 5,
            web_search_requests: 2,
        };
        state.add_to_total_cost(1.5, usage, "test-model".to_string());
        assert_eq!(state.get_total_cost_usd(), 1.5);
        assert_eq!(state.get_total_input_tokens(), 100);
        assert_eq!(state.get_total_output_tokens(), 50);
    }

    #[test]
    fn test_invoked_skills() {
        let state = GlobalState::new();
        state.add_invoked_skill(
            "test_skill".to_string(),
            "/path/to/skill".to_string(),
            "content".to_string(),
            None,
        );
        assert_eq!(state.state.lock().unwrap().invoked_skills.len(), 1);
    }

    #[test]
    fn test_post_compaction_flag() {
        let state = GlobalState::new();
        assert!(!state.consume_post_compaction());
        state.mark_post_compaction();
        assert!(state.consume_post_compaction());
        assert!(!state.consume_post_compaction());
    }
}
