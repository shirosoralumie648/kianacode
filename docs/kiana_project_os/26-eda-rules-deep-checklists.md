# Volume 26: EDA 深度规则清单

## 1. 目标

本卷把 `/eda` 的规则从高层流程展开成可执行审查清单。P0 不自动画板，但必须能系统化审查。

## 2. 电源输入

检查：

- 输入电压范围是否明确。
- 反接保护。
- 保险丝/限流。
- TVS。
- 输入电容耐压。
- 接口额定电流。
- 接地路径。

风险：

- 输入范围不清。
- 保护缺失。
- 电容耐压不足。

## 3. 电源树

检查：

- 每个 rail 电压。
- 每个 rail 电流预算。
- regulator 余量。
- dropout。
- thermal。
- enable sequencing。
- power good。

输出：

- power_tree_summary。
- rail risk。

## 4. MCU/SoC

检查：

- 供电 rail。
- decoupling。
- reset。
- boot mode。
- programming pins。
- debug header。
- clock。
- unused pins。

## 5. 接口电平

检查：

- UART。
- I2C。
- SPI。
- USB。
- CAN。
- RS485。
- GPIO。

关注：

- 电平兼容。
- 上拉。
- 终端电阻。
- ESD。
- 方向控制。

## 6. 模拟信号

检查：

- ADC input range。
- anti-aliasing。
- reference。
- ground split。
- sensor excitation。
- shielding。

## 7. 时钟

检查：

- crystal frequency。
- load caps。
- routing。
- startup。
- fallback clock。

## 8. 连接器

检查：

- pinout。
- keying。
- current rating。
- mechanical。
- polarity。
- label。

## 9. 测试点

必须考虑：

- power rails。
- reset。
- boot。
- UART。
- SWD/JTAG。
- critical analog。
- ground。

## 10. BOM 字段

必需：

- designator。
- quantity。
- value。
- footprint。
- MPN。
- manufacturer。
- supplier。
- assembly。
- DNP。

## 11. BOM 风险规则

风险：

- no MPN。
- no footprint。
- uncommon package。
- low stock。
- single source。
- lifecycle risk。
- high price。
- substitute missing。

## 12. DFM 规则

检查：

- min trace。
- min spacing。
- min drill。
- annular ring。
- solder mask sliver。
- silkscreen overlap。
- board outline。
- copper to edge。

## 13. Gerber 完整性

文件：

- copper layers。
- soldermask。
- silkscreen。
- paste。
- drill。
- outline。

缺失则 Block。

## 14. Assembly 检查

- BOM/CPL designator match。
- rotation。
- side。
- fiducials。
- package mismatch。
- DNP excluded。

## 15. Bring-up 安全

要求：

- current-limited supply。
- no-load power。
- short check。
- thermal check。
- rail by rail。
- stop on overcurrent。

## 16. EDA Review Severity

| Severity | 示例 |
| --- | --- |
| blocker | Gerber missing drill、BOM/CPL mismatch |
| high | no input protection、rail over current |
| medium | missing test point、substitute missing |
| low | silkscreen clarity |
| note | improvement suggestion |

## 17. 输出结构

EDA review 输出：

- summary。
- artifacts。
- blockers。
- risks。
- checklist results。
- bring-up plan。
- approvals required。

## 18. 验收

P1 验收：

- BOM 风险可识别。
- Gerber 缺失可 block。
- bring-up plan 可生成。
- 下单需要 approval。
- 高压/电池风险高亮。
