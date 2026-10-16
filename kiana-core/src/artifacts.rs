//! 产物（artifact）存储：把「已经读进内存的字节」落成一个**不可变的、有来源的**文件，
//! 同时保证它**出不去项目目录**。
//!
//! # 最重要的一条：字节不进事件，摘要进事件
//!
//! 一个产物可能有几十兆。把 blob 塞进事件载荷，会让 EventLog 体积失控，
//! 让任何读这条事件的人（投影、日志、对账）都要处理一个巨大字符串。
//! 所以：**事件里只有 hash 与 provenance，blob 留在产物存储里。**
//!
//! 这也是「不可变」的技术基础——事件里记下了 hash，之后任何人重新读这个产物，
//! 都能验证「拿到的还是不是当初那一份」。
//!
//! # 第二条：写出去之前先确认它出不去
//!
//! `confined_artifact_path` / `validated_artifact_components` 负责这一层。
//! 产物路径是**由内容或调用方给的**，所以它必须被当作不可信输入：
//! 绝对路径、`..`、以及指向项目外的软链，都要在写之前被拒绝。
use super::*;

/// 为已经读进内存的字节构建**不可变元数据记录**。
///
/// 这个函数封装了从原始字节到「产物版本」这一步的校验和摘要计算。
/// blob 本身不会进入事件载荷（那会导致 EventLog 体积爆炸），只有 hash 与
/// provenance 会被记录到 CompanyProof 里。这样做的好处是：
/// - 事件永远不会比实际产物还大（海量产物也不会拖累事件系统）
/// - 读者可以通过 hash 验证「拿到的还是不是当初那一份」
///
/// # 参数
/// - `artifact_id`: 产物的全局唯一 ID
/// - `version`: 产物的版本号
/// - `artifact_schema`: 产物的 schema 类型标签（如 "DECISION" 或 "REVIEW"）
/// - `content`: 待存储的原始字节
/// - `scope_digest`: 作用域摘要（通常是项目 scope 的 SHA256）
/// - `producer_kind`: 生产者类别（如 "symposium" 或 "builder"）
/// - `producer_id`: 生产者 ID（会话 ID 或角色）
/// - `recorded_by`: 记录者身份
/// - `created_at_unix_ms`: 创建时间戳（毫秒级 Unix 时间）
///
/// # 返回值
/// 成功返回 ArtifactVersion（包含计算得到的 hash 和 provenance）。
/// 失败返回静态错误消息。
pub(crate) fn artifact_version_from_content(
    artifact_id: kiana_domain::ArtifactId,
    version: u64,
    artifact_schema: &str,
    content: &[u8],
    scope_digest: &str,
    producer_kind: &str,
    producer_id: &str,
    recorded_by: &str,
    created_at_unix_ms: u64,
) -> Result<kiana_domain::ArtifactVersion, &'static str> {
    // 委托给 domain crate 的构造函数，它负责计算 content 的 SHA256 hash
    // 并将 provenance 包装成不可变结构。如果构造失败（通常是字段超长），
    // 返回通用错误码。
    kiana_domain::ArtifactVersion::new(
        artifact_id,
        version,
        artifact_schema,
        content,
        scope_digest,
        kiana_domain::ArtifactProvenance {
            producer_kind: producer_kind.to_owned(),
            producer_id: producer_id.to_owned(),
            source_event_id: None,
            source_run_id: None,
            recorded_by: recorded_by.to_owned(),
        },
        created_at_unix_ms,
    )
    .map_err(|_| "artifact_version_invalid")
}

/// 验证已读取的 blob 是否与其不可变引用匹配。
///
/// 当我们从磁盘读回一个产物时，需要通过 hash 确认「拿到的还是原来那一份」。
/// 这防止了：
/// - 产物被无意修改
/// - 磁盘损坏导致数据污染
/// - 恶意替换
///
/// # 参数
/// - `reference`: ArtifactRef，其中包含了预期的 content_hash
/// - `content`: 从磁盘读取的实际字节
///
/// # 返回值
/// 成功返回 Ok(())；任何不匹配都返回错误码。
pub(crate) fn validate_artifact_reference_content(
    reference: &kiana_domain::ArtifactRef,
    content: &[u8],
) -> Result<(), &'static str> {
    // 1. 验证 reference 本身的有效性（检查其 ID、hash 格式等）
    reference
        .validate()
        .map_err(|_| "company_artifact_reference_invalid")?;

    // 2. 检查读取的内容是否超过了 MAX_ARTIFACT_BYTES 限制，
    //    或者其 SHA256 hash 与 reference 中记录的 hash 不匹配
    if content.len() > kiana_domain::MAX_ARTIFACT_BYTES
        || kiana_domain::journal_sha256(content) != reference.content_hash
    {
        return Err("company_artifact_content_hash_mismatch");
    }
    Ok(())
}

/// 将 Symposium 的决策记录与可选的工作包写入项目目录。
///
/// 这是「Planning 票房」输出的两个关键产物：
/// - DecisionRecord：记录了规划的决策、理由和验证条件
/// - WorkPacket：下一步的工作单元（如果有的话）
///
/// 两个文件都会经过内容验证（检查密钥泄露等）并通过原子写操作落盘。
///
/// # 参数
/// - `project_root`: 项目根目录的绝对路径
/// - `meeting`: Symposium 元数据（包含文件路径信息）
/// - `decision`: 决策记录的结构化数据
/// - `packet`: 可选的工作包，若无则只写决策记录
///
/// # 返回值
/// 成功返回 Ok(())；任何写操作失败都返回错误码。
pub(crate) fn write_symposium_artifacts(
    project_root: &str,
    meeting: &Symposium,
    decision: &DecisionRecord,
    packet: Option<&WorkPacket>,
) -> Result<(), &'static str> {
    // 检查项目根是否存在且有效
    let root = Path::new(project_root);
    if project_root.trim().is_empty() || !root.is_dir() {
        return Err("symposium_artifact_write_failed");
    }

    // 将 DecisionRecord 序列化为漂亮的 JSON 格式并写入
    let decision_path = Path::new(meeting.decision_path());
    let decision_json =
        serde_json::to_string_pretty(decision).map_err(|_| "symposium_artifact_write_failed")?;
    write_project_artifact(
        root,
        decision_path,
        decision_json.as_bytes(),
        "symposium_artifact_write_failed",
    )?;

    // 如果提供了工作包，也序列化并写入（通常路径是 packet/TASK.json）
    if let Some(packet) = packet {
        let packet_json =
            serde_json::to_string_pretty(packet).map_err(|_| "symposium_artifact_write_failed")?;
        write_project_artifact(
            root,
            Path::new(WORK_PACKET_PATH),
            packet_json.as_bytes(),
            "symposium_artifact_write_failed",
        )?;
    }
    Ok(())
}

/// 从项目目录读取 Reviewer 的审查包。
///
/// ReviewPacket 是「Monitoring 票房」的产物，包含了对 Builder 输出的审查结果。
/// 此函数负责从磁盘加载这个结构化数据并进行基本验证。
///
/// # 参数
/// - `project_root`: 项目根目录路径
///
/// # 返回值
/// 成功返回 ReviewPacket；失败返回错误码。
pub(crate) fn read_review_artifact(project_root: &str) -> Result<ReviewPacket, &'static str> {
    // 检查项目根有效性
    let root = Path::new(project_root);
    if !root.is_dir() {
        return Err("close_project_not_found");
    }

    // 从约定的路径 (REVIEW_PACKET_PATH) 读取原始 JSON 文本
    let raw = read_project_artifact(
        root,
        Path::new(REVIEW_PACKET_PATH),
        "close_review_not_found",
    )?;

    // 反序列化 JSON 为 ReviewPacket 结构
    let review: ReviewPacket = serde_json::from_str(&raw).map_err(|_| "close_review_invalid")?;

    // 验证 ReviewPacket 的业务规则（如：分数在合理范围内、必填字段不为空等）
    review.validate().map_err(|_| "close_review_invalid")?;
    Ok(review)
}

/// 将 Closer 的完成凭证写入项目目录。
///
/// ClosingReceipt 是「Closing 票房」的主产物，表示项目已通过审查并被接受。
/// 此外还会创建一个 `lessons/LEARNED.md` 文件记录此次关闭的总结。
///
/// # 参数
/// - `project_root`: 项目根目录路径
/// - `receipt`: ClosingReceipt 结构
///
/// # 返回值
/// 成功返回 Ok(())；任何写操作失败都返回错误码。
pub(crate) fn write_closing_artifact(
    project_root: &str,
    receipt: &ClosingReceipt,
) -> Result<(), &'static str> {
    // 检查项目根有效性
    let root = Path::new(project_root);
    if project_root.trim().is_empty() || !root.is_dir() {
        return Err("closing_artifact_write_failed");
    }

    // 序列化 ClosingReceipt 为 JSON 并写入约定的文件路径
    let receipt_json =
        serde_json::to_string_pretty(receipt).map_err(|_| "closing_artifact_write_failed")?;
    write_project_artifact(
        root,
        Path::new(kiana_domain::CLOSING_RECEIPT_PATH),
        receipt_json.as_bytes(),
        "closing_artifact_write_failed",
    )?;

    // 构建一个「经验教训」的 Markdown 文件，记录本次关闭的摘要
    let mut learned = String::from("# Closing lessons\n\n");
    learned.push_str("The Builder output passed independent Review and was accepted by Closing.\n");

    // 如果有验证过的文件列表，附加到经验文档中
    if !receipt.files_verified.is_empty() {
        learned.push_str("\nVerified files:\n");
        for file in &receipt.files_verified {
            learned.push_str("- ");
            learned.push_str(file);
            learned.push('\n');
        }
    }

    // 将经验文档写入 lessons/LEARNED.md
    write_project_artifact(
        root,
        Path::new("lessons/LEARNED.md"),
        learned.as_bytes(),
        "closing_artifact_write_failed",
    )
}

/// 从项目目录读取合并凭证。
///
/// MergeReceipt 记录了代码变更从一个分支合并到主线的结果。
/// 这通常是「Closing 票房」的一部分，表示 Builder 的成果已被集成。
///
/// # 参数
/// - `project_root`: 项目根目录路径
///
/// # 返回值
/// 成功返回 MergeReceipt；失败返回错误码。
pub(crate) fn read_merge_artifact(project_root: &str) -> Result<MergeReceipt, &'static str> {
    let root = Path::new(project_root);

    // 从约定的路径读取原始 JSON
    let raw = read_project_artifact(
        root,
        Path::new(kiana_domain::MERGE_RECEIPT_PATH),
        "close_merge_receipt_not_found",
    )?;

    // 反序列化为 MergeReceipt
    let merge: MergeReceipt =
        serde_json::from_str(&raw).map_err(|_| "close_merge_receipt_invalid")?;

    // 验证 MergeReceipt 的业务规则
    merge
        .validate()
        .map_err(|_| "close_merge_receipt_invalid")?;
    Ok(merge)
}

/// 将合并凭证写入项目目录。
///
/// # 参数
/// - `project_root`: 项目根目录路径
/// - `receipt`: MergeReceipt 结构
///
/// # 返回值
/// 成功返回 Ok(())；失败返回错误码。
pub(crate) fn write_merge_artifact(
    project_root: &str,
    receipt: &MergeReceipt,
) -> Result<(), &'static str> {
    // 检查项目根有效性
    let root = Path::new(project_root);
    if project_root.trim().is_empty() || !root.is_dir() {
        return Err("merge_artifact_write_failed");
    }

    // 序列化并写入
    let receipt_json =
        serde_json::to_string_pretty(receipt).map_err(|_| "merge_artifact_write_failed")?;
    write_project_artifact(
        root,
        Path::new(kiana_domain::MERGE_RECEIPT_PATH),
        receipt_json.as_bytes(),
        "merge_artifact_write_failed",
    )
}

/// 将审查包写入项目目录。
///
/// ReviewPacket 是 Reviewer 对 Builder 输出的评价，会写入约定的路径 (REVIEW_PACKET_PATH)。
///
/// # 参数
/// - `project_root`: 项目根目录路径
/// - `packet`: ReviewPacket 结构
///
/// # 返回值
/// 成功返回 Ok(())；失败返回错误码。
pub(crate) fn write_review_artifact(
    project_root: &str,
    packet: &ReviewPacket,
) -> Result<(), &'static str> {
    // 检查项目根有效性
    let root = Path::new(project_root);
    if project_root.trim().is_empty() || !root.is_dir() {
        return Err("review_artifact_write_failed");
    }

    // 序列化并写入
    let packet_json =
        serde_json::to_string_pretty(packet).map_err(|_| "review_artifact_write_failed")?;
    write_project_artifact(
        root,
        Path::new(REVIEW_PACKET_PATH),
        packet_json.as_bytes(),
        "review_artifact_write_failed",
    )?;
    Ok(())
}

// 常数定义
const ARTIFACT_TEMP_ATTEMPTS: usize = 16; // 临时文件创建尝试次数上限

/// Linux 特定：renameat2 的 RENAME_NOREPLACE flag（值为 1）
/// 表示如果目标已存在则失败，不覆盖——这保证了原子性和安全性
#[cfg(target_os = "linux")]
const LINUX_RENAME_NOREPLACE: u32 = 1;

/// Linux 特定：renameat2 的 RENAME_EXCHANGE flag（值为 2）
/// 表示原子地交换两个文件的位置，即使目标存在也不会丢失数据
#[cfg(target_os = "linux")]
const LINUX_RENAME_EXCHANGE: u32 = 2;

/// 验证产物内容：检查密钥泄露并确保 UTF-8 合法性。
///
/// 这是**落盘前的最后一道防线**——防止敏感信息（如 API 密钥、认证令牌）
/// 被意外写入产物文件。验证分为两层：
///
/// 1. 如果内容是 JSON：用 `validate_secret_free` 检查是否有 secret_ref 或其他敏感标记
/// 2. 否则，视为纯文本：用 `redact_text` 检查是否包含需要消除的敏感模式
///
/// 【为什么要这样做】
/// 产物通常会被集成到版本控制、备份、审查流程中。一旦泄密数据进去，
/// 历史记录就永远无法彻底清除。所以写之前必须拒绝它。
///
/// # 参数
/// - `contents`: 待验证的字节
/// - `error`: 验证失败时返回的错误码
///
/// # 返回值
/// 通过返回 Ok(())；任何可疑内容都返回 Err(error)。
fn validate_artifact_contents(contents: &[u8], error: &'static str) -> Result<(), &'static str> {
    // 尝试将字节解析为 JSON 并验证其中没有敏感字段
    if let Ok(value) = serde_json::from_slice::<Value>(contents) {
        kiana_domain::validate_secret_free(&value).map_err(|_| error)?;
        return Ok(());
    }

    // 如果不是 JSON，视为纯文本，进行 UTF-8 和敏感内容检查
    let text = std::str::from_utf8(contents).map_err(|_| error)?;

    // 调用与 EventLog 统一的文本消除函数，确保格式一致
    // 如果消除后的文本和原文本不同，说明包含了敏感模式，拒绝
    if kiana_domain::redact_text(text) != text {
        return Err(error);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub(crate) fn read_project_artifact(
    root: &Path,
    relative: &Path,
    error: &'static str,
) -> Result<String, &'static str> {
    read_project_artifact_linux(root, relative, error)
}

/// 【非 Linux 平台】读取项目产物文件的内容。
///
/// 通过标准 Rust 文件 API，打开文件时带上 O_NOFOLLOW 标志防止跟随符号链接。
///
/// # 参数
/// - `root`: 项目根目录的 Path
/// - `relative`: 相对于 root 的相对路径（已在 Linux 版本中验证过不会跑出去）
/// - `error`: 失败时返回的错误码
///
/// # 返回值
/// 成功返回文件内容的 String；失败返回错误码。
#[cfg(not(target_os = "linux"))]
pub(crate) fn read_project_artifact(
    root: &Path,
    relative: &Path,
    error: &'static str,
) -> Result<String, &'static str> {
    // 先确保目标路径确实在 root 内部（没有 .. 或绝对路径等诡异情况）
    let path = confined_artifact_path(root, relative, error)?;

    let mut options = OpenOptions::new();
    options.read(true);

    // 在 Unix 系统上加上 O_NOFOLLOW，防止打开符号链接
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);

    // 打开文件并读取全部内容
    let mut file = options.open(path).map_err(|_| error)?;
    let mut raw = String::new();
    file.read_to_string(&mut raw).map_err(|_| error)?;
    Ok(raw)
}

/// 【Linux 平台】写产物：验证 → 准备 → 提交。
///
/// 这是 Linux 上高安全性的写操作实现，使用 renameat2 原子操作和 TOCTOU 防护。
/// 与非 Linux 版本的主要差异是使用 Linux 特定的系统调用，获得更强的原子性保证。
///
/// # 参数、返回值与非 Linux 版本一致
#[cfg(target_os = "linux")]
fn write_project_artifact(
    root: &Path,
    relative: &Path,
    contents: &[u8],
    error: &'static str,
) -> Result<(), &'static str> {
    // 1. 先验证内容（防止泄密）
    validate_artifact_contents(contents, error)?;
    // 2. 准备写操作（打开文件、创建临时文件、写入内容）
    // 3. 调用 prepare_project_artifact_linux，返回一个待提交的写操作对象
    prepare_project_artifact_linux(root, relative, contents, error)?.commit()
}

/// 【非 Linux 平台】写产物：验证 → 创建临时兄弟 → 写入 → fsync → 原子改名。
///
/// 写入流程遵循「写-后改名」范式，确保：
/// - 写入过程中系统崩溃，原文件不受影响（原子性）
/// - 成功落盘前，内容只存在于临时文件
/// - 改名是文件系统原子操作
///
/// 【为什么创建「同目录的临时兄弟」】
/// `rename` 仅在同一文件系统内才是原子的。若临时文件在 `/tmp` 而目标在
/// 项目目录，它们可能跨越分区，改名退化成「复制 + 删除」——崩溃时会留下
/// 半个文件。同目录内的改名才保证真正原子。
///
/// # 参数、返回值与 Linux 版本一致
#[cfg(not(target_os = "linux"))]
fn write_project_artifact(
    root: &Path,
    relative: &Path,
    contents: &[u8],
    error: &'static str,
) -> Result<(), &'static str> {
    // 1. 先验证内容不包含敏感信息（防止泄密）
    validate_artifact_contents(contents, error)?;

    // 2. 检查目标路径合法性（不能含 .. 或绝对路径等）
    let path = confined_artifact_path(root, relative, error)?;

    // 3. 创建目标文件的父目录（可能多层，所以用 create_dir_all）
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| error)?;
    }

    // 4. 再次验证路径（双重检查）
    confined_artifact_path(root, relative, error)?;

    // 5. 在同目录创建唯一的临时文件，返回临时路径和句柄
    let (temporary, mut file) = create_artifact_temp_sibling(&path, error)?;

    // 6. 尝试写入内容
    let write_result = (|| {
        file.write_all(contents).map_err(|_| error)?; // 写入全部字节
        file.flush().map_err(|_| error)?; // 刷新到内核缓冲
        file.sync_all().map_err(|_| error) // 刷新到磁盘（fsync）
    })();
    drop(file); // 释放文件句柄

    // 7. 如果写入失败，删除临时文件并返回错误
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
        return write_result;
    }

    // 8. 再次验证目标路径（防止中途被替换）
    if confined_artifact_path(root, relative, error).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }

    // 9. 原子改名：临时文件改名为目标路径
    if fs::rename(&temporary, &path).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

/// 【非 Linux 平台】验证产物路径确实在项目 root 内部。
///
/// 这是**第一道防线**——阻止相对路径诡异用法（`../../etc/passwd` 等）或
/// 软链跳出去。算法如下：
///
/// 1. 拒绝空路径、绝对路径
/// 2. 拒绝任何包含 `.` (当前目录) 或 `..` (父目录) 或根目录前缀的路径
/// 3. 逐个处理路径组件，每个组件都检查是否已经是软链（若是则拒绝）
/// 4. 返回最终的绝对路径
///
/// 【为什么要逐个检查】
/// 攻击者可能在 root 内创建符号链接指向项目外，然后我们的相对路径
/// 恰好指向这个符号链接。所以不仅要检查目标，还要检查路径上的每一个
/// 中间组件。
///
/// # 参数
/// - `root`: 项目根目录
/// - `relative`: 待验证的相对路径
/// - `error`: 验证失败时返回的错误码
///
/// # 返回值
/// 验证通过返回最终的绝对路径；验证失败返回错误码。
#[cfg(not(target_os = "linux"))]
fn confined_artifact_path(
    root: &Path,
    relative: &Path,
    error: &'static str,
) -> Result<PathBuf, &'static str> {
    // 拒绝空路径或绝对路径
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        // 拒绝任何路径组件包含 ParentDir (..) / RootDir (/) / Prefix (在 Windows 上)
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(error);
    }

    let path = root.join(relative); // 最终的绝对路径
    let mut current = root.to_path_buf(); // 从根目录开始逐步向下

    // 逐个处理 relative 中的每个组件
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(error);
        };
        current.push(name);

        // 检查 current 这个路径是否存在且其属性
        match fs::symlink_metadata(&current) {
            // 如果是符号链接，拒绝（防止跳出 root）
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(error),
            // 如果是普通文件或目录，接受并继续
            Ok(_) => {}
            // 如果不存在，那也可以（这是新建文件/目录的情况）
            Err(io_error) if io_error.kind() == std::io::ErrorKind::NotFound => {}
            // 其他错误（权限问题等）都拒绝
            Err(_) => return Err(error),
        }
    }
    Ok(path)
}

/// 在目标路径旁边创建一个临时文件，返回其路径和句柄。
///
/// 【为什么是「同目录的临时兄弟」而不是 `/tmp`】
/// 因为 `rename` 只在同一文件系统内是原子的。临时文件放到别的分区，
/// 最后的改名就退化成「复制 + 删除」，而复制过程中断电就会留下半个文件。
/// 放在同一个目录里，改名才是原子的。
///
/// 【名字里为什么塞了 pid、纳秒时间戳和 attempt 序号】
/// 三个都在防同一类事故：两个写入者抢同一个临时名。
/// - pid：不同进程；
/// - 纳秒时间戳：同一进程内的两次写入；
/// - attempt：同一纳秒内的重试。
/// 配合下面的 `create_new`（文件已存在就失败而不是覆盖），
/// 任何一次碰撞都表现为「重试下一次」，而不是「两个写入者互相截断对方的文件」。
///
/// # 返回值
/// 成功返回 (临时路径, 文件句柄)；失败返回错误码。
///
/// # Panics
/// 无（所有错误都通过 Result 返回）。
#[cfg(not(target_os = "linux"))]
fn create_artifact_temp_sibling(
    path: &Path,
    error: &'static str,
) -> Result<(PathBuf, File), &'static str> {
    // 提取目标文件名
    let name = path.file_name().ok_or(error)?.to_string_lossy();

    // 获取当前纳秒级时间戳作为唯一性标记
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);

    // 尝试 16 次创建不同名字的临时文件
    for attempt in 0..ARTIFACT_TEMP_ATTEMPTS {
        // 临时文件名格式：.{目标名}.kiana-artifact-{pid}-{纳秒时间戳}-{尝试序号}
        let temporary = path.with_file_name(format!(
            ".{name}.kiana-artifact-{}-{stamp}-{attempt}",
            std::process::id()
        ));

        // 尝试创建这个临时文件，使用 create_new 确保文件不存在
        match OpenOptions::new()
            .write(true)
            .create_new(true) // 文件存在则失败而不覆盖
            .open(&temporary)
        {
            Ok(file) => return Ok((temporary, file)),
            // 如果文件已存在（碰撞了），继续下一次尝试
            Err(io_error) if io_error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            // 其他错误直接返回
            Err(_) => return Err(error),
        }
    }

    // 如果 16 次都失败，说明遇到了严重的碰撞，放弃
    Err(error)
}

/// 【Linux 平台】产物目标文件的状态快照。
///
/// 用于检测文件在操作过程中是否被替换或修改。存储 stat() 的关键字段。
/// 如果文件是常规文件且只有一个硬链接，会记录 device/inode/mode/size/mtime/ctime 等。
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LinuxArtifactTargetState {
    /// 文件不存在
    Missing,
    /// 文件存在且是常规单链文件，包含其完整元数据
    Regular {
        device: u64,               // 设备号
        inode: u64,                // inode 号
        mode: u32,                 // 文件权限和类型
        links: u64,                // 硬链接数（必须是 1）
        size: u64,                 // 文件大小
        modified_seconds: i64,     // mtime 秒
        modified_nanoseconds: i64, // mtime 纳秒
        changed_seconds: i64,      // ctime 秒
        changed_nanoseconds: i64,  // ctime 纳秒
    },
}

/// 【Linux 平台】产物文件位置的句柄包。
///
/// 持有项目根目录、其父目录，以及要操作的文件名。所有文件操作都通过
/// 相对于 parent 的 openat/renameat2 进行，以避免 TOCTOU 漏洞。
#[cfg(target_os = "linux")]
struct LinuxArtifactLocation {
    root_path: PathBuf,               // root 的绝对路径（用于重新验证）
    root: File,                       // root 目录的打开句柄
    parent: File,                     // 目标文件的父目录句柄
    parent_components: Vec<OsString>, // root 到 parent 的路径组件
    target: CString,                  // 目标文件名（C 字符串格式）
}

/// 【Linux 平台】LinuxArtifactLocation 的实现方法。
#[cfg(target_os = "linux")]
impl LinuxArtifactLocation {
    /// 打开产物文件位置的所有句柄。
    ///
    /// 逐层打开从 root 到 parent 的每一个目录，确保路径合法且不含符号链接。
    /// 如果 `create_parents` 为真，缺失的目录会被创建。
    ///
    /// # 参数
    /// - `root_path`: 项目根目录的绝对路径
    /// - `relative`: 相对于 root 的相对路径
    /// - `create_parents`: 是否创建缺失的父目录
    /// - `error`: 失败时返回的错误码
    ///
    /// # 返回值
    /// 成功返回 LinuxArtifactLocation（包含了所有的打开句柄）；失败返回错误码。
    fn open(
        root_path: &Path,
        relative: &Path,
        create_parents: bool,
        error: &'static str,
    ) -> Result<Self, &'static str> {
        // 验证并分解相对路径为组件列表，最后一个是目标文件名
        let mut components = validated_artifact_components(relative, error)?;
        let target = components.pop().ok_or(error)?; // 弹出目标文件名

        // 打开 root 目录（带 O_DIRECTORY 和 O_NOFOLLOW）
        let root = linux_open_artifact_root(root_path).map_err(|_| error)?;
        let mut parent = root.try_clone().map_err(|_| error)?;

        // 逐层打开中间目录，建立到 parent 的路径
        for component in &components {
            parent =
                linux_open_artifact_directory_at(parent.as_raw_fd(), component, create_parents)
                    .map_err(|_| error)?;
        }

        Ok(Self {
            root_path: root_path.to_path_buf(),
            root,
            parent,
            parent_components: components,
            target: linux_artifact_cstring(&target).map_err(|_| error)?,
        })
    }

    /// 检查这个 Location 是否还有效（没有被替换或移动）。
    ///
    /// 【TOCTOU 防护】当我们准备提交时，需要确认根目录和父目录还是原来的，
    /// 防止有人在准备过程中偷偷替换了某个目录（比如用符号链接）。
    fn is_current(&self) -> bool {
        // 1. 重新打开 root 并检查它是否还是同一个（device/inode 相同）
        let Ok(root) = linux_open_artifact_root(&self.root_path) else {
            return false;
        };
        if !linux_same_directory(&self.root, &root) {
            return false;
        }

        // 2. 重新逐层打开 parent，检查每一层是否都还是原来的
        let mut parent = root;
        for component in &self.parent_components {
            let Ok(next) = linux_open_artifact_directory_at(parent.as_raw_fd(), component, false)
            else {
                return false;
            };
            parent = next;
        }

        // 3. 最终检查 parent 目录是否还是原来的
        linux_same_directory(&self.parent, &parent)
    }
}

/// 【Linux 平台】准备好的产物写操作。
///
/// 封装了已经写入临时文件、但尚未提交的写操作。所有验证都已完成，
/// 只等 commit() 调用时进行最后的原子改名。
///
/// 如果这个结构体被 drop（无论是否 commit），会自动清理临时文件。
#[cfg(target_os = "linux")]
struct PreparedLinuxArtifactWrite {
    location: LinuxArtifactLocation,    // 文件位置句柄
    temporary: CString,                 // 临时文件名
    expected: LinuxArtifactTargetState, // 目标文件的预期状态快照
    temporary_present: bool,            // 临时文件是否还在磁盘上
    error: &'static str,                // 错误码
}

/// 【Linux 平台】PreparedLinuxArtifactWrite 的实现方法。
#[cfg(target_os = "linux")]
impl PreparedLinuxArtifactWrite {
    /// 提交这个写操作：原子地改名临时文件为目标文件。
    ///
    /// 提交时会再次验证：
    /// 1. Location 还是有效的（父目录没被替换）
    /// 2. 目标文件的状态还是预期的（没被改动）
    /// 3. 如果目标不存在，用 RENAME_NOREPLACE 创建；如果存在，用 RENAME_EXCHANGE 交换
    /// 4. 如果交换后原文件还是预期的状态，删除它；否则交换回去
    ///
    /// # 返回值
    /// 成功返回 Ok(())；任何检查失败都返回 error 码。
    fn commit(mut self) -> Result<(), &'static str> {
        // 1. 最后一次检查 Location 是否还有效
        if !self.location.is_current() {
            return Err(self.error);
        }

        // 2. 检查目标文件的当前状态是否还是预期的
        let current =
            linux_artifact_target_state(self.location.parent.as_raw_fd(), &self.location.target)
                .map_err(|_| self.error)?;
        if current != self.expected {
            return Err(self.error);
        }

        // 3. 根据目标文件的预期状态，选择合适的改名策略
        match self.expected {
            // 如果目标文件原来不存在，用 RENAME_NOREPLACE 创建新文件
            LinuxArtifactTargetState::Missing => {
                linux_artifact_renameat2(
                    self.location.parent.as_raw_fd(),
                    &self.temporary,
                    &self.location.target,
                    LINUX_RENAME_NOREPLACE,
                )
                .map_err(|_| self.error)?;
                self.temporary_present = false; // 改名成功，临时文件不再存在
            }
            // 如果目标文件原来存在，用 RENAME_EXCHANGE 原子地交换
            LinuxArtifactTargetState::Regular { .. } => {
                linux_artifact_renameat2(
                    self.location.parent.as_raw_fd(),
                    &self.temporary,
                    &self.location.target,
                    LINUX_RENAME_EXCHANGE,
                )
                .map_err(|_| self.error)?;

                // 交换后，原文件被移到临时位置。检查它是否还是预期的状态
                let displaced =
                    linux_artifact_target_state(self.location.parent.as_raw_fd(), &self.temporary);

                // 如果原文件的状态和预期不符，说明有人在改名过程中偷偷改了文件，
                // 那就需要交换回去以恢复原状
                if !matches!(displaced, Ok(state) if state == self.expected) {
                    // 尝试交换回去恢复原状
                    if linux_artifact_renameat2(
                        self.location.parent.as_raw_fd(),
                        &self.temporary,
                        &self.location.target,
                        LINUX_RENAME_EXCHANGE,
                    )
                    .is_err()
                    {
                        // 如果连交换回去都失败了，至少标记临时文件已经不在这个位置
                        self.temporary_present = false;
                    }
                    return Err(self.error);
                }

                // 删除原来的文件（现在在 temporary 位置）
                linux_artifact_unlinkat(self.location.parent.as_raw_fd(), &self.temporary)
                    .map_err(|_| self.error)?;
                self.temporary_present = false;
            }
        }

        // 4. 最后，对父目录调用 fsync 以确保 inode 更新落盘
        let _ = linux_artifact_sync_directory(self.location.parent.as_raw_fd());
        Ok(())
    }
}

/// 【Linux 平台】Drop 实现：自动清理临时文件。
///
/// 如果由于某种原因（比如提交失败）没有删除临时文件，Drop 会确保它被清理。
#[cfg(target_os = "linux")]
impl Drop for PreparedLinuxArtifactWrite {
    fn drop(&mut self) {
        // 如果临时文件还在，删除它
        if self.temporary_present {
            let _ = linux_artifact_unlinkat(self.location.parent.as_raw_fd(), &self.temporary);
        }
    }
}

/// 【Linux 平台】读取项目产物文件。
///
/// Linux 版本使用系统调用 openat、fstat、fsync 等原语，确保：
/// - 路径验证与打开之间没有 TOCTOU（time-of-check-time-of-use）窗口
/// - 整个读取过程中文件元数据一致（防止中途被替换）
/// - O_NOFOLLOW 阻止符号链接
///
/// # 参数与返回值
/// 同非 Linux 版本。
#[cfg(target_os = "linux")]
fn read_project_artifact_linux(
    root: &Path,
    relative: &Path,
    error: &'static str,
) -> Result<String, &'static str> {
    // 打开父目录和目标文件的句柄，验证路径组成部分都是合法的
    let location = LinuxArtifactLocation::open(root, relative, false, error)?;

    // 获取目标文件当前的元数据快照（device、inode、mode、mtime 等）
    // 这个快照会在最后重新验证，以检测中途是否有人修改
    let expected = linux_artifact_target_state(location.parent.as_raw_fd(), &location.target)
        .map_err(|_| error)?;

    // 如果文件不存在，返回错误
    if matches!(expected, LinuxArtifactTargetState::Missing) {
        return Err(error);
    }

    // 用 openat 系统调用打开目标文件，带上：
    // - O_RDONLY: 只读
    // - O_NOFOLLOW: 不跟随符号链接
    // - O_CLOEXEC: 新进程自动关闭（fork/exec 后）
    // SAFETY: openat 是标准系统调用，入参都来自 CString，安全使用
    let fd = unsafe {
        libc::openat(
            location.parent.as_raw_fd(),
            location.target.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };

    if fd < 0 {
        return Err(error);
    }

    // SAFETY: fd >= 0，从系统调用返回，转换为 File 所有权
    let mut file = unsafe { File::from_raw_fd(fd) };

    // 再次验证打开后的文件元数据与预期相符，防止符号链接攻击
    if linux_artifact_file_state(&file).map_err(|_| error)? != expected {
        return Err(error);
    }

    // 读取全部文件内容
    let mut raw = String::new();
    file.read_to_string(&mut raw).map_err(|_| error)?;

    // 【TOCTOU 防护】读取后再验证一遍：
    // 1. 文件本身的元数据是否还是原样
    // 2. 父目录是否还是原样（防止目录被替换）
    // 3. 目标文件在父目录中的元数据是否还是原样
    if linux_artifact_file_state(&file).map_err(|_| error)? != expected
        || !location.is_current()
        || linux_artifact_target_state(location.parent.as_raw_fd(), &location.target)
            .map_err(|_| error)?
            != expected
    {
        return Err(error);
    }
    Ok(raw)
}

/// 【Linux 平台】准备产物写操作：打开位置、创建临时文件、写入内容。
///
/// 这个函数不进行最后的改名操作（commit），而是返回一个准备好的写操作对象。
/// 调用者可以在提交之前进行额外的检查或测试。
///
/// # 参数
/// - `root`: 项目根目录
/// - `relative`: 相对路径
/// - `contents`: 待写入的字节
/// - `error`: 错误码
///
/// # 返回值
/// 成功返回 PreparedLinuxArtifactWrite（可以调用 .commit() 来完成操作）。
#[cfg(target_os = "linux")]
fn prepare_project_artifact_linux(
    root: &Path,
    relative: &Path,
    contents: &[u8],
    error: &'static str,
) -> Result<PreparedLinuxArtifactWrite, &'static str> {
    // 1. 打开位置的所有句柄（包括父目录）
    let location = LinuxArtifactLocation::open(root, relative, true, error)?;

    // 2. 获取目标文件的当前状态（如果存在的话）
    let expected = linux_artifact_target_state(location.parent.as_raw_fd(), &location.target)
        .map_err(|_| error)?;

    // 3. 创建临时文件并获取其句柄
    let (temporary, mut file) =
        linux_create_artifact_temp(location.parent.as_raw_fd()).map_err(|_| error)?;

    // 4. 尝试写入内容
    let result = (|| {
        file.write_all(contents).map_err(|_| error)?; // 写入全部字节
        file.flush().map_err(|_| error)?; // 刷新到内核缓冲
        file.sync_all().map_err(|_| error) // fsync 到磁盘
    })();
    drop(file); // 释放文件句柄

    // 5. 如果写入失败，删除临时文件并返回错误
    if let Err(write_error) = result {
        let _ = linux_artifact_unlinkat(location.parent.as_raw_fd(), &temporary);
        return Err(write_error);
    }

    // 6. 写入成功，返回准备好的写操作对象
    Ok(PreparedLinuxArtifactWrite {
        location,
        temporary,
        expected,
        temporary_present: true, // 临时文件现在存在
        error,
    })
}

/// 【Linux 平台】验证相对路径的组成部分。
///
/// 确保路径不为空、不是绝对路径、不包含特殊组件（.. 等），
/// 然后返回分解后的 OsString 组件列表。
///
/// # 返回值
/// 成功返回组件列表；失败返回错误码。
#[cfg(target_os = "linux")]
fn validated_artifact_components(
    relative: &Path,
    error: &'static str,
) -> Result<Vec<OsString>, &'static str> {
    // 拒绝空路径或绝对路径
    if relative.as_os_str().is_empty() || relative.is_absolute() {
        return Err(error);
    }

    // 分解路径并检查每个组件都是「普通」组件（不是 .. 或 / 等）
    relative
        .components()
        .map(|component| match component {
            Component::Normal(value) => Ok(value.to_os_string()),
            _ => Err(error),
        })
        .collect()
}

/// 【Linux 平台】打开项目根目录。
///
/// 使用 O_DIRECTORY 确保打开的是目录；O_NOFOLLOW 防止符号链接；O_CLOEXEC 防止子进程继承。
///
/// # 参数
/// - `path`: 项目根目录的路径
///
/// # 返回值
/// 成功返回打开的 File 句柄；失败返回 io::Error。
#[cfg(target_os = "linux")]
fn linux_open_artifact_root(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .read(true)
        // O_DIRECTORY: 仅在目标是目录时才打开成功
        // O_NOFOLLOW: 如果是符号链接则失败
        // O_CLOEXEC: 新进程自动关闭此句柄
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
}

/// 【Linux 平台】使用 openat 打开相对于 parent 的目录。
///
/// 这是 POSIX 的标准做法，避免了 TOCTOU 漏洞：不需要先获得完整路径，
/// 直接相对于一个已打开的目录句柄操作。
///
/// # 参数
/// - `parent`: 父目录的文件描述符
/// - `component`: 目录名（OsStr）
/// - `create`: 如果目录不存在，是否创建（mode 0o755）
///
/// # 返回值
/// 成功返回打开的 File；失败返回 io::Error。
///
/// # 关键系统调用
/// - `openat(parent, component, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)`: 打开目录
/// - `mkdirat(parent, component, 0o755)`: 创建目录（如果需要）
#[cfg(target_os = "linux")]
fn linux_open_artifact_directory_at(
    parent: RawFd,
    component: &std::ffi::OsStr,
    create: bool,
) -> std::io::Result<File> {
    // 将 OsStr 转换为 CString（C 函数需要 NUL 终止的字符串）
    let component = linux_artifact_cstring(component)?;

    // 定义一个闭包用于打开目录，便于重试
    let open = || unsafe {
        // SAFETY: parent 是有效的文件描述符，component 是 CString，安全
        libc::openat(
            parent,
            component.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };

    let mut fd = open();

    // 如果打开失败且错误是 ENOENT（不存在），并且 create 为真，尝试创建目录
    if fd < 0 && create && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
        // SAFETY: parent 是有效文件描述符，component 是 CString
        let mkdir = unsafe { libc::mkdirat(parent, component.as_ptr(), 0o755) };
        // 如果创建失败且错误不是 EEXIST（已存在），返回错误
        if mkdir != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
            return Err(std::io::Error::last_os_error());
        }
        // 重新尝试打开（目录现在应该存在了）
        fd = open();
    }

    if fd < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        // SAFETY: fd >= 0，从系统调用返回
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

/// 【Linux 平台】获取文件或目录的元数据快照。
///
/// 使用 O_PATH 打开文件（不需要实际读写权限），然后查询其 stat 信息。
/// 返回的元数据用于检测文件在操作过程中是否被替换或修改。
///
/// # 参数
/// - `parent`: 父目录的文件描述符
/// - `name`: 文件名（CString）
///
/// # 返回值
/// 文件不存在返回 Ok(Missing)；正常文件返回 Ok(Regular {...})；其他错误返回 io::Error。
///
/// # 关键系统调用
/// - `openat(parent, name, O_PATH | O_NOFOLLOW)`: 打开文件（不跟随符号链接）
#[cfg(target_os = "linux")]
fn linux_artifact_target_state(
    parent: RawFd,
    name: &CString,
) -> std::io::Result<LinuxArtifactTargetState> {
    // 使用 openat 打开文件，带上：
    // - O_PATH: 不需要读写权限，只需要路径查询权限
    // - O_NOFOLLOW: 如果是符号链接就失败
    // - O_CLOEXEC: 新进程自动关闭
    // SAFETY: parent 是有效文件描述符，name 是 CString
    let fd = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };

    if fd < 0 {
        let error = std::io::Error::last_os_error();
        // 如果错误是 ENOENT（不存在），返回 Missing 状态
        return if error.raw_os_error() == Some(libc::ENOENT) {
            Ok(LinuxArtifactTargetState::Missing)
        } else {
            Err(error)
        };
    }

    // SAFETY: fd >= 0
    let file = unsafe { File::from_raw_fd(fd) };
    // 查询这个文件的元数据并返回
    linux_artifact_file_state(&file)
}

/// 【Linux 平台】从已打开的 File 句柄查询其元数据。
///
/// 这是 `linux_artifact_target_state` 的内部工具函数。
/// 提取 stat 信息并构建 LinuxArtifactTargetState 快照。
///
/// # 约束条件
/// 只有满足以下条件的文件才会返回 Regular 状态：
/// - 是常规文件（不是目录、符号链接等）
/// - 硬链接数为 1（防止多个硬链接指向同一个文件，这会绕过我们的防护）
///
/// 其他情况都返回错误，因为这表示文件的状态不符合我们的安全假设。
#[cfg(target_os = "linux")]
fn linux_artifact_file_state(file: &File) -> std::io::Result<LinuxArtifactTargetState> {
    let metadata = file.metadata()?;

    // 检查文件是否满足安全假设
    if !metadata.file_type().is_file() || metadata.nlink() != 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "artifact target is not a single-link regular file",
        ));
    }

    // 构建元数据快照
    Ok(LinuxArtifactTargetState::Regular {
        device: metadata.dev(),
        inode: metadata.ino(),
        mode: metadata.mode(),
        links: metadata.nlink(),
        size: metadata.size(),
        modified_seconds: metadata.mtime(),
        modified_nanoseconds: metadata.mtime_nsec(),
        changed_seconds: metadata.ctime(),
        changed_nanoseconds: metadata.ctime_nsec(),
    })
}

/// 【Linux 平台】检查两个已打开的目录是否是同一个。
///
/// 通过比较 device + inode 来判断，这是标准的 POSIX 做法。
///
/// # 返回值
/// 如果两个目录是同一个（来自同一个设备、同一个 inode），返回 true；否则 false。
#[cfg(target_os = "linux")]
fn linux_same_directory(left: &File, right: &File) -> bool {
    match (left.metadata(), right.metadata()) {
        (Ok(left), Ok(right)) => left.dev() == right.dev() && left.ino() == right.ino(),
        _ => false,
    }
}

/// 【Linux 平台】创建唯一的临时文件。
///
/// 使用 O_CREAT | O_EXCL 原子地创建文件，如果已存在则失败。
/// 尝试 16 次，每次用不同的名字（pid + 纳秒时间戳 + 尝试序号）。
///
/// # 参数
/// - `parent`: 父目录的文件描述符
///
/// # 返回值
/// 成功返回 (临时文件名, 打开的文件句柄)；失败返回 io::Error。
///
/// # 关键系统调用
/// - `openat(..., O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, 0o600)`: 创建文件
#[cfg(target_os = "linux")]
fn linux_create_artifact_temp(parent: RawFd) -> std::io::Result<(CString, File)> {
    // 获取当前纳秒级时间戳作为唯一性标记
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);

    // 尝试 16 次创建临时文件
    for attempt in 0..ARTIFACT_TEMP_ATTEMPTS {
        // 生成临时文件名：.kiana-artifact-{pid}-{纳秒时间戳}-{尝试序号}
        let name = CString::new(format!(
            ".kiana-artifact-{}-{stamp}-{attempt}",
            std::process::id()
        ))
        .expect("generated artifact name cannot contain NUL");

        // 使用 openat 原子地创建文件（O_EXCL 确保文件不存在）
        // SAFETY: parent 是有效的文件描述符，name 是 CString
        let fd = unsafe {
            libc::openat(
                parent,
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600, // 权限：仅所有者可读写
            )
        };

        if fd >= 0 {
            // SAFETY: fd >= 0
            return Ok((name, unsafe { File::from_raw_fd(fd) }));
        }

        let error = std::io::Error::last_os_error();
        // 如果错误是 EEXIST（文件已存在），继续下一次尝试
        if error.raw_os_error() != Some(libc::EEXIST) {
            return Err(error);
        }
    }

    // 如果 16 次都失败，返回错误
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "artifact temporary name attempts exhausted",
    ))
}

/// 【Linux 平台】原子改名操作（renameat2 系统调用）。
///
/// 这是 Linux 特有的系统调用（不在 POSIX 标准中），提供了额外的原子操作选项：
/// - RENAME_NOREPLACE: 如果目标存在则失败
/// - RENAME_EXCHANGE: 原子地交换源和目标
///
/// # 参数
/// - `parent`: 父目录的文件描述符
/// - `source`: 源文件名（CString）
/// - `target`: 目标文件名（CString）
/// - `flags`: renameat2 的 flag（LINUX_RENAME_NOREPLACE 或 LINUX_RENAME_EXCHANGE）
///
/// # 返回值
/// 成功返回 Ok(())；失败返回 io::Error。
///
/// # 关键系统调用
/// - `renameat2(parent, source, parent, target, flags)`: 原子改名
///
/// 【为什么要用 renameat2】
/// 普通的 rename 不能保证「如果目标存在则不覆盖」的原子性。renameat2 提供了这种保证。
#[cfg(target_os = "linux")]
fn linux_artifact_renameat2(
    parent: RawFd,
    source: &CString,
    target: &CString,
    flags: u32,
) -> std::io::Result<()> {
    // 调用 renameat2 系统调用
    // SAFETY: parent 是有效的文件描述符，source 和 target 都是 CString（NUL 结尾）
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            parent,          // 源目录
            source.as_ptr(), // 源文件名
            parent,          // 目标目录（同一个父目录）
            target.as_ptr(), // 目标文件名
            flags,           // renameat2 flags
        )
    };

    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// 【Linux 平台】删除文件（unlinkat 系统调用）。
///
/// 使用 unlinkat 相对于一个已打开的父目录删除文件，避免 TOCTOU 漏洞。
///
/// # 参数
/// - `parent`: 父目录的文件描述符
/// - `name`: 文件名（CString）
///
/// # 返回值
/// 成功返回 Ok(())；失败返回 io::Error。
///
/// # 关键系统调用
/// - `unlinkat(parent, name, 0)`: 删除文件
#[cfg(target_os = "linux")]
fn linux_artifact_unlinkat(parent: RawFd, name: &CString) -> std::io::Result<()> {
    // SAFETY: parent 是有效的文件描述符，name 是 CString
    if unsafe { libc::unlinkat(parent, name.as_ptr(), 0) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// 【Linux 平台】对目录调用 fsync，确保 inode 变更落盘。
///
/// 在 Linux 上，目录的 fsync 会确保所有元数据更新（新增/删除文件、改名等）
/// 都被刷新到磁盘。这对持久化至关重要：防止系统崩溃时丢失产物写入。
///
/// # 参数
/// - `parent`: 目录的文件描述符
///
/// # 返回值
/// 成功返回 Ok(())；失败返回 io::Error。
///
/// # 关键系统调用
/// - `fsync(parent)`: 对目录进行同步（确保目录的元数据落盘）
#[cfg(target_os = "linux")]
fn linux_artifact_sync_directory(parent: RawFd) -> std::io::Result<()> {
    // SAFETY: parent 是有效的文件描述符
    if unsafe { libc::fsync(parent) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// 【Linux 平台】将 OsStr 转换为 CString。
///
/// C 系统调用需要 NUL 终止的字符串。如果转换过程中发现 NUL 字节，返回错误。
///
/// # 参数
/// - `value`: OsStr（可能包含任意字节）
///
/// # 返回值
/// 成功返回 CString；如果包含 NUL 字节则返回 InvalidInput 错误。
#[cfg(target_os = "linux")]
fn linux_artifact_cstring(value: &std::ffi::OsStr) -> std::io::Result<CString> {
    CString::new(value.as_bytes()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "artifact path contains NUL",
        )
    })
}
#[cfg(all(test, target_os = "linux"))]
mod artifact_path_tests {
    use super::{prepare_project_artifact_linux, write_project_artifact};
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_root(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after epoch")
            .as_nanos();
        for attempt in 0..16 {
            let root = std::env::temp_dir().join(format!(
                "kiana-core-artifact-{label}-{}-{stamp}-{attempt}",
                std::process::id()
            ));
            if fs::create_dir(&root).is_ok() {
                return root;
            }
        }
        panic!("could not create temporary artifact test root");
    }

    #[test]
    fn prepared_artifact_write_rejects_parent_rename_before_commit() {
        let root = temporary_root("parent-rename");
        let outside = temporary_root("parent-rename-outside");
        fs::create_dir(root.join("gate")).unwrap();
        let prepared = prepare_project_artifact_linux(
            &root,
            Path::new("gate/REVIEW.json"),
            b"replacement",
            "artifact_write_failed",
        )
        .unwrap();

        fs::rename(root.join("gate"), root.join("gate-before-rename")).unwrap();
        symlink(&outside, root.join("gate")).unwrap();

        assert_eq!(prepared.commit(), Err("artifact_write_failed"));
        assert!(!outside.join("REVIEW.json").exists());
        assert!(!root.join("gate-before-rename/REVIEW.json").exists());

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn prepared_artifact_write_rejects_final_symlink_replacement_before_commit() {
        let root = temporary_root("target-symlink");
        let outside = temporary_root("target-symlink-outside");
        fs::create_dir(root.join("gate")).unwrap();
        let outside_target = outside.join("outside-target");
        fs::write(&outside_target, "outside sentinel").unwrap();
        let prepared = prepare_project_artifact_linux(
            &root,
            Path::new("gate/REVIEW.json"),
            b"replacement",
            "artifact_write_failed",
        )
        .unwrap();

        symlink(&outside_target, root.join("gate/REVIEW.json")).unwrap();

        assert_eq!(prepared.commit(), Err("artifact_write_failed"));
        assert_eq!(
            fs::read_to_string(&outside_target).unwrap(),
            "outside sentinel"
        );
        assert!(fs::symlink_metadata(root.join("gate/REVIEW.json"))
            .unwrap()
            .file_type()
            .is_symlink());

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn prepared_artifact_write_rejects_target_version_replacement_before_commit() {
        let root = temporary_root("target-version");
        fs::create_dir(root.join("gate")).unwrap();
        let target = root.join("gate/REVIEW.json");
        fs::write(&target, "original").unwrap();
        let prepared = prepare_project_artifact_linux(
            &root,
            Path::new("gate/REVIEW.json"),
            b"replacement",
            "artifact_write_failed",
        )
        .unwrap();

        fs::write(&target, "changed after prepare").unwrap();

        assert_eq!(prepared.commit(), Err("artifact_write_failed"));
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "changed after prepare"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn artifact_write_rejects_hardlinked_target_without_mutating_source() {
        let root = temporary_root("hardlink");
        let outside = temporary_root("hardlink-outside");
        fs::create_dir(root.join("gate")).unwrap();
        let outside_target = outside.join("outside-target");
        fs::write(&outside_target, "outside sentinel").unwrap();
        fs::hard_link(&outside_target, root.join("gate/REVIEW.json")).unwrap();

        assert_eq!(
            write_project_artifact(
                &root,
                Path::new("gate/REVIEW.json"),
                b"replacement",
                "artifact_write_failed",
            ),
            Err("artifact_write_failed")
        );
        assert_eq!(
            fs::read_to_string(&outside_target).unwrap(),
            "outside sentinel"
        );

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }
}
