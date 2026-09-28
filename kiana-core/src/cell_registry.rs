//! Process-local CompanyOS cell admission.
//!
//! This is the local adapter for the CompanyOS resource boundary. It keeps
//! admission, path locks, and budget reservations in one critical section.
//! The state is intentionally process-local; durable recovery belongs to the
//! event/state-store phase and must implement the same port contract.

use async_trait::async_trait;
use kiana_domain::{
    builder_lock_paths, AgentTemplate, BudgetLease, BudgetLeaseId, CellId, CellLifecycle, CellSpec,
    FenceTokenId, RequestId, RetirementRecord, RoleSpec, RunId, SpawnPlanId, SpawnPlanStatus,
    WorkFingerprint,
};
use kiana_ports::{
    CapabilityLease, CapabilityOutcome, CellRegistryPort, PortError, SpawnReservation,
    SpawnReservationRequest,
};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

/// 三个准入上限，都是**硬边界**而不是建议值。
///
/// - `MAX_ROOT_CELLS = 64`：同时存活的**根** Cell 数。一个根 Cell 就是一个自治执行域，
///   它的存在意味着进程里有一段独立的预算、路径锁和生命周期。根 Cell 不设上限的话，
///   单次批量派发就能把进程的内存和路径锁表撑满，而这些东西回收要等 Cell 真正结束。
/// - `MAX_ACTIVE_CELLS = 64`：所有层级的活跃 Cell 总数。它和上一个分开，是因为
///   「根少但每个根生很多子」同样会撑爆，只盯根数是不够的。
/// - `MAX_CHILDREN_PER_PARENT = 8`：单个父 Cell 的活跃子 Cell 数。它比前两个小得多，
///   因为父子之间还有别的共享状态；一个父派生太多子，父子预算的求交结果会迅速收敛到
///   最小的那个，父的预算也就失去了意义。
///
/// 三个数字都写在这里而不是散落，是为了让「准入能长到多大」有一个可以一眼看完的答案。
pub(crate) const TEMPLATE_VERSION: &str = "1.0.0";
const MAX_ROOT_CELLS: usize = 64;
const MAX_ACTIVE_CELLS: usize = 64;
const MAX_CHILDREN_PER_PARENT: usize = 8;

#[derive(Clone)]
struct CellRecord {
    reservation: SpawnReservation,
    locked_paths: Vec<String>,
    resources_released: bool,
    active_capabilities: HashMap<RequestId, CapabilityLease>,
}

#[derive(Clone, Default)]
struct RegistryState {
    model_turns: std::collections::HashSet<String>,
    templates: HashMap<(String, String), AgentTemplate>,
    records: HashMap<SpawnPlanId, CellRecord>,
    by_idempotency: HashMap<String, SpawnPlanId>,
    by_fingerprint: HashMap<WorkFingerprint, SpawnPlanId>,
    by_cell: HashMap<CellId, SpawnPlanId>,
    by_run: HashMap<RunId, Vec<CellId>>,
    budgets: HashMap<kiana_domain::BudgetLeaseId, BudgetLease>,
    path_locks: HashMap<String, CellId>,
}

/// Process-local registry used by the default control-plane composition.
#[derive(Default)]
pub struct MemoryCellRegistry {
    state: Mutex<RegistryState>,
}

impl MemoryCellRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reservation(&self, plan_id: SpawnPlanId) -> Option<SpawnReservation> {
        self.state.lock().ok().and_then(|state| {
            state
                .records
                .get(&plan_id)
                .map(|record| record.reservation.clone())
        })
    }

    pub fn active_cells(&self) -> usize {
        self.state
            .lock()
            .map(|state| {
                state
                    .records
                    .values()
                    .filter(|record| !record.reservation.cell.lifecycle.is_terminal())
                    .count()
            })
            .unwrap_or(0)
    }

    pub fn reservations(&self) -> Vec<SpawnReservation> {
        self.state
            .lock()
            .map(|state| {
                state
                    .records
                    .values()
                    .map(|record| record.reservation.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, RegistryState>, PortError> {
        self.state
            .lock()
            .map_err(|_| PortError::Failed("cell_registry_poisoned".to_owned()))
    }

    fn active_root_cells(state: &RegistryState) -> usize {
        state
            .records
            .values()
            .filter(|record| {
                record.reservation.cell.parent_cell_id.is_none()
                    && !record.reservation.cell.lifecycle.is_terminal()
            })
            .count()
    }

    fn active_children(state: &RegistryState, parent: CellId) -> usize {
        state
            .records
            .values()
            .filter(|record| {
                record.reservation.cell.parent_cell_id == Some(parent)
                    && !record.reservation.cell.lifecycle.is_terminal()
            })
            .count()
    }

    fn active_by_fingerprint(
        state: &RegistryState,
        fingerprint: &WorkFingerprint,
    ) -> Option<SpawnPlanId> {
        state
            .by_fingerprint
            .get(fingerprint)
            .copied()
            .filter(|plan_id| {
                state
                    .records
                    .get(plan_id)
                    .is_some_and(|record| !record.reservation.cell.lifecycle.is_terminal())
            })
    }

    /// 判断两份预算是**同一份**（逐字段相等）。
    ///
    /// 【和 `budget_is_subset` 的区别】
    /// 这是相等判断，只在「这就是同一个租约的同一份预算」时为真。
    /// 它服务的是幂等：同一个 spawn plan 重复到达时，应该认出「我见过了」，
    /// 而不是重新分配一份。
    ///
    /// 【为什么要逐字段比，而不是比 digest】
    /// 因为预算是**不可变值**：一旦分配，任何字段变化都意味着另一份预算。
    /// 少比一个字段，就等于允许一个字段在无人察觉的情况下变化——
    /// 而预算字段是钱。
    fn immutable_budget_matches(left: &BudgetLease, right: &BudgetLease) -> bool {
        left.schema == right.schema
            && left.lease_id == right.lease_id
            && left.max_tool_calls == right.max_tool_calls
            && left.model_call_limit() == right.model_call_limit()
            && left.max_tokens == right.max_tokens
            && left.max_wall_clock_ms == right.max_wall_clock_ms
            && left.max_concurrency == right.max_concurrency
            && left.max_effects == right.max_effects
            && left.max_reserved_budget == right.max_reserved_budget
    }

    /// 判断子预算是否**每一项都不超过**父预算。
    ///
    /// 【这是整个仓库最重要的一条权限规则】
    /// 仓库宪法写的是「子 Cell 权限只能是父级、模板、部门、项目、packet、approval 的**交集**」。
    /// 交集在这里被实现成了「逐项 `<=`」：只要有**任何一项**超出，就不成立。
    ///
    /// 【为什么不用并集 / 为什么不是「取小的」】
    /// 如果实现成「子超出就截断成父的值」，调用方会以为它申请到了自己请求的额度，
    /// 实际上拿到的是被悄悄削过的额度——**静默收窄比明确拒绝危险得多**。
    /// 拒绝让调用方知道自己的请求不成立，从而去改请求或者去申请更大的父预算。
    ///
    /// 【为什么是每一项都比】
    /// 因为预算是多个维度的合取约束：工具调用数、模型调用数、token、墙钟、并发、
    /// effect 数、预留额度。任何一项超出，child 就可能做父不允许它做的事。
    /// 少比一项，那一项就是一个可以被子 Cell 放大的口子。
    ///
    /// 【⚠ 改这个函数前先想清楚】
    /// 放宽任何一个 `<=` 成 `<` 之外的比较，都会让「权限并集」这个反模式重新长出来。
    fn budget_is_subset(child: &BudgetLease, parent: &BudgetLease) -> bool {
        child.max_tool_calls <= parent.max_tool_calls
            && child.model_call_limit() <= parent.model_call_limit()
            && child.max_tokens <= parent.max_tokens
            && child.max_wall_clock_ms <= parent.max_wall_clock_ms
            && child.max_concurrency <= parent.max_concurrency
            && child.max_effects <= parent.max_effects
            && child.max_reserved_budget <= parent.max_reserved_budget
    }

    /// 释放一个 Cell 持有的资源，并且**可重复调用**。
    ///
    /// 【作用】
    /// 归还预算预留、释放它持有的路径锁。
    ///
    /// 【为什么第一行是 `if record.resources_released { return Ok(()) }`】
    /// 因为释放是**幂等**的。retire 路径可能被走两次（一次是显式 retire，一次是
    /// retire_cell 触发的清理），两次都调用释放时，第二次必须什么都不做而不是报错。
    /// 预算重复释放会算成两次退款，路径锁重复释放会把别人的锁删掉。
    ///
    /// 【路径锁为什么要先确认「现在这把锁还是我的」】
    /// ```rust
    /// if state.path_locks.get(path) == Some(&record.reservation.cell.cell_id)
    /// ```
    /// 因为锁可能已经被回收并**重新授予**给另一个 Cell。此时无条件 `remove` 会删掉新主人的锁。
    /// 所有权检查是这个函数里最容易写错、也最难在测试里发现的一行：
    /// 它只在「A 释放、锁已经给了 B」这个时序下才有区别。
    ///
    /// 【⚠ 注意本函数的边界】
    /// 它释放的是**进程内**资源。仓库的 review 记录指出：retire 目前没有追加
    /// `grant.revoked` / `lease.fenced` 这类可投影的 server-owned 事实，
    /// 所以 `authority_read_model` 理论上仍可能把已退役 Cell 的 grant 当作 active。
    /// 那是持久化事实层的缺口，不是本函数能补的——本函数不能凭空写事件。
    fn release_resources(
        state: &mut RegistryState,
        record: &mut CellRecord,
    ) -> Result<(), PortError> {
        if record.resources_released {
            return Ok(());
        }
        let budget = state
            .budgets
            .get_mut(&record.reservation.budget.lease_id)
            .ok_or_else(|| PortError::Failed("spawn_budget_ledger_missing".to_owned()))?;
        budget
            .release(record.reservation.plan.budget_reservation)
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        record.reservation.budget = budget.clone();
        for path in &record.locked_paths {
            if state.path_locks.get(path) == Some(&record.reservation.cell.cell_id) {
                state.path_locks.remove(path);
            }
        }
        record.resources_released = true;
        Ok(())
    }

    /// 当前墙钟毫秒。
    ///
    /// 【两个降级都是刻意的】
    /// - `.min(u128::from(u64::MAX))`：时钟返回的毫秒数在 5.8 亿年后才会溢出 `u64`，
    ///   但**夹住**比 `as u64` 的截断更诚实——截断会得到一个看起来合理但完全错误的数字，
    ///   而夹住得到的是「至少是极大值」；
    /// - `.unwrap_or(0)`：系统时钟早于 Unix 纪元时返回 0，而不是 panic。
    ///   一个还没对时的机器不应该因为读一次时间就把 Cell 注册表搞崩。
    ///
    /// 【⚠ 这不是单调时钟】
    /// 墙钟会往回走（对时、NTP 校正）。预算的墙钟维度依赖它，所以时钟回拨会让一个
    /// 已经用掉的预算看起来没用完。SC-40 的 `capacity_fault_clock_rollback` 就是为这类
    /// 情况准备的：检测到回拨时，先拒绝、再谈其它数字，因为用倒拨过的钟测出来的一切都不算数。
    fn now_unix_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
            .unwrap_or(0)
    }
}

#[async_trait]
impl CellRegistryPort for MemoryCellRegistry {
    async fn account_model_usage(
        &self,
        cell_id: CellId,
        turn_key: &str,
        tokens: u64,
    ) -> Result<BudgetLease, PortError> {
        let mut state = self.lock_state()?;
        let plan_id = *state
            .by_cell
            .get(&cell_id)
            .ok_or_else(|| PortError::Failed("cell_not_found".to_owned()))?;
        let record = state
            .records
            .get(&plan_id)
            .ok_or_else(|| PortError::Failed("cell_not_found".to_owned()))?;
        if record.resources_released || record.reservation.cell.lifecycle != CellLifecycle::Running
        {
            return Err(PortError::Conflict("cell_model_not_running".to_owned()));
        }
        let budget_id = record.reservation.budget.lease_id;
        let key = format!("{cell_id}:{turn_key}");
        let already_charged = state.model_turns.contains(&key);
        let budget = state
            .budgets
            .get_mut(&budget_id)
            .ok_or_else(|| PortError::Failed("spawn_budget_ledger_missing".to_owned()))?;
        if !already_charged {
            budget
                .consume_model_call(tokens)
                .map_err(|reason| PortError::Conflict(reason.to_owned()))?;
        }
        let snapshot = budget.clone();
        state.model_turns.insert(key);
        state
            .records
            .get_mut(&plan_id)
            .expect("existing record")
            .reservation
            .budget = snapshot.clone();
        Ok(snapshot)
    }

    async fn checkpoint_run(&self, run_id: RunId) -> Result<serde_json::Value, PortError> {
        let state = self.lock_state()?;
        let mut reservations = Vec::new();
        for record in state.records.values().filter(|record| {
            record.reservation.cell.root_run_id == run_id && !record.resources_released
        }) {
            if !record.active_capabilities.is_empty() {
                return Err(PortError::Conflict(
                    "cell_checkpoint_invocation_in_flight".to_owned(),
                ));
            }
            let mut reservation = record.reservation.clone();
            reservation.budget = state
                .budgets
                .get(&reservation.budget.lease_id)
                .cloned()
                .ok_or_else(|| PortError::Failed("spawn_budget_ledger_missing".to_owned()))?;
            reservations.push(reservation);
        }
        reservations.sort_by_key(|reservation| {
            (reservation.cell.depth, reservation.cell.cell_id.to_string())
        });
        Ok(
            serde_json::json!({"schema":"kiana.cell-checkpoint.v1", "run_id":run_id,"reservations":reservations}),
        )
    }

    async fn restore_run(
        &self,
        run_id: RunId,
        snapshot: serde_json::Value,
    ) -> Result<(), PortError> {
        let reject = |reason: &str| PortError::Conflict(reason.to_owned());
        if snapshot["schema"] != "kiana.cell-checkpoint.v1"
            || snapshot["run_id"] != serde_json::json!(run_id)
        {
            return Err(reject("cell_snapshot_invalid"));
        }
        let mut reservations: Vec<SpawnReservation> =
            serde_json::from_value(snapshot["reservations"].clone())
                .map_err(|_| reject("cell_snapshot_invalid"))?;
        if reservations.is_empty() || reservations.len() > MAX_ACTIVE_CELLS {
            return Err(reject("cell_snapshot_invalid"));
        }
        reservations.sort_by_key(|reservation| {
            (reservation.cell.depth, reservation.cell.cell_id.to_string())
        });
        // Template contents are re-resolved from trusted role packs. Historical IDs remain
        // part of the receipt; random IDs do not make unchanged template contents stale.
        for reservation in &reservations {
            let mut current = self
                .resolve_template(&reservation.template.role_id, &reservation.template.version)
                .await?;
            current.template_id = reservation.template.template_id;
            if current != reservation.template {
                return Err(reject("cell_snapshot_template_changed"));
            }
        }
        let now = Self::now_unix_ms();
        let mut guard = self.lock_state()?;
        let mut next = guard.clone();
        let mut seen = std::collections::HashSet::new();
        for reservation in reservations {
            let plan = &reservation.plan;
            let cell = &reservation.cell;
            let grant = &reservation.grant;
            let budget = &reservation.budget;
            if !seen.insert(cell.cell_id)
                || cell.root_run_id != run_id
                || plan.status != SpawnPlanStatus::Committed
                || !matches!(
                    cell.lifecycle,
                    CellLifecycle::Running | CellLifecycle::WaitingInput
                )
                || plan.deadline_unix_ms <= now
                || grant.expires_at_unix_ms <= now
                || grant.expires_at_unix_ms < plan.deadline_unix_ms
            {
                return Err(reject("cell_snapshot_expired_or_inactive"));
            }
            plan.validate().map_err(reject)?;
            cell.validate(&reservation.template).map_err(reject)?;
            grant.validate().map_err(reject)?;
            budget.validate().map_err(reject)?;
            reservation.supervision.validate().map_err(reject)?;
            if cell.parent_cell_id != plan.parent_cell_id
                || cell.capability_grant_id != grant.grant_id
                || cell.budget_lease_id != budget.lease_id
                || cell.supervision_lease_id != reservation.supervision.lease_id
                || cell.input_refs != plan.input_refs
                || cell.partition_key != plan.partition
                || cell.output_contract != plan.output_contract
                || plan.count != 1
                || plan.candidate_templates != vec![reservation.template.template_id]
                || reservation.owned_paths != builder_lock_paths(&cell.owned_paths)
                || cell
                    .owned_paths
                    .iter()
                    .any(|path| kiana_domain::enforce_path_containment(&grant.paths, path).is_err())
            {
                return Err(reject("cell_snapshot_authority_mismatch"));
            }
            if let Some(existing) = next.records.get(&plan.plan_id) {
                if existing.reservation != reservation
                    || existing.resources_released
                    || !existing.active_capabilities.is_empty()
                {
                    return Err(reject("cell_snapshot_existing_state_changed"));
                }
                continue;
            }
            if next.by_cell.contains_key(&cell.cell_id)
                || next
                    .by_idempotency
                    .contains_key(plan.idempotency_key.trim())
                || Self::active_by_fingerprint(&next, &reservation.fingerprint).is_some()
                || next
                    .records
                    .values()
                    .filter(|record| !record.resources_released)
                    .count()
                    >= MAX_ACTIVE_CELLS
            {
                return Err(reject("cell_snapshot_resource_conflict"));
            }
            if let Some(parent_id) = cell.parent_cell_id {
                let parent = next
                    .by_cell
                    .get(&parent_id)
                    .and_then(|id| next.records.get(id))
                    .ok_or_else(|| reject("cell_snapshot_parent_missing"))?;
                if parent.resources_released
                    || !parent.reservation.template.delegation_allowed
                    || !parent.reservation.grant.delegation_allowed
                    || !parent.reservation.grant.contains(grant)
                    || !Self::budget_is_subset(budget, &parent.reservation.budget)
                    || cell.depth != parent.reservation.cell.depth.saturating_add(1)
                    || Self::active_children(&next, parent_id) >= MAX_CHILDREN_PER_PARENT
                    || Self::active_children(&next, parent_id)
                        >= parent.reservation.cell.spawn_quota as usize
                {
                    return Err(reject("cell_snapshot_parent_scope_changed"));
                }
            } else if cell.depth != 0 || Self::active_root_cells(&next) >= MAX_ROOT_CELLS {
                return Err(reject("cell_snapshot_root_limit"));
            }
            for path in &reservation.owned_paths {
                if next.path_locks.iter().any(|(held, owner)| {
                    *owner != cell.cell_id && kiana_domain::path_locks_conflict(path, held)
                }) {
                    return Err(reject("path_lock_conflict"));
                }
            }
            if let Some(existing) = next.budgets.get(&budget.lease_id) {
                if existing != budget {
                    return Err(reject("cell_snapshot_budget_changed"));
                }
            } else {
                next.budgets.insert(budget.lease_id, budget.clone());
            }
            let reserved_total = next
                .records
                .values()
                .filter(|record| {
                    !record.resources_released
                        && record.reservation.budget.lease_id == budget.lease_id
                })
                .try_fold(plan.budget_reservation, |sum, record| {
                    sum.checked_add(record.reservation.plan.budget_reservation)
                })
                .ok_or_else(|| reject("cell_snapshot_budget_invalid"))?;
            if reserved_total > budget.reserved_budget {
                return Err(reject("cell_snapshot_budget_invalid"));
            }
            let plan_id = plan.plan_id;
            let cell_id = cell.cell_id;
            next.by_idempotency
                .insert(plan.idempotency_key.trim().to_owned(), plan_id);
            next.by_fingerprint
                .insert(reservation.fingerprint.clone(), plan_id);
            next.by_cell.insert(cell_id, plan_id);
            next.by_run.entry(run_id).or_default().push(cell_id);
            for path in &reservation.owned_paths {
                next.path_locks.insert(path.clone(), cell_id);
            }
            next.records.insert(
                plan_id,
                CellRecord {
                    locked_paths: reservation.owned_paths.clone(),
                    reservation,
                    resources_released: false,
                    active_capabilities: HashMap::new(),
                },
            );
        }
        *guard = next;
        Ok(())
    }

    async fn resolve_template(
        &self,
        role_id: &str,
        version: &str,
    ) -> Result<AgentTemplate, PortError> {
        let role_id = role_id.trim();
        let version = version.trim();
        let role = RoleSpec::lookup(role_id)
            .ok_or_else(|| PortError::Failed("spawn_template_not_found".to_owned()))?;
        if version.is_empty() {
            return Err(PortError::Failed(
                "spawn_template_version_required".to_owned(),
            ));
        }
        let swarm_template = role_id == "builder"
            && matches!(
                version,
                kiana_domain::SWARM_CONTROLLER_TEMPLATE | kiana_domain::SWARM_CHILD_TEMPLATE
            );
        if version != TEMPLATE_VERSION && !swarm_template {
            return Err(PortError::Failed(
                "spawn_template_version_mismatch".to_owned(),
            ));
        }
        let mut state = self.lock_state()?;
        let key = (role.role_id.clone(), version.to_owned());
        if let Some(template) = state.templates.get(&key) {
            return Ok(template.clone());
        }
        let mut template = AgentTemplate::for_role(&role, version.to_owned());
        if version == kiana_domain::SWARM_CONTROLLER_TEMPLATE {
            template.template_id = kiana_domain::swarm_controller_template_id();
            template.default_capabilities.clear();
            template.max_children = 8;
            template.max_depth = 0;
            template.delegation_allowed = true;
        } else if version == kiana_domain::SWARM_CHILD_TEMPLATE {
            template.template_id = kiana_domain::swarm_child_template_id();
            template.max_depth = 1;
        }
        template
            .validate()
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        state.templates.insert(key, template.clone());
        Ok(template)
    }

    async fn reserve_spawn(
        &self,
        request: SpawnReservationRequest,
    ) -> Result<SpawnReservation, PortError> {
        let mut state = self.lock_state()?;
        let SpawnReservationRequest {
            mut plan,
            mut cell,
            template,
            budget,
            grant,
            supervision,
            fingerprint,
            owned_paths,
        } = request;

        plan.validate()
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        if plan.status != SpawnPlanStatus::Proposed {
            return Err(PortError::Failed("spawn_plan_not_proposed".to_owned()));
        }
        if plan.count != 1 || plan.candidate_templates.len() != 1 {
            return Err(PortError::Failed("spawn_count_unsupported".to_owned()));
        }
        if plan.candidate_templates[0] != template.template_id {
            return Err(PortError::Failed(
                "spawn_template_version_mismatch".to_owned(),
            ));
        }
        let template_key = (template.role_id.clone(), template.version.clone());
        if state.templates.get(&template_key) != Some(&template) {
            return Err(PortError::Failed(
                "spawn_template_not_registered".to_owned(),
            ));
        }
        let now = Self::now_unix_ms();
        if plan.deadline_unix_ms <= now {
            return Err(PortError::Failed("spawn_deadline_expired".to_owned()));
        }
        if grant.expires_at_unix_ms <= now
            || grant.expires_at_unix_ms < plan.deadline_unix_ms
            || grant.expires_at_unix_ms
                > now.saturating_add(template.ttl_seconds.saturating_mul(1_000))
            || supervision.stall_threshold_seconds.saturating_mul(1_000)
                < plan.deadline_unix_ms.saturating_sub(now)
        {
            return Err(PortError::Failed("spawn_ttl_invalid".to_owned()));
        }
        template
            .validate()
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        grant
            .validate()
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        budget
            .validate()
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        supervision
            .validate()
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        cell.validate(&template)
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        if cell.lifecycle != CellLifecycle::Proposed {
            return Err(PortError::Failed("cell_not_proposed".to_owned()));
        }
        if cell.parent_cell_id != plan.parent_cell_id {
            return Err(PortError::Failed("spawn_parent_mismatch".to_owned()));
        }
        if cell.capability_grant_id != grant.grant_id
            || cell.budget_lease_id != budget.lease_id
            || cell.supervision_lease_id != supervision.lease_id
        {
            return Err(PortError::Failed(
                "spawn_resource_identity_mismatch".to_owned(),
            ));
        }
        if cell.input_refs != plan.input_refs
            || cell.partition_key != plan.partition
            || cell.output_contract != plan.output_contract
        {
            return Err(PortError::Failed("spawn_contract_mismatch".to_owned()));
        }
        if cell.owned_paths != owned_paths {
            return Err(PortError::Failed("spawn_owned_paths_mismatch".to_owned()));
        }
        if cell
            .owned_paths
            .iter()
            .any(|path| kiana_domain::enforce_path_containment(&grant.paths, path).is_err())
        {
            return Err(PortError::Failed("spawn_grant_scope_invalid".to_owned()));
        }

        let idempotency_key = plan.idempotency_key.trim();
        if let Some(existing_plan_id) = state.by_idempotency.get(idempotency_key).copied() {
            let existing = state
                .records
                .get(&existing_plan_id)
                .ok_or_else(|| PortError::Failed("spawn_registry_inconsistent".to_owned()))?;
            if existing.reservation.fingerprint != fingerprint {
                return Err(PortError::Conflict("spawn_idempotency_conflict".to_owned()));
            }
            let mut replay = existing.reservation.clone();
            replay.replayed = true;
            return Ok(replay);
        }
        if state.records.contains_key(&plan.plan_id) || state.by_cell.contains_key(&cell.cell_id) {
            return Err(PortError::Conflict("spawn_identity_conflict".to_owned()));
        }
        if Self::active_by_fingerprint(&state, &fingerprint).is_some() {
            return Err(PortError::Conflict("spawn_fingerprint_active".to_owned()));
        }

        let active_cells = state
            .records
            .values()
            .filter(|record| !record.reservation.cell.lifecycle.is_terminal())
            .count();
        if active_cells >= MAX_ACTIVE_CELLS {
            return Err(PortError::Conflict(
                "spawn_concurrency_limit_exceeded".to_owned(),
            ));
        }
        if cell.parent_cell_id.is_none() {
            if Self::active_root_cells(&state) >= MAX_ROOT_CELLS {
                return Err(PortError::Conflict("spawn_root_limit_exceeded".to_owned()));
            }
            if cell.depth != 0 {
                return Err(PortError::Failed("spawn_depth_exceeded".to_owned()));
            }
        } else {
            let parent_id = cell.parent_cell_id.expect("checked above");
            let parent_plan = state
                .by_cell
                .get(&parent_id)
                .copied()
                .ok_or_else(|| PortError::Failed("spawn_parent_not_found".to_owned()))?;
            let parent = state
                .records
                .get(&parent_plan)
                .ok_or_else(|| PortError::Failed("spawn_registry_inconsistent".to_owned()))?;
            if !matches!(
                parent.reservation.cell.lifecycle,
                CellLifecycle::Running | CellLifecycle::ReadyToMerge
            ) {
                return Err(PortError::Failed("spawn_parent_not_active".to_owned()));
            }
            if !parent.reservation.template.delegation_allowed
                || !parent.reservation.grant.delegation_allowed
            {
                return Err(PortError::Failed("spawn_delegation_denied".to_owned()));
            }
            let child_count = Self::active_children(&state, parent_id);
            if child_count >= MAX_CHILDREN_PER_PARENT
                || child_count >= parent.reservation.cell.spawn_quota as usize
            {
                return Err(PortError::Conflict(
                    "spawn_children_limit_exceeded".to_owned(),
                ));
            }
            if cell.depth != parent.reservation.cell.depth.saturating_add(1) {
                return Err(PortError::Failed("spawn_depth_exceeded".to_owned()));
            }
            if !parent.reservation.grant.contains(&grant) {
                return Err(PortError::Failed("spawn_grant_not_contained".to_owned()));
            }
            if !Self::budget_is_subset(&budget, &parent.reservation.budget) {
                return Err(PortError::Failed("spawn_budget_not_contained".to_owned()));
            }
        }

        let locked_paths = builder_lock_paths(&owned_paths);
        for path in &locked_paths {
            if state.path_locks.iter().any(|(held, owner)| {
                *owner != cell.cell_id && kiana_domain::path_locks_conflict(path, held)
            }) {
                return Err(PortError::Conflict("path_lock_conflict".to_owned()));
            }
        }

        let budget_ledger = state
            .budgets
            .entry(budget.lease_id)
            .or_insert_with(|| budget.clone());
        if !Self::immutable_budget_matches(budget_ledger, &budget) {
            return Err(PortError::Conflict(
                "spawn_budget_identity_conflict".to_owned(),
            ));
        }
        budget_ledger
            .reserve(plan.budget_reservation)
            .map_err(|reason| PortError::Conflict(reason.to_owned()))?;
        let budget = budget_ledger.clone();

        plan.status = plan
            .status
            .transition(SpawnPlanStatus::Validated)
            .map_err(|error| PortError::Failed(error.to_string()))?;
        plan.status = plan
            .status
            .transition(SpawnPlanStatus::Reserved)
            .map_err(|error| PortError::Failed(error.to_string()))?;
        cell.lifecycle = cell
            .lifecycle
            .transition(CellLifecycle::Validated)
            .map_err(|error| PortError::Failed(error.to_string()))?;

        let reservation = SpawnReservation {
            plan,
            cell,
            template,
            budget,
            grant,
            supervision,
            fingerprint,
            owned_paths: locked_paths.clone(),
            replayed: false,
        };
        let plan_id = reservation.plan.plan_id;
        let cell_id = reservation.cell.cell_id;
        state
            .by_idempotency
            .insert(reservation.plan.idempotency_key.trim().to_owned(), plan_id);
        state
            .by_fingerprint
            .insert(reservation.fingerprint.clone(), plan_id);
        state.by_cell.insert(cell_id, plan_id);
        state
            .by_run
            .entry(reservation.cell.root_run_id)
            .or_default()
            .push(cell_id);
        for path in &locked_paths {
            state.path_locks.insert(path.clone(), cell_id);
        }
        state.records.insert(
            plan_id,
            CellRecord {
                reservation: reservation.clone(),
                locked_paths,
                resources_released: false,
                active_capabilities: HashMap::new(),
            },
        );
        Ok(reservation)
    }

    async fn transition_cell(
        &self,
        cell_id: CellId,
        expected: CellLifecycle,
        next: CellLifecycle,
    ) -> Result<CellSpec, PortError> {
        let mut state = self.lock_state()?;
        let plan_id = state
            .by_cell
            .get(&cell_id)
            .copied()
            .ok_or_else(|| PortError::Failed("cell_not_found".to_owned()))?;
        let mut record = state
            .records
            .remove(&plan_id)
            .ok_or_else(|| PortError::Failed("cell_registry_inconsistent".to_owned()))?;
        let result = (|| {
            if record.reservation.cell.lifecycle != expected {
                return Err(PortError::Conflict("cell_state_conflict".to_owned()));
            }
            let next_lifecycle = record
                .reservation
                .cell
                .lifecycle
                .transition(next)
                .map_err(|error| PortError::Conflict(error.to_string()))?;
            let next_plan = if next == CellLifecycle::Running
                && record.reservation.plan.status == SpawnPlanStatus::Reserved
            {
                Some(
                    record
                        .reservation
                        .plan
                        .status
                        .transition(SpawnPlanStatus::Committed)
                        .map_err(|error| PortError::Conflict(error.to_string()))?,
                )
            } else {
                None
            };
            record.reservation.cell.lifecycle = next_lifecycle;
            if let Some(next_plan) = next_plan {
                record.reservation.plan.status = next_plan;
            }
            Ok(record.reservation.cell.clone())
        })();
        state.records.insert(plan_id, record);
        result
    }

    async fn begin_capability(
        &self,
        cell_id: CellId,
        capability_grant_id: kiana_domain::CapabilityGrantId,
        budget_lease_id: BudgetLeaseId,
        request: &kiana_domain::CapabilityRequest,
    ) -> Result<CapabilityLease, PortError> {
        let mut state = self.lock_state()?;
        let plan_id = state
            .by_cell
            .get(&cell_id)
            .copied()
            .ok_or_else(|| PortError::Failed("cell_not_found".to_owned()))?;
        let (reservation, active_count, resources_released) = {
            let record = state
                .records
                .get(&plan_id)
                .ok_or_else(|| PortError::Failed("cell_registry_inconsistent".to_owned()))?;
            (
                record.reservation.clone(),
                record.active_capabilities.len(),
                record.resources_released,
            )
        };
        if reservation.cell.lifecycle != CellLifecycle::Running || resources_released {
            return Err(PortError::Conflict(
                "cell_capability_cell_not_running".to_owned(),
            ));
        }
        if request.cell_id != Some(cell_id)
            || request.capability_grant_id != Some(capability_grant_id)
            || request.budget_lease_id != Some(budget_lease_id)
        {
            return Err(PortError::Failed(
                "cell_capability_scope_mismatch".to_owned(),
            ));
        }
        if reservation.grant.grant_id != capability_grant_id
            || reservation.budget.lease_id != budget_lease_id
        {
            return Err(PortError::Failed(
                "cell_capability_resource_mismatch".to_owned(),
            ));
        }
        if reservation.grant.expires_at_unix_ms <= Self::now_unix_ms() {
            return Err(PortError::Failed(
                "cell_capability_grant_expired".to_owned(),
            ));
        }
        if !reservation.grant.allows_request(request) {
            return Err(PortError::Failed("cell_capability_grant_denied".to_owned()));
        }
        if state
            .records
            .get(&plan_id)
            .is_some_and(|record| record.active_capabilities.contains_key(&request.request_id))
        {
            return Err(PortError::Conflict(
                "cell_capability_invocation_duplicate".to_owned(),
            ));
        }
        let effect_count = u32::from(request.risk != kiana_domain::RiskLevel::ReadOnly);
        let budget = state
            .budgets
            .get_mut(&budget_lease_id)
            .ok_or_else(|| PortError::Failed("spawn_budget_ledger_missing".to_owned()))?;
        if active_count >= budget.max_concurrency as usize {
            return Err(PortError::Conflict(
                "cell_capability_concurrency_exceeded".to_owned(),
            ));
        }
        budget
            .consume(1, 0, effect_count)
            .map_err(|reason| PortError::Conflict(reason.to_owned()))?;
        let budget_snapshot = budget.clone();
        let lease = CapabilityLease {
            request_id: request.request_id,
            cell_id,
            capability_grant_id,
            budget_lease_id,
            fencing_token: FenceTokenId::new(),
            effect_count,
        };
        let record = state
            .records
            .get_mut(&plan_id)
            .ok_or_else(|| PortError::Failed("cell_registry_inconsistent".to_owned()))?;
        record.reservation.budget = budget_snapshot;
        record
            .active_capabilities
            .insert(request.request_id, lease.clone());
        Ok(lease)
    }

    async fn finish_capability(
        &self,
        lease: CapabilityLease,
        _outcome: CapabilityOutcome,
    ) -> Result<(), PortError> {
        let mut state = self.lock_state()?;
        let plan_id = state
            .by_cell
            .get(&lease.cell_id)
            .copied()
            .ok_or_else(|| PortError::Failed("cell_not_found".to_owned()))?;
        let active = state
            .records
            .get(&plan_id)
            .and_then(|record| record.active_capabilities.get(&lease.request_id))
            .cloned()
            .ok_or_else(|| PortError::Conflict("cell_capability_not_active".to_owned()))?;
        if active != lease {
            return Err(PortError::Conflict(
                "cell_capability_lease_mismatch".to_owned(),
            ));
        }
        let budget_snapshot = state
            .budgets
            .get(&lease.budget_lease_id)
            .cloned()
            .ok_or_else(|| PortError::Failed("spawn_budget_ledger_missing".to_owned()))?;
        let record = state
            .records
            .get_mut(&plan_id)
            .ok_or_else(|| PortError::Failed("cell_registry_inconsistent".to_owned()))?;
        record.active_capabilities.remove(&lease.request_id);
        record.reservation.budget = budget_snapshot;
        Ok(())
    }

    async fn commit_spawn(&self, plan_id: SpawnPlanId) -> Result<SpawnReservation, PortError> {
        let mut state = self.lock_state()?;
        let record = state
            .records
            .get_mut(&plan_id)
            .ok_or_else(|| PortError::Failed("spawn_reservation_not_found".to_owned()))?;
        if record.reservation.plan.status != SpawnPlanStatus::Reserved {
            if record.reservation.plan.status == SpawnPlanStatus::Committed {
                return Ok(record.reservation.clone());
            }
            return Err(PortError::Conflict("spawn_plan_not_reserved".to_owned()));
        }
        if record.reservation.cell.lifecycle != CellLifecycle::Validated {
            return Err(PortError::Conflict("cell_not_validated".to_owned()));
        }
        let committed_plan = record
            .reservation
            .plan
            .status
            .transition(SpawnPlanStatus::Committed)
            .map_err(|error| PortError::Conflict(error.to_string()))?;
        let spawning_cell = record
            .reservation
            .cell
            .lifecycle
            .transition(CellLifecycle::Spawning)
            .map_err(|error| PortError::Conflict(error.to_string()))?;
        let ready_cell = spawning_cell
            .transition(CellLifecycle::Ready)
            .map_err(|error| PortError::Conflict(error.to_string()))?;
        record.reservation.plan.status = committed_plan;
        record.reservation.cell.lifecycle = ready_cell;
        Ok(record.reservation.clone())
    }

    async fn abort_spawn(&self, plan_id: SpawnPlanId, _reason: &str) -> Result<(), PortError> {
        let mut state = self.lock_state()?;
        let mut record = state
            .records
            .remove(&plan_id)
            .ok_or_else(|| PortError::Failed("spawn_reservation_not_found".to_owned()))?;
        if record.resources_released {
            state.records.insert(plan_id, record);
            return Ok(());
        }
        let lifecycle = record.reservation.cell.lifecycle;
        if lifecycle.can_transition_to(CellLifecycle::CancelRequested) {
            let cancelled = lifecycle
                .transition(CellLifecycle::CancelRequested)
                .map_err(|error| PortError::Failed(error.to_string()))?
                .transition(CellLifecycle::Cancelled)
                .map_err(|error| PortError::Failed(error.to_string()))?;
            record.reservation.cell.lifecycle = cancelled;
        } else if !lifecycle.is_terminal() {
            state.records.insert(plan_id, record);
            return Err(PortError::Conflict("cell_abort_state_conflict".to_owned()));
        }
        if record
            .reservation
            .plan
            .status
            .can_transition_to(SpawnPlanStatus::RolledBack)
        {
            record.reservation.plan.status = record
                .reservation
                .plan
                .status
                .transition(SpawnPlanStatus::RolledBack)
                .map_err(|error| PortError::Failed(error.to_string()))?;
        }
        Self::release_resources(&mut state, &mut record)?;
        state.records.insert(plan_id, record);
        Ok(())
    }

    async fn retire_cell(
        &self,
        cell_id: CellId,
        reason: &str,
    ) -> Result<RetirementRecord, PortError> {
        let mut authoritative = self.lock_state()?;
        let mut state = authoritative.clone();
        let plan_id = state
            .by_cell
            .get(&cell_id)
            .copied()
            .ok_or_else(|| PortError::Failed("cell_not_found".to_owned()))?;
        let mut record = state
            .records
            .remove(&plan_id)
            .ok_or_else(|| PortError::Failed("cell_registry_inconsistent".to_owned()))?;
        if record.reservation.cell.lifecycle == CellLifecycle::Retired {
            state.records.insert(plan_id, record);
            return Err(PortError::Conflict(
                "spawn_reservation_already_released".to_owned(),
            ));
        }
        if !record.resources_released {
            if !record.active_capabilities.is_empty() {
                state.records.insert(plan_id, record);
                return Err(PortError::Conflict("cell_capability_in_flight".to_owned()));
            }
            let lifecycle = record.reservation.cell.lifecycle;
            let retired = match lifecycle {
                CellLifecycle::Succeeded | CellLifecycle::ReadyToMerge => lifecycle
                    .transition(CellLifecycle::Retiring)
                    .map_err(|error| PortError::Failed(error.to_string()))?
                    .transition(CellLifecycle::Retired)
                    .map_err(|error| PortError::Failed(error.to_string()))?,
                CellLifecycle::Cancelled | CellLifecycle::Quarantined | CellLifecycle::Blocked => {
                    lifecycle
                        .transition(CellLifecycle::Retiring)
                        .map_err(|error| PortError::Failed(error.to_string()))?
                        .transition(CellLifecycle::Retired)
                        .map_err(|error| PortError::Failed(error.to_string()))?
                }
                CellLifecycle::Failed => lifecycle
                    .transition(CellLifecycle::Quarantined)
                    .map_err(|error| PortError::Failed(error.to_string()))?
                    .transition(CellLifecycle::Retiring)
                    .map_err(|error| PortError::Failed(error.to_string()))?
                    .transition(CellLifecycle::Retired)
                    .map_err(|error| PortError::Failed(error.to_string()))?,
                CellLifecycle::Retiring => lifecycle
                    .transition(CellLifecycle::Retired)
                    .map_err(|error| PortError::Failed(error.to_string()))?,
                _ => {
                    state.records.insert(plan_id, record);
                    return Err(PortError::Conflict("cell_not_ready_to_retire".to_owned()));
                }
            };
            record.reservation.cell.lifecycle = retired;
            Self::release_resources(&mut state, &mut record)?;
        }
        let retirement = RetirementRecord {
            schema: kiana_domain::RETIREMENT_RECORD_SCHEMA.to_owned(),
            cell_id,
            grant_id: record.reservation.grant.grant_id,
            budget_lease_id: record.reservation.budget.lease_id,
            supervision_lease_id: record.reservation.supervision.lease_id,
            released_paths: record.reservation.owned_paths.clone(),
            reason: if reason.trim().is_empty() {
                "retired".to_owned()
            } else {
                reason.trim().to_owned()
            },
            retired_at_unix_ms: Self::now_unix_ms(),
        };
        retirement
            .validate()
            .map_err(|error| PortError::Failed(error.to_owned()))?;
        state.records.insert(plan_id, record);
        *authoritative = state;
        Ok(retirement)
    }

    async fn reservation_for_cell(
        &self,
        cell_id: CellId,
    ) -> Result<Option<SpawnReservation>, PortError> {
        let state = self.lock_state()?;
        let Some(plan_id) = state.by_cell.get(&cell_id).copied() else {
            return Ok(None);
        };
        state
            .records
            .get(&plan_id)
            .map(|record| record.reservation.clone())
            .ok_or_else(|| PortError::Failed("cell_registry_inconsistent".to_owned()))
            .map(Some)
    }

    async fn cell_for_run(&self, run_id: RunId) -> Result<Option<CellId>, PortError> {
        let state = self.lock_state()?;
        let Some(cell_ids) = state.by_run.get(&run_id) else {
            return Ok(None);
        };
        Ok(cell_ids
            .iter()
            .find(|cell_id| {
                state
                    .by_cell
                    .get(cell_id)
                    .and_then(|plan_id| state.records.get(plan_id))
                    .is_some_and(|record| !record.reservation.cell.lifecycle.is_terminal())
            })
            .copied()
            .or_else(|| cell_ids.first().copied()))
    }
}
