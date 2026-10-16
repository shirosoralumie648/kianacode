//! 工具完整输出溢出引用与分页响应的领域 DTO。
//!
//! 本模块把“大输出不放进一次模型/事件响应，而由受控读端分段取回”的契约表达为 `ToolOutputSpill`
//! 和 `ToolOutputPage`。spill 将完整内容的 `ExecutionOutputRef`、短预览、Run/Turn/Capability 与数据
//! 范围绑定在一起；page 把一段字节、偏移和下一页游标绑定回同一份 spill。
//!
//! ```text
//! 调用者持有完整输出 bytes
//!   -> ToolOutputSpill::new() 创建可校验的身份/摘要描述
//!   -> 受控 reader 提供 bytes 与当前 run/turn/capability/scope
//!   -> ToolOutputSpill::page() 核对身份和整份内容后切一页
//!   -> ToolOutputPage 携带本页、offset 与 next_cursor 返回调用者
//! ```
//!
//! 这里不保存完整 bytes、不读写 EventStore/文件，也不授权访问；调用方必须先完成访问控制，再把
//! 正确的完整 bytes 与当前身份传入 `page()`。当前源码中该 DTO 由 Domain/Protocol 暴露并有契约测试，
//! 尚未找到生产 adapter 的构造或分页调用，因此不能把此模块的存在当作完整输出功能已接通的证明。

use crate::{
    journal_sha256, json_digest, ExecutionOutputRef, InvocationId, RequestId, RunId, SchemaVersion,
    TurnId,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// spill 描述 DTO 的稳定 schema 名称。
pub const TOOL_OUTPUT_SPILL_SCHEMA: &str = "kiana.tool-output-spill.v1";
/// 单页响应 DTO 的稳定 schema 名称。
pub const TOOL_OUTPUT_PAGE_SCHEMA: &str = "kiana.tool-output-page.v1";
/// spill 与 page 共用的版本号；兼容性按 major 判断。
pub const TOOL_OUTPUT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// preview 字符串最多占用的 UTF-8 字节数；不包含完整输出本体。
pub const MAX_TOOL_OUTPUT_PREVIEW_BYTES: usize = 16 * 1024;
/// page 响应数据最大字节数（64 KiB）；spill 的 page_size 还可以设置更小的单页上限。
pub const MAX_TOOL_OUTPUT_PAGE_BYTES: usize = 64 * 1024;

/// 校验需存在且长度受限的字符串字段。
///
/// 长度按 UTF-8 字节数计算；前后空白仅用于判断是否全空，不会被裁掉或规范化。失败时只返回字段名
/// 对应的错误码，不把原始输入放入错误中。
fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        Err(format!("{field}_invalid"))
    } else {
        Ok(())
    }
}

/// 检查摘要是否为 `sha256:<64位十六进制>` 形式，不重新读取或哈希对应内容。
fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 为某个 spill 的起始 byte offset 生成分页续读指纹。
///
/// 指纹绑定 output、完整内容摘要、turn、capability、source scope 与 offset；不含 page limit，
/// 因此同一个 offset 的 cursor 不表示固定页长。它是未加密钥的 JSON digest，不是授权 token。
fn page_cursor(
    output_id: RequestId,
    content_digest: &str,
    turn_id: TurnId,
    capability_id: &str,
    source_scope_digest: &str,
    offset: u64,
) -> String {
    json_digest(&json!({
        "output_id": output_id,
        "content_digest": content_digest,
        "turn_id": turn_id,
        "capability_id": capability_id,
        "source_scope_digest": source_scope_digest,
        "offset": offset,
    }))
}

/// 完整工具输出的轻量引用、预览及分页边界声明；完整内容由调用者另外保存/读取。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolOutputSpill {
    /// spill DTO schema。
    pub schema: String,
    /// spill 格式版本。
    pub version: SchemaVersion,
    /// 完整内容引用，绑定 output ID、可选 Run、invocation、内容摘要/大小、数据范围和过期时间。
    /// `new()` 会把参数中的 RunId 包成 Some，但当前验证只拒绝 None，没有拒绝 Some(nil UUID)。
    pub output_ref: ExecutionOutputRef,
    /// 产生该输出的 Turn；分页时要求与调用者提供的当前 Turn 相同。
    pub turn_id: TurnId,
    /// 产生输出的 Capability 名称；分页时要求调用者传入完全相同的名称。
    pub capability_id: String,
    /// 输出产生时的数据范围摘要；须与嵌套引用的 `scope_digest` 相同，分页时还要与当前范围相同。
    pub source_scope_digest: String,
    /// 给模型/界面先行展示的短预览；本 DTO 只限制长度和摘要一致性，不检查其是否真是完整内容前缀。
    pub preview: String,
    /// 对 `preview` JSON 值生成的摘要，用于检测 DTO 内部预览被意外改动。
    pub preview_digest: String,
    /// 调用方声明预览是否截断；这里只保留标记，不从内容重新推导。
    pub preview_truncated: bool,
    /// 完整内容是二进制还是文本；文本分页要求整份内容合法 UTF-8 且分页边界位于字符边界。
    pub binary: bool,
    /// 调用方允许的最大单页字节数，范围为 1..=64 KiB。
    pub page_size: u32,
    /// 除自身以外全部 spill 字段的自摘要。
    pub spill_digest: String,
}

impl ToolOutputSpill {
    /// 组合完整内容引用、可展示预览和分页配置，并生成 spill 自摘要。
    ///
    /// 【调用者】预计由产出/存储工具完整输出的 adapter 构造；构造器不会自行创建该 adapter 的归档。
    /// 【输入】请求/Run/Turn/invocation 身份、Capability 与 source scope、内容摘要/字节数、预览、
    /// 过期时间和页长。完整内容 bytes 不在参数中，因此本函数不能验证摘要是否对应真实内容。
    /// 【输出】成功时返回带 `Some(run_id)` 的 spill；其他字段通过当前 DTO 校验。
    /// 【失败】嵌套引用或 spill 字段不合法时返回领域错误字符串；没有文件或 EventStore 副作用。
    /// ⚠ 当前校验允许传入 nil UUID 的 RunId：构造可成功，但之后 `page()` 会在生成单页时因 page
    /// DTO 拒绝 nil run_id 而失败。这里记录校验边界，不在注释任务中改变行为。
    /// 【设计原因】preview 与完整输出分开传输，较大的结果可以在初次响应中只带小预览。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        output_id: RequestId,
        run_id: RunId,
        turn_id: TurnId,
        invocation_id: InvocationId,
        capability_id: impl Into<String>,
        source_scope_digest: impl Into<String>,
        content_digest: impl Into<String>,
        size_bytes: u64,
        preview: impl Into<String>,
        preview_truncated: bool,
        binary: bool,
        expires_at_unix_ms: u64,
        page_size: u32,
    ) -> Result<Self, String> {
        let capability_id = capability_id.into();
        let source_scope_digest = source_scope_digest.into();
        let preview = preview.into();
        let output_ref = ExecutionOutputRef::new(
            output_id,
            Some(run_id),
            invocation_id,
            content_digest,
            size_bytes,
            source_scope_digest.clone(),
            expires_at_unix_ms,
        )?;
        let mut spill = Self {
            schema: TOOL_OUTPUT_SPILL_SCHEMA.to_owned(),
            version: TOOL_OUTPUT_VERSION,
            output_ref,
            turn_id,
            capability_id,
            source_scope_digest,
            preview_digest: json_digest(&json!({"preview": preview})),
            preview,
            preview_truncated,
            binary,
            page_size,
            spill_digest: String::new(),
        };
        spill.spill_digest = spill.digest();
        spill.validate()?;
        Ok(spill)
    }

    /// 检查 spill 自身及其嵌套完整输出引用的静态约束和摘要绑定。
    ///
    /// 它要求合法 schema/version、非 nil Turn、preview/page 大小在界限内、Capability 字段形状有效，
    /// 且 scope 与嵌套引用一致。此方法看不到完整内容，也不验证 preview 是真实内容前缀、不检查
    /// 当前身份/授权/过期时间或归档是否存在；这些责任属于受控 reader。
    /// ⚠ 对 Run 只检查 `Option` 为 `Some`，未调用 `as_uuid().is_nil()`；nil Run 会推迟到 `ToolOutputPage`
    /// 校验时才失败，形成 spill 可构造、却不可分页的状态。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != TOOL_OUTPUT_SPILL_SCHEMA
            || !self.version.is_compatible_with(&TOOL_OUTPUT_VERSION)
            || self.turn_id.as_uuid().is_nil()
            || self.preview.len() > MAX_TOOL_OUTPUT_PREVIEW_BYTES
            || self.page_size == 0
            || self.page_size as usize > MAX_TOOL_OUTPUT_PAGE_BYTES
        {
            return Err("tool_output_spill_header_invalid".to_owned());
        }
        self.output_ref.validate()?;
        required(&self.capability_id, "tool_output_capability", 256)?;
        digest(&self.source_scope_digest, "tool_output_source_scope_digest")?;
        digest(&self.preview_digest, "tool_output_preview_digest")?;
        digest(&self.spill_digest, "tool_output_spill_digest")?;
        if self.output_ref.run_id.is_none()
            || self.output_ref.invocation_id.as_uuid().is_nil()
            || self.output_ref.scope_digest != self.source_scope_digest
            || self.preview_digest != json_digest(&json!({"preview": self.preview}))
            || self.spill_digest != self.digest()
        {
            return Err("tool_output_spill_binding_invalid".to_owned());
        }
        Ok(())
    }

    /// 对 spill 的所有业务字段（除 `spill_digest` 自身）计算 canonical JSON SHA-256 指纹。
    ///
    /// 指纹帮助发现 DTO 内容改动，不是带密钥的签名，也不能单独作为读取许可。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "output_ref": self.output_ref,
            "turn_id": self.turn_id,
            "capability_id": self.capability_id,
            "source_scope_digest": self.source_scope_digest,
            "preview": self.preview,
            "preview_digest": self.preview_digest,
            "preview_truncated": self.preview_truncated,
            "binary": self.binary,
            "page_size": self.page_size,
        }))
    }

    /// 为当前 spill 和指定 byte offset 生成分页游标摘要。
    ///
    /// 此 helper 只计算值，不验证 offset 范围；实际 page 边界由 `page()` 检查。返回的 cursor 绑定
    /// output/content/turn/capability/source scope/offset，但不绑定 limit，所以同一 offset 可以用不同
    /// 的合法请求页长继续读取。
    pub fn cursor_for(&self, offset: u64) -> String {
        page_cursor(
            self.output_ref.output_id,
            &self.output_ref.content_digest,
            self.turn_id,
            &self.capability_id,
            &self.source_scope_digest,
            offset,
        )
    }

    /// 从调用方提供的完整内容中校验身份/摘要，再返回一个有界分页。
    ///
    /// 【调用者】应由已完成权限与存储读取检查的 adapter 调用；“trusted reader”是调用方前提，本方法
    /// 自身不查文件或 EventStore，也不决定用户是否有权读取。
    /// 【输入】完整 `content` bytes、当前 Run/Turn/Capability/source scope、byte offset、请求页长以及
    /// 可选 cursor。cursor 若提供必须匹配；当前实现允许不提供 cursor。
    /// 【核心流程】先校验 spill，再逐项匹配调用上下文；重算完整 content 的 SHA-256 和长度；检查
    /// offset/limit/cursor；文本模式下拒绝非法 UTF-8 或落在字符中间的页边界；最后构造 `ToolOutputPage`。
    /// 【失败】任一身份、摘要、长度、边界或编码不匹配时返回具体领域错误，不产生部分 page。
    /// 【设计原因】一次只复制最多 `page_size` 字节，避免大工具结果再次整体进入调用方响应。
    pub fn page(
        &self,
        content: &[u8],
        run_id: RunId,
        turn_id: TurnId,
        capability_id: &str,
        source_scope_digest: &str,
        offset: u64,
        limit: u32,
        cursor: Option<&str>,
    ) -> Result<ToolOutputPage, String> {
        self.validate()?;
        if self.output_ref.run_id != Some(run_id) {
            return Err("tool_output_spill_run_mismatch".to_owned());
        }
        if self.turn_id != turn_id {
            return Err("tool_output_spill_turn_mismatch".to_owned());
        }
        if self.capability_id != capability_id {
            return Err("tool_output_spill_capability_mismatch".to_owned());
        }
        if self.source_scope_digest != source_scope_digest {
            return Err("tool_output_spill_scope_mismatch".to_owned());
        }
        if format!("sha256:{}", journal_sha256(content)) != self.output_ref.content_digest
            || content.len() as u64 != self.output_ref.size_bytes
        {
            return Err("tool_output_spill_content_mismatch".to_owned());
        }
        let start = usize::try_from(offset).map_err(|_| "tool_output_page_offset_invalid")?;
        if start > content.len()
            || limit == 0
            || limit > self.page_size
            || limit as usize > MAX_TOOL_OUTPUT_PAGE_BYTES
        {
            return Err("tool_output_page_bounds_invalid".to_owned());
        }
        let expected_cursor = self.cursor_for(offset);
        if cursor.is_some_and(|provided| provided != expected_cursor) {
            return Err("tool_output_page_cursor_invalid".to_owned());
        }
        let end = start.saturating_add(limit as usize).min(content.len());
        if !self.binary {
            let text = std::str::from_utf8(content)
                .map_err(|_| "tool_output_text_encoding_invalid".to_owned())?;
            if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
                return Err("tool_output_page_utf8_boundary_invalid".to_owned());
            }
        }
        ToolOutputPage::new(
            self,
            offset,
            end as u64,
            content[start..end].to_vec(),
            end == content.len(),
        )
    }
}

/// 一次受限读取返回的字节页及其续读信息。
///
/// page 同时回带 spill/output/run/turn/capability/scope 身份，接收端可在拼接数据前确认各页属于同一
/// 个内容引用。这里的偏移以完整输出中的 UTF-8/二进制字节为单位，而不是字符数或页序号。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolOutputPage {
    /// 单页 DTO 的 schema 名称。
    pub schema: String,
    /// 单页格式版本。
    pub version: SchemaVersion,
    /// 生成该页的 spill 自摘要；接收方可将其与此前持有的 spill 对照。
    pub spill_digest: String,
    /// 完整输出记录 ID。
    pub output_id: RequestId,
    /// 完整输出所属 Run。
    pub run_id: RunId,
    /// 产生完整输出的 Turn。
    pub turn_id: TurnId,
    /// 产生输出的 Capability 名称。
    pub capability_id: String,
    /// 完整输出的 source scope 摘要。
    pub source_scope_digest: String,
    /// 当前页在完整内容中的起始字节偏移。
    pub offset: u64,
    /// 当前页结束后的字节偏移，采用不含该端点的区间 `[offset, next_offset)`。
    pub next_offset: u64,
    /// 本页字节；文本模式下整个 page 必须是合法 UTF-8。
    pub data: Vec<u8>,
    /// `true` 时允许任意二进制字节；否则只允许 UTF-8 文本。
    pub binary: bool,
    /// 完整输出的内容摘要，不是本页 `data` 的摘要。
    pub content_digest: String,
    /// 当前页起始 offset 的 cursor 指纹。
    pub cursor: String,
    /// 未读完时指向 `next_offset` 的 cursor；最后一页必须为 None。
    pub next_cursor: Option<String>,
    /// 当前页是否到达完整内容末尾，与 `next_cursor` 是否存在必须相反。
    pub complete: bool,
    /// 对本页其余字段计算的自摘要。
    pub page_digest: String,
}

impl ToolOutputPage {
    /// 从已经校验过的 spill/page 区间构造响应页并填入游标与自摘要。
    ///
    /// 该私有构造器只由 `ToolOutputSpill::page()` 调用；缺少 Run 绑定时拒绝生成响应。最后调用
    /// `validate()` 做 DTO 自检，但自检不替代接收方对已知 spill 身份的比对。
    fn new(
        spill: &ToolOutputSpill,
        offset: u64,
        next_offset: u64,
        data: Vec<u8>,
        complete: bool,
    ) -> Result<Self, String> {
        let cursor = spill.cursor_for(offset);
        let next_cursor = (!complete).then(|| spill.cursor_for(next_offset));
        let mut page = Self {
            schema: TOOL_OUTPUT_PAGE_SCHEMA.to_owned(),
            version: TOOL_OUTPUT_VERSION,
            spill_digest: spill.spill_digest.clone(),
            output_id: spill.output_ref.output_id,
            run_id: spill
                .output_ref
                .run_id
                .ok_or_else(|| "tool_output_page_run_binding_missing".to_owned())?,
            turn_id: spill.turn_id,
            capability_id: spill.capability_id.clone(),
            source_scope_digest: spill.source_scope_digest.clone(),
            offset,
            next_offset,
            data,
            binary: spill.binary,
            content_digest: spill.output_ref.content_digest.clone(),
            cursor,
            next_cursor,
            complete,
            page_digest: String::new(),
        };
        page.page_digest = page.digest();
        page.validate()?;
        Ok(page)
    }

    /// 校验响应页的尺寸、身份字段格式、offset 连续性、cursor 绑定、编码和自摘要。
    ///
    /// 由于本 DTO 不含 spill 对象或完整内容，本方法不能证明 page.data 真的是该完整内容在此 offset
    /// 的切片；它只能验证字段间的自洽关系。接收方仍应将 `spill_digest` 等身份字段与自己持有的
    /// spill/请求上下文比对。错误只返回结构化程度有限的字符串码，不触发重试或修改任何状态。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != TOOL_OUTPUT_PAGE_SCHEMA
            || !self.version.is_compatible_with(&TOOL_OUTPUT_VERSION)
            || self.output_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self.turn_id.as_uuid().is_nil()
            || self.data.len() > MAX_TOOL_OUTPUT_PAGE_BYTES
            || self.next_offset != self.offset.saturating_add(self.data.len() as u64)
            || self.complete == self.next_cursor.is_some()
        {
            return Err("tool_output_page_invalid".to_owned());
        }
        required(&self.capability_id, "tool_output_page_capability", 256)?;
        digest(&self.spill_digest, "tool_output_page_spill_digest")?;
        digest(&self.source_scope_digest, "tool_output_page_scope_digest")?;
        digest(&self.content_digest, "tool_output_page_content_digest")?;
        digest(&self.cursor, "tool_output_page_cursor")?;
        if let Some(next_cursor) = &self.next_cursor {
            digest(next_cursor, "tool_output_page_next_cursor")?;
        }
        digest(&self.page_digest, "tool_output_page_digest")?;
        if self.cursor
            != page_cursor(
                self.output_id,
                &self.content_digest,
                self.turn_id,
                &self.capability_id,
                &self.source_scope_digest,
                self.offset,
            )
        {
            return Err("tool_output_page_cursor_binding_invalid".to_owned());
        }
        if self.next_cursor.as_deref().is_some_and(|cursor| {
            cursor
                != page_cursor(
                    self.output_id,
                    &self.content_digest,
                    self.turn_id,
                    &self.capability_id,
                    &self.source_scope_digest,
                    self.next_offset,
                )
        }) {
            return Err("tool_output_page_next_cursor_binding_invalid".to_owned());
        }
        if !self.binary && std::str::from_utf8(&self.data).is_err() {
            return Err("tool_output_page_text_encoding_invalid".to_owned());
        }
        if self.page_digest != self.digest() {
            return Err("tool_output_page_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对本页除 `page_digest` 外的全部字段计算 canonical JSON SHA-256 指纹。
    ///
    /// 这是内容一致性检查，不是签名或不可伪造证明。
    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "spill_digest": self.spill_digest,
            "output_id": self.output_id,
            "run_id": self.run_id,
            "turn_id": self.turn_id,
            "capability_id": self.capability_id,
            "source_scope_digest": self.source_scope_digest,
            "offset": self.offset,
            "next_offset": self.next_offset,
            "data": self.data,
            "binary": self.binary,
            "content_digest": self.content_digest,
            "cursor": self.cursor,
            "next_cursor": self.next_cursor,
            "complete": self.complete,
        }))
    }
}
