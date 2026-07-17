# Kiana 能力治理权威边界

本目录保存 Kiana Phase 1 的版本化治理事实。它回答“我们依据了哪些公开来源、怎样分类、当前有什么证据”，不回答“整个产品是否完成”。任何 Markdown 报告、表格、进度百分比或产品入口都只能消费这里经验证的 JSON，不能另建完成状态。

## 四类 canonical family

D-20 将治理事实严格拆为四类不可互相替代的 family：

1. `public-baselines/`：Claude Code 公开合同的不可变 public-parity revision，包含 journey、独立 capability、来源映射和显式差异。
2. `repository-registry/`：`reference/` 仓库身份、冻结 revision、license 与 freshness 的不可变 revision。
3. `capability-decisions/`：逐能力的 Adopt、Adapt、Reject 决策、owner、目标位置、风险、测试和复审 revision。
4. `evidence/`：追加式 evidence index；失败复测和 supersede 事件只能追加，不能删除旧证据。

`source-artifacts/` 是上述 family 的受控输入，不是第五种完成权威。它只保存公开来源字节、检索元数据和可离线重算的身份。legacy 文档也只能被分类为迁移输入、生成投影或无状态叙事，不能覆盖 canonical JSON。

## 当前 selector

根级 `current.json` 是封闭的 bundle selector，只允许为每个 family 保存 `revision_id`、仓库相对 `path` 和 canonical SHA-256；evidence 使用同样三字段的 `evidence_head`。根级 `evaluation_time` 只用于确定性 freshness/expiry 计算。selector 不得保存 coverage、proof、freshness、count、percent 或 completion 字段，也不得通过“最新文件名”隐式选头。

消费者必须先验证 selector 指向文件的 hash，再遍历每个 head 的完整 predecessor identity triple：`previous_revision_id`、`previous_revision_path`、`previous_revision_sha256`。任一文件缺失、hash 不符、祖先不可达或 revision 重复都 fail closed。

## 官方公开来源冻结

`source-artifacts/claude-code-public-2026-07-15.json` 保留批准的 `2026-07-15` 基线标签，但其 `retrieved_at` 是实际公开字节检索时间 `2026-07-17T10:51:49Z`。这两个日期含义不同：前者是计划采用的基线名称，后者是本工件能证明的真实观察时间。本工件不声称 2026-07-17 的滚动页面与 2026-07-15 的页面逐字相同，也不伪造 Web Archive 快照。

来源包括官方文档索引、绑定 Anthropic 官方仓库 commit `67f390c9a0b1440d369aebe2ff6a5023db35bf8e` 的 release notes、已安装公开 CLI `2.1.209` 的 help/schema 声明，以及官方支持与部署文档。`archive_uri` 是该真实不可变 Git revision；滚动文档和本地 help 的精确被审查字节则直接保存在条目 `inline-base64:` locator 内，由每条 SHA-256 和整个工件的 SHA-256 约束。该 archive URI 不被解释为滚动文档在 2026-07-15 的历史副本。

规范化方法 `inline-base64-entry-lf-join` v1：公开 UTF-8 文本转为 Unicode NFC；CRLF/CR 转为 LF；移除每行末尾空白和每个条目的终止空行；按 `entries` 数组顺序以一个 LF 连接，并在整体末尾增加一个 LF。`entry_sha256` 对各自解码字节计算，`content_sha256` 对整体连接字节计算。验证器只使用 checked-in bytes，因此上游页面变化后仍可离线复算原身份。

受控工件只含公开材料。不得写入凭据、cookie、私有/专有响应、主机绝对路径、临时路径或只依赖当前时钟的身份。来源冲突保留为不同条目或 blocked capability，不静默挑选方便的结论。

## 不可变工作流

首次导入创建 `revision_kind=genesis` 和具体 `genesis_reason`。任何来源、解释、decision、evidence 或目标 revision 变化都创建新文件；successor 必须绑定完整 predecessor identity triple。旧 revision 不覆盖、不改写，也不因新结论删除旧证据。

`freeze` 只把受控离线输入包装为不可覆盖的冻结收据；`check-drift` 比较 checked-in 指纹和明确传入的本地观察；`refresh` 根据 drift report 创建自包含 successor，并只推进受影响 family。refresh 之后先完成来源解释、license/security 和 evidence review，再人工更新 path/hash-only selector。不得把 selector 的移动当作功能完成证明。

```bash
tmp="$(mktemp -d)"

python3 scripts/freeze-capability-governance.py freeze \
  --kind official-source \
  --input docs/agent-program/kiana-completion/governance/source-artifacts/claude-code-public-2026-07-15.json \
  --output "$tmp/official-source-freeze.json" \
  --artifact-id cc-public-official-2026-07-15 \
  --retrieved-at 2026-07-17T10:51:49Z \
  --archive-uri https://github.com/anthropics/claude-code/tree/67f390c9a0b1440d369aebe2ff6a5023db35bf8e

python3 scripts/freeze-capability-governance.py check-drift \
  --manifest docs/agent-program/kiana-completion/governance/current.json \
  --live-reference-root reference \
  --target-root . \
  --official-source-artifact docs/agent-program/kiana-completion/governance/source-artifacts/claude-code-public-2026-07-15.json \
  --output "$tmp/drift.json"

python3 scripts/freeze-capability-governance.py refresh \
  --manifest docs/agent-program/kiana-completion/governance/current.json \
  --drift-report "$tmp/drift.json" \
  --output-root "$tmp/refreshed" \
  --revision-id governance-refresh-YYYY-MM-DD \
  --review-revision governance-review-YYYY-MM-DD
```

这些 Phase 1 CLI 禁止直接访问网络，也不安装新 package；它们只使用 Python 标准库、checked-in 工件和显式本地根。新的网络采集必须经过共享网络策略和人工来源审查，在 CLI 之外产出新的受控输入；抓取失败产生 blocked/stale 事实，不能沿用旧字节冒充 current。

## Proof 边界

Coverage state、proof level、freshness 和 evidence 是四个独立轴。只有 `coverage_state=verified`、`freshness=current`、当前 evidence 绑定到同一 subject/revision，且 `proof_level` 达到 `required_proof_level` 时，capability 才能计入完成。journey 还要求全部 required children 分别满足该规则；optional、unsupported、not applicable 和 intentional difference 都必须显式保留。

Phase 1 的公开文档、源码检查、schema 和本地命令最多证明 `source`、`local_contract` 或真实执行过的 `local_behavior`。它们绝不制造 `target_environment` 或 `user_value` 证明。目标平台、真实云/企业部署和最终用户验收必须由后续 owner phase 的匹配 evidence 提升；测试数量、stub、类型存在、模型自述和长篇状态说明都不能代替这些证据。
