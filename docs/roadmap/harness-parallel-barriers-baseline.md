# H16 有界并行工具组与独占屏障基线

> 快照日期：2026-10-17（初版 2026-09-18）。本页记录工具批次的调度分类和授权屏障；本地不运行测试，夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H16`](harness.md#step-h16) |
| feature_status | `implemented`（确定性并行计划、有界执行器与独占 barrier 合同；实际副作用仍逐调用回 ControlPlane） |
| proof_level | `source`；本地仅静态核对（未编译未运行），GitHub Actions 负责 fixtures |
| focused CI | 手动 workflow [`h16-parallel-barriers-diagnostic.yml`](../../.github/workflows/h16-parallel-barriers-diagnostic.yml) 并行运行 `h16_tool_scheduling`、`h16_parallel_barriers` 与 `h16_bounded_parallel`（含 `cargo fmt --check`）；不把未等待的远程结果提升为更高 proof |
| authority | ToolCatalog owns scheduling/resource metadata; ControlPlane still owns every authorization, permit, result and CAS |
| this step does | descriptor 标记 `parallel_read`/`exclusive`、粗粒度 resource claims 和 max parallelism；`plan_tool_batch` 保持 assistant source order，把相邻只读调用分组，把写/网络/副作用调用分成单调用独占屏障；`ToolBatchScheduler` 按组顺序执行：只读组受 `dispatch_width`（组上限 ∩ run/部署/角色额度）限制，独占组前后各 drain 一次，结果按源 ordinal 回填；Runner 在入队前验证计划，Core 对每个调用独立重验 |
| this step does not | 不让批次级 Allow 覆盖兄弟调用，不把并行计划当 capability，不让未知资源/目录变化绕过 ControlPlane；产品模型循环仍按 H12 串行消费调用，跨进程并发池、队头缓存上限与外部 effect exactly-once 仍未证明 |

## 1. Contract

`ToolCatalogSnapshot` 的调度字段是 server-owned metadata：只有无副作用的 `memory.search`
可成为 `parallel_read`，其余模型工具默认 `exclusive` 且 `max_parallelism=1`。规划器只合并
相邻同资源的只读 calls，所有独占调用自动形成前后 barrier；计划和 call IDs 有界、去重并按
source order 校验。

Runner 在模型输出进入 pending queue 前执行 `plan_tool_batch`；实际请求仍按 H12 的
`queued → dispatched → settled` 与 Core `prepare → authorize → permit → result` 边界流转。
因此撤销、authority/resource read-set 变化、取消和未知结果会阻止尚未执行的调用，后续历史
按声明顺序消费，而不是由一个首个 allow 推导整批权限。

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `write_barrier_never_overlaps_prior_reads` | descriptor/planner 明确读组与独占写调用的 barrier 关系；执行器夹具再用会合点与活跃计数证明写调用执行期间没有任何读调用在途 |
| `revocation_blocks_not_started_parallel_call` | 每个调用独立走 authority/authorization/permit，撤销时未启动调用生成 not_executed；执行器夹具证明被拒后同批尚未派发的兄弟调用不再进入 `execute` |
| `crash_with_later_outcome_ready_does_not_rerun_it` | H13 committed outcome projection 可重用，不能因批次并行重跑；执行器夹具用 `AlreadyCommitted` 准入证明该调用不重新执行 |
| `independent_reads_overlap_but_history_is_source_ordered` | 相邻只读调用形成有界 parallel group，call IDs 保持源顺序；执行器夹具用三线程会合点证明真正并发，同时历史仍是源 ordinal 顺序 |
| `concurrency_one_matches_the_serial_batch_contract` | 额度为 1 时在途调用数恒为 1，历史与完成顺序都等于源顺序（H12 行为一致） |

Domain 侧的纯计划夹具（`kiana-domain/tests/h16_tool_scheduling.rs`）另加三个：
`parallelism_cap_is_the_intersection_of_run_deployment_and_role`（三来源交集与 0→串行退化）、
`dispatch_width_intersects_group_and_environment_limits`（组上限 ∩ 环境额度；独占组恒为 1）、
`dispatch_units_preserve_source_order_across_barriers`（ordinal 连续、组号与模式按源顺序）。

`kiana-core/tests/h16_parallel_barriers.rs` 保留逐调用授权/恢复的源码 marker 守卫，并新增
`h16_behavioural_fixtures_are_wired_and_deny_first` 把上述夹具名绑定到 Runner 执行器、两个
fixture 文件与 focused workflow（要求三个 target 都在矩阵内、执行器在派发前 `admit` 且不得出现
`execute_without_admission`）。

## 3. Proof ceiling and handoff

H16 proof ceiling 为 `source`：调度分类、resource metadata、有界 group、独占 barrier、逐调用
ControlPlane 重验，以及一个有界执行器（会合点/计数器夹具驱动）已经固化。执行器只调度调用方
传入的闭包，不执行工具副作用，也不构成第二条执行循环；产品模型循环对外仍以 H12 的串行结果
消费为安全基线。真正的产品接线（让 Harness 使用该执行器并保持 core 逐调用授权/落账）、
跨进程 resource CAS、队头缓存上限的持久证据、OS/provider overlap 与 external/live/physical
proof 留待后续 CAP/ER/PD/INT 组合步骤。
