//! # 文件职责
//!
//! 本文件把系统允许模型看见的工具整理成一份有版本、有摘要（digest）、可校验的目录。
//! 它位于 `kiana-domain`：这里定义跨 Runner、Daemon、ControlPlane 和 Broker 共用的
//! 工具身份与目录合同，不负责真正执行 shell、写文件或访问 MCP。
//!
//! # 在系统中的位置
//!
//! ```text
//! DaemonHost 启动
//!   -> validate_tool_authority() 检查固定工具面和跨目录绑定
//! Runner / KianaHarness
//!   -> current_tool_catalog() 构造模型可见目录并固定目录摘要
//! 模型返回工具调用
//!   -> Runner 用 tool_spec() / ToolDescriptor 映射为 CapabilityRequest
//!   -> ControlPlane 重新检查策略、审批和执行许可
//!   -> Capability Broker 校验 handler 绑定后才调用具体 handler
//! ```
//!
//! 上游主要是 `kiana-daemon` 的组合根和 `kiana-runner` 的模型调用流程；下游包括
//! `kiana-domain/src/tool_catalog.rs` 的参数 schema、`actions.rs` 的动作合同、
//! Provider 的 wire 名称适配，以及 Core/Broker 的独立校验。相关调用点可从
//! `kiana-daemon/src/lib.rs`、`kiana-runner/src/tools.rs`、`kiana-runner/src/harness.rs`
//! 和 `kiana-capability-broker/src/lib.rs` 继续追踪。
//!
//! # 权限边界
//!
//! `OPERATION_SPECS` 是这里生成 `ToolSpec` 的操作（operation）定义来源；模型参数结构约定
//!（schema）和动作描述符（descriptor）还会分别参与一致性检查。目录说明“模型可以提出哪些调用、
//! 调用应映射到什么操作”，
//! 并通过摘要让调用/恢复流程发现目录漂移，但它本身不是授权凭证。即使某工具出现在
//! 目录中，每次执行仍须经过 Core 的策略、Gate、审批和 Broker 路径。
//!
//! `ToolSpecCollection` 使用惰性缓存只是为了兼容返回静态引用的旧调用方式；缓存由
//! 操作规格投影得到，不是第二份可独立编辑的工具清单。
use crate::{
    json_digest, model_tool_name, tool_schemas, validate_schema_contract, CapabilityKind,
    RiskLevel, OPERATION_SPECS,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// 工具身份权威合同使用的稳定 schema 名称。
pub const TOOL_AUTHORITY_SCHEMA: &str = "kiana.tool-authority.v1";
/// 序列化工具目录快照时使用的 schema 名称。
pub const TOOL_CATALOG_SCHEMA: &str = "kiana.tool-catalog.v1";
/// 当前工具目录格式版本；格式演进时，消费者可以据此拒绝不兼容快照。
pub const TOOL_CATALOG_VERSION: crate::SchemaVersion = crate::SchemaVersion::new(1, 0);
/// 当前目录填入 descriptor 的默认结果大小数字：1024 KiB，即 1 MiB。
/// 这只是 `output_limit` 元数据；仅凭该值不能断言执行路径已经截断工具输出。
pub const TOOL_DEFAULT_OUTPUT_LIMIT: u64 = 1024 * 1024;

/// 从操作规格投影出的模型工具身份和策略元数据。
///
/// 这里保存的是模型工具名到 Domain operation 的稳定映射。Runner 用它识别模型返回的
/// 名称，校验器用它与参数 schema、Capability Action descriptor 对账；执行权仍在后续
/// ControlPlane/Broker 流程中。
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ToolSpec {
    /// 模型和内部代码使用的规范名称；别名最终必须归一化回这个名称。
    pub name: &'static str,
    /// 兼容旧调用格式的名称集合；别名只是名称解析方式，不代表额外能力。
    pub aliases: &'static [&'static str],
    /// 调用该工具时声明的能力类别，后续仍要与动作目录和 Core 策略匹配。
    pub capability: CapabilityKind,
    /// 工具最终对应的内部操作标识，供 CapabilityRequest 和 handler 路由使用。
    pub operation: &'static str,
    /// 该操作的最低风险分类；它是目录元数据，不是已授予的权限。
    pub risk_policy: RiskLevel,
    /// 该工具是否可能产生副作用，用于目录中的 replay/scheduling 分类。
    pub side_effecting: bool,
    /// 工具参数 schema 的稳定标识，供模型 schema 投影与目录校验对齐。
    pub schema: &'static str,
}

/// 对外保留 slice/iterator 风格 API 的轻量视图。
///
/// 实际元素存放在 `TOOL_SPECS_CACHE`。使用零大小类型而不是公开 `Vec`，可以让旧调用者
/// 继续按静态目录遍历，同时避免调用者拥有或修改权威列表。
#[derive(Clone, Copy, Debug)]
pub struct ToolSpecCollection;

/// 所有模块共享的工具规格视图；数据第一次被读取时才从操作目录构造。
pub const TOOL_SPECS: ToolSpecCollection = ToolSpecCollection;
// LazyLock 只把 OPERATION_SPECS 中带有模型工具投影的项转换一次；并发首次访问也只初始化一次。
// 后续调用共享这份只读缓存，减少重复构造，但缓存内容仍由同一操作规格派生，不是第二个事实源。
static TOOL_SPECS_CACHE: std::sync::LazyLock<Vec<ToolSpec>> = std::sync::LazyLock::new(|| {
    OPERATION_SPECS
        .iter()
        .filter_map(tool_spec_from_operation)
        .collect()
});

impl ToolSpecCollection {
    /// 按权威目录顺序借出静态规格，供调用者查找或生成投影。
    pub fn iter(&self) -> impl Iterator<Item = &'static ToolSpec> + '_ {
        TOOL_SPECS_CACHE.iter()
    }

    /// 返回当前模型工具规格数；该数不等于全部可供 Broker 使用的操作数。
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    /// 判断模型工具目录是否为空；启动校验还会检查固定工具数量和绑定关系。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 实现 Rust 标准库的 `Deref`，让只读代码可以把 `TOOL_SPECS` 当作 `[ToolSpec]` slice 使用。
impl std::ops::Deref for ToolSpecCollection {
    type Target = [ToolSpec];

    fn deref(&self) -> &Self::Target {
        // 这里返回缓存切片，让现有 `TOOL_SPECS.iter()` / 索引式消费者保持只读访问。
        TOOL_SPECS_CACHE.as_slice()
    }
}

/// 实现显式的 slice 借用转换，供要求 `AsRef<[ToolSpec]>` 的通用 API 接收该目录视图。
impl AsRef<[ToolSpec]> for ToolSpecCollection {
    fn as_ref(&self) -> &[ToolSpec] {
        TOOL_SPECS_CACHE.as_slice()
    }
}

/// 让 `for spec in TOOL_SPECS` 这种写法直接遍历静态规格引用。
impl IntoIterator for ToolSpecCollection {
    type Item = &'static ToolSpec;
    type IntoIter = std::slice::Iter<'static, ToolSpec>;

    fn into_iter(self) -> Self::IntoIter {
        TOOL_SPECS_CACHE.iter()
    }
}

/// 对借用的目录也提供 `for` 遍历能力，避免消费者为了迭代而转移目录值。
impl IntoIterator for &ToolSpecCollection {
    type Item = &'static ToolSpec;
    type IntoIter = std::slice::Iter<'static, ToolSpec>;

    fn into_iter(self) -> Self::IntoIter {
        TOOL_SPECS_CACHE.iter()
    }
}

/// 把一个操作目录项转换成模型工具规格；没有模型投影的操作不会进入此列表。
///
/// 该函数只在 `TOOL_SPECS_CACHE` 初始化时调用。返回 `None` 表示这个 operation 是内部或
/// operator-only 操作，而不是模型可见工具；其余字段直接复制自操作规格中的模型投影与
/// policy 元数据，避免在这里再维护另一张名称表。`spec.model?` 使用 Rust `Option` 的早退
/// 传播：没有模型投影时直接返回 `None`，不会构造部分规格。
fn tool_spec_from_operation(spec: &crate::OperationSpec) -> Option<ToolSpec> {
    let model = spec.model?;
    Some(ToolSpec {
        name: model.name,
        aliases: model.aliases,
        capability: spec.capability.clone(),
        operation: spec.operation,
        risk_policy: spec.minimum_risk,
        side_effecting: model.side_effecting,
        schema: model.schema,
    })
}

/// 版本化、不可变的工具目录快照，由模型 schema 映射、启动绑定检查和 Broker 注册流程读取。
///
/// 快照把模型工具名称、Provider wire 名称、操作身份、schema 和调度元数据放在同一份可摘要
/// 的值中。Runner/Harness 可把摘要附加到准备执行的数据或 checkpoint 上，之后发现目录变动；
/// 但快照不是授权票据，也不能替代 Core 对具体请求的重新授权。
/// `Serialize` 将结构体编码到 JSON 等外部格式，`Deserialize` 则把外部数据还原为结构体；
/// `deny_unknown_fields` 会拒绝未认识的字段，避免旧版本把新版本合同静默解释成另一种含义。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCatalogSnapshot {
    /// 快照合同名称；不匹配时消费者无法按当前结构解释其字段。
    pub schema: String,
    /// 合同版本；与 schema 一起限定字段语义。
    pub version: crate::SchemaVersion,
    /// 对 schema、版本和全部 descriptor 计算的无密钥摘要，用于发现内容漂移。
    pub digest: String,
    /// 当前模型可见工具及其 wire/schema/policy 映射。
    pub tools: Vec<ToolDescriptor>,
}

/// 单个模型工具在内部目录与外部模型/Provider 表示之间的绑定记录。
///
/// 本结构承载的是描述信息和一致性约束。特别是 `scheduling`、`resources`、
/// `max_parallelism` 不会自行授予资源访问权；每个 CapabilityRequest 仍由 Core/Broker
/// 根据当前 authority 和策略判断。
/// Serde derive 提供 JSON 等外部格式的读写，`deny_unknown_fields` 用于拒绝当前版本不认识的字段。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDescriptor {
    /// 对模型和内部 Runner 可见的规范工具名。
    pub name: String,
    /// Provider 协议层使用的工具名；响应回来后仍须映射回规范名。
    pub wire_name: String,
    /// 仅供兼容解析的输入别名，不会新增 operation 或 Capability。
    pub aliases: Vec<String>,
    /// Runner 生成 CapabilityRequest 时声明的能力类别。
    pub capability: CapabilityKind,
    /// 该工具绑定的内部 operation 名称。
    pub operation: String,
    /// 工具映射声明的最低风险；真正风险判断仍由执行路径完成。
    pub minimum_risk: RiskLevel,
    /// 是否可能产生副作用，影响 replay 与调度分类。
    pub side_effecting: bool,
    /// 模型可传入参数的 JSON Schema（描述输入字段名称、类型和约束的结构化格式）。
    pub argument_schema: Value,
    /// 工具对外结果形状的 JSON Schema；不必与内部 action result schema 相同。
    pub result_schema: Value,
    /// 结果数据的目录大小预算；不是请求体或工具输入的限额。
    /// `validate()` 仅检查该值在 1 字节至 16 MiB 之间；当前源码搜索未找到按此字段截断结果的生产调用点。
    pub output_limit: u64,
    /// 执行模式声明。当前目录只接受由 Broker 中转的 `brokered` 模式。
    pub execution_mode: String,
    /// 重放分类：纯读取为 `replay_safe`，可能有副作用的调用为 `reconcile`。
    pub replay_class: String,
    /// 确定性调度类别：只读调用可以进入有界并行组，其余操作作为独占屏障；Core 仍会逐项重新授权。
    pub scheduling: String,
    /// batch planner 使用的粗粒度资源声明；字段值本身不会授予资源访问权。
    pub resources: Vec<String>,
    /// 调度描述中的最大并行度，由 `tool_scheduling::plan_tool_batch()` 用于限制规划分组大小。
    /// 它不授予能力；规划本身也不执行工具。
    pub max_parallelism: u32,
}

impl ToolCatalogSnapshot {
    /// 从当前 Domain 权威规格生成完整工具目录，并计算其内容摘要。
    ///
    /// 【调用者】Runner 的 schema/请求映射、Harness 的 run/checkpoint 固定流程、Daemon 与
    /// Broker 的启动绑定校验都会读取当前快照。
    ///
    /// 【输入与输出】无输入；返回包含当前版本、固定工具 descriptor 和 digest 的新快照。
    /// descriptor 的名称/operation/capability/risk 来自 `TOOL_SPECS`，模型参数/结果 schema
    /// 与资源分类来自 `model_tool_projection_for_operation()`。
    ///
    /// 【副作用与失败】不访问磁盘、网络或数据库，也不返回错误；第一次读取时会触发上方的
    /// `LazyLock` 内存缓存初始化。生成后通过 `validate()` 检查合同。
    ///
    /// 【为什么这样设计】快照将同一次模型调用要使用的 schema、Provider 名称和内部操作
    /// 绑定到一份值中，便于摘要 pin 和漂移拒绝；调用者不能因此跳过 ControlPlane 授权。
    pub fn current() -> Self {
        let tools = TOOL_SPECS
            .iter()
            .filter_map(|spec| {
                let model = crate::model_tool_projection_for_operation(spec.operation)?;
                let argument_schema = (model.parameters)();
                Some(ToolDescriptor {
                    name: model.name.to_owned(),
                    wire_name: model.name.replace('.', "_"),
                    aliases: model
                        .aliases
                        .iter()
                        .map(|alias| (*alias).to_owned())
                        .collect(),
                    capability: spec.capability.clone(),
                    operation: spec.operation.to_owned(),
                    minimum_risk: spec.risk_policy,
                    side_effecting: spec.side_effecting,
                    argument_schema,
                    result_schema: (model.result_schema)(),
                    // 统一填入 1 MiB 的 output_limit 元数据；快照 validator 允许最高 16 MiB，
                    // 但不能只凭目录字段宣称 handler 已经按它实际截断输出。
                    output_limit: TOOL_DEFAULT_OUTPUT_LIMIT,
                    execution_mode: "brokered".to_owned(),
                    // 有副作用的结果不能盲目重放，需进入 reconcile；只读调用可安全重放。
                    replay_class: if spec.side_effecting {
                        "reconcile".to_owned()
                    } else {
                        "replay_safe".to_owned()
                    },
                    scheduling: if spec.side_effecting {
                        "exclusive".to_owned()
                    } else {
                        "parallel_read".to_owned()
                    },
                    resources: model
                        .catalog_resources
                        .iter()
                        .map(|resource| (*resource).to_owned())
                        .collect(),
                    // 1 表示副作用调用规划成单调用独占组；4 是只读调用的目录并行度上限。
                    // 有界 planner 消费该值来规划组大小；它不是授权，也不表示当前已并发执行。
                    max_parallelism: if spec.side_effecting { 1 } else { 4 },
                })
            })
            .collect::<Vec<_>>();
        let mut snapshot = Self {
            schema: TOOL_CATALOG_SCHEMA.to_owned(),
            version: TOOL_CATALOG_VERSION,
            digest: String::new(),
            tools,
        };
        snapshot.digest = snapshot.unsigned_digest();
        snapshot
    }

    /// 计算不包含 `digest` 字段本身的快照摘要，避免摘要递归依赖自身。
    ///
    /// `json_digest()` 会先按键排序规范化 JSON，再计算 SHA-256。
    /// 这是无密钥内容摘要：它可以发现字段变化，但不能证明快照由可信进程签发。
    fn unsigned_digest(&self) -> String {
        // serde_json 的 `json!` 宏把 schema/version/tools 组合成结构化 JSON Value；
        // 再交给 json_digest 做键顺序无关的规范摘要，不靠拼接字符串避免转义错误。
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "tools": self.tools,
        }))
    }

    /// 检查快照的版本、摘要、descriptor 形状和操作规格绑定。
    ///
    /// 【调用者】Daemon/Runner/Broker 的目录消费边界会在使用快照前调用；固定的全局目录
    /// 还会由 `validate_tool_authority()` 做更完整的名称集合和 alias 校验。
    ///
    /// 【输入与输出】输入是当前对象（可能来自本地构造或反序列化）；合法时返回 `Ok(())`。
    /// 失败时返回稳定错误字符串，例如 `tool_catalog_snapshot_invalid`、
    /// `tool_catalog_descriptor_invalid`、schema 错误或 `tool_catalog_authority_drift`。
    ///
    /// 【副作用】只读字段并计算摘要，不访问权威存储、不执行工具，也不改变对象。
    /// 上游收到错误后应拒绝继续使用该快照；DaemonHost 的启动校验会把目录错误转换为
    /// `PortError::Failed`，Runner/其他消费者则按各自边界向上返回错误。
    ///
    /// 【核心流程】先核对 schema/version/摘要，再逐项检查名称、wire、alias、模式、调度、
    /// 资源、并行度、输出限额和 JSON Schema，最后把 descriptor identity 与 `ToolSpec` 对账，
    /// 并核对快照是否覆盖完整的模型工具集合。
    ///
    /// ⚠ 注意：alias 与 canonical name 的冲突检查在遍历时使用“当前已见到的名称集合”。
    /// 因而一个 alias 若与后面才出现的 canonical name 相同，单独调用本方法未必能发现。
    /// `validate_tool_authority()` 会对固定 `ToolSpec` 的完整规范名集合检查权威 alias，
    /// 但不会把任意外部快照中的 alias/wire/output 元数据逐字段与重新生成的快照比较。
    /// 不要把这里的快照自校验当成对任意自制 descriptor 的完整 authority 审计。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != TOOL_CATALOG_SCHEMA
            || self.version != TOOL_CATALOG_VERSION
            || self.tools.is_empty()
            || self.digest != self.unsigned_digest()
        {
            return Err("tool_catalog_snapshot_invalid".to_owned());
        }
        // 分别累计规范名/alias 与 wire 名；names 在循环中只是“已处理项”的前缀集合。
        let mut names = std::collections::BTreeSet::new();
        let mut wires = std::collections::BTreeSet::new();
        let mut aliases = std::collections::BTreeSet::new();
        for tool in &self.tools {
            // 先拒绝当前合同不接受的值组合：执行必须经 Broker，重放/调度只能取列出的枚举，
            // 只读调用不能声明副作用，exclusive 必须串行，输出必须大于 0 且不超过 16 MiB，
            // 并且规范名、wire 名和 alias 不能产生已知重复。
            // `matches!` 是 Rust 宏，用于检查值是否符合列出的枚举分支之一。
            if tool.name.trim().is_empty()
                || tool.wire_name.trim().is_empty()
                || tool.operation.trim().is_empty()
                || tool.execution_mode != "brokered"
                || !matches!(tool.replay_class.as_str(), "replay_safe" | "reconcile")
                || !matches!(tool.scheduling.as_str(), "parallel_read" | "exclusive")
                || tool.resources.is_empty()
                || tool
                    .resources
                    .iter()
                    .any(|resource| resource.trim().is_empty())
                || tool.max_parallelism == 0
                || tool.scheduling == "parallel_read" && tool.side_effecting
                || tool.scheduling == "exclusive" && tool.max_parallelism != 1
                || tool.output_limit == 0
                || tool.output_limit > 16 * 1024 * 1024
                || !names.insert(tool.name.clone())
                || !wires.insert(tool.wire_name.clone())
                || tool.aliases.iter().any(|alias| {
                    alias.trim().is_empty()
                        || names.contains(alias)
                        || !aliases.insert(alias.clone())
                })
            {
                return Err("tool_catalog_descriptor_invalid".to_owned());
            }
            // 语法合法的 JSON Schema 也必须能被 Domain schema 合同接受，避免消费端收到
            // 看似可解析、实际违反本项目约定的参数或结果定义。
            validate_schema_contract(&tool.argument_schema)
                .map_err(|error| format!("tool_catalog_argument_schema_invalid:{error}"))?;
            validate_schema_contract(&tool.result_schema)
                .map_err(|error| format!("tool_catalog_result_schema_invalid:{error}"))?;
            let spec =
                tool_spec(&tool.name).ok_or_else(|| "tool_catalog_tool_unadvertised".to_owned())?;
            if tool.operation != spec.operation
                || tool.capability != spec.capability.clone()
                || tool.minimum_risk != spec.risk_policy
                || tool.side_effecting != spec.side_effecting
            {
                return Err("tool_catalog_authority_drift".to_owned());
            }
        }
        if self.tools.len() != TOOL_SPECS.len()
            || self
                .tools
                .iter()
                .any(|tool| TOOL_SPECS.iter().all(|spec| spec.name != tool.name))
        {
            return Err("tool_catalog_authority_drift".to_owned());
        }
        Ok(())
    }

    /// 按规范名称或兼容 alias 查找 descriptor；调用者随后仍须校验并重新授权请求。
    ///
    /// 找不到时返回 `None`，Runner 可将其转换为不支持的工具错误；该方法本身不修改快照。
    pub fn descriptor(&self, name: &str) -> Option<&ToolDescriptor> {
        self.tools
            .iter()
            .find(|tool| tool.name == name || tool.aliases.iter().any(|alias| alias == name))
    }
}

/// 返回由当前权威规格派生的目录快照，供非 Domain 模块使用的便捷入口。
///
/// 调用关系与 `ToolCatalogSnapshot::current()` 相同；每次返回新快照值，目录元素从惰性
/// 缓存读取，随后重新计算 digest。
pub fn current_tool_catalog() -> ToolCatalogSnapshot {
    ToolCatalogSnapshot::current()
}

/// 返回当前工具目录的摘要，供 Prepared 请求、配置 revision 或 checkpoint pin 使用。
///
/// 摘要变化表示当前目录内容不同，消费者应拒绝沿用绑定旧目录的准备结果；摘要不是授权
/// token，也没有密钥签名。
pub fn tool_catalog_digest() -> String {
    ToolCatalogSnapshot::current().digest
}

/// 把准备中的模型工具列表与目录版本一并绑定成摘要。
///
/// 【调用者】Provider 请求编译、PreparedModelCall 校验和 Harness 准备路径会用它固定本轮实际
/// 暴露给模型的 schema 列表。
/// 【输入】`tools` 是本轮将发送给模型的精确 JSON schema 列表。
/// 【输出】返回同时包含目录 schema、版本、当前目录 digest 和该列表的无密钥摘要。
/// 【副作用/失败】只做规范 JSON 序列化与 SHA-256 摘要，不访问外部系统；当前签名没有失败返回值。
/// 【设计原因】仅 pin 全量目录不能证明某一次请求实际暴露了哪些工具，因此还要纳入精确
/// 列表；反过来只 hash 列表又无法绑定目录版本和内部映射。调用方在执行/恢复时比较摘要。
pub fn tool_catalog_hash(tools: &[Value]) -> String {
    json_digest(&json!({
        "catalog_schema": TOOL_CATALOG_SCHEMA,
        "catalog_version": TOOL_CATALOG_VERSION,
        "catalog_digest": tool_catalog_digest(),
        "tools": tools,
    }))
}

/// 把规范工具名或兼容 alias 转成 Provider 请求所需的 wire 名称。
///
/// Provider 收到响应后仍需由同一目录映射回内部规范工具。找不到名称时返回 `None`；这里
/// 只做协议名称适配，不授权调用，也不执行网络请求。
pub fn tool_wire_name(name: &str) -> Option<String> {
    ToolCatalogSnapshot::current()
        .descriptor(name)
        .map(|tool| tool.wire_name.clone())
}

/// 通过规范名称或 alias 查找静态 `ToolSpec`。
///
/// Runner 用该查找把模型名称归一化到 server-owned operation 元数据。成功返回的是缓存中
/// 借出的静态引用；未知名称返回 `None`，不会自动降级到 shell 或其他工具。
pub fn tool_spec(name: &str) -> Option<&'static ToolSpec> {
    TOOL_SPECS
        .iter()
        .find(|spec| spec.name == name || spec.aliases.iter().any(|alias| *alias == name))
}

/// 检查一个模型工具规格是否与动作目录、模型 schema 和 Capability Action 描述符一致。
///
/// 【调用者】`validate_tool_action_bindings()` 汇总调用；`validate_tool_authority()` 在
/// DaemonHost 构造阶段触发整组检查。
/// 【输入】一个候选 `ToolSpec`，通常来自 `TOOL_SPECS`；【输出】一致返回 `Ok(())`，否则
/// 返回稳定错误字符串，启动/映射上游据此失败关闭。
/// 【检查内容】规范名称、操作归属、action descriptor 的 operation/capability/minimum risk、
/// alias 回指关系，以及 action 参数 schema 与模型参数 schema 的兼容关系。
/// 【副作用】只读静态目录和 JSON schema，不调用 handler。这里把模型 schema 顶层
/// `additionalProperties` 设为 `true` 后再比较，是为了兼容 action descriptor 允许额外字段
/// 的边界；它不表示模型 schema 本身已经允许任意字段。
///
/// ⚠ 该检查并不比较所有 descriptor 字段，也没有要求 action result schema 与模型工具结果
/// schema 完全相同。通过这里表示有限范围的绑定相符，不表示实际执行成功或已经授权。
pub fn validate_tool_action_binding(spec: &ToolSpec) -> Result<(), String> {
    let model_name =
        model_tool_name(spec.name).ok_or_else(|| "tool_action_tool_unknown".to_owned())?;
    if model_name != spec.name {
        return Err("tool_action_model_name_mismatch".to_owned());
    }

    let operation = crate::canonical_action_operation(model_name)
        .ok_or_else(|| "tool_action_operation_unknown".to_owned())?;
    if operation != spec.operation || !crate::ACTION_OPERATIONS.contains(&spec.operation) {
        return Err("tool_action_operation_mismatch".to_owned());
    }
    let descriptor = crate::capability_action_descriptor(operation)
        .ok_or_else(|| "tool_action_descriptor_missing".to_owned())?;
    if descriptor.operation != spec.operation {
        return Err("tool_action_operation_mismatch".to_owned());
    }
    if descriptor.capability != spec.capability || descriptor.minimum_risk != spec.risk_policy {
        return Err("tool_action_metadata_mismatch".to_owned());
    }

    for alias in spec.aliases {
        if model_tool_name(alias) != Some(spec.name)
            || crate::canonical_action_operation(alias) != Some(spec.operation)
        {
            return Err("tool_action_alias_mismatch".to_owned());
        }
    }

    let model_schema = tool_schemas()
        .into_iter()
        .find(|schema| schema["name"] == spec.name)
        .and_then(|schema| schema.get("parameters").cloned())
        .ok_or_else(|| "tool_action_model_schema_missing".to_owned())?;
    // Provider/model 输入合同与内部 action 参数合同需要按项目定义的兼容规则比较。
    // 先放宽这一项，再做结构相等检查；不能据此把模型输入校验整体关闭。
    let mut compatible_action_schema = model_schema;
    compatible_action_schema["additionalProperties"] = json!(true);
    if descriptor.argument_schema != compatible_action_schema {
        return Err("tool_action_schema_mismatch".to_owned());
    }

    Ok(())
}

/// 对当前完整模型工具目录逐项执行 action binding 校验。
///
/// 由 `validate_tool_authority()` 在启动总检时调用。遇到第一个失败便原样返回原因，不吞掉
/// 具体错误；该过程不执行工具，也不修改规格。
pub fn validate_tool_action_bindings() -> Result<(), String> {
    for spec in TOOL_SPECS {
        validate_tool_action_binding(spec)?;
    }
    Ok(())
}

/// 在组合根校验当前唯一的模型可见工具目录及其跨合同绑定。
///
/// 【调用者】`kiana-daemon::DaemonHost` 构造过程中调用；失败会映射为 `PortError::Failed`，
/// 使宿主无法以不一致的工具 surface 启动。
/// 【输入/输出】不接收用户输入；完整目录合法时返回 `Ok(())`，否则返回具体稳定错误码字符串。
/// 【核心流程】固定数量 → 快照自校验 → 规范名称唯一性 → operation/schema 非空与 schema
/// 覆盖 → 对完整规范名称集合检查 alias 冲突 → 确认模型 schema 名集合完全相等 → 对照
/// Capability Action 绑定。
/// 【副作用】只读编译期/内存目录，不读配置、不触发模型调用、不授予任何 capability。
/// 【为什么固定检查五项】当前安全边界把模型可见工具面限定为五项；若增加或删除，必须
/// 同步审查 Runner 映射、schema、Provider wire 名、Broker 绑定和对应守卫，不能只改一个列表。
/// 该约束是启动时 fail-closed 的保护，不应通过改写常量或绕过校验来“兼容”新工具。
pub fn validate_tool_authority() -> Result<(), String> {
    let model_tool_count = TOOL_SPECS.len();
    if model_tool_count == 0 || model_tool_count != 5 {
        return Err("tool_authority_surface_invalid".to_owned());
    }
    ToolCatalogSnapshot::current().validate()?;
    let schemas = tool_schemas();
    // 先一次性建立完整 canonical name 集合，后面的 alias 检查才能发现“指向未来项”的冲突。
    let names = TOOL_SPECS
        .iter()
        .map(|spec| spec.name)
        .collect::<std::collections::BTreeSet<_>>();
    if names.len() != TOOL_SPECS.len() {
        return Err("tool_authority_spec_invalid".to_owned());
    }
    let mut aliases = std::collections::BTreeSet::new();
    for spec in TOOL_SPECS {
        if spec.operation.trim().is_empty()
            || spec.schema.trim().is_empty()
            || !schemas.iter().any(|schema| schema["name"] == spec.name)
        {
            return Err("tool_authority_spec_invalid".to_owned());
        }
        for alias in spec.aliases {
            if alias.trim().is_empty() || names.contains(alias) || !aliases.insert(*alias) {
                return Err("tool_authority_alias_conflict".to_owned());
            }
        }
    }
    if schemas
        .iter()
        .filter_map(|schema| schema["name"].as_str())
        .collect::<std::collections::BTreeSet<_>>()
        != names
    {
        return Err("tool_authority_schema_drift".to_owned());
    }
    // 最后与执行动作目录核对；目录中的 capability/risk 仍只是映射输入，运行时由 Core 再审。
    validate_tool_action_bindings()?;
    Ok(())
}
