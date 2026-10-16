//! 外部 workflow 事件的 Daemon 验签与进程内去重 adapter。
//!
//! Domain 只定义外部 envelope/policy/occurrence 的结构；本文件在服务端保存允许的 source policy 和 HMAC key，
//! 执行真实签名验证，再把合法事件折叠成不含原始 payload 的 occurrence：
//!
//! ```text
//! 上层 connector/入口提供 ingress + 服务端 now
//!     -> WorkflowEventVerifier::verify()
//!     -> 结构/allow-list/time filter -> HMAC-SHA256 -> source+event 去重
//!     -> Some(WorkflowEventOccurrence) 或同摘要重放的 None
//!     -> 调用者持久化 ingress 事实并交给既有 Workflow Fire / Core evidence 路径
//! ```
//!
//! 本 adapter 不写 EventLog、不直接调用 ControlPlane，也不触发 Automation。重复键只保存在当前进程的内存
//! `seen` map 中，重启后会丢失；本 adapter 当前没有提供外部 HTTP/Webhook 传输或持久化去重。仓库中可见
//! verifier 定义、Daemon crate 的公开 re-export 和测试，但尚未找到产品入口实际创建/调用它。

use kiana_domain::{WorkflowEventIngress, WorkflowEventOccurrence, WorkflowEventSourcePolicy};
use kiana_ports::PortError;
use ring::hmac;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

/// 一条服务端允许的 source policy 与对应 HMAC key。
/// key 只保存在此私有结构中，不进入日志 Debug 输出或外部 DTO。
#[derive(Clone)]
struct SourceVerifier {
    /// 与此来源绑定的事件类型、项目、字段和时间筛选规则。
    policy: WorkflowEventSourcePolicy,
    /// `Arc` 让克隆 verifier 共享同一把不可变签名 key。
    key: Arc<hmac::Key>,
}

/// 按来源 allow-list 验签并在本进程内对 source/event key 做幂等去重。
///
/// 克隆本对象会共享来源配置和 `seen` mutex，所以同一进程内多个持有者不会各自接受同一事件；该状态没有
/// 序列化或 EventLog 落盘能力，不等同于跨进程 durable dedupe。
#[derive(Clone, Default)]
pub struct WorkflowEventVerifier {
    /// 以 source_id 索引的只读配置。构造时重复 source 会被拒绝，因此每个来源当前只能装一条 key/policy。
    sources: Arc<BTreeMap<String, SourceVerifier>>,
    /// occurrence_key → ingress_digest 的内存去重表；mutex 把“检查旧值 + 插入新值”变成一个临界区。
    seen: Arc<Mutex<BTreeMap<String, String>>>,
}

impl std::fmt::Debug for WorkflowEventVerifier {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkflowEventVerifier")
            .field("source_count", &self.sources.len())
            .finish_non_exhaustive()
    }
}

impl WorkflowEventVerifier {
    /// 从服务端配置创建 source verifier 集合并初始化空去重表。
    ///
    /// 【调用者】Daemon/connector 装配层在处理外部事件前。
    /// 【输入】一组 policy 与对应 key bytes；key 长度必须为 16 到 4,096 字节，source ID 必须唯一。
    /// 【输出】不可变 source map 与所有 clone 共享的空内存 dedupe map。
    /// 【失败】policy 不合法、key 太短/太长或来源重复时返回 `PortError::Failed/Conflict`，不会返回半成品。
    /// 【副作用】仅把 key 包装在进程内 `ring::hmac::Key`；不访问文件/密钥服务，也不注册网络路由。
    /// 【边界】空配置允许构造，但任何事件都会因为 source 未 allow-list 而被拒绝。
    pub fn new(
        entries: impl IntoIterator<Item = (WorkflowEventSourcePolicy, Vec<u8>)>,
    ) -> Result<Self, PortError> {
        let mut sources = BTreeMap::new();
        for (policy, secret) in entries {
            policy.validate().map_err(PortError::Failed)?;
            if secret.len() < 16 || secret.len() > 4_096 {
                return Err(PortError::Failed(
                    "workflow_event_signing_key_invalid".to_owned(),
                ));
            }
            if sources.contains_key(&policy.source_id) {
                return Err(PortError::Conflict(
                    "workflow_event_source_duplicate".to_owned(),
                ));
            }
            sources.insert(
                policy.source_id.clone(),
                SourceVerifier {
                    policy,
                    key: Arc::new(hmac::Key::new(hmac::HMAC_SHA256, &secret)),
                },
            );
        }
        Ok(Self {
            sources: Arc::new(sources),
            seen: Arc::new(Mutex::new(BTreeMap::new())),
        })
    }

    /// 验证 envelope、来源策略、时间窗口和 HMAC，并按 source/event ID 去重。
    ///
    /// 【调用者】受信任的 connector/入口 adapter 收到外部 Webhook 后；当前尚未发现生产调用点。
    /// 【输入】不可信 ingress 与服务端当前 Unix 毫秒时间。
    /// 【输出】首次接受返回 `Ok(Some(occurrence))`；同一 key+digest 重放返回 `Ok(None)`，上层不再产生第二份
    /// occurrence；同 key 不同 digest 返回冲突。
    /// 【失败】envelope 无效、source 未配置、policy 不匹配、签名编码/HMAC 错误时返回结构化 PortError，拒绝
    /// occurrence。失败不会登记为 seen。
    /// 【核心顺序】先界限/字段校验，再 source lookup 与时间/payload policy，再解析 signature 并验证 HMAC，
    /// 然后进入共享锁做去重，最后构造 occurrence 并记录 digest。
    /// 【并发注意】去重的查找与插入必须在同一个 mutex 临界区；拆开后两个并发副本可能都返回 Some。
    /// 当前锁也覆盖 occurrence 构造，串行化同 verifier 的验收；key 配置本身通过 Arc 不可变共享。
    /// 【状态边界】seen 只在内存存在、无容量上限且不跨重启；持久 ingress 和 Core Fire/evidence 仍由调用者负责。
    pub fn verify(
        &self,
        event: WorkflowEventIngress,
        now_unix_ms: u64,
    ) -> Result<Option<WorkflowEventOccurrence>, PortError> {
        event.validate().map_err(PortError::Failed)?;
        let source = self.sources.get(&event.source_id).ok_or_else(|| {
            PortError::Conflict("workflow_event_source_not_allowlisted".to_owned())
        })?;
        source
            .policy
            .matches(&event, now_unix_ms)
            .map_err(PortError::Conflict)?;
        let signature = decode_hex(&event.signature)?;
        hmac::verify(&source.key, event.signing_digest().as_bytes(), &signature)
            .map_err(|_| PortError::Conflict("workflow_event_signature_invalid".to_owned()))?;
        let occurrence_key = event.occurrence_key();
        // 同一 occurrence 的比对和记录合并在一把共享锁中，避免 verifier clone 并发地重复接受。
        // mutex 曾被 panic 污染时取回其内部 map；这只恢复锁可用性，不代表去重状态已持久恢复。
        let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(previous) = seen.get(&occurrence_key) {
            if previous == &event.ingress_digest {
                return Ok(None);
            }
            return Err(PortError::Conflict(
                "workflow_event_dedupe_payload_conflict".to_owned(),
            ));
        }
        let occurrence = WorkflowEventOccurrence::from_verified(&event, &source.policy)
            .map_err(PortError::Failed)?;
        seen.insert(occurrence_key, event.ingress_digest);
        Ok(Some(occurrence))
    }
}

/// 将 64 个十六进制字符解码为 HMAC-SHA256 的 32 字节签名。
/// 格式/字符错误返回 PortError::Failed，验证器不会把它当作签名不匹配继续处理。
fn decode_hex(value: &str) -> Result<Vec<u8>, PortError> {
    if value.len() != 64 {
        return Err(PortError::Failed(
            "workflow_event_signature_encoding_invalid".to_owned(),
        ));
    }
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(32);
    for pair in bytes.chunks_exact(2) {
        let high = hex_digit(pair[0]).ok_or_else(|| {
            PortError::Failed("workflow_event_signature_encoding_invalid".to_owned())
        })?;
        let low = hex_digit(pair[1]).ok_or_else(|| {
            PortError::Failed("workflow_event_signature_encoding_invalid".to_owned())
        })?;
        output.push((high << 4) | low);
    }
    Ok(output)
}

/// 将单个 ASCII 十六进制字符转为 0..15；接受大小写字母。
fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}
