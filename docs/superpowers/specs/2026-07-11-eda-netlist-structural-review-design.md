# Kiana EDA Netlist 结构审查设计

## 1. 目标

在现有 `/eda review` 只读工作台上增加 KiCad XML netlist 输入，使 Kiana 能够在不执行完整 ERC、仿真、改板或生产动作的前提下，识别组件覆盖、网络结构、电源网络和常见接口网络的可审计风险。

本切片继续复用 `WorkflowRun -> immutable artifacts -> EvidenceEvent -> VerificationPacket` 闭环，不创建第二套 EDA 状态机。

## 2. 产品边界

### 2.1 支持

- 新增 `--netlist <project-relative-path>`。
- 仅支持 KiCad XML netlist 的 `<export>/<components>/<nets>` 结构。
- 输入在 WorkflowRun 创建前完成 containment、regular-file、大小、UTF-8/XML 结构和快照校验。
- 报告增加 netlist 组件数、网络数、电源网络数、接口网络数和悬空网络数。
- 结构规则结果进入原有 `eda_review.json`、Evidence Ledger 和 `eda_review` VerificationPacket。

### 2.2 不支持

- 不解析 KiCad 原理图内部语义，不替代 KiCad ERC。
- 不计算电气参数、时序、阻抗、SI/PI、热或安全认证结果。
- 不自动修复原理图、PCB、BOM、netlist 或生产文件。
- 不对供应链、价格、库存、替代料或下单做未经 provenance 的结论。

## 3. 命令契约

```text
kiana eda review [existing options] [--netlist hardware/main.xml] [--json]
```

`--netlist` 是可选增强输入。未提供时保持 P0 行为；提供时必须成功解析，否则命令在创建 WorkflowRun 前失败。

## 4. 模块边界

新增 `kiana-commands/src/eda_netlist.rs`，只负责：

- KiCad XML 流式解析。
- 规范化组件引用和 net/node 属性。
- 网络分类与结构规则。
- 返回与 CLI/Workflow 无关的 `NetlistAnalysis`。

`kiana-commands/src/eda.rs` 继续负责：

- CLI 参数与项目路径解析。
- 输入快照和 SHA-256。
- BOM/CPL/Gerber/netlist 分析编排。
- netlist 与 BOM 的交叉检查。
- 报告、Evidence、VerificationPacket 和用户输出。

## 5. 数据模型

```rust
pub(crate) struct NetlistAnalysis {
    pub component_refs: BTreeSet<String>,
    pub component_count: usize,
    pub net_count: usize,
    pub power_net_count: usize,
    pub interface_net_count: usize,
    pub dangling_net_count: usize,
    pub issues: Vec<NetlistIssue>,
}

pub(crate) struct NetlistIssue {
    pub code: &'static str,
    pub severity: &'static str,
    pub message: String,
    pub evidence: Vec<String>,
    pub designator: Option<String>,
}
```

`EdaSummary` 新增以下字段，命令始终输出，JSON Schema 将它们定义为可选字段以兼容已归档的 v1 报告：

- `netlist_components`
- `net_count`
- `power_net_count`
- `interface_net_count`
- `dangling_net_count`

## 6. 解析规则

- XML 必须存在 `export` 根、`components` 和 `nets` 区段。
- 每个 `comp` 必须有非空 `ref`，组件引用不得重复。
- 每个 `net` 必须有非空 `code` 与 `name`，net code 不得重复。
- 每个 `node` 必须有 `ref` 与 `pin`；引用必须指向 netlist 中声明的组件。
- XML 事件数量受输入字节上限约束，解析器不得读取 resolve 阶段之外的新文件内容。

## 7. 结构规则

### 7.1 组件覆盖

- `netlist_component_missing_from_bom`：netlist 组件未出现在 BOM，severity=`error`。
- `bom_component_missing_from_netlist`：BOM 组件未出现在 netlist，severity=`warning`。

### 7.2 网络完整性

- `netlist_empty_net`：网络没有 node，severity=`error`。
- `netlist_dangling_net`：非 no-connect 网络只有一个 node，severity=`warning`。
- `netlist_unknown_component_reference`：node 引用未知组件，解析失败并阻止 WorkflowRun 创建。

### 7.3 电源网络

网络名称匹配 GND、VCC/VDD/VSS/VBAT/VIN/VOUT、常见电压轨，或 node 的 `pintype` 为 `power_in/power_out` 时归类为 power net。

- 有多个 `power_out`：`netlist_power_driver_conflict`，severity=`warning`。
- 有 `power_in` 但没有 `power_out` 且不是 GND：`netlist_power_source_unresolved`，severity=`warning`。该规则只提示人工 ERC，不把连接器、稳压器模型或 power flag 推断为错误。

### 7.4 接口网络

名称匹配 USB、I2C、SDA/SCL、SPI、MOSI/MISO/SCK、UART、TX/RX、CAN、SWD、JTAG 时归类为 interface net。

- 单节点接口网络：`netlist_interface_dangling`，severity=`warning`。

## 8. 状态与验证语义

- `blocked`：仍只由关键 P0 输入缺失产生。
- `review_required`：netlist/BOM 组件覆盖错误或空网络等确定性结构错误。
- `pass`：无 blocked/error；warning 允许 pass，但必须保留 finding 和人工下一步。
- netlist findings 进入同一个 EDA EvidenceEvent；VerificationPacket 状态继续从报告状态派生。

## 9. 版本与兼容性

- 报告 schema 保持 `kiana.eda-review.v1`。
- `rule_version` 升级为 `eda-review-rules.v2`；schema 同时接受 v1/v2，便于验证历史归档。
- `review_id` 包含 netlist path 与 SHA-256，输入变化必然生成新的 review identity。

## 10. 验收标准

1. 合法 KiCad XML netlist 可产生 `pass` 报告及准确 summary。
2. netlist 包含 BOM 外组件时产生 `review_required` 和 Fail VerificationPacket。
3. malformed XML、未知组件引用、重复组件/net code 在 WorkflowRun 创建前失败。
4. netlist 文件修改发生在 resolve 后时，分析仍使用已哈希快照。
5. package/release smoke 使用真实 netlist，并通过 schema validator、事件链和 VerificationPacket 检查。
6. 全 workspace 测试和本地 RC preflight 通过；不声明完整 ERC 或工程签字能力。
