# P1-J3-04 Hybrid 检索基线

> 快照日期：2026-09-18。本文记录当前源码可复核的 hybrid 检索切片；运行时验收由 GitHub Actions 负责，本地不运行测试。

## 已交付的确定性合同

`kiana-daemon/src/memory_retrieval.rs` 在已由 J3-02 ACL 过滤的 `MemoryRecord` 集合上提供单一排序实现：

- `kiana-domain::memory_tokens` 统一产生 Unicode 词项和重叠 CJK 双字组；
- 本地 BM25 风格稀疏得分按查询集合、词频、文档频率和长度归一化计算；
- 可选的本地 `token-vectors` fixture 模型由绝对路径 manifest 指定，manifest/schema、维度、格式、文件大小、普通文件和 SHA-256 均严格校验；
- sparse/dense 两个排序通过 RRF-60 合并，再以固定 `0.7/0.3` 权重做 MMR，所有并列项按记录索引、创建时间、collection/id 取得确定性顺序；
- 命中写入 BM25/dense/RRF/MMR 组件、模型 id/hash、算法版本、matched terms 和 `degraded`/`degraded_reason`，由现有 harness 和 receipt projection 继续携带检索会话、请求和事件 provenance；
- manifest 缺失或模型不可用时保持纯词项降级并显式标记；哈希漂移、非法格式、越界维度和 symlink/TOCTOU 风险 fail-closed。

CI fixture `hybrid_retrieval_is_deterministic_for_a_pinned_model` 在 daemon 私有排序函数上两次运行同一钉版本 fixture，断言完整命中集合、首命中、模型 id/hash、非 degraded 状态和算法版本一致；独立 source guard 约束无网络 embedding、CJK 双字组、RRF/MMR、fail-closed 与 receipt wiring。

## 边界与后续工作

当前实现有意只支持无网络的 JSON `token-vectors` fixture，不把哈希生成向量伪装成真实 embedding。路线卡中提到的 `ort`/ONNX feature-gated 推理、模型注册表/下载与生产索引缓存尚未实现，仍属于后续受控扩展；没有 manifest 时的纯词项降级是当前可用行为。索引持久化、删除/撤销传播、context selection/sent/cited 分层和跨进程恢复不在本切片范围。

## CI 命令

```text
cargo fmt --all --check
cargo test -p kiana-daemon --lib memory_retrieval::tests::hybrid_retrieval_is_deterministic_for_a_pinned_model --locked -- --test-threads=1
cargo test -p kiana-daemon --test p1_j3_04_hybrid_retrieval --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行 `cargo fmt --all`、`cargo fmt --all --check`、`cargo check --workspace --tests --locked --offline` 和 `git diff --check`；不执行任何测试或 smoke binary，也不等待 GitHub CI 结果。

