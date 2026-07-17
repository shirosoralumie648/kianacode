# Volume 15: 安全审计与商用化准备

## 1. 目标

Kiana 如果要长期自用并走向可商用，必须把安全和交付证明做成流程，而不是发布前临时检查。

本卷定义：

- 安全审计面。
- 商用化 readiness。
- release proof。
- plugin/MCP trust。
- enterprise offline 要求。

## 2. 安全审计面

检查：

- secrets。
- hardcoded credentials。
- unsafe shell。
- untrusted network。
- dependency risk。
- plugin trust。
- MCP exposure。
- hooks fail-open。
- permission bypass。
- path traversal。
- unsafe file edit。

## 3. Strict Audit Finding

字段：

- finding_id。
- category。
- severity。
- confidence。
- evidence。
- affected_files。
- exploitability。
- remediation。
- owner。

Severity：

- critical。
- high。
- medium。
- low。
- note。

## 4. Security Gate

Critical：

- stop。
- block ship。
- require fix。

High：

- fix before ship。
- user risk acceptance only for non-release work。

Medium：

- fix or record risk。

Low：

- follow-up。

## 5. Commercial Readiness

可商用不等于“功能很多”。

需要：

- install works。
- package lifecycle works。
- license known。
- auth/provider config works。
- plugin policy works。
- release smoke passes。
- docs complete。
- support path clear。
- security proof present。

## 6. Release Proof

Release proof 包含：

- build proof。
- package proof。
- install proof。
- smoke proof。
- signature/notarization if applicable。
- source control proof。
- release notes。
- rollback plan。

## 7. Enterprise Offline

要求：

- no mandatory cloud。
- offline license path。
- local plugin install。
- local MCP policy。
- audit export。
- deterministic config。
- redaction。

## 8. Trust Report

Trust report 输出：

- plugins enabled。
- MCP servers。
- hooks。
- network allowlist。
- shell policy。
- high-risk approvals。
- disabled components。

## 9. Security Review 与 Strix 吸收

Strix 贡献：

- validated findings。
- PoC mindset。
- CI security gate。
- remediation report。

Kiana 吸收：

- finding 必须可复现或有 evidence。
- critical finding block。
- security report 可交付。

不吸收：

- P0 自动攻击性测试。
- 未授权目标扫描。

## 10. Fake/Stub Audit

检查：

- `todo!()`。
- `unimplemented!()`。
- “not implemented”。
- placeholder values。
- fake pass。
- hardcoded success。
- tests-only implementation。
- docs claim without code。

Finding 必须区分：

- production gap。
- test helper。
- intentional placeholder。
- docs-only future work。

## 11. Supply Chain

检查：

- dependency source。
- lock files。
- package scripts。
- postinstall。
- plugin source。
- MCP binary/source。

Policy：

- new dependency ask or review。
- unknown plugin block。
- external binary requires provenance。

## 12. Secrets

行为：

- never print secret。
- redact output。
- detect `.env` access。
- block upload。
- evidence uses redacted summary。

## 13. 商用化 Gate

Gate：

- local_blocking = 0。
- external_blocking known。
- release smoke pass。
- docs match behavior。
- trust report pass。
- security critical = 0。

## 14. 验收

P0 验收：

- `/audit strict` 输出 finding。
- security finding 有 severity。
- release proof 区分 local/external blockers。
- plugin/MCP trust 可报告。
- 不把 roadmap 当商用完成。
