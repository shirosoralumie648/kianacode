# H15 工具输出有界与分段读取基线

> 快照日期：2026-10-17（初版 2026-09-18）。本页记录工具输出采集、受保护引用与分页读取的接缝；本地不运行测试，夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H15`](harness.md#step-h15) |
| feature_status | `implemented`（shell/进程输出与受控 output-read；ArtifactStore 全量治理仍由后续 PD/ER 补齐） |
| proof_level | `source`；本地仅做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures 与行为夹具 |
| focused CI | 手动 workflow [`h15-output-bounds-diagnostic.yml`](../../.github/workflows/h15-output-bounds-diagnostic.yml) 单 job 运行 `cargo fmt -p kiana-daemon -p kiana-domain --check`、domain `h15_output_ref`、daemon `h15_output_limits`、daemon `--lib h15` 行为夹具及各自 `--no-run` 编译；不把未等待的远程结果提升为更高 proof |
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

调用点标记夹具（`kiana-domain/tests/h15_output_ref.rs`、`kiana-daemon/tests/h15_output_limits.rs`）
固定生产代码路径与命名；下列 **行为夹具** 在临时归档目录上直接驱动同一份逻辑：

| Fixture | 断言 |
|---|---|
| `huge_tool_output_is_bounded_before_buffering` | chunk reader 在内存增长前限制每流/总输出并保留截断元数据 |
| `artifact_from_other_run_is_denied` | typed output ref 和 owner/run 校验阻止另一个 Run 读取相同 output id |
| `expired_output_reference_is_not_returned_from_cache` | expiry/data epoch 失效后不会从 output cache 返回内容 |
| `truncated_result_can_be_read_in_verified_pages` | 内容摘要、UTF-8 边界和稳定 cursor 约束 offset/limit 分页 |

行为夹具（daemon lib `h15_bounds`/`h15_output_read`，经 `cargo test -p kiana-daemon --lib -- h15` 运行）：

| Fixture | 断言 |
|---|---|
| `h15_bounds::h15_behavior_huge_output_is_bounded_before_buffering` | 4 MiB 内存流下保留量等于 `collect_max_bytes`（先限额再收集）、观测量最多越过硬上限一个读取块、`output_total_limit` 截断元数据保真 |
| `h15_bounds::h15_behavior_line_limit_stops_reader_with_metadata` | LF 行数超限以 `output_line_limit` 停止 reader，保留量不越预算 |
| `h15_bounds::h15_behavior_preview_is_bounded_and_utf8_safe` | 预览正文不超过预算、区分预览/采集截断标记、非法 UTF-8 与 NUL 不以原始字节返回 |
| `h15_output_read::h15_behavior_truncated_result_reads_in_verified_pages` | `store_output_at` 只回有界预览 + typed ref；`read_output_at` 用稳定 cursor/`next_offset` 跨页复原完整安全文本 |
| `h15_output_read::h15_behavior_artifact_from_other_run_is_denied` | 同 output id 换 Run/缺 Run/换 owner 均拒绝，原 owner+Run 仍可读 |
| `h15_output_read::h15_behavior_expired_output_reference_is_not_returned` | 过期引用 `output_reference_expired`，未过期同形状记录仍可读 |
| `h15_output_read::h15_behavior_revoked_data_revision_is_denied` | 记录数据 revision 与当前策略不符 → `output_data_revoked` |
| `h15_output_read::h15_behavior_tampered_content_is_denied` | 引用不变只改正文 → `output_content_digest_mismatch` |
| `h15_output_read::h15_behavior_invalid_offset_stream_and_cursor_are_denied` | 多字节字符中间 offset / 未知 stream / 伪造 cursor 拒绝，合法 cursor 读取下一稳定页 |
| `kiana-domain h15_output_ref` | 预算正数/非递减/绝对上限/封闭 schema；空过期、超上限、invocation 漂移在构造期拒绝 |

## 3. Proof ceiling and handoff

H15 proof ceiling 为 `source`：daemon 已有有界 shell/进程采集和受控 output-read，新增 typed 引用、
Run scope、expiry、内容完整性与 cursor 验证，CI-only fixtures 固化拒绝/成功语义。2026-10-17 追加
daemon lib 行为夹具 `h15_bounds`/`h15_output_read` 与 domain 预算/引用边界夹具，把四条命名验收从
调用点标记提升为树内运行期断言；`store_output`/`read_output` 拆出显式归档目录的
`store_output_at`/`read_output_at`（仅目录来源可注入，校验语义不变）以便在临时目录驱动同一逻辑。
当前输出记录仍是本地 adapter；跨进程 ArtifactStore、生命周期删除传播、备份/保留、MCP 全量
bounded result 和 live/physical effect proof 留待 ER/PD/INT。
