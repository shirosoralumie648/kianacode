//! ControlPlane 内部对能力请求进行规范化、授权、派发和结果收尾。
//!
//! 普通 Harness 工具请求的主路径是 `lifecycle.rs::drive_run` 收到
//! `RunnerEvent::CapabilityRequested` 后调用本文件的
//! `broker_harness_capability`：这里重新整理请求、运行策略与关卡、必要时挂起审批，
//! 然后才把获准请求交给 `dispatch.rs::dispatch_authorized`，并把执行结果交回 Runner。
//! 旧的直接能力请求则从 `approvals.rs::authorize_and_execute` 进入，共用准备、授权、
//! 审批事件和派发收尾逻辑。两种入口都在 Core 判断权限；真正的 handler 执行位于 Broker。
//!
//! 阅读时先看 `broker_harness_capability` 与 `dispatch_capability_action`，再看
//! `prepare_capability_action_cancellable` 如何净化调用方数据并绑定服务端 authority，
//! 最后看 `stage_capability_action` 和 `finalize_capability_action` 如何持久化审批与结果。
use super::capability_scheduler::{AdmissionLease, AdmissionOutcome};
use super::events::*;
use super::redaction::*;
use super::*;
use kiana_policy::GrantScope;

const SWARM_CHILD_OPERATION: &str = "builder.packet";

/// 把授予中的路径约束转换为统一 ScopeDimension，并先规范化、排序和去重。
/// 空列表表示这一层没有额外路径约束；任何非法路径都会让整份授予派生失败。
fn grant_paths_dimension(
    paths: &[String],
    field: &str,
) -> Result<kiana_domain::ScopeDimension, String> {
    if paths.is_empty() {
        return Ok(kiana_domain::ScopeDimension::NotApplicable);
    }
    // 统一词法路径形式后再排序去重，使后续 scope digest 不受输入顺序和重复项影响。
    let mut normalized = paths
        .iter()
        .map(|path| {
            kiana_domain::normalize_role_path(path).ok_or_else(|| format!("{field}_path_invalid"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    normalized.sort();
    normalized.dedup();
    if normalized.is_empty() {
        return Err(format!("{field}_path_empty"));
    }
    Ok(kiana_domain::ScopeDimension::Restricted(normalized))
}

/// 为 swarm 子代理构造一层可参加权限交集的 GrantScope。
/// 这里只表达单层授权条件，不会创建 Cell，也不直接授予执行许可。
fn grant_scope_layer(
    principal_id: kiana_domain::PrincipalId,
    project_id: kiana_domain::ProjectId,
    authority_epoch: u64,
    paths: &[String],
    data_scope: &[String],
    expires_at_unix_ms: u64,
    delegation_allowed: bool,
) -> Result<GrantScope, String> {
    // 数据命名空间和文件路径属于不同 scope 维度，分别保留，避免跨维度误合并。
    let namespaces = if data_scope.is_empty() {
        kiana_domain::ScopeDimension::NotApplicable
    } else {
        kiana_domain::ScopeDimension::Restricted(data_scope.to_vec())
    };
    // 子代理的操作类型固定为 builder.packet；其他维度只从调用者提供的受限集合生成。
    let scope = kiana_domain::ScopeSet::new(
        kiana_domain::ScopeDimension::Restricted(vec![SWARM_CHILD_OPERATION.to_owned()]),
        grant_paths_dimension(paths, "grant_scope")?,
        namespaces,
        kiana_domain::ScopeDimension::NotApplicable,
        kiana_domain::ScopeLimit::NotApplicable,
        kiana_domain::ScopeLimit::NotApplicable,
    )?;
    GrantScope::new(
        kiana_domain::GrantId::new(),
        None,
        principal_id,
        project_id,
        scope,
        vec![kiana_domain::CapabilityKind::Other("coding".to_owned())],
        false,
        false,
        delegation_allowed,
        authority_epoch,
        expires_at_unix_ms,
    )
}

/// 从父级授予派生出**子代理**的授予。
///
/// 【调用者】`collaboration.rs` 在建立 swarm 子代理前调用；生成的 Grant 仍需由调用方在 Cell registry
/// 中原子预留，单独调用本函数不会创建 Cell、Broker permit 或模型 session。
/// 【输入】父级 CapabilityGrant、模板和角色目录项、项目路径、WorkPacket、principal/project ID、
/// 当前 authority epoch、可选审批 ID 和当前时间。
/// 【输出】通过全部授权层校验后的子代理 CapabilityGrant；任何角色/模板漂移、过期、路径、项目或
/// 父级授权不满足时返回稳定原因字符串。
/// 【核心流程】验证 authority 与输入对象 → 固定 builder/执行部门/模板关系 → 检查 packet 项目和时限
/// → 分别构造父级、模板、部门、项目、packet 与 approval scope → 求交集并验证子授权不扩大权限。
/// 【设计原因】各授权来源保持独立维度，避免父级或角色一个来源替代项目、packet、审批等其他限制。
///
/// 【这个函数是「权限交集」规则的一个具体实现】
/// 仓库宪法那句「子 Cell 权限只能是父级、模板、部门、项目、packet、approval 的
/// **交集**」，在这里变成一段代码：输入是父级授予、AgentTemplate、RoleSpec、
/// 项目路径、WorkPacket 与 principal，输出是一份**每一项都不超过**父级的授予。
///
/// 与 `cell_registry.rs` 里 `budget_is_subset` 同一个原则：**逐项 `<=`**，
/// 而不是「取交集值」。区别在于，静默收窄会让调用方以为自己申请到了
/// 请求的额度，而实际上没有——所以宁可拒绝。
///
/// 【它先验证输入，再看角色】
/// 顺序是：authority_epoch 非零 → 父级授予合法 → 模板合法 → 角色合法
/// → 角色确实是 builder / 执行部门、模板与角色对得上……
///
/// 也就是说，**先确认「这是一个被授权过的角色」**，再谈它能拿到什么。
/// 反过来做的话，一个随便什么角色都能先拿到一份授予再被检查。
pub fn derive_swarm_child_grant(
    parent: &kiana_domain::CapabilityGrant,
    template: &kiana_domain::AgentTemplate,
    role: &kiana_domain::RoleSpec,
    project_paths: &[String],
    packet: &kiana_domain::WorkPacket,
    principal_id: kiana_domain::PrincipalId,
    project_id: kiana_domain::ProjectId,
    authority_epoch: u64,
    approval_id: Option<kiana_domain::ApprovalId>,
    now_unix_ms: u64,
) -> Result<kiana_domain::CapabilityGrant, String> {
    if authority_epoch == 0 {
        return Err("swarm_authority_epoch_missing".to_owned());
    }
    parent.validate().map_err(|error| error.to_owned())?;
    template.validate().map_err(|error| error.to_owned())?;
    role.validate()?;
    if role.role_id != kiana_domain::ROLE_BUILDER
        || role.department_id != kiana_domain::DEPARTMENT_EXECUTING
        || template.role_id != role.role_id
        || template.version != kiana_domain::SWARM_CHILD_TEMPLATE
        || !parent.delegation_allowed
        || parent.operation != SWARM_CHILD_OPERATION
        || parent.capability != kiana_domain::CapabilityKind::Other("coding".to_owned())
    {
        return Err("swarm_controller_delegation_invalid".to_owned());
    }
    // 模板声明的能力必须与受信任角色定义完全一致，不能让模板单独扩展角色权限。
    let mut expected_tools = role.tools.clone();
    let mut template_tools = template.default_capabilities.clone();
    expected_tools.sort();
    template_tools.sort();
    if expected_tools != template_tools {
        return Err("swarm_template_role_capability_drift".to_owned());
    }
    if packet
        .path_allow
        .iter()
        .any(|path| kiana_domain::normalize_role_path(path).is_none())
    {
        return Err("swarm_packet_path_invalid".to_owned());
    }
    if packet
        .project_id
        .is_some_and(|packet_project| packet_project != project_id)
    {
        return Err("swarm_packet_project_mismatch".to_owned());
    }
    let packet_expiry = packet
        .deadline_unix_ms
        .unwrap_or(parent.expires_at_unix_ms)
        .min(parent.expires_at_unix_ms);
    if packet_expiry <= now_unix_ms {
        return Err("swarm_packet_authority_expired".to_owned());
    }

    // 每一层单独建模后统一求交集；显式保留模板和部门层，避免未来修改时相互代替。
    let parent_scope =
        GrantScope::from_capability_grant(parent, principal_id, project_id, authority_epoch)?;
    // 模板层和部门层即使当前都引用 RoleSpec 路径，也必须分开保留；否则未来修改模板时，
    // 可能无意中把模板变化当成部门策略授权。
    let template_scope = grant_scope_layer(
        principal_id,
        project_id,
        authority_epoch,
        &role.path_allow,
        &[],
        // ttl_seconds 用秒表示；乘 1,000 转成毫秒后与 packet 的绝对截止时间取较早者。
        packet_expiry.min(now_unix_ms.saturating_add(template.ttl_seconds.saturating_mul(1_000))),
        false,
    )?;
    let department_scope = grant_scope_layer(
        principal_id,
        project_id,
        authority_epoch,
        &role.path_allow,
        &[],
        packet_expiry,
        false,
    )?;
    let project_scope = grant_scope_layer(
        principal_id,
        project_id,
        authority_epoch,
        project_paths,
        &[],
        packet_expiry,
        false,
    )?;
    let packet_scope = grant_scope_layer(
        principal_id,
        project_id,
        authority_epoch,
        &packet.path_allow,
        &packet.data_scope,
        packet_expiry,
        false,
    )?;
    // 审批是独立的收窄层，结果 Grant 会绑定 approval_id；审批命令/引用必须来自服务端审批流程，
    // 不能把模型输出或 packet 文本当作授权来源。
    let approval_scope = grant_scope_layer(
        principal_id,
        project_id,
        authority_epoch,
        &packet.path_allow,
        &packet.data_scope,
        packet_expiry,
        false,
    )?;
    // 子授权只能同时满足所有 authority layer；任何一层缩小都会缩小最终 Scope。
    let derived = GrantScope::intersect_all(&[
        parent_scope,
        template_scope,
        department_scope,
        project_scope,
        packet_scope,
        approval_scope,
    ])?;
    // 资源集合沿用父级范围；父级未列出资源时此路径按源码约定落到 workspace。
    let resources = if parent.resources.is_empty() {
        vec!["workspace".to_owned()]
    } else {
        parent.resources.clone()
    };
    let child = derived.to_capability_grant(
        kiana_domain::CapabilityKind::Other("coding".to_owned()),
        SWARM_CHILD_OPERATION,
        resources,
        approval_id.or(parent.approval_id),
    )?;
    if !parent.contains(&child) {
        return Err("swarm_child_grant_not_contained".to_owned());
    }
    Ok(child)
}

/// 把单个动作声明的目标与当前请求上下文合成为策略可以比较的有效权限范围。
///
/// 【调用者】`prepare_capability_action_cancellable()` 在请求规范化和 hook 后调用。
/// 【输入/输出】服务端上下文与已规范化 CapabilityRequest；返回 operation、path、collection、server
/// 等维度组成的 ScopeSet。
/// 【数据流】从动作参数抽取并规范化具体资源路径 → 把操作、collection、MCP server 投影到 scope 维度
/// → 将上下文允许路径单独构造成另一组维度 → 只在路径维度求交集。
/// 【失败情况】路径无法规范化或 ScopeSet/交集无效时返回结构化 CoreError，授权和派发不会继续。
/// 【为什么这样做】动作参数描述“想访问什么”，RequestContext 描述“当前身份最多能访问什么”；
/// 两者必须交集，不能让用户输入中的路径取代服务端限制，也不能把不存在的维度误当成通配权限。
fn effective_action_scope(
    context: &RequestContext,
    request: &CapabilityRequest,
) -> Result<kiana_domain::ScopeSet, CoreError> {
    // capability_request_paths 提取该操作声明的文件目标；规范化失败时不能继续授权。
    let action_paths = kiana_domain::capability_request_paths(request)
        .into_iter()
        .map(|path| {
            kiana_domain::normalize_role_path(&path)
                .ok_or_else(|| action_error("action_path_scope_invalid"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    // 空路径列表或显式通配范围按当前 ScopeDimension 约定表示此上下文不另加路径限制。
    let context_paths = if context.path_allow.is_empty()
        || context
            .path_allow
            .iter()
            .any(|path| matches!(path.trim(), "." | "*"))
    {
        kiana_domain::ScopeDimension::NotApplicable
    } else {
        kiana_domain::ScopeDimension::Restricted(
            context
                .path_allow
                .iter()
                .map(|path| {
                    kiana_domain::normalize_role_path(path)
                        .ok_or_else(|| action_error("context_path_scope_invalid"))
                })
                .collect::<Result<Vec<_>, _>>()?,
        )
    };
    // 把单个动作投影到 ScopeSet 的各维度；不存在的参数维度保持 NotApplicable。
    let action_scope = kiana_domain::ScopeSet::new(
        kiana_domain::ScopeDimension::Restricted(vec![request.operation.clone()]),
        if action_paths.is_empty() {
            kiana_domain::ScopeDimension::NotApplicable
        } else {
            kiana_domain::ScopeDimension::Restricted(action_paths)
        },
        request
            .arguments
            .get("collection")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(|value| kiana_domain::ScopeDimension::Restricted(vec![value.to_owned()]))
            .unwrap_or_default(),
        request
            .arguments
            .get("server")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(|value| kiana_domain::ScopeDimension::Restricted(vec![value.to_owned()]))
            .unwrap_or_default(),
        kiana_domain::ScopeLimit::NotApplicable,
        kiana_domain::ScopeLimit::NotApplicable,
    )
    .map_err(|error| action_error(&format!("action_scope_invalid:{error}")))?;
    // Context 只在这里贡献路径限制，其余维度由动作自身或其他授权层约束。
    let context_scope = kiana_domain::ScopeSet::new(
        kiana_domain::ScopeDimension::NotApplicable,
        context_paths,
        kiana_domain::ScopeDimension::NotApplicable,
        kiana_domain::ScopeDimension::NotApplicable,
        kiana_domain::ScopeLimit::NotApplicable,
        kiana_domain::ScopeLimit::NotApplicable,
    )
    .map_err(|error| action_error(&format!("context_scope_invalid:{error}")))?;
    action_scope
        .intersect(&context_scope)
        .map_err(|error| action_error(&format!("scope_intersection_invalid:{error}")))
}

/// 构造随 CapabilityRequest 一起交给 Broker 的服务端执行范围快照。
///
/// 【调用者】`prepare_capability_action_cancellable()` 在 action scope 与 Cell 引用已绑定后调用。
/// 【输入/输出】RequestContext、规范化动作及其 permission ScopeSet；返回通过领域校验的 ExecutionScope。
/// 【快照内容】主体和项目身份、session/run/turn、Cell/grant/budget 引用、只读/可写根、memory/server
/// scope、authority/trust/data/cancellation 代次，以及 action/scope digest。
/// 【副作用/失败】纯构造，不写存储；缺角色、无效 collection、身份或最终快照校验失败时返回 CoreError。
/// 【为什么要预先绑定】Broker 收到后可以核对实际要执行的动作是否仍对应这份服务端授权快照；
/// 若让 handler 自己从不可信参数重建 scope，就可能执行超出策略检查的资源范围。
fn build_execution_scope(
    context: &RequestContext,
    request: &CapabilityRequest,
    permission_scope: &kiana_domain::ScopeSet,
) -> Result<kiana_domain::ExecutionScope, CoreError> {
    // 当前运行环境按本地主体建模，再用请求上下文中的 actor id 替换默认身份并重算摘要。
    let mut principal = kiana_domain::AuthenticatedPrincipalRef::local();
    principal.principal_id = context.actor_id.clone().unwrap_or_default();
    principal.principal_digest = principal.digest();
    principal
        .validate()
        .map_err(|error| action_error(&format!("execution_scope_principal_invalid:{error}")))?;
    // 保留原始项目路径用于显示，同时把 canonical 路径写入稳定身份，便于 Broker 校验边界。
    let canonical_root = ControlPlane::canonical_project_root(&context.project_root);
    let project = kiana_domain::ProjectIdentity::new(
        context.project_root.clone(),
        canonical_root.to_string_lossy().into_owned(),
        None,
        None,
        kiana_domain::json_digest(&json!({"trusted":context.project_trusted})),
    )
    .map_err(|error| action_error(&format!("execution_scope_project_invalid:{error}")))?;
    // 授权引用排序后进入 scope，确保相同授权集合产生相同摘要。
    let mut grant_refs = request.capability_grant_id.into_iter().collect::<Vec<_>>();
    grant_refs.sort_by_key(|grant| grant.to_string());
    // scope 自身不能参与它所绑定的动作摘要，否则会形成摘要互相依赖的循环。
    let action_digest = {
        let mut without_scope = request.clone();
        without_scope.execution_scope = None;
        kiana_domain::capability_action_digest(&without_scope)
    };
    let path_values = match &permission_scope.paths {
        kiana_domain::ScopeDimension::Restricted(values) => values.clone(),
        kiana_domain::ScopeDimension::NotApplicable => Vec::new(),
    };
    let requested_collection = request
        .arguments
        .get("collection")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned);
    // Memory 操作要在构造执行范围时再次按当前 RoleSpec 收窄 collection；写入必须点名集合。
    let memory_scopes = if matches!(request.operation.as_str(), "memory.search" | "memory.write") {
        let role = RoleSpec::lookup(&context.role_id)
            .ok_or_else(|| action_error("memory_scope_role_unknown"))?;
        let collections = if let Some(collection) = requested_collection.as_deref() {
            let parsed = kiana_domain::MemoryCollection::parse(collection)
                .ok_or_else(|| action_error("memory_scope_collection_unknown"))?;
            if request.operation == "memory.write" {
                if !role.allows_memory_write(&parsed.collection) {
                    return Err(action_error("memory_scope_write_denied"));
                }
            } else if !role.allows_knowledge(&parsed.collection) {
                return Err(action_error("memory_scope_read_denied"));
            }
            vec![parsed.collection]
        } else if request.operation == "memory.write" {
            return Err(action_error("memory_scope_collection_required"));
        } else {
            role.granted_collections()
                .into_iter()
                .map(|collection| collection.collection)
                .collect::<Vec<_>>()
        };
        if collections.is_empty() {
            return Err(action_error("memory_scope_empty"));
        }
        collections
    } else {
        requested_collection.into_iter().collect()
    };
    let server = request
        .arguments
        .get("server")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned);
    // 读取根固定为 canonical 项目根；只读动作没有写根，写动作使用有效权限路径。
    // ⚠ 这些字段在当前实现中是固定构造值，不是从项目 authority/data/cancellation 账本读取的当前版本：
    // 1 表示满足 ExecutionScope 的非零校验，u64::MAX 表示不在此对象内设置有限 deadline。
    // `pin_action_authority()` 会另外记录 authority stream version，但不会改写下面的 scope epoch。
    // memory adapter 与 connector effect fence 会读取 authority_epoch/data_epoch，因此不要把这里的 1
    // 理解成“已读取最新版本”；版本来源是否需要与那些账本对齐，是当前需确认的潜在问题。
    let mut scope = kiana_domain::ExecutionScope {
        schema: kiana_domain::EXECUTION_SCOPE_SCHEMA.to_owned(),
        version: kiana_domain::EXECUTION_SCOPE_SCHEMA_VERSION,
        principal,
        project,
        session_id: context.session_id.clone(),
        run_id: request
            .arguments
            .get("run_id")
            .and_then(Value::as_str)
            .and_then(RunId::parse_str),
        turn_id: request
            .arguments
            .get("turn_id")
            .and_then(|value| serde_json::from_value::<kiana_domain::TurnId>(value.clone()).ok()),
        cell_id: context.cell_id,
        grant_refs,
        budget_lease_id: request.budget_lease_id,
        work_packet_id: context.work_packet_id.clone(),
        environment_id: "kiana-local".to_owned(),
        workspace_revision: None,
        permission_scope: permission_scope.clone(),
        read_roots: vec![canonical_root.to_string_lossy().into_owned()],
        write_roots: if request.risk == RiskLevel::ReadOnly {
            Vec::new()
        } else if path_values.is_empty() {
            vec![".".to_owned()]
        } else {
            path_values.clone()
        },
        read_denies: Vec::new(),
        write_denies: Vec::new(),
        memory_scopes,
        server_scopes: server.into_iter().collect(),
        network_policy: Vec::new(),
        authority_epoch: 1,
        trust_revision: kiana_domain::json_digest(&json!({"trusted":context.project_trusted})),
        data_epoch: 1,
        cancellation_epoch: 1,
        deadline_unix_ms: u64::MAX,
        fencing_token: 1,
        catalog_digest: kiana_domain::capability_action_catalog_digest(),
        action_digest,
        permission_scope_digest: permission_scope.digest(),
        scope_digest: String::new(),
    };
    // 先填空摘要再计算自身摘要，随后由领域校验检查字段之间的一致性。
    scope.scope_digest = scope.digest();
    scope
        .validate()
        .map_err(|error| action_error(&format!("execution_scope_invalid:{error}")))?;
    Ok(scope)
}

impl ControlPlane {
    /// 用服务端 Cell reservation 给动作补齐 Cell、Grant 和 Budget 引用。
    /// 对非 Cell 请求要求这些引用也全部缺席，避免半套客户端字段混入执行范围。
    pub(crate) async fn bind_cell_scope(
        &self,
        context: &RequestContext,
        request: &mut CapabilityRequest,
    ) -> Result<(), CoreError> {
        self.guard_company_capability(context, request).await?;
        let Some(cell_id) = context.cell_id else {
            if request.capability_grant_id.is_some() || request.budget_lease_id.is_some() {
                return Err(CoreError::Port(PortError::Failed(
                    "cell_capability_scope_incomplete".to_owned(),
                )));
            }
            return Ok(());
        };
        // reservation 由 registry 持有，是此处 grant/budget ID 的权威来源。
        let reservation = self
            .cell_registry
            .reservation_for_cell(cell_id)
            .await?
            .ok_or_else(|| CoreError::Port(PortError::Failed("cell_not_found".to_owned())))?;
        request.cell_id = Some(cell_id);
        request.capability_grant_id = Some(reservation.grant.grant_id);
        request.budget_lease_id = Some(reservation.budget.lease_id);
        Ok(())
    }

    /// 若请求属于 Cell，则原子登记这次 capability 使用并返回结算 lease。
    /// 非 Cell 请求返回 None；调用方必须把 lease 交给统一收尾函数完成结算。
    pub(crate) async fn begin_cell_capability_from_request(
        &self,
        request: &CapabilityRequest,
    ) -> Result<Option<CapabilityLease>, CoreError> {
        let Some(cell_id) = request.cell_id else {
            if request.capability_grant_id.is_some() || request.budget_lease_id.is_some() {
                return Err(CoreError::Port(PortError::Failed(
                    "cell_capability_scope_incomplete".to_owned(),
                )));
            }
            return Ok(None);
        };
        let grant_id = request.capability_grant_id.ok_or_else(|| {
            CoreError::Port(PortError::Failed(
                "cell_capability_scope_incomplete".to_owned(),
            ))
        })?;
        let budget_id = request.budget_lease_id.ok_or_else(|| {
            CoreError::Port(PortError::Failed(
                "cell_capability_scope_incomplete".to_owned(),
            ))
        })?;
        // registry 在 begin_capability 内校验 grant、budget 与请求范围，并占用相应额度。
        Ok(Some(
            self.cell_registry
                .begin_capability(cell_id, grant_id, budget_id, request)
                .await?,
        ))
    }

    /// 按最终执行状态结算前面取得的 Cell capability lease。
    pub(crate) async fn finish_cell_capability(
        &self,
        lease: CapabilityLease,
        outcome: CapabilityOutcome,
    ) -> Result<(), CoreError> {
        self.cell_registry
            .finish_capability(lease, outcome)
            .await
            .map_err(CoreError::from)
    }

    /// 直接能力入口使用的非取消版薄包装；所有规范化和授权上下文构造集中在下方实现。
    pub(crate) async fn prepare_capability_action(
        &self,
        context: &RequestContext,
        request: CapabilityRequest,
        sandbox: Option<&str>,
        from_runner: bool,
    ) -> Result<CapabilityRequest, CoreError> {
        self.prepare_capability_action_cancellable(context, request, sandbox, from_runner, None)
            .await
    }

    /// 把入口提供的 CapabilityRequest 规范化为受服务端身份和权限范围约束的动作。
    ///
    /// 【调用者】direct 能力入口由 `prepare_capability_action()` 转入；Harness 入口由
    /// `broker_harness_capability()` 直接调用。
    /// 【输入/输出】请求上下文、入口请求、可选 sandbox、是否来自 Runner，以及可选取消 watch；
    /// 成功返回已规范化并绑定 ExecutionScope/Cell authority 的 CapabilityRequest。
    /// 【副作用】运行 pre-tool prepare hook、读取 Cell registry、固定当前 authority snapshot；不调用工具 handler。
    /// 【失败情况】未知 operation、模型请求非公开工具、无效 scope、hook/registry/pinning 失败都会返回
    /// CoreError；Harness 调用者会把可审计的拒绝事实写入 EventLog，direct 调用者由外层处理。
    /// 【为什么要删除后重建字段】authorization、grant、approval、run、Cell 等字段具有授权含义，
    /// 它们必须来自 ControlPlane/registry，不能接受模型或 operator 通过 arguments 冒充。
    pub(crate) async fn prepare_capability_action_cancellable(
        &self,
        context: &RequestContext,
        mut request: CapabilityRequest,
        sandbox: Option<&str>,
        from_runner: bool,
        cancellation: Option<&watch::Receiver<bool>>,
    ) -> Result<CapabilityRequest, CoreError> {
        if !request.arguments.is_object() {
            return Err(action_error("action_arguments_object_required"));
        }
        // 先把别名折叠到已知 operation，再做模型可见面检查，后续策略只处理规范名称。
        let operation = kiana_domain::canonical_action_operation(&request.operation)
            .ok_or_else(|| action_error("action_operation_unknown"))?;
        if from_runner && kiana_domain::model_tool_name(operation).is_none() {
            return Err(action_error("action_not_model_visible"));
        }
        if context.cell_id.is_none()
            && (request.cell_id.is_some()
                || request.capability_grant_id.is_some()
                || request.budget_lease_id.is_some())
        {
            return Err(action_error("cell_capability_scope_incomplete"));
        }
        request.operation = operation.to_owned();
        // 这些字段只能由上下文与 registry 决定；能力参数本身不能签发授权或修改预算。
        let arguments = request.arguments.as_object_mut().expect("checked object");
        for key in [
            "authorization_id",
            "permission_profile",
            "cell_id",
            "capability_grant_id",
            "budget_lease_id",
            "work_packet_id",
            "approval_id",
            "dispatch_permit",
            "execution_context",
        ] {
            arguments.remove(key);
        }
        if from_runner || context.cell_id.is_some() {
            arguments.remove("operator_authorized");
        }
        // 只有 continuation 类操作需要保留既有 process/run 引用，其余入口不能自行挑选 Run。
        let preserves_continuation_scope = matches!(
            operation,
            "execution.output.read"
                | "process.poll"
                | "process.stdin"
                | "process.resize"
                | "process.stop"
        );
        if !from_runner && !preserves_continuation_scope {
            // Direct/operator 请求不能通过兼容参数挑选 Harness Run/Turn；这类服务端引用只应由
            // lifecycle/Runner 路径携带，续接已有进程的只读/控制操作则按上面的白名单保留。
            arguments.remove("run_id");
            arguments.remove("turn_id");
        }
        // 旧 execution_scope 可能来自调用方；完成 action/context 规范化后必须从服务端状态重新构造。
        request.execution_scope = None;
        if kiana_domain::model_tool_name(operation).is_some() {
            let selected = sandbox
                .or_else(|| arguments.get("sandbox").and_then(Value::as_str))
                .unwrap_or("read-only");
            let selected = crate::lifecycle::authorized_harness_sandbox(context, Some(selected))
                .map_err(action_error)?
                .to_owned();
            arguments.insert("sandbox".to_owned(), json!(selected));
        }
        request.cell_id = None;
        request.capability_grant_id = None;
        request.budget_lease_id = None;
        stamp_request_identity(&mut request, context);
        kiana_domain::normalize_capability_action(&mut request).map_err(action_error)?;
        // 没有外部取消信号时给 hook 一个永不置真的 watch receiver，复用同一异步接口。
        let cancellation = cancellation.cloned().unwrap_or_else(|| {
            let (_sender, receiver) = watch::channel(false);
            receiver
        });
        // Hook 可对动作做预执行检查；hook 返回后再次规范化，避免改写结果绕过 canonical 规则。
        request = self
            .pre_tool_hooks
            .prepare_action(context, &request, cancellation)
            .await
            .map_err(CoreError::from)?;
        kiana_domain::normalize_capability_action(&mut request).map_err(action_error)?;
        // Scope 与 Cell 引用均由当前上下文/registry 产生，而非信任模型工具参数。
        let effective_scope = effective_action_scope(context, &request)?;
        self.bind_cell_scope(context, &mut request).await?;
        request.execution_scope = Some(build_execution_scope(context, &request, &effective_scope)?);
        let action = kiana_domain::PreparedAction::new(request).map_err(action_error)?;
        // pin 在授权前固定 authority snapshot，使之后执行时能发现策略/信任版本漂移。
        self.pin_action_authority(context, action.request()).await?;
        Ok(action.into_request())
    }

    /// 为 direct 与 Harness 两类入口统一计算策略决策和 Gate 决策。
    ///
    /// 【调用者】`authorize_and_execute()` 与 `broker_harness_capability()` 的公共授权阶段会调用。
    /// 【输入/输出】服务端 RequestContext、已规范化并绑定权限范围的 CapabilityRequest、可选的已批准
    /// approval ID/原始要求；返回 PolicyDecision 与 GateDecision，不执行工具。
    /// 【副作用/失败】异步调用 pre-tool hook；hook 的 Block/错误变为 Denied，Ask 并入审批要求，
    /// 已有 Deny 不会被 Hook 的 Allow 覆盖。策略/Gate 计算以结构化决策返回；端口错误才返回 CoreError。
    /// 【审批恢复约束】只能解除原审批明确批准过的同一组要求；恢复时出现的新要求仍需再次审批。
    /// 不同授权层的 Allow 不能清除另一层的 Ask 或 Deny。
    pub(crate) async fn authorize_capability_action(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
        approval: Option<(ApprovalId, &str)>,
    ) -> Result<(PolicyDecision, GateDecision), CoreError> {
        self.authorize_capability_action_cancellable(context, request, approval, None)
            .await
    }

    pub(crate) async fn authorize_capability_action_cancellable(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
        approval: Option<(ApprovalId, &str)>,
        cancellation: Option<&watch::Receiver<bool>>,
    ) -> Result<(PolicyDecision, GateDecision), CoreError> {
        // 参数与返回值沿用 `authorize_capability_action()`；额外的取消 watch 只供异步 hook 检查。
        // 带 cancellation receiver 时，hook 可在长时间预检查过程中响应 Run 的取消通知；
        // 没有 receiver 的 direct 入口仍复用相同策略顺序，但不会伪造一个可取消的执行状态。
        // Policy 描述规则矩阵的结论，Gate 再把该结论映射为允许、拒绝或需审批。
        let policy = self.evaluate_policy(context, request);
        let mut gate = self.evaluate_gate(request, &policy);
        // 已明确 Deny 时不再执行 Hook，避免 Hook 的 Allow 覆盖更高层拒绝。
        if !matches!(gate, GateDecision::Denied { .. }) {
            let decision = if let Some(cancellation) = cancellation {
                self.pre_tool_hooks
                    .decide_cancellable(context, request, cancellation.clone())
                    .await
            } else {
                self.pre_tool_hooks.decide(context, request).await
            };
            // Hook 的 Block 变成拒绝，Ask 合并审批条件，Allow 只保留既有 Gate 结论。
            gate = match decision {
                Ok(PreToolHookDecision::Allow) => gate,
                Ok(PreToolHookDecision::Block(reason)) => GateDecision::Denied {
                    reason: format!("hook_blocked:{reason}"),
                },
                Ok(PreToolHookDecision::Ask { reason }) => {
                    let required = format!("hook_approval_required:{reason}");
                    match gate {
                        GateDecision::AwaitingApproval { reason } => {
                            GateDecision::AwaitingApproval {
                                reason: merge_requirements(&reason, &required),
                            }
                        }
                        _ => GateDecision::AwaitingApproval { reason: required },
                    }
                }
                Err(error) => GateDecision::Denied {
                    reason: format!("hook_blocked:{error}"),
                },
            };
        }
        // 恢复审批时只解除此前明确批准的同一组要求；新出现的要求仍会留下 AwaitingApproval。
        if let Some((approval_id, original_reason)) = approval {
            if let GateDecision::AwaitingApproval { reason } = &gate {
                let approved = approval_requirements(original_reason);
                if approval_requirements(reason)
                    .iter()
                    .all(|requirement| approved.contains(requirement))
                {
                    gate = GateDecision::Allowed {
                        authorization_id: format!("approval:{approval_id}"),
                    };
                }
            } else if matches!(gate, GateDecision::Allowed { .. }) {
                gate = GateDecision::Allowed {
                    authorization_id: format!("approval:{approval_id}"),
                };
            }
        }
        Ok((policy, gate))
    }

    /// 将审批挑战激活事实与可见的等待状态作为一个事务提交。
    ///
    /// 单独 `stage()` 得到的 challenge 只是候选审批请求，不代表已有授权；只有
    /// `prepare_activation()` 与 EventStore transition batch 一起提交成功，入口或恢复中的 Runner
    /// 才能看到 `approval.requested`，Harness Run 才能进入 `run.awaiting_approval`。
    /// 把审批挑战、`approval.requested` 和可选的 `run.awaiting_approval` 作为一个状态转换提交。
    /// direct 入口没有 Run 关联；Harness 入口会一并保存 resume binding，供后续恢复校验。
    ///
    /// 【调用者/输入】授权结果为 AwaitingApproval 时调用；输入包括上下文、已准备动作、审批原因、
    /// 可选 Run/event request、恢复绑定和当前事件序号。
    /// 【输出】成功返回 ApprovalChallenge；摘要冲突、Run 已终态、审批激活冲突或存储错误返回 CoreError。
    /// 【副作用】审批存储先 stage 候选项，再与审批事件和可选的 Run 等待事件通过 command receipt、
    /// authority versions 和 stream CAS 原子激活。重复命令只有 digest 完全相同才按幂等重放处理。
    pub(crate) async fn stage_capability_action(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
        reason: &str,
        run_id: Option<RunId>,
        event_request_id: RequestId,
        resume_binding: Option<&kiana_domain::InvocationResumeBinding>,
        sequence: &mut u64,
    ) -> Result<kiana_domain::ApprovalChallenge, CoreError> {
        // 审批存储用能力请求本身的 request id 作为上下文身份，不沿用外层 Run 事件 id。
        let mut approval_context = context.clone();
        approval_context.request_id = request.request_id;
        let challenge = self
            .approvals
            .stage(&approval_context, request.clone(), reason)
            .await?;
        // 派生稳定命令 id，重试时能查到同一次激活的回执而不是再追加一套审批事件。
        let command_id = kiana_domain::derived_request_id(
            "approval.activate_with_pause",
            &challenge.approval_id.to_string(),
        );
        let command_digest = kiana_domain::json_digest(
            &json!({"approval_id":challenge.approval_id,
            "run_id":run_id,"event_request_id":event_request_id,"action_digest":kiana_domain::capability_action_digest(request)}),
        );
        let command_digest = command_digest.trim_start_matches("sha256:").to_owned();
        let event_count = if run_id.is_some() { 2 } else { 1 };
        // 已存在回执代表可能是提交成功后的重试；只有摘要完全一致才按幂等重放处理。
        if let Some(receipt) = self.events.read_command(&command_id).await? {
            if receipt.command_digest != command_digest {
                return Err(action_error("approval_activation_command_conflict"));
            }
            *sequence = sequence.saturating_add(event_count);
            return Ok(challenge);
        }
        // stage 只产生候选 challenge；prepare_activation 再生成可与 EventLog 一起提交的激活事实。
        let activation = self
            .approvals
            .prepare_activation(challenge.approval_id, command_id)
            .await?;
        // Run 内审批写入 Run stream；没有 Run 的 direct 请求使用独立 request stream。
        let (aggregate_type, aggregate_id) = match run_id {
            Some(run_id) => ("run", run_id.to_string()),
            None => ("request", event_request_id.to_string()),
        };
        let history = self
            .events
            .read_stream(aggregate_type, &aggregate_id)
            .await?;
        // 只检查当前 prompt 之后的历史，避免已终止的本轮运行重新进入审批等待状态。
        if run_id.is_some() {
            let turn = history
                .iter()
                .rposition(|event| event.kind == "run.prompt")
                .unwrap_or(0);
            if history[turn..].iter().any(|event| {
                matches!(
                    event.kind.as_str(),
                    "run.cancelling"
                        | "run.cancelled"
                        | "run.completed"
                        | "run.failed"
                        | "run.result_unknown"
                )
            }) {
                return Err(action_error("approval_run_already_terminal"));
            }
        }
        // 以当前 stream 版本构造 CAS 前置条件；并与审批聚合自身的版本要求合并。
        let version = history
            .iter()
            .filter_map(|event| event.stream_version)
            .max()
            .unwrap_or(0);
        let mut expected_versions = activation.authority_versions;
        for required in [
            activation.expected_version,
            kiana_domain::AggregateVersion::new(aggregate_type, &aggregate_id, version),
        ] {
            if let Some(existing) = expected_versions.iter().find(|v| {
                v.aggregate_type == required.aggregate_type
                    && v.aggregate_id == required.aggregate_id
            }) {
                if existing.version != required.version {
                    return Err(action_error("approval_activation_authority_conflict"));
                }
            } else {
                expected_versions.push(required);
            }
        }
        // 事件只记录“副作用尚未开始”的待审批事实；敏感参数在写入前做递归脱敏。
        let mut data = json!({"approval_id":challenge.approval_id,"request_hash":challenge.request_hash,
            "session_id":context.session_id,"actor_id":context.actor_id,"expires_at_unix_ms":challenge.expires_at_unix_ms,
            "capability_request_id":request.request_id,"action_digest":kiana_domain::capability_action_digest(request),
            "attempt":1,"effect_started":false,"effect_known":true,"zero_effect":true,
            "stop_state":"not_requested","fenced":false});
        if let Some(run_id) = run_id {
            data["run_id"] = json!(run_id);
        }
        if let Some(binding) = resume_binding {
            data["resume_binding"] = json!(binding);
        }
        // approval.requested 和 run.awaiting_approval 共用递增 stream version，形成可恢复顺序。
        let requested = RuntimeEvent::new(
            event_request_id,
            *sequence,
            "approval.requested",
            redact_event_value(&data),
        )?
        .with_stream_metadata(aggregate_type, &aggregate_id, version + 1);
        let mut events = vec![activation.event, requested];
        if let Some(run_id) = run_id {
            events.push(
                RuntimeEvent::new(
                    event_request_id,
                    sequence.saturating_add(1),
                    "run.awaiting_approval",
                    json!({"run_id":run_id,"approval_id":challenge.approval_id,
                        "capability_request_id":request.request_id,"attempt":1,"effect_started":false,
                        "effect_known":true,"zero_effect":true,"stop_state":"not_requested","fenced":false,
                        "resume_binding":resume_binding}),
                )?
                .with_stream_metadata(aggregate_type, &aggregate_id, version + 2),
            );
        }
        // command id、digest、预期版本与事件批次一起提交，避免只激活审批却没留下等待事实。
        let outcome = self
            .events
            .commit_transition(kiana_domain::TransitionBatch {
                command_id,
                command_digest: command_digest.clone(),
                expected_versions,
                events,
            })
            .await?;
        // Unknown 不能直接当失败：回读 command receipt 可确认存储是否已提交该命令。
        match outcome {
            kiana_domain::CommitOutcome::Committed { .. }
            | kiana_domain::CommitOutcome::Replayed { .. } => {}
            kiana_domain::CommitOutcome::Conflict { .. } => {
                return Err(action_error("approval_activation_conflict"))
            }
            kiana_domain::CommitOutcome::Unknown { .. } => {
                if self
                    .events
                    .read_command(&command_id)
                    .await?
                    .is_none_or(|receipt| receipt.command_digest != command_digest)
                {
                    return Err(action_error(
                        "result_unknown:approval_activation_unconfirmed",
                    ));
                }
            }
        }
        // 事件投影缓存仅在提交确认后失效；随后推进调用方维护的事件序号。
        if let Some(run_id) = run_id {
            self.invalidate_invocation_projection(run_id);
        }
        *sequence = sequence.saturating_add(event_count);
        Ok(challenge)
    }

    /// 在当前 Runner 工具批次无法继续时，取消尚待处理的工具并记录明确未执行结果。
    ///
    /// 【调用者】Harness 能力准备失败、授权拒绝、审批激活失败或派发结果要求停止批次时调用。
    /// 【输入】Run、事件 request/sequence、当前被拒绝的首个请求（可选）和拒绝/取消原因。
    /// 【数据流】向 Runner 发 Cancel → 对当前请求生成 `not_executed=true` 的 tool result →
    /// 收集 Runner 同批发出的其他 ToolCancelled 事件并逐个落账，同时跳过已由首项记录的 request ID。
    /// 【失败语义】Runner 取消端口失败不会伪造其他取消事件；EventStore 写入错误会在尽可能收集完
    /// 同批结果后返回 CoreError。该函数不执行 handler，也不把拒绝响应当成 handler 输出。
    pub(crate) async fn cancel_pending_tools(
        &self,
        run_id: RunId,
        event_request_id: RequestId,
        sequence: &mut u64,
        first: Option<&CapabilityRequest>,
        reason: &str,
    ) -> Result<(), CoreError> {
        // 取消命令让 Runner 关闭当前待处理工具批次；返回事件中可能还包含同批其他调用。
        let cancelled = self
            .runner
            .send(RunnerCommand::Cancel {
                run_id,
                reason: reason.to_owned(),
            })
            .await;
        let mut recording_error = None;
        // 当前拒绝/取消的首个请求没有进入 handler，生成 not_executed 结果交回该 call_id。
        if let Some(request) = first {
            let mut result = CapabilityResult::failure(request.request_id, reason);
            result.output["not_executed"] = json!(true);
            let result = kiana_domain::normalize_capability_result(request.request_id, result);
            recording_error = self.record_event(event_request_id, sequence, "run.tool_result", json!({
                "run_id":run_id,"capability_request_id":request.request_id,"call_id":request.arguments["call_id"],
                "result":result.output,"cancelled":kiana_domain::CapabilityErrorCode::from_reason(reason) == kiana_domain::CapabilityErrorCode::Cancelled,"not_executed":true,
                "attempt":1,"effect_started":false,"effect_known":true,"zero_effect":true,
                "stop_state":"confirmed","stop_confirmed":true,"fenced":false,
            })).await.err();
        }
        // Runner 同批取消的其余工具也要形成结果事实；跳过首个 request_id 防止重复写入。
        if let Ok(events) = cancelled {
            for event in events {
                if let RunnerEvent::ToolCancelled {
                    run_id: event_run,
                    request_id,
                    call_id,
                    result,
                } = event
                {
                    if event_run == run_id
                        && !first.is_some_and(|request| request.request_id == request_id)
                    {
                        if let Err(error) = self.record_event(event_request_id, sequence, "run.tool_result", json!({
                            "run_id":run_id,"capability_request_id":request_id,"call_id":call_id,
                            "result":result,"cancelled":true,"not_executed":true,
                            "attempt":1,"effect_started":false,"effect_known":true,"zero_effect":true,
                            "stop_state":"confirmed","stop_confirmed":true,"fenced":false,
                        })).await { recording_error = Some(error); }
                    }
                }
            }
        }
        // 即使继续收集其他取消事件，仍保留最近一次持久化错误并在最后向上报告。
        match recording_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// 对获准的能力动作完成资源预留、一次派发和统一结果收尾。
    ///
    /// 【调用者】Harness 工具入口、direct 能力入口和审批恢复后的执行入口共用此路径。
    /// 【输入】服务端上下文、可选 Run、规范化请求、授权 ID、取消 watch 和 EventLog 序号。
    /// 【输出】`FinalizedCapabilityAction` 包含归一化结果和 Completed/Failed/Cancelled/ResultUnknown 状态；
    /// EventLog 或结算端口错误仍以 CoreError 返回。
    /// 【核心流程】取消前置检查 → 写操作 checkpoint → 形成 AuthorizedCapabilityRequest → 等待并发准入
    /// → 占用 Cell budget → 调用 Broker dispatcher → 无论 dispatch 结果如何都进入 finalizer。
    /// 准备阶段失败明确记 not_executed；一旦进入 Broker 后只有返回的结构化结果能证明执行状态。
    /// 【为什么统一收尾】Cell lease 和 Admission lease 跨越异步派发；所有出口必须按同一结果分类结算，
    /// 否则失败/取消分支可能泄漏并发槽、额度，或把未知副作用错记成确定失败。
    /// 执行前依次检查取消、写权限 checkpoint、调度 admission 与 Cell lease；无论派发成功与否，
    /// 都进入 `finalize_capability_action` 统一归一化结果、写事实并释放/结算资源。
    pub(crate) async fn dispatch_capability_action(
        &self,
        context: &RequestContext,
        run_id: Option<RunId>,
        request: &CapabilityRequest,
        authorization_id: String,
        cancellation: watch::Receiver<bool>,
        event_request_id: RequestId,
        sequence: &mut u64,
    ) -> Result<FinalizedCapabilityAction, CoreError> {
        // 两种 lease 跨越异步派发保存在栈帧中，供统一 finalizer 按真实结果结算。
        let mut lease = None;
        let mut admission_lease: Option<AdmissionLease> = None;
        // 在排队前已取消时明确记录未执行；否则进入预留资源的异步准备段。
        let execution = if *cancellation.borrow() {
            let mut result =
                CapabilityResult::failure(request.request_id, "cancelled:before_dispatch");
            result.output["not_executed"] = json!(true);
            Ok(result)
        } else {
            let prepared = async {
                // 写操作进入 Broker 前检查 run checkpoint，确保恢复/取消状态允许发生副作用。
                if let Some(run_id) = run_id {
                    self.checkpoint_before_write(context, run_id, request, *sequence)
                        .await?;
                }
                // 只在 Core 已给出的 authorization_id 上包装请求，Broker 收到的是授权型请求。
                let authorized =
                    AuthorizedCapabilityRequest::new(authorization_id, request.clone())?;
                // Admission scheduler 限制并发；等待期间接收 cancellation watch 的变化。
                let scheduler_lease = self
                    .admission_scheduler
                    .acquire(request, cancellation.clone())
                    .await?;
                if *cancellation.borrow() {
                    self.admission_scheduler
                        .release(scheduler_lease, AdmissionOutcome::Known);
                    return Err(action_error("cancelled:before_dispatch"));
                }
                // Cell 额度在进入 handler 前占用；失败时立即释放刚取得的并发槽。
                match self.begin_cell_capability_from_request(request).await {
                    Ok(cell_lease) => {
                        lease = cell_lease;
                        admission_lease = Some(scheduler_lease);
                    }
                    Err(error) => {
                        self.admission_scheduler
                            .release(scheduler_lease, AdmissionOutcome::Known);
                        return Err(error);
                    }
                }
                Ok::<_, CoreError>(authorized)
            }
            .await;
            // 准备阶段失败表示 handler 未开始，因而可把结果标成 dispatch_rejected/not_executed。
            match prepared {
                Ok(authorized) => {
                    self.dispatch_authorized(context, run_id, authorized, cancellation.clone())
                        .await
                }
                Err(error) => {
                    let mut result =
                        CapabilityResult::failure(request.request_id, error.to_string());
                    result.output["not_executed"] = json!(true);
                    result.output["dispatch_rejected"] = json!(true);
                    Ok(result)
                }
            }
        };
        self.finalize_capability_action(
            context,
            run_id,
            request,
            execution,
            lease,
            admission_lease,
            &cancellation,
            event_request_id,
            sequence,
        )
        .await
    }

    /// 将 Broker 执行结果归一化为事件、Receipt 和资源结算使用的同一状态。
    ///
    /// 【调用者】`dispatch_capability_action()` 在成功、拒绝、取消或端口错误后统一调用。
    /// 【输入】执行结果、可选 Run、Cell/Admission leases、取消 watch 和事件身份。
    /// 【输出】FinalizedCapabilityAction；如果日志持久化、Cell 额度结算或 Receipt 构造无法确认，
    /// 结果会提升为 ResultUnknown，防止调用方收到虚假的成功。
    /// 【副作用】脱敏结果并构造 CapabilityResultReceipt → 写 capability terminal 事实 → 结算 Cell lease
    /// → 释放 Admission 槽；其中 Unknown 使用单独结算类别，不能当作已知失败自动重试。
    /// 【关键边界】handler 已进入执行后，普通 Broker 错误不等于“没有副作用”；没有明确的
    /// not_executed/stop proof 时必须保留 Unknown。
    async fn finalize_capability_action(
        &self,
        context: &RequestContext,
        run_id: Option<RunId>,
        request: &CapabilityRequest,
        execution: Result<CapabilityResult, PortError>,
        lease: Option<CapabilityLease>,
        admission_lease: Option<AdmissionLease>,
        cancellation: &watch::Receiver<bool>,
        event_request_id: RequestId,
        sequence: &mut u64,
    ) -> Result<FinalizedCapabilityAction, CoreError> {
        // 将未返回 CapabilityResult 的 Broker/Port 错误折成统一结果；除明确的派发前取消外，
        // 不能从传输错误推断副作用没发生，因此标记 result_unknown。
        let result = execution.unwrap_or_else(|error| {
            let raw = match &error {
                PortError::Failed(reason)
                | PortError::Conflict(reason)
                | PortError::Unavailable(reason) => reason.as_str(),
            };
            let cancelled_before_dispatch =
                matches!(raw, "cancelled:before_dispatch" | "cancelled:before_broker");
            // 没有已提交 handler 结果的传输/端口错误，不能证明副作用是否发生；只有 dispatcher
            // 明确标记派发前取消，才足以证明 handler 尚未开始。
            let mut result = CapabilityResult::failure(
                request.request_id,
                if cancelled_before_dispatch {
                    raw.to_owned()
                } else {
                    format!("result_unknown:{}", redact_event_text(&error.to_string()))
                },
            );
            result.output["dispatch_rejected"] = json!(true);
            if cancelled_before_dispatch {
                result.output["not_executed"] = json!(true);
            }
            result
        });
        // 领域归一化补齐稳定错误码和必需字段，使后续 Receipt 与事件使用同一结果形状。
        let mut result = kiana_domain::normalize_capability_result(request.request_id, result);
        if *cancellation.borrow()
            && result.failure_code() != Some(kiana_domain::CapabilityErrorCode::ResultUnknown)
        {
            // 取消后只有未执行标记、handler 停止确认或 Run stop proof 才能证明副作用已停。
            let stopped = result.output["stop_confirmed"] == true
                || result.output["not_executed"] == true
                || match run_id {
                    Some(run_id) => self.await_capability_stop(run_id).await,
                    None => false,
                };
            result.success = false;
            if !result.output.is_object() {
                result.output = json!({"detail":result.output});
            }
            result.output["error"] = json!(if stopped {
                "cancelled:user"
            } else {
                "result_unknown:cancel_stop_unconfirmed"
            });
            result = kiana_domain::normalize_capability_result(request.request_id, result);
        }
        // 先脱敏再分类，避免模型或 UI 看到的返回值及 EventLog 中的结果携带敏感内容。
        let mut finalized = FinalizedCapabilityAction::new(redact_capability_result(result));
        let mut receipt = match kiana_domain::CapabilityResultReceipt::from_result(
            &finalized.result,
            None,
            None,
            1,
            true,
        ) {
            Ok(receipt) => receipt,
            Err(error) => {
                // Receipt 不能由当前结果构造时，将结果提升为 unknown，避免产生貌似可靠的回执。
                finalized = FinalizedCapabilityAction::unknown(
                    request.request_id,
                    &format!("capability_result_receipt_invalid:{error}"),
                );
                kiana_domain::CapabilityResultReceipt::from_result(
                    &finalized.result,
                    None,
                    None,
                    1,
                    true,
                )
                .map_err(|error| action_error(&error))?
            }
        };
        // Handler output may expose a normal completion detail named `stop_confirmed`; durable
        // cancellation evidence exists only when the ControlPlane cancellation watch is set.
        // Keep that output visible while keeping the capability terminal receipt in the same
        // three-state shape as the execution result envelope.
        if !*cancellation.borrow() && receipt.stop_confirmed.is_some() {
            receipt.stop_confirmed = None;
            receipt.receipt_digest = receipt.digest();
        }
        // Run 工具事件携带运行关联字段；direct 请求采用无 Run 的事件负载格式。
        let mut payload = match run_id {
            Some(run_id) => {
                capability_event_payload(&finalized.result.output, request, context, run_id)
            }
            None => direct_capability_event_payload(&finalized.result.output, request),
        };
        payload["result_receipt"] = json!(receipt);
        let kind = match finalized.status {
            ExecutionStatus::Completed => "capability.completed",
            ExecutionStatus::Cancelled => "capability.cancelled",
            ExecutionStatus::ResultUnknown => "capability.result_unknown",
            _ => "capability.failed",
        };
        // 结果事件无法落盘时，执行结果失去可核验事实，故不能仍向上报告成功。
        if let Err(error) = self
            .record_event(event_request_id, sequence, kind, payload)
            .await
        {
            finalized = FinalizedCapabilityAction::unknown(
                request.request_id,
                "result_event_persistence_failed",
            );
            finalized.result.output["persistence_reason"] =
                json!(stable_event_persistence_reason(&error));
        }
        // 先结算 Cell 额度，再释放 Admission 槽；不确定结果单独记为 Unknown 而非已知失败。
        if let Some(lease) = lease {
            let outcome = match finalized.status {
                ExecutionStatus::Completed => CapabilityOutcome::Succeeded,
                ExecutionStatus::ResultUnknown => CapabilityOutcome::Unknown,
                _ => CapabilityOutcome::Failed,
            };
            if self.finish_cell_capability(lease, outcome).await.is_err() {
                finalized = FinalizedCapabilityAction::unknown(
                    request.request_id,
                    "cell_capability_settlement_failed",
                );
            }
        }
        if let Some(admission_lease) = admission_lease {
            let outcome = if finalized.status == ExecutionStatus::ResultUnknown {
                AdmissionOutcome::Unknown
            } else {
                AdmissionOutcome::Known
            };
            self.admission_scheduler.release(admission_lease, outcome);
        }
        Ok(finalized)
    }

    /// 将 Runner 发来的模型工具意图送回 ControlPlane 授权链，并把结果回灌 Runner。
    ///
    /// 【调用者】`lifecycle.rs::drive_run()` 收到 `RunnerEvent::CapabilityRequested` 后调用。
    /// 【输入】服务端 RequestContext、外层 request/Run ID、已授权 sandbox、事件序号、Runner 的
    /// CapabilityRequest 和取消 watch。
    ///
    /// 【返回值为何有三层】外层 `Err(CoreError)` 表示事件存储/端口等控制面基础设施失败；
    /// `Ok(Err(reason))` 表示工具被拒绝、取消或结果不能确认，调用方按 Runner 失败路径处理；
    /// `Ok(Ok(None))` 表示审批挑战已持久化、Run 正等待用户决定；
    /// `Ok(Ok(Some(events)))` 表示工具结果已交回 Runner，可继续同一个事件驱动循环。
    ///
    /// 【数据流】补齐服务端 Run/Turn 身份 → 规范化请求并固定权限 scope → 先记录 tool-call/request
    /// 事实 → policy + gate + pre-tool hook → Deny 时记录 blocked 并停止批次，Ask 时原子激活审批并保存
    /// resume binding，Allow 时经 scheduler/Cell lease/dispatch/finalize → 将结果转换为 RunnerEvent。
    ///
    /// 【副作用/设计原因】此函数不会直接调用工具 handler；只有 `dispatch_capability_action()` 的
    /// Broker 路径执行获准动作。把模型意图送回 ControlPlane 是为了使模型可见工具无法绕过授权、
    /// 审批、取消、EventLog 与 Receipt 记录。
    pub(crate) async fn broker_harness_capability(
        &self,
        context: &RequestContext,
        request_id: RequestId,
        run_id: RunId,
        sandbox: &str,
        sequence: &mut u64,
        request: CapabilityRequest,
        cancel_rx: &watch::Receiver<bool>,
    ) -> Result<Result<Option<Vec<RunnerEvent>>, String>, CoreError> {
        let mut request = request;
        if let Some(arguments) = request.arguments.as_object_mut() {
            arguments.insert("run_id".to_owned(), json!(run_id));
            arguments
                .entry("turn_id".to_owned())
                .or_insert_with(|| json!(kiana_domain::TurnId::from_uuid(request_id.as_uuid())));
        }
        let request_turn_id = request
            .arguments
            .get("turn_id")
            .and_then(|value| serde_json::from_value::<kiana_domain::TurnId>(value.clone()).ok())
            .unwrap_or_else(|| kiana_domain::TurnId::from_uuid(request_id.as_uuid()));
        let request_step_id = request
            .arguments
            .get("step_id")
            .and_then(|value| serde_json::from_value::<kiana_domain::StepId>(value.clone()).ok());
        let original = request.clone();
        let request = match self
            .prepare_capability_action_cancellable(
                context,
                request,
                Some(sandbox),
                true,
                Some(cancel_rx),
            )
            .await
        {
            Ok(request) => request,
            Err(error) => {
                let reason = redact_event_text(&error.to_string());
                // 准备阶段可能在常规 capability request 事实写入前拒绝畸形、不可信或越出 sandbox 的输入。
                // 这里仍写入脱敏后的请求摘要和明确的 blocked 事实，让进程重启后的投影能识别这次拒绝，
                // 而不必从缺失事件猜测它是否曾经发生。
                self.record_event(
                    request_id,
                    sequence,
                    "run.capability_requested",
                    json!({"run_id":run_id,
                        "request_id":original.request_id,
                        "capability_request_id":original.request_id,
                        "capability":original.capability,
                        "operation":original.operation,
                        "risk":original.risk,
                        "cell_id":original.cell_id,
                        "capability_grant_id":original.capability_grant_id,
                        "budget_lease_id":original.budget_lease_id,
                        "attempt":1,"effect_started":false,"effect_known":true,
                        "zero_effect":true,"stop_state":"not_requested","fenced":false,
                        "action_digest":kiana_domain::capability_action_digest(&original),
                        "turn_id":request_turn_id,
                        "step_id":request_step_id,
                        "invocation_id":kiana_domain::InvocationId::from_uuid(original.request_id.as_uuid()),
                        "arguments":redact_event_value(&original.arguments)}),
                )
                .await?;
                self.record_event(
                    request_id,
                    sequence,
                    "run.capability_blocked",
                    json!({"run_id":run_id,"capability_request_id":original.request_id,"error":reason,
                        "attempt":1,"effect_started":false,"effect_known":true,"zero_effect":true,
                        "stop_state":"not_requested","fenced":false}),
                )
                .await?;
                self.cancel_pending_tools(run_id, request_id, sequence, Some(&original), &reason)
                    .await?;
                return Ok(Err(reason));
            }
        };
        let input_digest = kiana_domain::canonical_action_input_digest(&request).ok();
        self.record_event(
            request_id,
            sequence,
            "run.tool_call",
            json!({"run_id":run_id,
            "capability_request_id":request.request_id,"call_id":request.arguments["call_id"],
            "turn_id":request_turn_id,
            "step_id":request_step_id,
            "invocation_id":kiana_domain::InvocationId::from_uuid(request.request_id.as_uuid()),
            "execution_scope":request.execution_scope,
            "input_digest":input_digest.clone(),
            "tool":request.capability,"operation":request.operation}),
        )
        .await?;
        self.record_event(request_id,sequence,"run.capability_requested",json!({"run_id":run_id,
            "request_id":request.request_id,"capability_request_id":request.request_id,
            "capability":request.capability,"operation":request.operation,"risk":request.risk,
            "cell_id":request.cell_id,"capability_grant_id":request.capability_grant_id,"budget_lease_id":request.budget_lease_id,
            "attempt":1,"effect_started":false,"effect_known":true,"zero_effect":true,
            "stop_state":"not_requested","fenced":false,
            "action_digest":kiana_domain::capability_action_digest(&request),
            "input_digest":input_digest,
            "turn_id":request_turn_id,
            "step_id":request_step_id,
            "invocation_id":kiana_domain::InvocationId::from_uuid(request.request_id.as_uuid()),
            "execution_scope":request.execution_scope,
            "arguments":redact_event_value(&request.arguments)})).await?;
        if *cancel_rx.borrow() {
            self.cancel_pending_tools(
                run_id,
                request_id,
                sequence,
                Some(&request),
                "cancelled:user",
            )
            .await?;
            return Ok(Err("cancelled:user".to_owned()));
        }
        let (policy, gate) = self
            .authorize_capability_action_cancellable(context, &request, None, Some(cancel_rx))
            .await?;
        self.record_event(
            request_id,
            sequence,
            "capability.decision",
            json!({"run_id":run_id,"capability_request_id":request.request_id,"policy":policy,"gate":gate,
                "attempt":1,"effect_started":false,"effect_known":true,"zero_effect":true,
                "stop_state":"not_requested","fenced":false}),
        )
        .await?;
        let authorization_id = match gate {
            GateDecision::Denied { reason } => {
                self.record_event(
                    request_id,
                    sequence,
                    "run.capability_blocked",
                    json!({"run_id":run_id,"capability_request_id":request.request_id,"reason":reason,
                        "attempt":1,"effect_started":false,"effect_known":true,"zero_effect":true,
                        "stop_state":"not_requested","fenced":false}),
                )
                .await?;
                self.cancel_pending_tools(run_id, request_id, sequence, Some(&request), &reason)
                    .await?;
                return Ok(Err(reason));
            }
            GateDecision::AwaitingApproval { reason } => {
                let pending_batch_digest = request
                    .arguments
                    .get("pending_batch_digest")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        kiana_domain::json_digest(&json!({
                            "request_id": request.request_id,
                            "call_id": request.arguments.get("call_id"),
                        }))
                    });
                let resume_binding = kiana_domain::InvocationResumeBinding::from_request(
                    run_id,
                    request_id,
                    &request,
                    context,
                    sandbox,
                    pending_batch_digest,
                )
                .map_err(|error| action_error(&error))?;
                let challenge = match self
                    .stage_capability_action(
                        context,
                        &request,
                        &reason,
                        Some(run_id),
                        request_id,
                        Some(&resume_binding),
                        sequence,
                    )
                    .await
                {
                    Ok(challenge) => challenge,
                    Err(error) => {
                        let reason = redact_event_text(&error.to_string());
                        self.cancel_pending_tools(
                            run_id,
                            request_id,
                            sequence,
                            Some(&request),
                            &reason,
                        )
                        .await?;
                        return Ok(Err(reason));
                    }
                };
                self.pending_invocations
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(
                        challenge.approval_id,
                        PendingInvocation {
                            approval_id: challenge.approval_id,
                            challenge: challenge.clone(),
                            request_id: request.request_id,
                            event_request_id: request_id,
                            event_sequence: *sequence,
                            run_id,
                            request,
                            context: context.clone(),
                            sandbox: sandbox.to_owned(),
                            resume_binding: Some(resume_binding),
                        },
                    );
                return Ok(Ok(None));
            }
            GateDecision::Allowed { authorization_id } => authorization_id,
        };
        let finalized = self
            .dispatch_capability_action(
                context,
                Some(run_id),
                &request,
                authorization_id,
                cancel_rx.clone(),
                request_id,
                sequence,
            )
            .await?;
        if matches!(
            finalized.status,
            ExecutionStatus::Cancelled | ExecutionStatus::ResultUnknown
        ) || finalized.result.output["dispatch_rejected"] == true
        {
            let reason = finalized
                .error
                .unwrap_or_else(|| "result_unknown:capability_result".to_owned());
            self.cancel_pending_tools(
                run_id,
                request_id,
                sequence,
                (finalized.result.output["not_executed"] == true).then_some(&request),
                &reason,
            )
            .await?;
            return Ok(Err(reason));
        }
        match self
            .deliver_capability_result(run_id, finalized.result)
            .await
        {
            Ok(events) => Ok(Ok(Some(events))),
            Err(error) => Ok(Err(redact_event_text(&error.to_string()))),
        }
    }
}

/// 已完成 Broker 派发后供 Core 统一使用的结果与状态快照。
///
/// 它把能力结果的领域错误码映射到外层 ExecutionStatus，并保留直接响应要展示的错误文字；
/// 不代表 EventLog 写入或资源结算必然成功，finalizer 会在这些步骤失败时把状态改为 Unknown。
pub(crate) struct FinalizedCapabilityAction {
    pub(crate) result: CapabilityResult,
    pub(crate) status: ExecutionStatus,
    pub(crate) error: Option<String>,
}
impl FinalizedCapabilityAction {
    /// 从已规范化 CapabilityResult 推导外层执行状态；没有 failure_code 才视为 Completed。
    fn new(result: CapabilityResult) -> Self {
        let status = match result.failure_code() {
            None => ExecutionStatus::Completed,
            Some(kiana_domain::CapabilityErrorCode::Cancelled) => ExecutionStatus::Cancelled,
            Some(
                kiana_domain::CapabilityErrorCode::ResultUnknown
                | kiana_domain::CapabilityErrorCode::CompensationRequired,
            ) => ExecutionStatus::ResultUnknown,
            _ => ExecutionStatus::Failed,
        };
        let error = (!result.success).then(|| {
            result.output["error"]
                .as_str()
                .unwrap_or("capability_failed")
                .to_owned()
        });
        Self {
            result,
            status,
            error,
        }
    }
    /// 构造保守的未知结果，用于无法证明执行结论的后续失败情况。
    fn unknown(request_id: RequestId, reason: &str) -> Self {
        Self::new(kiana_domain::normalize_capability_result(
            request_id,
            CapabilityResult::failure_with_code(
                request_id,
                kiana_domain::CapabilityErrorCode::ResultUnknown,
                Some(reason),
            ),
        ))
    }
}
/// 将稳定原因字符串包装成 ControlPlane 可返回的端口失败。
fn action_error(reason: &str) -> CoreError {
    PortError::Failed(reason.to_owned()).into()
}

/// 将内部 EventStore 错误归并成不暴露实现细节的稳定诊断分类。
fn stable_event_persistence_reason(error: &CoreError) -> &'static str {
    match error {
        CoreError::Port(PortError::Failed(reason)) if reason.starts_with("event_contract_") => {
            "event_contract"
        }
        CoreError::Port(PortError::Failed(reason))
            if reason.starts_with("event_identity_links_") =>
        {
            "event_identity_links"
        }
        CoreError::Port(PortError::Failed(reason)) if reason.starts_with("eventlog_") => {
            "eventlog_boundary"
        }
        CoreError::Port(PortError::Conflict(_)) => "event_conflict",
        CoreError::Port(PortError::Unavailable(_)) => "event_store_unavailable",
        CoreError::Port(PortError::Failed(_)) => "event_store_failed",
        CoreError::Domain(_) => "event_domain_failed",
    }
}
/// 将 Gate 中可能合并编码的审批原因还原为逐项审批条件。
///
/// `approval_requirements:` 前缀后是 JSON 字符串数组；其他原因按单项条件保留。逐项表示后，
/// 恢复审批才能精确核对批准内容，避免对子串或整段拼接文本做脆弱判断。
pub(crate) fn approval_requirements(reason: &str) -> Vec<String> {
    reason
        .strip_prefix("approval_requirements:")
        .and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or_else(|| vec![reason.to_owned()])
}
/// 合并两个审批原因，形成排序且去重的稳定条件集合。
///
/// 排序和去重使同一组要求不会因为 Hook/Gate 返回顺序不同而产生不同表示或重复审批项。
pub(crate) fn merge_requirements(left: &str, right: &str) -> String {
    let mut requirements = approval_requirements(left);
    requirements.extend(approval_requirements(right));
    requirements.sort();
    requirements.dedup();
    format!(
        "approval_requirements:{}",
        serde_json::to_string(&requirements).expect("string array")
    )
}
