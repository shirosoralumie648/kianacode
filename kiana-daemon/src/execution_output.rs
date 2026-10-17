//! 普通 shell 执行结果的有界采集、脱敏、预览与诊断元数据投影。
//!
//! 本模块由 `harness_capabilities::run_confined_cancellable()` 在子进程 stdout/stderr pipe 上
//! 启动 reader。它把两个并行字节流分别限制、清理并转成短文本预览；完整采集字节随后由
//! `execution_control::store_output()` 存入受控本地输出目录，并通过 `ExecutionOutputRef`
//! 供 `execution.output.read` 分页读取。
//!
//! ```text
//! child stdout / stderr
//!     -> read_capped：计数、收集上限、观测/行数硬停
//!     -> drain_capped：有界等待 reader 收尾
//!     -> render_capped_for：UTF-8容错、secret/ANSI清理、有界预览
//!     -> shell result + metadata
//!     -> store_output 持久化完整的受限采集内容，并替换对话中的长文本
//! ```
//!
//! stdout 和 stderr 各自得到独立预算，因此这里的每流上限不能直接当成两流合计上限。
//! `ExecutionOutputBudget` 的收集值、观测值、预览值和持久化值也是不同阶段的限制；预算 DTO
//! 只声明数值，实际调用者必须选择对应 helper 执行。长驻 `process.start` 走另一条
//! `ExecutionControl::capture()` 路径，只留内存滚动预览，不使用这里的归档 reader。

use kiana_domain::ExecutionOutputBudget;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::task::JoinHandle;

/// 每次从 pipe 最多读 8192 字节；硬观测上限是在整块读入后才检查，最后一块可能越过上限。
const READ_CHUNK_SIZE: usize = 8192;
/// stop 之后最多等待 2000ms 收齐某一条输出 pipe 的 reader task。
const IO_DRAIN_TIMEOUT: Duration = Duration::from_millis(2000);

#[derive(Debug)]
/// 一条 stdout 或 stderr 流的采集结果；两个 reader 各自产生一个值。
pub(crate) struct CappedOutput {
    /// 已保留供脱敏/预览/存档使用的原始字节，不超过 collect budget。
    pub(crate) bytes: Vec<u8>,
    /// 是否发生 collect、observed 或 line 数量截断；read I/O error 单独放在 read_error。
    pub(crate) truncated: bool,
    /// 从 pipe 实际读入并计数的原始字节数；超过 observed limit 的最后一块也计入。
    pub(crate) observed_bytes: u64,
    /// 读入字节中 LF（`\n`）的总数量，不把单独 CR 作为一行。
    pub(crate) lines: u64,
    /// OS read error kind 或本模块生成的硬上限/reader 错误标记；None 表示未记录此类问题。
    pub(crate) read_error: Option<String>,
}

/// 有界采集一条异步输出流，同时限制保存量、观测量和行数。
///
/// 【调用者】`run_confined_cancellable()` 为 stdout、stderr 各 spawn 一个独立任务。
/// 【输入/输出】异步字节 reader 与 `ExecutionOutputBudget`；返回保留字节、计数、截断位和
/// 可选 read_error。这里不解码 UTF-8、不脱敏，也不写文件。
/// 【核心流程】每次最多读取 8192 字节；累计观测字节与 LF 行数；超过 observed/line 硬限额时
/// 标记截断、记录错误并停止读取；仅收集至 collect_max_bytes，但达到收集上限后仍继续 drain，
/// 直到 EOF 或 observed/line 上限，以免 child 因 pipe 无人读取而阻塞。
/// 【失败】budget 不合法转成 io::Error；底层 read 错误放入结果的 `read_error` 并返回已有部分数据，
/// 不把整个调用变成 Err。观测/行数超额也以结果 metadata 表示，而不是 Rust Err。
/// 【注意】上限在整块 read 后检查，observed_bytes 和 lines 会计入触发超限的整块；越界最多
/// 可多读一个 READ_CHUNK_SIZE。stdout 与 stderr 分开调用，聚合总量可接近两份预算。
/// 【设计原因】collect limit 负责内存保留，observed/line limit 负责何时停止继续 drain，不能
/// 用“只读 collect_max_bytes 后立即停”代替，否则子进程仍能写满未读 pipe 并卡住。
pub(crate) async fn read_capped<R>(
    mut reader: R,
    budget: ExecutionOutputBudget,
) -> std::io::Result<CappedOutput>
where
    R: AsyncRead + Unpin,
{
    budget.validate().map_err(std::io::Error::other)?;
    // 预分配不会超过一个读取块或收集预算，避免一开始就按更大的 persisted/observed 上限占内存。
    let mut bytes = Vec::with_capacity(READ_CHUNK_SIZE.min(budget.collect_max_bytes));
    let mut tmp = [0u8; READ_CHUNK_SIZE];
    let mut truncated = false;
    let mut observed_bytes = 0u64;
    let mut lines = 0u64;
    let mut read_error = None;
    loop {
        let n = match reader.read(&mut tmp).await {
            Ok(count) => count,
            Err(error) => {
                read_error = Some(error.kind().to_string());
                break;
            }
        };
        if n == 0 {
            break;
        }
        // 两类统计都使用饱和加法，极端长流不会因 u64 回绕而表现成更小的合法值。
        observed_bytes = observed_bytes.saturating_add(n as u64);
        lines = lines.saturating_add(tmp[..n].iter().filter(|byte| **byte == b'\n').count() as u64);
        if observed_bytes > budget.observed_max_bytes as u64 {
            truncated = true;
            read_error = Some("output_total_limit".to_owned());
            break;
        }
        if lines > budget.max_lines as u64 {
            truncated = true;
            read_error = Some("output_line_limit".to_owned());
            break;
        }
        // 已经收满仍需读走并丢弃这块内容，使 child 的 pipe 继续可写；真正硬停由上面的
        // observed/line 限制负责。
        if bytes.len() >= budget.collect_max_bytes {
            truncated = true;
            continue;
        }
        let take = (budget.collect_max_bytes - bytes.len()).min(n);
        bytes.extend_from_slice(&tmp[..take]);
        if take < n {
            truncated = true;
        }
    }
    Ok(CappedOutput {
        bytes,
        truncated,
        observed_bytes,
        lines,
        read_error,
    })
}

/// 等待一条 reader task 完成并取回结果，超过 drain 时限则 abort。
///
/// 【调用者】命令已经由 supervisor 确认停止后，shell handler 才调用本函数收尾 stdout/stderr。
/// 【输出】正常 join 返回 reader 结果；join/read 错误会映射为 `output_reader_failed`；超过
/// 2000ms 则 abort task 并返回 `output_drain_timeout`。异常回执不含 reader 先前已在 task 内收集的
/// 部分 bytes，因为只有 task 成功 join 才能取回 CappedOutput。
/// 【为什么有超时】child 后代或外部进程可能仍持有 pipe 写端，导致 leader 已退出而 EOF 一直不来；
/// 有界等待避免 shell 请求永久挂起。
pub(crate) async fn drain_capped(
    handle: &mut JoinHandle<std::io::Result<CappedOutput>>,
) -> CappedOutput {
    match tokio::time::timeout(IO_DRAIN_TIMEOUT, &mut *handle).await {
        Ok(Ok(Ok(output))) => output,
        Ok(Ok(Err(_))) | Ok(Err(_)) => CappedOutput {
            bytes: Vec::new(),
            truncated: false,
            observed_bytes: 0,
            lines: 0,
            read_error: Some("output_reader_failed".to_owned()),
        },
        Err(_) => {
            handle.abort();
            CappedOutput {
                bytes: Vec::new(),
                truncated: false,
                observed_bytes: 0,
                lines: 0,
                read_error: Some("output_drain_timeout".to_owned()),
            }
        }
    }
}

/// 用默认 stdout 标签清理完整文本，供没有 stream channel 信息的调用点使用。
///
/// 处理顺序是先通用文本脱敏，再移除/过滤终端控制字符，最后运行 sentinel 扫描；扫描器发现
/// 哨兵冲突时会把整段文本替换为 `[REDACTED]`。扫描规则对各 channel 相同；channel 只用于
/// 标记 finding 的来源。若调用方知道真实 stdout/stderr channel，可改用 `safe_output_text_for()`，
/// 让诊断信息保留准确来源。
pub(crate) fn safe_output_text(text: &str) -> String {
    safe_output_text_for(kiana_domain::SecretScanChannel::Stdout, text)
}

/// 先脱敏，再清理 ANSI 控制序列和其他控制字符，最后扫描 sentinel 并用 channel 标记 finding。
///
/// `SecretScanChannel` 不会切换扫描规则；它让拒绝结果能指出文本来自 stdout、stderr 等哪个边界。
///
/// ESC `[` 分支跳过 CSI 控制序列（如颜色/光标控制），ESC `]` 分支跳过 OSC 序列（常见于
/// 终端标题/超链接）；普通控制字符只保留换行与 tab。最后的 sentinel 检查是兜底：如扫描器
/// 返回错误，就宁可隐藏整段输出而非把含风险内容部分展示出来。
///
/// 该 helper 对单个完整文本做处理，不保留跨 chunk 状态；持续字节流应先经过 Domain
/// `StreamingRedactor`，然后再调用它生成展示文本。
pub(crate) fn safe_output_text_for(channel: kiana_domain::SecretScanChannel, text: &str) -> String {
    let text = kiana_domain::redact_text(text);
    let mut output = String::new();
    let mut state = 0u8;
    for ch in text.chars() {
        match state {
            0 if ch == '\u{1b}' => state = 1,
            0 if ch == '\n' || ch == '\t' || !ch.is_control() => output.push(ch),
            0 => {}
            1 if ch == '[' => state = 2,
            1 if ch == ']' => state = 3,
            1 => state = 0,
            2 if ('@'..='~').contains(&ch) => state = 0,
            3 if ch == '\u{7}' => state = 0,
            3 if ch == '\u{1b}' => state = 4,
            4 if ch == '\\' => state = 0,
            4 => state = 3,
            _ => {}
        }
    }
    if kiana_domain::scan_secret_sentinels(channel, &output).is_ok() {
        output
    } else {
        "[REDACTED]".to_owned()
    }
}

/// 用默认 stdout 来源标签把受限字节转换为预览文本；有真实 stream channel 时用 `render_capped_for()`，
/// 使扫描 finding 保留真实来源。各 channel 当前共用同一组 sentinel 规则。
pub(crate) fn render_capped(bytes: &[u8], truncated: bool, preview_max_bytes: usize) -> String {
    render_capped_for(
        kiana_domain::SecretScanChannel::Stdout,
        bytes,
        truncated,
        preview_max_bytes,
    )
}

/// 将采集字节投影为安全、UTF-8 对齐且预算内的内容片段，并附上截断提示。
///
/// 【数据流】原始 bytes → UTF-8 有损解码（无效字节替换为 U+FFFD）→ 通用脱敏/终端过滤
/// → preview_max_bytes 截断 → 可选提示后缀。
/// `channel` 只作为 sentinel finding 的来源标签，不改变扫描规则。
/// 【输出长度】正文不会超过 `preview_max_bytes`，但追加的 `...preview-truncated...` 或
/// `...truncated...` 标记不计入该预算，所以返回 String 总字节数可能更大。
/// 【副作用/失败】纯内存转换，不写日志或文件；即使输入含非法 UTF-8 也不会返回错误。
/// 【设计原因】模型和 UI 只需要短片段，调用方仍可通过 ExecutionOutputRef 读取受限完整归档；
/// 预览超长与 reader 已截断是不同原因，因此使用不同 marker。
pub(crate) fn render_capped_for(
    channel: kiana_domain::SecretScanChannel,
    bytes: &[u8],
    truncated: bool,
    preview_max_bytes: usize,
) -> String {
    // 子进程可以输出任意字节；有损解码让展示路径仍能生成合法 Rust String，而不把原始 bytes 猜成有效 UTF-8。
    let mut text = safe_output_text_for(channel, &String::from_utf8_lossy(bytes));
    if text.len() > preview_max_bytes {
        let mut end = preview_max_bytes.min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n...preview-truncated...");
    } else if truncated {
        text.push_str("\n...truncated...");
    }
    // 预览是发布给模型/界面的信号，必须走该通道的 bounded encoder：它复用同一份 profile 的
    // 字节上限与通道扫描；任何失败都退回固定占位文本，绝不把未通过检查的内容交出去。预览
    // 已被裁到 preview_max_bytes（默认 8 KiB），远小于文本通道 profile 上限，因此正常路径不会
    // 触发大小拒绝，这里只是把“发布前必须过 profile”的契约显式绑定到运行时 sink。
    let profile = kiana_domain::RedactionProfile::for_channel(channel);
    match kiana_domain::encode_bounded_text_for(&profile, channel, &text) {
        Ok(encoded) => encoded.text,
        Err(_) => "[REDACTED]".to_owned(),
    }
}

/// 返回治理某个输出通道的 bounded encoder 所采用的 profile 摘要。
///
/// `execution_control::store_output()` 把它写进归档记录，使一份持久化的 shell 输出能定位到
/// 实际使用的脱敏配置；它只暴露摘要，不暴露规则内部状态。
pub(crate) fn preview_profile_digest(channel: kiana_domain::SecretScanChannel) -> String {
    kiana_domain::RedactionProfile::for_channel(channel).profile_digest
}

/// 将已存在的文本重新走 stdout 安全清理并裁成预览；不表示该文本来自真实 stdout pipe。
pub(crate) fn bounded_preview(text: &str, preview_max_bytes: usize) -> String {
    render_capped(text.as_bytes(), false, preview_max_bytes)
}

/// 把采集计数、截断原因和采用的预算值打包到 shell 结果的 metadata 字段。
///
/// 这些值是两个 reader 的单流元数据；函数不自行聚合 stdout/stderr，也不重新验证 OS enforcement。
pub(crate) fn metadata(output: &CappedOutput, budget: &ExecutionOutputBudget) -> Value {
    json!({
        "captured_bytes": output.bytes.len(),
        "observed_bytes": output.observed_bytes,
        "lines": output.lines,
        "truncated": output.truncated,
        "read_error": output.read_error,
        "budget": {
            "collect_max_bytes": budget.collect_max_bytes,
            "preview_max_bytes": budget.preview_max_bytes,
            "persist_max_bytes": budget.persist_max_bytes,
            "observed_max_bytes": budget.observed_max_bytes,
            "max_lines": budget.max_lines,
        }
    })
}

#[cfg(test)]
/// H15 有界采集与预览的行为夹具。
///
/// 这些测试用内存 reader 直接驱动 `read_capped`/`render_capped`/`metadata`，证明超出预算的
/// 输出在进入内存前就被逐块限制、截断原因有稳定元数据、预览始终有界且 UTF-8 安全。它们不
/// 启动真实子进程，也不证明操作系统级 pipe/磁盘限额或跨进程归档行为。
mod h15_bounds {
    use super::*;

    #[tokio::test]
    async fn h15_behavior_huge_output_is_bounded_before_buffering() {
        let mut budget = ExecutionOutputBudget::default();
        budget.collect_max_bytes = 4 * 1024;
        budget.persist_max_bytes = 4 * 1024;
        budget.observed_max_bytes = 16 * 1024;
        budget.preview_max_bytes = 256;
        budget.max_lines = 1_000_000;
        budget.validate().expect("bounded policy");

        // 4 MiB 单一字节流：远大于收集/观测预算。
        let payload = vec![b'x'; 4 * 1024 * 1024];
        let output = read_capped(&payload[..], budget.clone())
            .await
            .expect("capped read");

        // 保留量由收集上限决定，而不是先读满整数 MiB 再截断。
        assert_eq!(output.bytes.len(), budget.collect_max_bytes);
        assert!(output.truncated);
        assert_eq!(output.read_error.as_deref(), Some("output_total_limit"));
        // 观测量最多越过硬上限一个读取块，不能无界增长。
        assert!(output.observed_bytes > budget.observed_max_bytes as u64);
        assert!(output.observed_bytes <= budget.observed_max_bytes as u64 + READ_CHUNK_SIZE as u64);
        let metadata = metadata(&output, &budget);
        assert_eq!(metadata["truncated"], json!(true));
        assert_eq!(metadata["captured_bytes"], json!(budget.collect_max_bytes));
    }

    #[tokio::test]
    async fn h15_behavior_line_limit_stops_reader_with_metadata() {
        let mut budget = ExecutionOutputBudget::default();
        budget.collect_max_bytes = 1024 * 1024;
        budget.persist_max_bytes = 1024 * 1024;
        budget.observed_max_bytes = 1024 * 1024;
        budget.max_lines = 4;
        budget.validate().expect("bounded policy");

        let payload = b"a\nb\nc\nd\ne\nf\n".repeat(200);
        let output = read_capped(&payload[..], budget.clone())
            .await
            .expect("capped read");
        assert!(output.truncated);
        assert_eq!(output.read_error.as_deref(), Some("output_line_limit"));
        assert!(output.lines > budget.max_lines as u64);
        assert!(output.bytes.len() <= budget.collect_max_bytes);
    }

    #[test]
    fn h15_behavior_preview_is_bounded_and_utf8_safe() {
        // 预览正文不超过预算，并显式标注截断来源（预览超长 vs 采集已截断）。
        let long = vec![b'z'; 4096];
        let preview = render_capped(&long, true, 128);
        let mut parts = preview.split("\n...preview-truncated...");
        let body = parts.next().expect("preview body");
        assert!(body.len() <= 128);
        assert!(preview.contains("...preview-truncated..."));

        let short = render_capped(b"hello", true, 128);
        assert!(short.contains("...truncated..."));

        // 非法 UTF-8 与二进制控制字节不会以原始字节返回：有损解码为替换字符并过滤控制字符。
        let raw = render_capped(&[0xff, 0xfe, 0x00, b'k'], false, 128);
        assert!(raw.contains('\u{fffd}'));
        assert!(!raw.contains('\u{0}'));
    }
}
