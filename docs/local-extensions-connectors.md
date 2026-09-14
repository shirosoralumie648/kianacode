# 本地扩展包与 Connector

这些入口使用 `CommandIntent { name, arguments }`，通过统一协议进入 DaemonHost、
ControlPlane、Policy、Approval 和 Broker。它们不增加模型可见工具。
修改入口只接受服务端认证的人类 actor，不接受 Cell；所有安装、撤销、绑定、回滚和
reconciliation 都要求最终参数的一次性审批。`list` 和 `inspect` 是只读入口。

## 签名扩展包

`extension.manage` 的 action 包括 `inspect`、`list`、`install`、`upgrade`、`revoke`、
`rollback`。`inspect` 返回签名校验、清单、当前 registry version、package SHA256 和
capability diff。安装成功不表示通过了模型评估或安全审计。

包是 `kiana.extension-package.v1` JSON，含 `manifest` 和 `files`。`files` 是相对 POSIX
路径到文件内容十六进制字符串的有序映射；拒绝绝对路径、`..`、反斜杠、符号链接、硬链接
和特殊文件。包文件最大 12 MiB，解码内容最大 4 MiB、128 个文件。Skill 包必须包含
UTF-8 `SKILL.md`，最大 64 KiB。

清单契约见 `kiana-domain/src/extensions.rs` 的 `ExtensionManifest`。必须包含：

- `schema: kiana.extension-manifest.v1`、`extension_id`、`version`、`publisher`、`license`。
- `extension_type`、`effect: read-only | read-write`、`required_capabilities`、
  `provided_capabilities`、`supported_roles`、`network_policy`。
- `content_hash` 和 `signature { algorithm: ed25519, key_id, value }`。
- `requires` 中的精确 `kiana_version`、`protocol_version`；可选能力版本、策略特性、
  Memory collection、平台和其它扩展精确版本依赖。当前不解释 semver 范围。

签名协议固定如下：

1. 按文件路径字典序，对 `kiana.extension-content.v1\0` 后每个文件依次追加：路径 UTF-8
   字节长度的 u64 小端值、路径字节、内容字节长度的 u64 小端值、解码后的内容。
   结果的 SHA256 小写十六进制字符串是 `content_hash`。
2. 清单删除整个 `signature` 属性，递归按对象键排序，再使用无空白 UTF-8 JSON 编码。
   用 Ed25519 对这些字节签名。`ExtensionManifest::signing_bytes()` 是实现权威；这是
   Kiana 固定编码，不声称支持其它 JSON canonicalization 协议。
3. `signature.value` 为 64 字节签名的小写十六进制字符串。
4. 操作员在 daemon 启动环境中配置
   `KIANA_EXTENSION_TRUSTED_KEYS_JSON={"publisher":{"key-id":"32字节公钥的小写十六进制"}}`。
   项目文件和扩展清单不能新增受信公钥。未配置公钥、签名或摘要不匹配时拒绝加载。

先提交只读命令：

```json
{"name":"extension.manage","arguments":{"action":"inspect","package_path":"packs/example.json"}}
```

包路径必须位于受信项目内。`install` / `upgrade` 参数为 `action`、`extension_id`、
`package_path`、从 inspect 得到的 `package_sha256`、`expected_registry_version`、
非空 `idempotency_key`、`reason`。审批后会重新读包并核对最终摘要，文件变更不继承原审批。
`revoke` 不需要包路径和摘要。`rollback` 需要目标历史 `package_sha256`，不需要包路径。
被撤销的包不能回滚复活；同版本不同内容、publisher 变更和依赖损坏会被拒绝。

磁盘只保存 `KIANA_HOME/extensions/packages/<package_sha256>.json` 不可变缓存。
`extension.lifecycle` 聚合事件以 CAS 决定安装、激活、撤销和回滚状态；无事件的缓存不启用。
事件记录许可证、签名、前后版本、权限 diff、操作员和授权 ID。

Skill 包启用后，下一次运行加载其文本为低信任 Context，并将签名包的限制保存进系统
PromptBundle。harness 覆盖模型传入的扩展来源字段，Broker 在每次工具派发前重验版本、
撤销状态和签名，执行所有已加载包限制的交集。`allowed-tools` 不产生授权。
read-only 包只允许声明的只读能力；通用 shell 按可能写入处理。撤销阻止下一次派发，
不会追溯撤销已经执行的效果。升级不会静默改变已运行的 PromptBundle。

当前执行器只启用 Skill 内容。其它类型可验签安装为 `staged`，不自动注册执行器；
Skill 包支持签名包内的无状态迁移声明：`migration_ref` 指向
`{"schema":"kiana.extension-migration.v1","kind":"stateless","from_version":"旧版本","to_version":"新版本"}`。
首次安装的 `from_version` 为 `null`；升级必须精确匹配当前版本与目标清单。迁移摘要、
前后版本及 `scripts_executed: false` 进入同一 CAS 收据；它只迁移无持久状态的 Skill 包。
任意脚本或有状态迁移仍返回 `extension_migration_adapter_unavailable`。
网络 allowlist 只是清单数据，当前扩展网络执行仍被拒绝。原生代码、迁移脚本、签名分发
和远程商店均未实现。

## 本地 Connector

`connector.manage` 的 action 包括 `list`、`bind`、`revoke`、`reconcile`。
`bind` 接收 `definition`、`binding`、`expected_registry_version`、`idempotency_key`、
`reason`。结构见 `kiana-domain/src/connectors.rs`。

`ConnectorDefinition` 只接受 `adapter: local_fixture`、`data_processing: local_only`，
必须启用 idempotency 和 reconciliation，声明每分钟配额及每个操作的
`effect: read_only | write`、`required_scope`。`AccountBinding` 固定 connector/account
身份、读写 scope、项目内 `fixture_path` 和 `fixture_sha256`。绑定审批后会验证 fixture。
不接受外部 transport 或隐式凭证。

fixture 采用以下形状；项目中的实际文件须先计算 SHA256 并写入 binding：

```json
{
  "schema":"kiana.connector-fixture.v1",
  "connector_id":"local-notes",
  "account_id":"local-account",
  "operations":{
    "lookup":[{"payload":{"id":"one"},"outcome":"succeeded","receipt_id":"fixture-one","result":{"text":"example"}}]
  }
}
```

人工调用命令：

```json
{"name":"connector.invoke","arguments":{"binding_id":"notes","operation":"lookup","payload":{"id":"one"},"idempotency_key":"lookup-one"}}
```

Core 从持久化绑定决定 operation 的 scope、effect 和审批等级，把固定 snapshot 和最终
payload 交回普通 capability 授权链。写操作必须单次确认最终 payload。Broker handler
重新核对绑定版本、scope、fixture 摘要和 payload 精确匹配；模型或调用方不能自行指定
更低风险或切换账户。绑定更新/撤销使尚未执行的旧 snapshot 失效。

`connector.invoked` 保存 `ProviderReceipt`、最终 payload SHA256、provider receipt ID、
授权 ID 和来源 `local_fixture`，并以 EventLog CAS 实施幂等和本地配额。
重复键与不同最终参数冲突；同一请求重放返回原收据。`unknown` 保持 `result_unknown`，
不隐式重试。所有结果明确 `external_effect_performed: false`。

`reconcile` 接收未知调用的 `invocation_event_id`、项目内 `receipt_path`、
`receipt_sha256`，以及 registry CAS、幂等键和审批原因。证据文件是
`kiana.provider-receipt.v1`，必须与原 connector、binding、account、operation、
idempotency key、payload SHA256 完全一致，且 outcome 为 succeeded 或 failed。
当前只接受 `source: local_fixture`。审批后追加 `connector.reconciled`，不覆盖原事实。
这验证本地记录的一致性，不声称外部供应商实际完成了动作。

当前没有 webhook、外部请求、退款、跨境处理或远程账户连接。状态和限制的证据等级
仍以 CURRENT_STATUS.md 为准；本切片按用户要求没有新增或运行测试。
