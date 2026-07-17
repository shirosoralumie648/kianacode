# Volume 09: EDA 领域工作流

## 1. EDA 定位

`/eda` 是 Kiana 的硬件项目操作系统入口。它不是自动画板工具，而是硬件项目推进、资料审查、风险识别和 bring-up 计划工具。

P0 目标：

- 需求捕获。
- 原理图审查清单。
- BOM 风险。
- DFM/Gerber 资料完整性。
- Bring-up 计划。
- Evidence Ledger。

## 2. 输入类型

| 输入 | 说明 |
| --- | --- |
| PRD/需求 | 电源、接口、尺寸、成本、环境 |
| EasyEDA project | 工程文件或导出资料 |
| Schematic PDF/PNG | 原理图审查 |
| BOM | 器件、封装、数量、料号 |
| Gerber | PCB 制造文件 |
| CPL/坐标文件 | 贴片坐标 |
| 约束 | 嘉立创工艺、层数、线宽线距 |

## 3. EDA WorkflowRun

`/eda review` 创建或复用 WorkflowRun。

领域差异：

- rules pack = eda。
- verification profile = eda_review。
- evidence includes hardware artifacts。
- approval includes hardware_order。

## 4. Capture

抽取：

- board purpose。
- power input。
- voltage rails。
- max current。
- MCU/SoC。
- interfaces。
- sensors。
- connectors。
- mechanical constraints。
- cost target。
- assembly target。
- environment。
- safety concerns。

## 5. 原理图审查

检查项：

- 电源输入保护。
- 反接保护。
- TVS/ESD。
- regulator dropout。
- regulator current margin。
- decoupling caps。
- reset circuit。
- boot straps。
- crystal/load caps。
- programming/debug header。
- test points。
- connector pinout。
- IO voltage compatibility。
- analog/digital separation。

输出：

- issue。
- severity。
- evidence。
- recommendation。

## 6. BOM 风险

检查项：

- MPN 是否存在。
- 封装是否匹配。
- 数量是否异常。
- 嘉立创贴片可得性。
- 替代料。
- 生命周期。
- 长交期。
- 成本异常。
- 关键器件单点风险。

注意：

- 库存和价格会变化。
- 必须标注来源时间。
- 不能保证下单可用。

## 7. DFM/Gerber 检查

检查项：

- Gerber 文件是否齐全。
- drill file。
- board outline。
- solder mask。
- silkscreen。
- paste layer。
- line width。
- clearance。
- via size。
- layer count。
- impedance constraints。
- panelization。
- fiducials。
- tooling holes。

P0 可以只做文件完整性和规则清单，不做完整 CAM 解析。

## 8. CPL/坐标检查

检查项：

- designator 匹配 BOM。
- package 匹配。
- rotation。
- side。
- missing components。
- DNP。
- origin。

风险：

- 旋转错误。
- 封装不匹配。
- BOM/CPL designator 不一致。

## 9. Bring-up 计划

Bring-up plan 包含：

- 上电前检查。
- 目检。
- 短路检查。
- 电源空载测试。
- 分阶段上电。
- 电压 rail 测量。
- 电流限制。
- MCU 烧录。
- 时钟检查。
- 通信接口检查。
- 传感器检查。
- 故障记录。

每步字段：

- step。
- tool。
- measurement。
- expected。
- fail action。
- safety note。

## 10. EDA Evidence

Evidence 类型：

- schematic_check。
- bom_check。
- gerber_check。
- cpl_check。
- bringup_step。
- approval_record。

ReviewPacket 字段：

- artifact。
- issue。
- severity。
- confidence。
- recommendation。
- manual_confirmation_required。

## 11. Approval

必须 approval：

- 下单。
- 打样。
- 替换关键器件。
- 高压/电池相关操作。
- 宣称满足认证。

Kiana 默认只输出建议和审查，不执行购买动作。

## 12. EDA 与软件项目统一

`/eda` 不单独建一套 runtime。

复用：

- WorkflowRun。
- Task Card。
- WorkPacket。
- Evidence Ledger。
- VerificationPacket。
- ReviewPacket。
- PolicyDecision。

差异：

- artifact 类型。
- rules pack。
- verification profile。
- approval policy。
