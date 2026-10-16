//! 单写者操作租约（operation lease）和 fencing（旧写入者隔离）合同，属于 `kiana-domain`。
//!
//! 【模块位置与数据流】
//! ```text
//! 上游给出操作（operation）/实例（instance）/存储根（storage root）/epoch/时钟/ttl
//!     -> OperationLeaseCas::acquire / heartbeat / expire / release
//!     -> CAS revision 和 active/last lease 投影
//!     -> 调用方持久化这个 CAS，并在真实写入副作用（effect）前再次校验 lease/fence
//! ```
//!
//! 这里的 `OperationLeaseCas` 是内存中的确定性比较并交换（compare-and-swap，CAS）模型：
//! `expected_revision` 能拒绝基于旧版本的操作，但本结构本身不是原子数据库事务，也不持有
//! 操作系统锁，不跨进程持久化 revision，不写 EventLog，不启动进程监督器（supervisor）或操作系统
//! 进程（process），也不派发 effect。每次成功得到的新 lease 还必须由后续 adapter 持久化，并在
//! 副作用边界重新核对时间、epoch 和 fence token，才能成为有效单写者保护。
//!
//! Core 的 restore-activation helper 源码会用 `new()`/`acquire()` 为恢复后的 root 建立新 lease；
//! recovery-rehearsal helper 会把 lease 当作恢复后状态的一部分校验。当前没有发现这些 helper
//! 被生产入口调用；`validate_operation_lease_cas()` facade 也没有仓库内生产调用者。Domain
//! 测试夹具（fixture）证明值合同，不证明跨进程租约、重启恢复或真实写者已被隔离。

use crate::{
    json_digest, FenceTokenId, InstanceId, OperationId, SchemaVersion, StorageLockId, StorageRootId,
};
use serde::{Deserialize, Serialize};

/// 单个 lease wire DTO 的固定 schema 标识。
pub const OPERATION_LEASE_SCHEMA: &str = "kiana.operation-lease.v1";
/// 一组 lease CAS 状态 wire DTO 的固定 schema 标识。
pub const OPERATION_LEASE_CAS_SCHEMA: &str = "kiana.operation-lease-cas.v1";
/// 单个 lease 和 CAS 投影共同使用的版本。
pub const OPERATION_LEASE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// acquire/heartbeat 接受的最长租约时长：86,400,000 毫秒，即 24 小时。
pub const MAX_OPERATION_LEASE_TTL_MS: u64 = 86_400_000;

/// 租约生命周期标签；只有 Active 才可能通过 `validate_current()`，但仍须检查当前时刻是否到期。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationLeaseState {
    /// 尚未由 `expire()`/`release()` 归档的活动槽记录；时间到期后也要显式 expire 才能清槽。
    /// 因此 Active 只表示尚未归档，不单独证明此刻仍有写入资格。
    Active,
    /// 持有者主动归还，不能再用于 effect-time 校验。
    Released,
    /// 截止时间已到，由显式 `expire()` 归档后不能继续使用。
    Expired,
}

/// 绑定一个 operation、运行实例、存储根和 fencing token 的单写者租约值。
///
/// 该对象记录调用方声称的时钟/epoch/心跳和到期信息；它不是 OS 锁句柄，也不会自行续租。
/// `Serialize`/`Deserialize` 让 adapter 可以传输/保存 DTO，Serde 的 `deny_unknown_fields`
/// 会拒绝当前版本不认识的字段，但这些转换本身不会验证来源身份或持久化内容。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationLease {
    /// 固定 operation lease schema 名称。
    pub schema: String,
    /// lease wire 版本。
    pub version: SchemaVersion,
    /// 这份租约记录的唯一 ID；不是内核锁或文件锁 ID。
    pub lease_id: StorageLockId,
    /// 被保护的部署 operation ID。
    pub operation_id: OperationId,
    /// 获得租约的运行实例 ID；用于拒绝其他 owner 心跳/释放。
    pub owner_instance_id: InstanceId,
    /// 租约约束的目标 storage root，避免同一 lease 被移用于另一份数据。
    pub storage_root: StorageRootId,
    /// 本轮持有者的 fencing token；effect adapter 要用它拒绝旧写者。
    pub fence_token: FenceTokenId,
    /// 产生 lease 时的 authority epoch；当前校验要求调用方 epoch 精确匹配。
    pub authority_epoch: u64,
    /// 产生 lease 时的数据 epoch；当前校验要求调用方 epoch 精确匹配。
    pub data_epoch: u64,
    /// 所属 CAS 投影的 revision；更新心跳或结束租约时随 CAS 一起递增。
    pub cas_revision: u64,
    /// 心跳序号；新 lease 从 1 开始，每次 heartbeat 成功后递增，记录此 lease 的续租次数。
    pub heartbeat_seq: u64,
    /// 创建时的 Unix epoch 毫秒；0 被视为缺失时间。
    pub issued_at_unix_ms: u64,
    /// 最近一次创建/心跳更新的 Unix epoch 毫秒。
    pub last_heartbeat_at_unix_ms: u64,
    /// 当前 lease 声称有效到的 Unix epoch 毫秒，达到该时间即视为过期。
    pub expires_at_unix_ms: u64,
    /// Active 可用；Released/Expired 是历史终态记录。
    pub state: OperationLeaseState,
    /// 对租约其余字段计算的内容摘要，不是签名或锁服务回执。
    pub lease_digest: String,
}

impl OperationLease {
    /// 创建一份由 CAS 获取流程使用的新 Active lease，并填入首次心跳和 expiry。
    ///
    /// 【调用者】仅由 `OperationLeaseCas::acquire()` 调用；调用方提供 operation/owner/root/fence、
    /// epochs、下一 CAS revision、签发时间和 TTL。
    ///
    /// 【输出/失败】成功返回带新 `lease_id`、`heartbeat_seq = 1`、`last_heartbeat = issued_at` 的
    /// lease；加法溢出或字段/digest 校验失败返回稳定字符串。此函数不校验是否已有其他进程持锁，
    /// 不写 CAS 存储，也不读取系统时钟。
    #[allow(clippy::too_many_arguments)]
    fn new(
        operation_id: OperationId,
        owner_instance_id: InstanceId,
        storage_root: StorageRootId,
        fence_token: FenceTokenId,
        authority_epoch: u64,
        data_epoch: u64,
        cas_revision: u64,
        issued_at_unix_ms: u64,
        ttl_ms: u64,
    ) -> Result<Self, String> {
        let expires_at_unix_ms = issued_at_unix_ms
            .checked_add(ttl_ms)
            .ok_or_else(|| "operation_lease_expiry_overflow".to_owned())?;
        let mut lease = Self {
            schema: OPERATION_LEASE_SCHEMA.to_owned(),
            version: OPERATION_LEASE_VERSION,
            lease_id: StorageLockId::new(),
            operation_id,
            owner_instance_id,
            storage_root,
            fence_token,
            authority_epoch,
            data_epoch,
            cas_revision,
            heartbeat_seq: 1,
            issued_at_unix_ms,
            last_heartbeat_at_unix_ms: issued_at_unix_ms,
            expires_at_unix_ms,
            state: OperationLeaseState::Active,
            lease_digest: String::new(),
        };
        lease.lease_digest = lease.digest();
        lease.validate()?;
        Ok(lease)
    }

    /// 验证 lease DTO 的 schema、身份、时间顺序、状态约束和自摘要。
    ///
    /// 它不接收当前时间，因而不会判断 Active lease 此刻是否已过期；也不检查 TTL 是否超过
    /// `MAX_OPERATION_LEASE_TTL_MS`。24 小时上限只在 `acquire()`/`heartbeat()` 的输入校验中检查；
    /// 从 wire 解码后直接调用本校验（包括由 CAS 嵌套调用）不会重新实施该上限。
    /// 失败返回可供上层分类的稳定 `String`，不会修复或写回 lease。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPERATION_LEASE_SCHEMA
            || self.version != OPERATION_LEASE_VERSION
            || self.lease_id.as_uuid().is_nil()
            || self.operation_id.as_uuid().is_nil()
            || self.owner_instance_id.as_uuid().is_nil()
            || self.storage_root.as_uuid().is_nil()
            || self.fence_token.as_uuid().is_nil()
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.cas_revision == 0
            || self.heartbeat_seq == 0
            || self.issued_at_unix_ms == 0
            || self.last_heartbeat_at_unix_ms < self.issued_at_unix_ms
            || self.expires_at_unix_ms <= self.last_heartbeat_at_unix_ms
        {
            return Err("operation_lease_header_invalid".to_owned());
        }
        // 历史终态 lease 仍保留自己的时间窗口；最后一次心跳不能发生在其声明的 expiry 之后。
        if self.state != OperationLeaseState::Active
            && self.last_heartbeat_at_unix_ms >= self.expires_at_unix_ms
        {
            return Err("operation_lease_terminal_time_invalid".to_owned());
        }
        valid_digest(&self.lease_digest, "operation_lease_digest")?;
        if self.lease_digest != self.digest() {
            return Err("operation_lease_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 在给定时刻、authority/data epoch 和 fence token 下确认此 lease 仍是当前 Active lease。
    ///
    /// 【调用者】CAS 的 heartbeat/release 会在修改前使用它；真正产生外部写入的 adapter 也必须
    /// 在 effect 边界用同一批实时输入复查，不能只信任早先读到的快照。
    ///
    /// 【失败情况】先执行结构校验，再拒绝非 Active、时钟倒退/已到期、epoch 不一致和 token 不匹配；
    /// 每类原因返回稳定字符串。时间和 epoch 都由调用方传入，本方法不读 clock、不续租、不获取锁。
    pub fn validate_current(
        &self,
        now_unix_ms: u64,
        authority_epoch: u64,
        data_epoch: u64,
        fence_token: FenceTokenId,
    ) -> Result<(), String> {
        self.validate()?;
        if self.state != OperationLeaseState::Active {
            return Err("operation_lease_not_active".to_owned());
        }
        if now_unix_ms < self.last_heartbeat_at_unix_ms || now_unix_ms >= self.expires_at_unix_ms {
            return Err("operation_lease_expired".to_owned());
        }
        // lease 只允许用创建时绑定的 epoch；降低表示回退，提高也表示这份 lease 已旧，需重新获取。
        if authority_epoch != self.authority_epoch {
            return Err(if authority_epoch < self.authority_epoch {
                "operation_lease_authority_epoch_rollback"
            } else {
                "operation_lease_authority_epoch_stale"
            }
            .to_owned());
        }
        if data_epoch != self.data_epoch {
            return Err(if data_epoch < self.data_epoch {
                "operation_lease_data_epoch_rollback"
            } else {
                "operation_lease_data_epoch_stale"
            }
            .to_owned());
        }
        if fence_token != self.fence_token {
            return Err("operation_lease_fence_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算绑定全部身份、epoch、revision、心跳、时间和状态字段的内容指纹。
    ///
    /// `lease_digest` 自身不进入输入以避免自引用；无密钥摘要只检测字段漂移，不认证 lease 持有者。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "lease_id": self.lease_id,
            "operation_id": self.operation_id,
            "owner_instance_id": self.owner_instance_id,
            "storage_root": self.storage_root,
            "fence_token": self.fence_token,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "cas_revision": self.cas_revision,
            "heartbeat_seq": self.heartbeat_seq,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "last_heartbeat_at_unix_ms": self.last_heartbeat_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "state": self.state,
        }))
    }

    /// 复制当前 lease 并产生下一 CAS revision/heartbeat/expiry；只在验证过的 heartbeat 路径使用。
    ///
    /// checked_add 防止时间或 heartbeat 计数溢出；此 helper 不独立核对 owner、fence 或当前时间，
    /// 这些条件由 `OperationLeaseCas::heartbeat()` 在调用前负责。
    fn renewed(&self, cas_revision: u64, now_unix_ms: u64, ttl_ms: u64) -> Result<Self, String> {
        let expires_at_unix_ms = now_unix_ms
            .checked_add(ttl_ms)
            .ok_or_else(|| "operation_lease_expiry_overflow".to_owned())?;
        let mut lease = self.clone();
        lease.cas_revision = cas_revision;
        lease.heartbeat_seq = lease
            .heartbeat_seq
            .checked_add(1)
            .ok_or_else(|| "operation_lease_heartbeat_exhausted".to_owned())?;
        lease.last_heartbeat_at_unix_ms = now_unix_ms;
        lease.expires_at_unix_ms = expires_at_unix_ms;
        lease.state = OperationLeaseState::Active;
        lease.lease_digest = lease.digest();
        lease.validate()?;
        Ok(lease)
    }

    /// 复制 lease 并记录 Released/Expired 终态及新 CAS revision。
    ///
    /// 由 `release()` 或 `expire()` 调用；这里仅更新值和摘要，不会自行判断释放权限或 expiry 是否到期。
    fn terminal(&self, cas_revision: u64, state: OperationLeaseState) -> Result<Self, String> {
        let mut lease = self.clone();
        lease.cas_revision = cas_revision;
        lease.state = state;
        lease.lease_digest = lease.digest();
        lease.validate()?;
        Ok(lease)
    }
}

/// 一份存储根（storage root）下 operation lease 的 CAS 状态投影。
///
/// `active` 是当前唯一租约；`last_lease` 和 `last_fence_token` 只保留最近一次结束记录及最近使用的 token，
/// 用来拒绝覆盖当前 active lease 和立即重用上一枚 fence。它不是完整 token 历史，不能独自保证更早
/// token 永不复用。`revision` 是调用方交回的 expected
/// revision 比较基准。此结构是普通可变值，不含锁/数据库；只有 adapter 对存储执行真正原子的
/// compare-and-swap，才能保护多个进程之间的并发竞争。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationLeaseCas {
    /// 固定 CAS schema 名称。
    pub schema: String,
    /// CAS wire 版本。
    pub version: SchemaVersion,
    /// 这份投影唯一管理的 storage root。
    pub storage_root: StorageRootId,
    /// 当前 CAS 操作所处的 authority epoch；只能保持或向前推进。
    pub authority_epoch: u64,
    /// 当前 CAS 操作所处的数据 epoch；只能保持或向前推进。
    pub data_epoch: u64,
    /// 当前投影 revision；新 CAS 从 0 开始，每次成功状态变更递增。
    pub revision: u64,
    /// 当前 Active lease；None 表示尚未取得，或已显式 expire/release。
    pub active: Option<OperationLease>,
    /// 最近一次结束的 lease 值，便于后续审计；它不是完整租约历史列表。
    pub last_lease: Option<OperationLease>,
    /// 最近使用的 fencing token；当前只拒绝与这一枚相同的 token，不能查出更早历史中的重复值。
    pub last_fence_token: Option<FenceTokenId>,
    /// 对 CAS 顶层字段和其中 lease 投影计算的内容摘要。
    pub cas_digest: String,
}

impl OperationLeaseCas {
    /// 创建一个没有 active/last lease 的 root 级 CAS 投影。
    ///
    /// 【输入/输出】输入非空 storage root ID、authority epoch 和 data epoch；成功返回 revision 0、
    /// 两个 lease Option 均为 None 且带有效 `cas_digest` 的初始 CAS。
    ///
    /// 【失败与副作用】无效 root、epoch 或摘要时返回稳定字符串；不会取得系统锁、创建持久文件或
    /// 让其他进程看到这个值。调用方之后需用持久化层承载其 revision。
    pub fn new(
        storage_root: StorageRootId,
        authority_epoch: u64,
        data_epoch: u64,
    ) -> Result<Self, String> {
        let mut cas = Self {
            schema: OPERATION_LEASE_CAS_SCHEMA.to_owned(),
            version: OPERATION_LEASE_VERSION,
            storage_root,
            authority_epoch,
            data_epoch,
            revision: 0,
            active: None,
            last_lease: None,
            last_fence_token: None,
            cas_digest: String::new(),
        };
        cas.cas_digest = cas.digest();
        cas.validate()?;
        Ok(cas)
    }

    /// 在当前 CAS revision 上尝试获取一个新的 Active lease。
    ///
    /// 【核心顺序】先校验 CAS 和 expected revision；已有 active lease 未到期时拒绝竞争，已到期时
    /// 仍要求先显式 `expire()`；再检查 epoch 不回退、身份非空、fence 不等于最近保存的 token、TTL 合法；最后
    /// 生成下一 revision 的 lease，更新 active、epochs、last fence 和 CAS digest。
    ///
    /// 【输入/输出】`expected_revision` 是调用方读 CAS 时看到的版本；其余参数分别绑定 operation、
    /// owner instance、唯一新 fence、authority/data epoch、调用方当前 Unix 毫秒时刻和 TTL。成功
    /// 返回新 lease 并修改本地 `self`；真实并发环境仍须由 adapter 原子比较/持久化 revision。
    ///
    /// 【失败】CAS 冲突、活动持有者、待显式回收的过期租约、epoch 回退、nil identity、最近 fence 重用、
    /// TTL 无效或时间加法溢出均返回稳定错误；这里没有重试、OS lock、EventLog 或进程副作用。
    pub fn acquire(
        &mut self,
        expected_revision: u64,
        operation_id: OperationId,
        owner_instance_id: InstanceId,
        fence_token: FenceTokenId,
        authority_epoch: u64,
        data_epoch: u64,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> Result<OperationLease, String> {
        self.validate()?;
        self.check_revision(expected_revision)?;
        // Expired active lease 仍占据 active 槽位；先归档为 last_lease，不能直接被新 owner 覆盖。
        if let Some(active) = &self.active {
            if now_unix_ms < active.expires_at_unix_ms {
                return Err("operation_lease_already_active".to_owned());
            }
            return Err("operation_lease_expired_reclaim_required".to_owned());
        }
        // 新 owner 必须使用不同 fence，且 epoch 只能与 CAS 相同或向前推进。
        self.validate_epochs(authority_epoch, data_epoch)?;
        validate_identity(operation_id, owner_instance_id, fence_token)?;
        if self.last_fence_token == Some(fence_token) {
            return Err("operation_lease_fence_token_reused".to_owned());
        }
        validate_ttl(now_unix_ms, ttl_ms)?;
        let revision = self.next_revision()?;
        let lease = OperationLease::new(
            operation_id,
            owner_instance_id,
            self.storage_root,
            fence_token,
            authority_epoch,
            data_epoch,
            revision,
            now_unix_ms,
            ttl_ms,
        )?;
        self.authority_epoch = authority_epoch;
        self.data_epoch = data_epoch;
        self.revision = revision;
        self.last_fence_token = Some(fence_token);
        self.active = Some(lease.clone());
        self.cas_digest = self.digest();
        self.validate()?;
        Ok(lease)
    }

    /// 使用当前 owner/fence 为 active lease 续租，并递增 CAS revision 与 heartbeat 序号。
    ///
    /// 调用前先用 `expected_revision` 处理并发读旧问题，再核对 operation ID、instance ID、当前
    /// 时间、两种 epoch、fence token 和新 TTL。成功后以 `now + ttl` 替换 expiry，不要求新 expiry
    /// 一定晚于旧 expiry；返回更新后的 lease。任何门槛失败都不应被忽略或自动换 owner 重试。
    /// 本方法只改变传入的内存投影，不向持久层发送 heartbeat。
    pub fn heartbeat(
        &mut self,
        expected_revision: u64,
        operation_id: OperationId,
        owner_instance_id: InstanceId,
        fence_token: FenceTokenId,
        authority_epoch: u64,
        data_epoch: u64,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> Result<OperationLease, String> {
        self.validate()?;
        self.check_revision(expected_revision)?;
        // 先复制出来，所有 owner/fence/时钟检查都通过后才生成并写入下一版本。
        let active = self
            .active
            .clone()
            .ok_or_else(|| "operation_lease_missing".to_owned())?;
        if active.operation_id != operation_id || active.owner_instance_id != owner_instance_id {
            return Err("operation_lease_owner_mismatch".to_owned());
        }
        active.validate_current(now_unix_ms, authority_epoch, data_epoch, fence_token)?;
        validate_ttl(now_unix_ms, ttl_ms)?;
        let revision = self.next_revision()?;
        let lease = active.renewed(revision, now_unix_ms, ttl_ms)?;
        self.revision = revision;
        self.active = Some(lease.clone());
        self.cas_digest = self.digest();
        self.validate()?;
        Ok(lease)
    }

    /// 在调用方观察时钟已到期后，把当前 active lease 归档为 Expired 并清空 active 槽位。
    ///
    /// 【输入/输出】需要匹配的 expected CAS revision 和调用方提供的 Unix 毫秒时间；`now` 必须
    /// 大于或等于 active lease 的 expiry。成功会递增 CAS revision、保存 `last_lease`、保留
    /// `last_fence_token` 并返回 Expired lease。
    ///
    /// 【边界/失败】此方法不要求 owner 或 fence 参数，不能代替上层对此管理动作的授权；它只检查
    /// CAS revision 和时间门槛。早于 expiry、CAS 冲突或无 active lease 时返回稳定错误。它不停止
    /// 进程或证明旧 writer 已在操作系统/远端存储处被隔离。
    pub fn expire(
        &mut self,
        expected_revision: u64,
        now_unix_ms: u64,
    ) -> Result<OperationLease, String> {
        self.validate()?;
        self.check_revision(expected_revision)?;
        let active = self
            .active
            .clone()
            .ok_or_else(|| "operation_lease_missing".to_owned())?;
        // deadline 的等号属于已到期；在此之前不能把 active lease 静默转成历史状态。
        if now_unix_ms < active.expires_at_unix_ms {
            return Err("operation_lease_expiry_not_due".to_owned());
        }
        let revision = self.next_revision()?;
        let expired = active.terminal(revision, OperationLeaseState::Expired)?;
        self.revision = revision;
        self.last_lease = Some(expired.clone());
        self.active = None;
        self.cas_digest = self.digest();
        self.validate()?;
        Ok(expired)
    }

    /// 由原 operation/instance 持有者主动释放仍有效的 active lease。
    ///
    /// 必须同时匹配 `expected_revision`、operation ID、owner instance、当前 CAS epochs、时刻和
    /// fence。成功时 lease 变为 Released、保存到 `last_lease`、清空 active 并递增 revision；被
    /// 释放的 fence 仍记在 `last_fence_token`，只要它仍是最近 token 就会被拒绝；后续 token 覆盖后，
    /// 此 CAS 本身没有完整历史可继续阻止更早 token 重用。
    ///
    /// 若 lease 已过期，`validate_current()` 会拒绝 release；调用方需按自己的授权流程显式 expire。
    /// 本函数只改变内存 CAS，不释放真实 OS lock 或其他 writer 资源。
    pub fn release(
        &mut self,
        expected_revision: u64,
        operation_id: OperationId,
        owner_instance_id: InstanceId,
        fence_token: FenceTokenId,
        now_unix_ms: u64,
    ) -> Result<OperationLease, String> {
        self.validate()?;
        self.check_revision(expected_revision)?;
        let active = self
            .active
            .clone()
            .ok_or_else(|| "operation_lease_missing".to_owned())?;
        // owner 不匹配在 fence/time 校验前就拒绝，避免其他 operation/instance 结束当前持有者租约。
        if active.operation_id != operation_id || active.owner_instance_id != owner_instance_id {
            return Err("operation_lease_owner_mismatch".to_owned());
        }
        active.validate_current(
            now_unix_ms,
            self.authority_epoch,
            self.data_epoch,
            fence_token,
        )?;
        let revision = self.next_revision()?;
        let released = active.terminal(revision, OperationLeaseState::Released)?;
        self.revision = revision;
        self.last_lease = Some(released.clone());
        self.active = None;
        self.cas_digest = self.digest();
        self.validate()?;
        Ok(released)
    }

    /// 检查一个 CAS DTO 的结构、活动租约绑定关系和顶层自摘要。
    ///
    /// 【调用者】每个 `acquire`、`heartbeat`、`expire`、`release` 都会在比较 revision 和修改前调用；
    /// Core facade 也可把它作为只读合同检查暴露给上层。
    ///
    /// 【检查内容】先检查 schema/version、root 和 epoch；有 `active` 时递归检查 lease 自身，再要求
    /// 它仍标记 Active、绑定同一 root/revision/epochs，并且 fence 等于 CAS 记录的最近 token；有
    /// `last_lease` 时检查其 lease 结构和 root；最后检查最近 token 非空 ID 及 CAS 自摘要。
    ///
    /// 【失败与边界】返回稳定字符串错误，不修复对象、不重试、不写存储。这里不会拿当前时刻判断
    /// active 是否已经过期，也不会要求 `last_lease` 必须为终态、与 CAS revision/epoch 对齐，或其
    /// fence 必须等于 `last_fence_token`；对嵌套 lease 的 `validate()` 也不会重新实施 24 小时 TTL 上限。
    /// 所以通过只表示这些已编码字段彼此满足当前检查，不等于持久化 CAS、完整租约历史或真实写入隔离。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPERATION_LEASE_CAS_SCHEMA
            || self.version != OPERATION_LEASE_VERSION
            || self.storage_root.as_uuid().is_nil()
            || self.authority_epoch == 0
            || self.data_epoch == 0
        {
            return Err("operation_lease_cas_header_invalid".to_owned());
        }
        if let Some(active) = &self.active {
            active.validate()?;
            if active.state != OperationLeaseState::Active
                || active.storage_root != self.storage_root
                || active.cas_revision != self.revision
                || active.authority_epoch != self.authority_epoch
                || active.data_epoch != self.data_epoch
            {
                return Err("operation_lease_cas_active_mismatch".to_owned());
            }
            if self.last_fence_token != Some(active.fence_token) {
                return Err("operation_lease_cas_fence_mismatch".to_owned());
            }
        }
        if let Some(last) = &self.last_lease {
            last.validate()?;
            if last.storage_root != self.storage_root {
                return Err("operation_lease_cas_last_root_mismatch".to_owned());
            }
        }
        if self
            .last_fence_token
            .is_some_and(|token| token.as_uuid().is_nil())
        {
            return Err("operation_lease_cas_fence_invalid".to_owned());
        }
        // 校验 CAS 顶层状态和摘要；它不判断 expiry 相对现实当前时间是否已到。
        valid_digest(&self.cas_digest, "operation_lease_cas_digest")?;
        if self.cas_digest != self.digest() {
            return Err("operation_lease_cas_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算 storage root、epochs、CAS revision、当前/历史 lease 和 fence 的整体内容摘要。
    ///
    /// `cas_digest` 自身不参与输入以避免自引用；这是状态指纹，不是持久层签名或原子提交回执。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "storage_root": self.storage_root,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "revision": self.revision,
            "active": self.active,
            "last_lease": self.last_lease,
            "last_fence_token": self.last_fence_token,
        }))
    }

    /// 返回本地投影当前 revision，供调用者把读到的版本作为下一次 mutation 的 expected revision。
    ///
    /// 该 getter 不加锁；若来源是共享存储，调用方必须在持久层执行原子比较，单独读取此值不能
    /// 防止并发写入。
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// 借用当前 Active lease；没有持有者或已完成 expire/release 时返回 None。
    ///
    /// `Option` 用来表达 CAS 中“目前没有活动写者”的合法状态，不会创建或续租任何对象。
    pub fn active(&self) -> Option<&OperationLease> {
        self.active.as_ref()
    }

    /// 对比调用方读取时见到的 revision 与当前本地投影，阻止旧快照覆盖新状态。
    ///
    /// 不匹配时返回 `operation_lease_cas_conflict`。在单进程内它只是值比较；要解决并发进程
    /// 竞争，adapter 还必须将同一条件放进真正原子的持久化 CAS。
    fn check_revision(&self, expected_revision: u64) -> Result<(), String> {
        if expected_revision != self.revision {
            return Err("operation_lease_cas_conflict".to_owned());
        }
        Ok(())
    }

    /// 计算下一 CAS revision；达到 `u64::MAX` 时拒绝而不是回绕到旧版本。
    fn next_revision(&self) -> Result<u64, String> {
        self.revision
            .checked_add(1)
            .ok_or_else(|| "operation_lease_cas_revision_exhausted".to_owned())
    }

    /// 检查新 acquire 使用的两个 epoch 非零且不低于此 CAS 当前值。
    ///
    /// 增大代表调用方带来了更高的 authority/data generation；当前 Active lease 不能在原地
    /// 改 epoch，因此方法只在没有占用者的 acquire 路径使用。发生回退时返回各自稳定错误码。
    fn validate_epochs(&self, authority_epoch: u64, data_epoch: u64) -> Result<(), String> {
        if authority_epoch == 0 || data_epoch == 0 {
            return Err("operation_lease_epoch_invalid".to_owned());
        }
        if authority_epoch < self.authority_epoch {
            return Err("operation_lease_authority_epoch_rollback".to_owned());
        }
        if data_epoch < self.data_epoch {
            return Err("operation_lease_data_epoch_rollback".to_owned());
        }
        Ok(())
    }
}

/// 拒绝空 operation、instance 或 fence ID，避免把 nil UUID 当作共享占位身份。
fn validate_identity(
    operation_id: OperationId,
    owner_instance_id: InstanceId,
    fence_token: FenceTokenId,
) -> Result<(), String> {
    if operation_id.as_uuid().is_nil()
        || owner_instance_id.as_uuid().is_nil()
        || fence_token.as_uuid().is_nil()
    {
        return Err("operation_lease_identity_invalid".to_owned());
    }
    Ok(())
}

/// 检查调用方时刻和 TTL 合法，并提前确认 `now + ttl` 不会溢出。
///
/// TTL 必须大于 0 且不超过 24 小时；错误分别说明输入范围非法或到期时间无法表示。此函数不读时钟。
fn validate_ttl(now_unix_ms: u64, ttl_ms: u64) -> Result<(), String> {
    if now_unix_ms == 0 || ttl_ms == 0 || ttl_ms > MAX_OPERATION_LEASE_TTL_MS {
        return Err("operation_lease_ttl_invalid".to_owned());
    }
    now_unix_ms
        .checked_add(ttl_ms)
        .ok_or_else(|| "operation_lease_expiry_overflow".to_owned())?;
    Ok(())
}

/// 只验证摘要外观为 `sha256:` 加 64 位十六进制，不回读摘要所指对象。
///
/// 十六进制大小写均接受；稳定错误码以前缀参数 `field` 命名。
fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
