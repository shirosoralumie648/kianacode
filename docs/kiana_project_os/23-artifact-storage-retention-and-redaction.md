# Volume 23: Artifact 存储、保留与脱敏

## 1. 目标

Kiana 的长期价值依赖 artifact。没有可追溯 artifact，就没有恢复、审计和报告。

Artifact 包括：

- workflow files。
- packets。
- command outputs。
- reports。
- review findings。
- approvals。
- EDA files。
- plugin receipts。

## 2. Artifact 分类

| 类型 | 示例 |
| --- | --- |
| state | workflow.yaml、state.json |
| log | eventlog.jsonl |
| plan | task_plan.md、decision-log.md |
| packet | workpacket/result/verification/review |
| evidence | command output、diff summary |
| report | progress、handoff、audit |
| proof | release/security/license |
| eda | BOM、Gerber summary、bring-up |
| trust | plugin receipt、MCP status |

## 3. 存储位置

默认：

```text
.kiana/workflows/<workflow_id>/
```

跨 workflow 共享：

```text
.kiana/memory/
.kiana/plugins/
.kiana/policy/
.kiana/reports/
```

P0 可以先使用文件系统。

## 4. 命名规则

文件名应：

- 稳定。
- 可排序。
- 可读。
- 包含 id。

示例：

- `workpackets/wp_001.json`
- `resultpackets/rp_001.json`
- `verification/vp_001.json`
- `review/rv_001.json`
- `reports/progress-2026-07-09.md`

## 5. EventLog 保留

EventLog：

- append-only。
- 永久保留。
- 可压缩旧 segment。
- 不应手动编辑。

压缩方式：

- snapshot state。
- archive old events。
- keep hash chain optional。

## 6. Command Output 保留

策略：

- 小输出完整保留。
- 大输出保留 head/tail/summary。
- 完整输出可保存 artifact path。
- 敏感输出必须 redaction。

字段：

- command。
- exit_code。
- stdout_summary。
- stderr_summary。
- full_output_path optional。

## 7. Redaction

需要脱敏：

- API keys。
- tokens。
- passwords。
- private URLs。
- internal hostnames if policy says。
- customer data。

Redaction 规则：

- 原始 secret 不写入 report。
- event 可记录 `redacted: true`。
- 用户可配置保留级别。

## 8. Retention Policy

保留级别：

- keep_forever。
- keep_until_workflow_done。
- keep_until_release。
- keep_summary_only。
- purge_sensitive。

默认：

- eventlog keep_forever。
- reports keep_forever。
- command full output keep_summary_only。
- secrets purge_sensitive。

## 9. Export

导出用途：

- handoff。
- enterprise audit。
- teacher report。
- support bundle。

导出包包含：

- workflow summary。
- state。
- eventlog。
- packets。
- evidence summaries。
- reports。
- trust status。

不包含：

- secrets。
- forbidden files。
- raw user private data unless approved。

## 10. Import

Import 用于恢复或迁移。

要求：

- schema validation。
- trust boundary。
- source record。
- conflict detection。

不允许：

- 导入后直接覆盖 current workflow。
- 不校验外部 artifact。

## 11. Artifact Integrity

可选：

- hash。
- size。
- created_at。
- writer。
- parent event id。

用于：

- tamper detection。
- export validation。
- support proof。

## 12. EDA Artifact

EDA 文件可能很大。

策略：

- 原文件不强制复制。
- 保存 path/hash/summary。
- 对 BOM 保存规范化表。
- 对 Gerber 保存完整性检查结果。

## 13. Artifact Gate

缺少 artifact 时：

- mark missing。
- block if required。
- continue with caution if optional。

## 14. 验收

P0 验收：

- workflow artifacts 可创建。
- eventlog append-only。
- report 可导出。
- command output 可摘要。
- secret redaction 生效。
- missing artifact 能 block。
