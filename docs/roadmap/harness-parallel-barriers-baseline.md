# H16 有界并行工具组与独占屏障基线

> 快照日期：2026-09-18。本页记录工具批次的调度分类和授权屏障；本地不运行测试，夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H16`](harness.md#step-h16) |
| feature_status | `implemented`（确定性并行计划与独占 barrier 合同；实际副作用仍逐调用回 ControlPlane） |
| proof_level | `source`；本地仅做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | ToolCatalog owns scheduling/resource metadata; ControlPlane still owns every authorization, permit, result and CAS |
| this step does | descriptor 标记 `parallel_read`/`exclusive`、粗粒度 resource claims 和 max parallelism；`plan_tool_batch` 保持 assistant source order，把相邻只读调用分组，把写/网络/副作用调用分成单调用独占屏障；Runner 在入队前验证计划，Core 对每个调用独立重验 |
| this step does not | 不让批次级 Allow 覆盖兄弟调用，不把并行计划当 capability，不让未知资源/目录变化绕过 ControlPlane；跨进程并发池和外部 effect exactly-once 仍未证明 |

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
| `write_barrier_never_overlaps_prior_reads` | descriptor/planner 明确读组与独占写调用的 barrier 关系 |
| `revocation_blocks_not_started_parallel_call` | 每个调用独立走 authority/authorization/permit，撤销时未启动调用生成 not_executed |
| `crash_with_later_outcome_ready_does_not_rerun_it` | H13 committed outcome projection 可重用，不能因批次并行重跑 |
| `independent_reads_overlap_but_history_is_source_ordered` | 相邻只读调用形成有界 parallel group，call IDs 保持源顺序 |

## 3. Proof ceiling and handoff

H16 proof ceiling 为 `source`：调度分类、resource metadata、有界 group、独占 barrier 和逐调用
ControlPlane 重验已经固化。当前 Harness 对外仍以串行结果消费为安全基线；真正的 bounded
worker pool、跨进程 resource CAS、队头缓存、OS/provider overlap 与 external/live/physical
proof 留待后续 CAP/ER/PD/INT 组合步骤。
