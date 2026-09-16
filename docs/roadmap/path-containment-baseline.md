# P1-H-03 shared path containment baseline

> 快照日期：2026-09-16。本页记录所有副作用边界共用的相对路径/root containment helper，并保留 filesystem adapter 的 symlink/hardlink/TOCTOU 二次检查；本地不运行测试，运行时/guard 夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`P1-H-03`](../roadmap.md#step-p1-h-03) |
| feature_status | `implemented`（domain shared containment + daemon/core consumers source） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | `enforce_path_containment`/`enforce_root_containment` → patch/shell/package/checkpoint/Cell/event scope → existing filesystem no-follow checks |
| this step does | 统一 normalize/allow-list/root checks、固定拒绝 reason，并让副作用边界消费同一 helper |
| this step does not | 不把 lexical containment 当作 symlink/hardlink/rename 的完整 TOCTOU 证明，不放宽 sandbox/MCP/network/secret policy |

## 2. Containment rules

- `enforce_path_containment` 只接受规范化相对路径，拒绝绝对路径、`..`、NUL 和不在 server-owned allow-list 内的路径；`.`/`*` 语义与既有 `allow_list_covers` 保持一致。
- `enforce_root_containment` 检查已经解析的 candidate 是否等于或位于 project root；shell workdir、apply_patch 绝对目标共同使用它。
- apply_patch、shell、package publication、checkpoint restore、execution workspace、Cell grant snapshot 和 event path intersection 已迁移到 helper。MCP/Memory 没有用户可写相对路径，仍通过 project-root/scope resolver 限制。
- helper 成功只代表 lexical scope，不能授予 capability/approval；daemon filesystem adapters 继续检查 canonical path、symlink/hardlink、file identity、rename 和 effect-time state。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `path_containment_is_shared_by_every_side_effecting_tool` | 相对路径规范化、越界/绝对/parent 拒绝和 root prefix 陷阱均 fail-closed |
| `side_effecting_boundaries_use_shared_path_containment` | patch/shell/package/checkpoint/Cell/event scope 共用 domain helper，MCP/Memory 保留 root scope guard |

`.github/workflows/p1-h03-path-containment.yml` 在 GitHub runner 执行 domain containment fixture、core source guard、fmt 和 domain/core/daemon test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 该 helper 是纯 lexical contract；完整 fd-relative/no-follow、目录 rename、hardlink、跨进程写锁和 effect-time TOCTOU 由 CAP/SC/ER/PD 继续收口。
- 旧 `allow_list_covers` 仍作为兼容 API 保留，但新副作用边界不得新增独立 containment 分支；ToolSpec/Policy/Grant/approval 仍是独立授权层。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
