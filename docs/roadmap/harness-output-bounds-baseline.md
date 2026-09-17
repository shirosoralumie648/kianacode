# H15 工具输出有界与分段读取基线

> 快照日期：2026-09-18。本页记录工具输出采集、受保护引用与分页读取的接缝；本地不运行测试，夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H15`](harness.md#step-h15) |
| feature_status | `implemented`（shell/进程输出与受控 output-read；ArtifactStore 全量治理仍由后续 PD/ER 补齐） |
| proof_level | `source`；本地仅做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | ControlPlane/Broker admit `execution.output.read`; daemon owns bounded capture and validates the typed reference before returning bytes |
| this step does | 读取流按 chunk 计数并在进入内存时限额，记录 observed/lines/truncated/read error；完整安全输出落到受保护本地记录，`ExecutionOutputRef` 绑定 output/run/invocation/content/scope/expiry；分页读取按 owner/run/data epoch、内容摘要、offset/limit 和稳定 cursor 校验 |
| this step does not | 不把截断文本称为完整结果，不让模型访问 operator-only output-read，不允许跨 Run/项目猜测 output id 读取，也不声称跨进程 ArtifactStore/删除传播或外部 effect proof |

## 1. Contract

shell stdout/stderr 通过固定 chunk reader，在数据流入时维护每流字节、总观测字节和行数上限；
达到上限只保留 bounded preview，并带 `...truncated...` 与元数据。可保存的安全输出由
`store_output` 脱敏后写入受保护目录，模型只得到 preview 和 typed `output_ref`（内容摘要、
scope、Run/Invocation、过期时间）。

`execution.output.read` 仍需走 ControlPlane/Broker；daemon 重新解析并验证 `ExecutionOutputRef`、
owner/run、data epoch、expiry 和 output content digest，再按 UTF-8 边界返回一页。`cursor` 是
由 output/stream/offset/content digest 派生的稳定摘要，伪造、过期、跨 Run、越界 offset 或内容
篡改均 fail-closed；返回 `next_offset`/`next_cursor`，不会把内部路径交给调用方。

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `huge_tool_output_is_bounded_before_buffering` | chunk reader 在内存增长前限制每流/总输出并保留截断元数据 |
| `artifact_from_other_run_is_denied` | typed output ref 和 owner/run 校验阻止另一个 Run 读取相同 output id |
| `expired_output_reference_is_not_returned_from_cache` | expiry/data epoch 失效后不会从 output cache 返回内容 |
| `truncated_result_can_be_read_in_verified_pages` | 内容摘要、UTF-8 边界和稳定 cursor 约束 offset/limit 分页 |

## 3. Proof ceiling and handoff

H15 proof ceiling 为 `source`：daemon 已有有界 shell/进程采集和受控 output-read，新增 typed 引用、
Run scope、expiry、内容完整性与 cursor 验证，CI-only fixtures 固化拒绝/成功语义。当前输出记录
仍是本地 adapter；跨进程 ArtifactStore、生命周期删除传播、备份/保留、MCP 全量 bounded result
和 live/physical effect proof 留待 ER/PD/INT。
