//! Typed, bounded spill references for tool output.
//!
//! H15 already owns the daemon read path. This module is the domain contract that makes a
//! spill's preview and pages explicitly belong to one run, turn, capability and source scope;
//! the reference itself never authorizes a read or stores the complete bytes.

use crate::{
    journal_sha256, json_digest, ExecutionOutputRef, InvocationId, RequestId, RunId, SchemaVersion,
    TurnId,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const TOOL_OUTPUT_SPILL_SCHEMA: &str = "kiana.tool-output-spill.v1";
pub const TOOL_OUTPUT_PAGE_SCHEMA: &str = "kiana.tool-output-page.v1";
pub const TOOL_OUTPUT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_TOOL_OUTPUT_PREVIEW_BYTES: usize = 16 * 1024;
pub const MAX_TOOL_OUTPUT_PAGE_BYTES: usize = 64 * 1024;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        Err(format!("{field}_invalid"))
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolOutputSpill {
    pub schema: String,
    pub version: SchemaVersion,
    pub output_ref: ExecutionOutputRef,
    pub turn_id: TurnId,
    pub capability_id: String,
    pub source_scope_digest: String,
    pub preview: String,
    pub preview_digest: String,
    pub preview_truncated: bool,
    pub binary: bool,
    pub page_size: u32,
    pub spill_digest: String,
}

impl ToolOutputSpill {
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

    pub fn cursor_for(&self, offset: u64) -> String {
        page_cursor(
            self.output_ref.output_id,
            &self.output_ref.content_hash,
            self.turn_id,
            &self.capability_id,
            &self.source_scope_digest,
            offset,
        )
    }

    /// Validate identity and content before returning one bounded page from a trusted reader.
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
        if format!("sha256:{}", journal_sha256(content)) != self.output_ref.content_hash
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolOutputPage {
    pub schema: String,
    pub version: SchemaVersion,
    pub spill_digest: String,
    pub output_id: RequestId,
    pub run_id: RunId,
    pub turn_id: TurnId,
    pub capability_id: String,
    pub source_scope_digest: String,
    pub offset: u64,
    pub next_offset: u64,
    pub data: Vec<u8>,
    pub binary: bool,
    pub content_digest: String,
    pub cursor: String,
    pub next_cursor: Option<String>,
    pub complete: bool,
    pub page_digest: String,
}

impl ToolOutputPage {
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
            content_digest: spill.output_ref.content_hash.clone(),
            cursor,
            next_cursor,
            complete,
            page_digest: String::new(),
        };
        page.page_digest = page.digest();
        page.validate()?;
        Ok(page)
    }

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
