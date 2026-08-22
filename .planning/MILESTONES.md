# Kiana 发布列车（MILESTONES）

**Recut:** 2026-08-22

当前只执行 **v0.2 Runnable Local Agent MVP**。旧 24 阶段 / M0–M6 1.0 列车文档已删除，不再作为执行依据。

## 当前列车

| 列车 | 名称 | 覆盖阶段 | 性质 |
|------|------|----------|------|
| **v0.2** | Runnable Local Agent MVP | Phase 3–6 | 当前执行 |
| Phase 1 | 证据治理 | 1 | 已完成 |
| Phase 2 | 可复现工具链 | 2 | 实现已落地；人审门禁并行，不阻塞 v0.2 |

## v0.2 退出门禁

全部默认 @ `local_behavior`：

- `PATH-01`–`PATH-04`：已信任仓库 + 一个真实 provider + `kiana -p` / `kiana run` / `kiana tui` + read/edit/shell；缺凭据时失败可见
- `SESS-01`–`SESS-03`：可列出/恢复/继续会话，可取消，失败原因明确
- `TRUST-01`–`TRUST-03`：未信任 fail-closed；权限拒绝可见；不能靠普通 CLI 开关绕过
- `EVD-01`–`EVD-03`：能看到工具调用和文件改动，并能区分模型声称与实际执行；重启后收据仍在

**已知限制：** 单真实 provider；表面限于 CLI print/run + `kiana tui`；不含 IDE/Desktop/Web/Cloud/Enterprise、Research/Daily 深度、38-reference 产品完成、签名 `dist/`、1.0 措辞。

Phase 2 人审门禁保持打开，不在本列车关闭。
