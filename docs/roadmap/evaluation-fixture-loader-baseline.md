# EQ-08 deterministic evaluation fixture loader baseline

> 快照日期：2026-09-17。本页记录 case manifest 与 fixture loader 边界；本地不运行测试，运行时夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`EQ-08`](../roadmap.md#step-eq-08) |
| feature_status | `implemented`（strict manifest + deterministic declared-file loader） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | `tests/eval/manifest.json` → `kiana_commands::eval_fixtures::load_fixture_manifest` → declared fixture bytes |
| this step does | manifest/case schema, case ID uniqueness/order, allow-listed fixture schema, canonical root/path containment, regular-file/size limits and SHA-256 loaded fixture digest |
| this step does not | 不实现 isolated workspace/home、FixtureStore production adapter、provider runner、TraceNormalizer、EventStore capture、Judge 或 quality promotion；EQ-09+ 负责 |

## 2. Loader rules

`EvalFixtureManifest` 使用 `kiana.eval-fixture-manifest.v1`、suite ID、description 和 strict case list；每个 `EvalFixtureCase` 必须有唯一 case ID、relative fixture ref、allow-listed `kiana.runtime-event-fixture.v1` 或 `kiana.provider-fixture.v1` schema 和 positive max bytes。manifest unknown fields/schema、duplicate/empty IDs、absolute/`..`/NUL path、unknown schema、zero/oversized limit fail-closed。

Loader 先 canonicalize root 与 manifest，再 canonicalize每个 declared fixture 并要求 `path.starts_with(root)`，只接受 regular file；metadata/bytes 同时受 per-case 与 global 16MiB limit，输出按 case ID 稳定排序并携带 manifest/file SHA-256。它不扫描隐式目录、不读取 operator home、不把 bytes 送给模型/provider；`tests/eval/` 提供受版本控制的 manifest/runtime fixture。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `fixture_loader_is_deterministic_and_enforces_declared_schema` | case order canonical、file digest、manifest schema/path/size 约束 |
| `fixture_path_escape_unknown_schema_duplicate_and_size_fail_closed` | path escape、unknown schema、duplicate ID、size overflow 均拒绝 |
| `fixture_manifest_unknown_fields_are_not_dropped` | manifest unknown field strict reject |
| `eval_fixture_loader_is_scoped_and_deterministic_without_a_second_runner` | source guard 无 tokio/provider/Broker/second runner |

`.github/workflows/eq08-fixture-loader.yml` 在 GitHub runner 执行 loader fixtures、core source guard、fmt 和 command/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 当前 loader 位于 legacy command crate，尚未通过 `FixtureStore` 端口接入 DaemonHost/ControlPlane；它是受控测试适配器，不是 production durable store。
- fixture schema allow-list 只约束 envelope 名称，不校验 JSONL event 内容、secret/trace/cursor、provider stream 或 case oracle；EQ-10/14/16/17+ 负责。
- canonical path/size 检查不替代文件系统 no-follow/TOCTOU 或 isolated `KIANA_HOME`；EQ-09/11/12/PD/SC 仍开放。
