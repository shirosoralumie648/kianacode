# Kiana Code - 开发笔记

## 项目重构历史

### 2026-06-11：TypeScript → Rust 完全重构
- 从 Claude Code 的 1,436 个 TypeScript 文件重构为 23 个 Rust crate
- 替换 7 个私有 shim 为开源 Rust 库
- 成本：约 $25（全 Sonnet subagent 执行）
- 详见 [REWRITE-REPORT.md](REWRITE-REPORT.md)

## 当前架构

### Workspace 结构
23 个独立 crate，按功能分层：
- 核心基础（constants、types、bootstrap、coordinator）
- 执行层（tasks、tools、query）
- 服务层（services、remote、bridge）
- 扩展层（skills、plugins）
- UI 层（components、ink、screens）
- 命令层（commands、entrypoints）
- 平台层（8 个跨平台功能 crate）

### 依赖管理
- 根 `Cargo.toml` workspace.dependencies 统一版本
- 每个 crate 独立 `Cargo.toml` 仅声明所需依赖
- 内部依赖通过 path 引用

## 技术决策

### 为什么选 Rust？
1. **类型安全** - 编译时捕获大量错误
2. **零成本抽象** - 性能接近 C/C++
3. **内存安全** - 无 GC，无数据竞争
4. **跨平台** - 单一工具链，统一体验
5. **生态成熟** - tokio、serde、clap 等工业级 crate

### 为什么替换 shim？
原 7 个私有 shim（NAPI/Swift/MCP）依赖：
- ❌ 闭源，无法审计
- ❌ macOS 专有，难以移植
- ❌ 构建复杂（Node.js、Xcode）

替换为开源 Rust crate：
- ✅ 完全开源，可审计
- ✅ 跨平台（macOS/Linux/Windows）
- ✅ 单一构建系统（cargo）
- ✅ 性能更优（原生编译）

### 关键技术选型

| 功能 | 选择 | 原因 |
|------|------|------|
| 异步运行时 | tokio | 事实标准，生态最丰富 |
| HTTP 客户端 | reqwest | 高层抽象，支持 rustls |
| HTTP 服务端 | axum | 现代设计，类型安全 |
| TUI 框架 | ratatui | 活跃维护，功能完整 |
| CLI 解析 | clap v4 | derive API，体验优秀 |
| 输入模拟 | enigo | 跨平台，API 简洁 |
| 截图 | xcap | 跨平台，支持多显示器 |
| 浏览器自动化 | chromiumoxide | 异步，基于 CDP |
| 语法高亮 | syntect | 基于 Sublime Text 定义 |

## 待办事项

### 短期（v0.1.0）
- [ ] 添加集成测试
- [ ] 补全文档注释
- [ ] 设置 CI/CD（GitHub Actions）
- [ ] 发布到 crates.io（准备工作）

### 中期（v0.2.0）
- [ ] 性能基准测试（criterion）
- [ ] 错误处理优化（统一错误类型）
- [ ] 日志系统完善（tracing）
- [ ] 配置系统（支持配置文件）

### 长期（v1.0.0）
- [ ] 插件 API 稳定化
- [ ] MCP server 完整实现
- [ ] 浏览器自动化测试
- [ ] 发布 Docker 镜像

## 开发工作流

### 添加新功能
1. 决定属于哪个 crate（或新建）
2. 在该 crate 的 `Cargo.toml` 添加依赖
3. 编写代码 + 测试
4. 运行 `cargo clippy` 和 `cargo test`
5. 提交 PR

### 修复 bug
1. 添加复现测试（失败）
2. 修复代码
3. 验证测试通过
4. 提交

### 优化性能
1. 添加 benchmark（criterion）
2. 分析瓶颈（flamegraph）
3. 优化
4. 验证 benchmark 改进
5. 提交

## 常见问题

### 编译慢？
```bash
# 使用 sccache 缓存编译
cargo install sccache
export RUSTC_WRAPPER=sccache

# 使用 mold 链接器（Linux）
sudo apt install mold
export RUSTFLAGS="-C link-arg=-fuse-ld=mold"
```

### 二进制体积大？
```bash
# 发布构建已优化（strip=true, LTO）
cargo build --release

# 进一步压缩（Linux/macOS）
strip target/release/kiana
upx --best target/release/kiana
```

### 如何调试？
```bash
# VSCode + rust-analyzer
# 或使用 LLDB/GDB

# 查看详细错误
RUST_BACKTRACE=1 cargo run

# 启用 tracing 日志
RUST_LOG=debug cargo run
```

## 资源

### 学习资源
- [Rust Book](https://doc.rust-lang.org/book/)
- [Async Book](https://rust-lang.github.io/async-book/)
- [Tokio Tutorial](https://tokio.rs/tokio/tutorial)

### API 文档
- [Tokio Docs](https://docs.rs/tokio)
- [Serde Docs](https://docs.rs/serde)
- [Ratatui Docs](https://docs.rs/ratatui)

### 工具
- [cargo-watch](https://crates.io/crates/cargo-watch) - 文件变化自动重编译
- [cargo-expand](https://crates.io/crates/cargo-expand) - 展开宏
- [cargo-audit](https://crates.io/crates/cargo-audit) - 安全审计

---

*保持更新，记录重要决策和坑*
