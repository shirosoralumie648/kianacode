use crate::ids::fnv1a64;
use crate::{
    normalize_role_path, CellId, RequestId, SessionId, CLOSING_DECISION_PATH, CLOSING_PATH_LESSONS,
    DECISION_RECORD_PATH, DEPARTMENT_CLOSING, DEPARTMENT_EXECUTING, DEPARTMENT_INITIATING,
    DEPARTMENT_MONITORING, DEPARTMENT_PLANNING, EXECUTING_DECISION_PATH, EXECUTING_PATH_RECEIPT,
    INITIATING_DECISION_PATH, MEMORY_COLLECTION_PLANNING_UNRELEASED, MEMORY_COLLECTION_USER_PREFS,
    MEMORY_COLLECTION_USER_PRIVATE, MEMORY_LAYER_COMPANY, MEMORY_LAYER_DEPARTMENT,
    MEMORY_LAYER_INSTANCE_SCRATCH, MEMORY_LAYER_PROJECT, MEMORY_LAYER_ROLE, MEMORY_LAYER_USER,
    MONITORING_DECISION_PATH, MONITORING_PATH_GATE, PLANNING_PATH_CHARTER, PLANNING_PATH_PACKET,
    PLANNING_PATH_PLAN, ROLE_ARCHITECT, ROLE_BUILDER, ROLE_CLOSER, ROLE_PM, ROLE_REVIEWER,
    ROLE_SANDBOX_READ_ONLY, ROLE_SANDBOX_WORKSPACE_WRITE, ROLE_SPONSOR,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 结构化会话消息的发送者类型。
pub enum ConversationRole {
    /// 用户提交的提示。
    User,
    /// 模型生成的文本。
    Assistant,
    /// 工具或能力执行结果；不代表执行成功。
    Tool,
}

/// 可沿协议传递的会话历史消息。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConversationMessage {
    /// 消息发送者角色。
    pub role: ConversationRole,
    /// 消息正文或工具结果文字。
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// 可选工具调用 ID，用于把工具结果关联回某次模型调用。
    pub tool_call_id: Option<String>,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 请求可声明的权限档位；它本身不构成授权。
pub enum PermissionProfile {
    #[default]
    /// 最保守档位，默认拒绝高风险或未明确授权动作。
    Safe,
    /// 允许经过策略与审批的有限动作。
    Balanced,
    /// 请求更宽能力，但仍不能跳过硬拒绝和 ControlPlane。
    Autonomous,
}

pub(crate) fn default_role_id() -> String {
    ROLE_BUILDER.to_owned()
}

pub(crate) fn default_department_id() -> String {
    DEPARTMENT_EXECUTING.to_owned()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
/// 一次协议请求在控制平面中的身份、信任和范围快照。
pub struct RequestContext {
    /// 当前请求的稳定 ID。
    pub request_id: RequestId,
    /// 所属会话 ID。
    pub session_id: SessionId,
    /// 项目根目录文字；消费方必须进一步规范化。
    pub project_root: String,
    /// 发起请求的主体标识。
    pub actor_id: Option<String>,
    /// 项目资源是否已通过 trust 检查。
    pub project_trusted: bool,
    /// 请求声明的权限档位，不能单独授予能力。
    pub permission_profile: PermissionProfile,
    #[serde(default = "default_role_id")]
    /// 角色 ID。
    pub role_id: String,
    #[serde(default = "default_department_id")]
    /// 部门 ID。
    pub department_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// 可选 WorkPacket ID。
    pub work_packet_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// 可选执行 cell ID。
    pub cell_id: Option<CellId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// 请求允许触达的路径集合；实际范围应与所有上游边界取交集。
    pub path_allow: Vec<String>,
}

impl RequestContext {
    /// 创建默认本地上下文：未信任项目、Safe 权限、Builder/Executing 身份。
    pub fn local(session_id: impl Into<String>, project_root: impl Into<String>) -> Self {
        Self {
            request_id: RequestId::new(),
            session_id: SessionId::new(session_id),
            project_root: project_root.into(),
            actor_id: Some("local-user".to_owned()),
            project_trusted: false,
            permission_profile: PermissionProfile::Safe,
            role_id: default_role_id(),
            department_id: default_department_id(),
            work_packet_id: None,
            cell_id: None,
            path_allow: Vec::new(),
        }
    }

    /// 将角色和部门字段同步为给定角色快照。
    pub fn assign_role(&mut self, role: &RoleSpec) {
        self.role_id = role.role_id.clone();
        self.department_id = role.department_id.clone();
    }
}

pub struct RoleSpec {
    /// 稳定角色标识，如 `builder` 或 `reviewer`。
    pub role_id: String,
    /// 所属部门标识。
    pub department_id: String,
    /// 注入模型的角色指令文字。
    pub prompt: String,
    /// 角色指令的内容指纹，用于检测指令漂移。
    pub prompt_hash: String,
    /// 模型可见工具名称白名单。
    pub tools: Vec<String>,
    /// 角色默认沙箱档位。
    pub sandbox: String,
    /// 角色可触达的相对路径前缀。
    pub path_allow: Vec<String>,
    /// 角色可读取的知识集合授权。
    pub knowledge_grants: Vec<String>,
    /// 是否可以主持 symposium。
    pub can_convene: bool,
    /// 是否可以参与投票。
    pub can_vote: bool,
    /// 角色对应的模型配置档位名。
    pub model_profile: String,
    /// 单个 harness 回合允许的最大步骤数。
    pub max_steps: u32,
}

impl RoleSpec {
    /// 返回执行 Builder 的固定角色快照。
    pub fn builder() -> Self {
        Self::new(
            ROLE_BUILDER,
            DEPARTMENT_EXECUTING,
            "You are Kiana's executing Builder. Use only the provided tools `shell`, `apply_patch`, `mcp`, `memory.search`, and `memory.write`. Never request danger-full-access.",
            vec![
                "shell".to_owned(),
                "apply_patch".to_owned(),
                "mcp".to_owned(),
                "memory.search".to_owned(),
                "memory.write".to_owned(),
            ],
            ROLE_SANDBOX_WORKSPACE_WRITE,
            vec![".".to_owned()],
            vec![
                MEMORY_LAYER_COMPANY.to_owned(),
                MEMORY_LAYER_PROJECT.to_owned(),
                "role:builder".to_owned(),
                "scratch".to_owned(),
            ],
            true,
            false,
            "executing",
            32,
        )
    }

    /// 返回规划 PM 的固定角色快照。
    pub fn pm() -> Self {
        Self::new(
            ROLE_PM,
            DEPARTMENT_PLANNING,
            "You are Kiana's planning PM. Write only charter/plan/packet artifacts. Do not patch source files. Do not run shell.",
            vec![
                "apply_patch".to_owned(),
                "memory.search".to_owned(),
                "memory.write".to_owned(),
            ],
            ROLE_SANDBOX_WORKSPACE_WRITE,
            vec![
                PLANNING_PATH_CHARTER.to_owned(),
                PLANNING_PATH_PLAN.to_owned(),
                PLANNING_PATH_PACKET.to_owned(),
            ],
            vec![
                MEMORY_LAYER_COMPANY.to_owned(),
                "department:planning".to_owned(),
                MEMORY_LAYER_PROJECT.to_owned(),
                MEMORY_COLLECTION_USER_PREFS.to_owned(),
            ],
            true,
            true,
            "planning",
            8,
        )
    }

    /// 返回只读规划 Architect 的固定角色快照。
    pub fn architect() -> Self {
        Self::new(
            ROLE_ARCHITECT,
            DEPARTMENT_PLANNING,
            "You are Kiana's planning Architect. Read and advise. Do not write files or run shell.",
            vec!["memory.search".to_owned()],
            ROLE_SANDBOX_READ_ONLY,
            Vec::new(),
            vec![
                MEMORY_LAYER_COMPANY.to_owned(),
                "department:planning".to_owned(),
                MEMORY_LAYER_PROJECT.to_owned(),
            ],
            false,
            true,
            "planning",
            8,
        )
    }

    /// 返回监控 Reviewer 的固定角色快照。
    pub fn reviewer() -> Self {
        Self::new(
            ROLE_REVIEWER,
            DEPARTMENT_MONITORING,
            "You are Kiana's monitoring Reviewer. Compare the author receipt to acceptance. Do not patch source or run shell. You are never the author.",
            vec!["memory.search".to_owned()],
            ROLE_SANDBOX_READ_ONLY,
            Vec::new(),
            vec![
                MEMORY_LAYER_COMPANY.to_owned(),
                "department:monitoring".to_owned(),
                "project:events".to_owned(),
            ],
            true,
            false,
            "monitoring",
            8,
        )
    }

    /// 返回 initiating Sponsor 的固定角色快照。
    pub fn sponsor() -> Self {
        Self::new(
            ROLE_SPONSOR,
            DEPARTMENT_INITIATING,
            "You are Kiana's initiating Sponsor. Write only charter artifacts. Do not patch source files. Do not run shell.",
            vec![
                "apply_patch".to_owned(),
                "memory.search".to_owned(),
                "memory.write".to_owned(),
            ],
            ROLE_SANDBOX_WORKSPACE_WRITE,
            vec![PLANNING_PATH_CHARTER.to_owned()],
            vec![
                MEMORY_LAYER_COMPANY.to_owned(),
                "department:initiating".to_owned(),
                MEMORY_COLLECTION_USER_PREFS.to_owned(),
            ],
            true,
            true,
            "initiating",
            8,
        )
    }

    /// 返回 closing Closer 的固定角色快照。
    pub fn closer() -> Self {
        Self::new(
            ROLE_CLOSER,
            DEPARTMENT_CLOSING,
            "You are Kiana's closing Closer. Write only lessons artifacts. Do not patch source files. Do not run shell.",
            vec![
                "apply_patch".to_owned(),
                "memory.search".to_owned(),
                "memory.write".to_owned(),
            ],
            ROLE_SANDBOX_WORKSPACE_WRITE,
            vec![CLOSING_PATH_LESSONS.to_owned()],
            vec![
                MEMORY_LAYER_COMPANY.to_owned(),
                "department:closing".to_owned(),
                "project:events".to_owned(),
            ],
            true,
            false,
            "closing",
            8,
        )
    }

    /// 返回全部内置角色，顺序固定用于目录和审计展示。
    pub fn catalog() -> [RoleSpec; 6] {
        [
            Self::sponsor(),
            Self::pm(),
            Self::architect(),
            Self::builder(),
            Self::reviewer(),
            Self::closer(),
        ]
    }

    /// 按角色 ID 查找内置角色；空 ID 默认返回 Builder。
    pub fn lookup(role_id: &str) -> Option<Self> {
        let role_id = role_id.trim();
        if role_id.is_empty() {
            return Some(Self::builder());
        }
        Self::catalog()
            .into_iter()
            .find(|role| role.role_id == role_id)
    }

    /// 判断角色白名单是否包含指定模型工具名称。
    pub fn allows_tool(&self, tool: &str) -> bool {
        self.tools.iter().any(|allowed| allowed == tool)
    }

    /// 判断角色路径白名单是否覆盖给定相对路径。
    ///
    /// 路径会先经过 `normalize_role_path`；非法绝对路径和目录穿越返回 false。
    pub fn allows_path(&self, path: &str) -> bool {
        let Some(path) = normalize_role_path(path) else {
            return false;
        };
        if self
            .path_allow
            .iter()
            .any(|allow| allow == "." || allow == "*")
        {
            return true;
        }
        self.path_allow.iter().any(|allow| {
            let allow = allow.trim().trim_matches('/');
            !allow.is_empty() && (path == allow || path.starts_with(&format!("{allow}/")))
        })
    }

    /// 判断角色默认沙箱是否为项目内可写档位。
    pub fn workspace_write_allowed(&self) -> bool {
        self.sandbox == ROLE_SANDBOX_WORKSPACE_WRITE
    }

    /// 判断角色是否获准读取指定知识集合。
    pub fn allows_knowledge(&self, collection: &str) -> bool {
        let Some(parsed) = MemoryCollection::parse(collection) else {
            return false;
        };
        self.knowledge_grants
            .iter()
            .any(|grant| MemoryCollection::parse(grant).is_some_and(|grant| grant.covers(&parsed)))
    }

    /// 将角色知识授权解析为可审计的集合对象。
    pub fn granted_collections(&self) -> Vec<MemoryCollection> {
        self.knowledge_grants
            .iter()
            .filter_map(|grant| MemoryCollection::parse(grant))
            .collect()
    }

    /// 判断角色是否可以向指定知识集合写入。
    ///
    /// 写入规则按角色硬编码为最小范围；未知角色和只读角色默认拒绝。
    pub fn allows_memory_write(&self, collection: &str) -> bool {
        let Some(parsed) = MemoryCollection::parse(collection) else {
            return false;
        };
        match self.role_id.as_str() {
            ROLE_BUILDER => parsed.collection == MEMORY_LAYER_INSTANCE_SCRATCH,
            ROLE_PM => {
                parsed.collection == "department:planning"
                    || parsed.collection == MEMORY_COLLECTION_PLANNING_UNRELEASED
            }
            ROLE_SPONSOR => parsed.collection == "department:initiating",
            ROLE_CLOSER => parsed.collection == "department:closing",
            _ => false,
        }
    }

    fn new(
        role_id: &str,
        department_id: &str,
        prompt: &str,
        tools: Vec<String>,
        sandbox: &str,
        path_allow: Vec<String>,
        knowledge_grants: Vec<String>,
        can_convene: bool,
        can_vote: bool,
        model_profile: &str,
        max_steps: u32,
    ) -> Self {
        Self {
            role_id: role_id.to_owned(),
            department_id: department_id.to_owned(),
            prompt_hash: prompt_hash(prompt),
            prompt: prompt.to_owned(),
            tools,
            sandbox: sandbox.to_owned(),
            path_allow,
            knowledge_grants,
            can_convene,
            can_vote,
            model_profile: model_profile.to_owned(),
            max_steps,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
/// 六层知识记忆中的一个规范化集合地址。
pub struct MemoryCollection {
    /// 记忆层级，例如 `project` 或 `user`。
    pub layer: String,
    /// 层内集合名，例如 `project:code`。
    pub collection: String,
}

impl MemoryCollection {
    /// 解析允许的集合别名；未知、空或非法集合返回 `None`。
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim().trim_matches('/');
        if raw.is_empty() {
            return None;
        }
        let (layer, collection) = match raw {
            MEMORY_LAYER_COMPANY => (MEMORY_LAYER_COMPANY, MEMORY_LAYER_COMPANY),
            MEMORY_LAYER_DEPARTMENT => (MEMORY_LAYER_DEPARTMENT, MEMORY_LAYER_DEPARTMENT),
            MEMORY_LAYER_ROLE => (MEMORY_LAYER_ROLE, MEMORY_LAYER_ROLE),
            MEMORY_LAYER_PROJECT | "project:code" | "project:docs" | "project:events" => {
                (MEMORY_LAYER_PROJECT, raw)
            }
            MEMORY_LAYER_USER | MEMORY_COLLECTION_USER_PREFS | MEMORY_COLLECTION_USER_PRIVATE => {
                (MEMORY_LAYER_USER, raw)
            }
            MEMORY_LAYER_INSTANCE_SCRATCH | "scratch" => {
                (MEMORY_LAYER_INSTANCE_SCRATCH, MEMORY_LAYER_INSTANCE_SCRATCH)
            }
            MEMORY_COLLECTION_PLANNING_UNRELEASED => (
                MEMORY_LAYER_DEPARTMENT,
                MEMORY_COLLECTION_PLANNING_UNRELEASED,
            ),
            other if other.starts_with("department:") => (MEMORY_LAYER_DEPARTMENT, other),
            other if other.starts_with("role:") => (MEMORY_LAYER_ROLE, other),
            _ => return None,
        };
        Some(Self {
            layer: layer.to_owned(),
            collection: collection.to_owned(),
        })
    }

    /// 判断当前授权集合是否覆盖请求集合。
    pub fn covers(&self, requested: &MemoryCollection) -> bool {
        if self.collection == requested.collection {
            return true;
        }
        if self.collection == MEMORY_LAYER_PROJECT
            && matches!(
                requested.collection.as_str(),
                MEMORY_LAYER_PROJECT | "project:code" | "project:docs"
            )
        {
            return true;
        }
        if self.collection == "scratch" && requested.collection == MEMORY_LAYER_INSTANCE_SCRATCH {
            return true;
        }
        if self.collection == "department:planning"
            && requested.collection == MEMORY_COLLECTION_PLANNING_UNRELEASED
        {
            return true;
        }
        false
    }

    /// 判断集合是否属于用户/公司 home 范围。
    pub fn home_scoped(&self) -> bool {
        matches!(
            self.layer.as_str(),
            MEMORY_LAYER_COMPANY | MEMORY_LAYER_USER
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
/// 部门的职责、角色、产物和 gate 配置快照。
pub struct DepartmentSpec {
    /// 部门稳定 ID。
    pub department_id: String,
    /// PMP 分组标识。
    pub pmp_group: String,
    /// 部门任务描述。
    pub mission: String,
    /// 部门可包含的角色 ID。
    pub roles: Vec<String>,
    /// 部门允许写入或读取的产物路径。
    pub artifacts: Vec<String>,
    /// 部门默认知识集合。
    pub rag_collection: String,
    /// 是否允许主持 symposium。
    pub can_convene: bool,
    /// 部门必须满足的 gate 名称。
    pub gates: Vec<String>,
}

impl DepartmentSpec {
    fn new(
        department_id: &str,
        mission: &str,
        roles: Vec<String>,
        artifacts: Vec<String>,
        can_convene: bool,
        gates: Vec<String>,
    ) -> Self {
        Self {
            department_id: department_id.to_owned(),
            pmp_group: department_id.to_owned(),
            mission: mission.to_owned(),
            roles,
            artifacts,
            rag_collection: format!("department:{department_id}"),
            can_convene,
            gates,
        }
    }

    /// 返回 executing 部门配置。
    pub fn executing() -> Self {
        Self::new(
            DEPARTMENT_EXECUTING,
            "Execute work packets. Write files and run allowed verification. Do not change acceptance.",
            vec![ROLE_BUILDER.to_owned()],
            vec![".".to_owned(), EXECUTING_PATH_RECEIPT.to_owned()],
            true,
            vec!["receipt".to_owned()],
        )
    }

    /// 返回 planning 部门配置。
    pub fn planning() -> Self {
        Self::new(
            DEPARTMENT_PLANNING,
            "Turn a charter into work packets with frozen acceptance. Do not implement source.",
            vec![ROLE_PM.to_owned(), ROLE_ARCHITECT.to_owned()],
            vec![
                PLANNING_PATH_CHARTER.to_owned(),
                PLANNING_PATH_PLAN.to_owned(),
                PLANNING_PATH_PACKET.to_owned(),
            ],
            true,
            vec!["packet_has_acceptance".to_owned()],
        )
    }

    /// 返回 monitoring 部门配置。
    pub fn monitoring() -> Self {
        Self::new(
            DEPARTMENT_MONITORING,
            "Review author receipts against acceptance. The reviewer is never the author.",
            vec![ROLE_REVIEWER.to_owned()],
            vec![MONITORING_PATH_GATE.to_owned()],
            true,
            vec!["review_packet".to_owned()],
        )
    }

    /// 返回 initiating 部门配置。
    pub fn initiating() -> Self {
        Self::new(
            DEPARTMENT_INITIATING,
            "Decide whether the work should exist and what success looks like. Do not write source.",
            vec![ROLE_SPONSOR.to_owned()],
            vec![PLANNING_PATH_CHARTER.to_owned()],
            true,
            vec!["charter_has_success_criteria".to_owned()],
        )
    }

    /// 返回 closing 部门配置。
    pub fn closing() -> Self {
        Self::new(
            DEPARTMENT_CLOSING,
            "Record lessons and close the receipt. Do not write source.",
            vec![ROLE_CLOSER.to_owned()],
            vec![CLOSING_PATH_LESSONS.to_owned()],
            true,
            vec!["lessons_logged".to_owned()],
        )
    }

    /// 返回全部内置部门，顺序固定用于目录和审计展示。
    pub fn catalog() -> [DepartmentSpec; 5] {
        [
            Self::initiating(),
            Self::planning(),
            Self::executing(),
            Self::monitoring(),
            Self::closing(),
        ]
    }

    /// 按部门 ID 查找内置部门；空 ID 默认返回 executing。
    pub fn lookup(department_id: &str) -> Option<Self> {
        let department_id = department_id.trim();
        if department_id.is_empty() {
            return Some(Self::executing());
        }
        Self::catalog()
            .into_iter()
            .find(|department| department.department_id == department_id)
    }

    /// 返回该部门中第一个可主持 symposium 的角色。
    pub fn convene_chair_id(&self) -> Option<String> {
        self.roles.iter().find_map(|role_id| {
            RoleSpec::lookup(role_id)
                .filter(|role| role.can_convene)
                .map(|role| role.role_id)
        })
    }

    /// 返回该部门决策记录的相对路径。
    pub fn decision_path(&self) -> &'static str {
        match self.department_id.as_str() {
            DEPARTMENT_INITIATING => INITIATING_DECISION_PATH,
            DEPARTMENT_PLANNING => DECISION_RECORD_PATH,
            DEPARTMENT_EXECUTING => EXECUTING_DECISION_PATH,
            DEPARTMENT_MONITORING => MONITORING_DECISION_PATH,
            DEPARTMENT_CLOSING => CLOSING_DECISION_PATH,
            _ => DECISION_RECORD_PATH,
        }
    }
}
pub fn prompt_hash(prompt: &str) -> String {
    format!("fnv1a64:{:016x}", fnv1a64(prompt.as_bytes()))
}
