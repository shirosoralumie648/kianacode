# 贡献指南

感谢参与 Kiana。本文说明如何在当前 Rust workspace 里开发、验证和提交改动。产品范围与完成判定以 [`.planning/PROJECT.md`](.planning/PROJECT.md)、[`.planning/ROADMAP.md`](.planning/ROADMAP.md) 和 [docs/planning-current.md](docs/planning-current.md) 为准。

## 环境

- Rust：workspace `rust-version`（当前 1.96）
- Git
- Python 3：`scripts/validate-json-schema.py` 等契约脚本
- Linux 上若启用 Bash sandbox，需要 `bubblewrap` / `bwrap`

不要把 `reference/` 加进 workspace 构建。

## 常用命令

```bash
cargo build -p kiana-entrypoints --bin kiana --locked
cargo fmt --all --check
cargo test --workspace --locked --offline --no-fail-fast
bash scripts/schema-contract-smoke.sh
bash scripts/release-smoke.sh
```

命令层改动可以先跑：

```bash
cargo test -p kiana-commands --no-fail-fast
```

## 分层约定

- 共享契约先进入 `kiana-types`、`kiana-domain`、`kiana-ports` 或 `kiana-protocol`
- 命令行为放在 `kiana-commands`，再由 `kiana-entrypoints` 接线
- 网络访问走 `kiana-services` 或带 policy 的 tool surface，不要在 command 里新建临时 HTTP client
- `--json`、stream、app-server、MCP、release-proof 合同变化时同步更新 `docs/schemas/` 和 schema smoke
- 产品入口不得直接调用 Tool / Service / Query implementation

更完整的代码约定见 [`.planning/codebase/CONVENTIONS.md`](.planning/codebase/CONVENTIONS.md)。

## 文档

用户文档入口是 [docs/README.md](docs/README.md)。当前规划权威是 [docs/planning-current.md](docs/planning-current.md) 与 [`.planning/PROJECT.md`](.planning/PROJECT.md)。改用户可见行为时，至少更新：

1. `README.md` 或 `QUICKSTART.md` 中对应入口
2. `USAGE.md` 中已接线命令
3. 如有 JSON 合同，`docs/schemas/` 与 smoke 脚本

不要把 crate 内部模块清单写成产品已交付功能。TUI 以 `kiana tui` 实际接线为准，见 [docs/tui.md](docs/tui.md)。

## 提交

- 默认工作分支前缀：`codex/`
- 保持无关脏文件不进本次提交
- 提交信息说明用户可见变化或守卫原因，而不是只写文件名
- 不要把 `graphify-out/`、`.superpowers/` 会话笔记、`target/`、`reference/` checkout 提交进主仓

## License

贡献按 MIT OR Apache-2.0 授权。涉及 `reference/` 的源码复用必须先完成许可证与安全审计。
