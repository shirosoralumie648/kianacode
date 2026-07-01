use crate::tool::*;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use tokio::fs;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

const PDF_INLINE_PAGE_THRESHOLD: usize = 10;
const PDF_MAX_PAGES_PER_READ: usize = 20;
const PDF_COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Deserialize)]
struct FileReadInput {
    file_path: String,
    #[serde(default = "default_offset")]
    offset: usize,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    pages: Option<String>,
}

fn default_offset() -> usize {
    1
}

#[derive(Debug, Serialize)]
struct FileReadOutput {
    #[serde(rename = "type")]
    output_type: String,
    file: FileContent,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum FileContent {
    Text {
        file_path: String,
        content: String,
        num_lines: usize,
        start_line: usize,
        total_lines: usize,
    },
    Image {
        base64: String,
        #[serde(rename = "type")]
        media_type: String,
        original_size: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PdfPageRange {
    first_page: usize,
    last_page: Option<usize>,
}

pub struct FileReadTool;

impl FileReadTool {
    pub fn new() -> Self {
        Self
    }

    fn text_content(
        path: &Path,
        content: String,
        offset: usize,
        limit: Option<usize>,
    ) -> FileContent {
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let start_idx = offset.saturating_sub(1).min(total_lines);
        let end_idx = match limit {
            Some(l) => (start_idx + l).min(total_lines),
            None => total_lines,
        };

        let selected_lines = &lines[start_idx..end_idx];
        let result_content = selected_lines.join("\n");
        let num_lines = selected_lines.len();

        FileContent::Text {
            file_path: path.display().to_string(),
            content: result_content,
            num_lines,
            start_line: offset,
            total_lines,
        }
    }

    async fn read_text_file(
        &self,
        path: &Path,
        offset: usize,
        limit: Option<usize>,
    ) -> ToolResult<FileContent> {
        let content = fs::read_to_string(path).await?;
        Ok(Self::text_content(path, content, offset, limit))
    }

    async fn read_image_file(&self, path: &Path) -> ToolResult<FileContent> {
        let bytes = fs::read(path).await?;
        use base64::Engine;
        let base64 = base64::engine::general_purpose::STANDARD.encode(&bytes);

        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("png");

        let media_type = match ext {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "gif" => "image/gif",
            "webp" => "image/webp",
            _ => "image/png",
        };

        Ok(FileContent::Image {
            base64,
            media_type: media_type.to_string(),
            original_size: bytes.len(),
        })
    }

    async fn read_pdf_file(
        &self,
        path: &Path,
        pages: Option<&str>,
        offset: usize,
        limit: Option<usize>,
    ) -> ToolResult<FileContent> {
        let page_count = self.pdf_page_count(path).await?;
        let page_range = pages.map(parse_pdf_page_range).transpose()?.flatten();

        if page_range.is_none() && page_count > PDF_INLINE_PAGE_THRESHOLD {
            return Err(ToolError::Other(format!(
                "This PDF has {page_count} pages, which is too many to read at once. Use the pages parameter to read a specific range (for example, pages: \"1-5\"). Maximum {PDF_MAX_PAGES_PER_READ} pages per request."
            )));
        }

        if let Some(range) = page_range {
            if range.first_page > page_count {
                return Err(ToolError::Other(format!(
                    "Page range starts at {}, but the PDF only has {page_count} pages.",
                    range.first_page
                )));
            }
        }

        let content = self.extract_pdf_text(path, page_range).await?;
        let content = if content.trim().is_empty() {
            "No extractable text was found in this PDF page range. The PDF may be scanned or image-only.".to_string()
        } else {
            content
        };

        Ok(Self::text_content(path, content, offset, limit))
    }

    async fn pdf_page_count(&self, path: &Path) -> ToolResult<usize> {
        let output = self
            .run_poppler_command("pdfinfo", |command| {
                command.arg(path);
            })
            .await?;

        if !output.status.success() {
            return Err(pdf_command_error("pdfinfo", &output.stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let pages = stdout
            .lines()
            .find_map(|line| line.strip_prefix("Pages:"))
            .and_then(|value| value.trim().parse::<usize>().ok());

        pages.ok_or_else(|| {
            ToolError::Other("pdfinfo did not report a page count for this PDF.".to_string())
        })
    }

    async fn extract_pdf_text(
        &self,
        path: &Path,
        pages: Option<PdfPageRange>,
    ) -> ToolResult<String> {
        let output = self
            .run_poppler_command("pdftotext", |command| {
                command.arg("-layout").arg("-enc").arg("UTF-8");
                if let Some(range) = pages {
                    command.arg("-f").arg(range.first_page.to_string());
                    if let Some(last_page) = range.last_page {
                        command.arg("-l").arg(last_page.to_string());
                    }
                }
                command.arg(path).arg("-");
            })
            .await?;

        if !output.status.success() {
            return Err(pdf_command_error("pdftotext", &output.stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    async fn run_poppler_command(
        &self,
        program: &str,
        configure: impl FnOnce(&mut Command),
    ) -> ToolResult<std::process::Output> {
        let mut command = Command::new(program);
        command.kill_on_drop(true);
        configure(&mut command);

        match timeout(PDF_COMMAND_TIMEOUT, command.output()).await {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(err)) if err.kind() == std::io::ErrorKind::NotFound => Err(ToolError::Other(
                format!("{program} is not installed. Install poppler-utils to enable PDF reading."),
            )),
            Ok(Err(err)) => Err(ToolError::IoError(err)),
            Err(_) => Err(ToolError::Other(format!(
                "{program} timed out after {} seconds while reading the PDF.",
                PDF_COMMAND_TIMEOUT.as_secs()
            ))),
        }
    }

    fn is_image_file(path: &Path) -> bool {
        matches!(
            path.extension()
                .and_then(|s| s.to_str())
                .map(|ext| ext.to_ascii_lowercase())
                .as_deref(),
            Some("png" | "jpg" | "jpeg" | "gif" | "webp")
        )
    }

    fn is_pdf_file(path: &Path) -> bool {
        matches!(
            path.extension()
                .and_then(|s| s.to_str())
                .map(|ext| ext.to_ascii_lowercase())
                .as_deref(),
            Some("pdf")
        )
    }
}

fn parse_pdf_page_range(pages: &str) -> ToolResult<Option<PdfPageRange>> {
    let trimmed = pages.trim();
    if trimmed.is_empty() {
        return Err(ToolError::ValidationError(
            "Invalid pages parameter: use formats like \"1-5\" or \"3\". Pages are 1-indexed."
                .to_string(),
        ));
    }

    if let Some(first) = trimmed.strip_suffix('-') {
        let first_page = parse_pdf_page_number(first)?;
        return Ok(Some(PdfPageRange {
            first_page,
            last_page: None,
        }));
    }

    if let Some((first, last)) = trimmed.split_once('-') {
        let first_page = parse_pdf_page_number(first)?;
        let last_page = parse_pdf_page_number(last)?;
        if last_page < first_page {
            return Err(ToolError::ValidationError(format!(
                "Invalid pages parameter: \"{pages}\". Page ranges must be increasing."
            )));
        }
        return Ok(Some(PdfPageRange {
            first_page,
            last_page: Some(last_page),
        }));
    }

    let page = parse_pdf_page_number(trimmed)?;
    Ok(Some(PdfPageRange {
        first_page: page,
        last_page: Some(page),
    }))
}

fn parse_pdf_page_number(value: &str) -> ToolResult<usize> {
    let page = value.trim().parse::<usize>().map_err(|_| {
        ToolError::ValidationError(
            "Invalid pages parameter: use numeric, 1-indexed page numbers.".to_string(),
        )
    })?;

    if page == 0 {
        return Err(ToolError::ValidationError(
            "Invalid pages parameter: page numbers are 1-indexed.".to_string(),
        ));
    }

    Ok(page)
}

fn validate_pdf_page_range(pages: &str) -> ValidationResult {
    let range = match parse_pdf_page_range(pages) {
        Ok(Some(range)) => range,
        Ok(None) => return ValidationResult::ok(),
        Err(err) => return ValidationResult::err(err.to_string(), 7),
    };

    let Some(last_page) = range.last_page else {
        return ValidationResult::err(
            format!(
                "Page range \"{pages}\" is open-ended. Use a range of at most {PDF_MAX_PAGES_PER_READ} pages."
            ),
            8,
        );
    };

    let range_size = last_page - range.first_page + 1;
    if range_size > PDF_MAX_PAGES_PER_READ {
        return ValidationResult::err(
            format!(
                "Page range \"{pages}\" exceeds maximum of {PDF_MAX_PAGES_PER_READ} pages per request. Please use a smaller range."
            ),
            8,
        );
    }

    ValidationResult::ok()
}

fn pdf_command_error(program: &str, stderr: &[u8]) -> ToolError {
    let stderr = String::from_utf8_lossy(stderr);
    let message = stderr.trim();
    let lower = message.to_ascii_lowercase();

    if lower.contains("password") || lower.contains("encrypted") {
        return ToolError::Other(
            "PDF is password-protected. Please provide an unprotected version.".to_string(),
        );
    }

    if lower.contains("damaged")
        || lower.contains("corrupt")
        || lower.contains("invalid")
        || lower.contains("syntax error")
    {
        return ToolError::Other("PDF file is corrupted or invalid.".to_string());
    }

    ToolError::Other(if message.is_empty() {
        format!("{program} failed while reading the PDF.")
    } else {
        format!("{program} failed while reading the PDF: {message}")
    })
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "Read"
    }

    fn description(&self) -> &str {
        "Reads a file from the local filesystem"
    }

    fn search_hint(&self) -> Option<&str> {
        Some("read files, images, PDFs, notebooks")
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "The absolute path to the file to read"
                },
                "offset": {
                    "type": "integer",
                    "description": "The line number to start reading from",
                    "default": 1
                },
                "limit": {
                    "type": "integer",
                    "description": "The number of lines to read"
                },
                "pages": {
                    "type": "string",
                    "description": format!("Page range for PDF files (e.g., \"1-5\", \"3\"). Maximum {PDF_MAX_PAGES_PER_READ} pages per request.")
                }
            },
            "required": ["file_path"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "type": { "type": "string", "enum": ["text", "image"] },
                "file": { "type": "object" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, context: &ToolContext) -> ValidationResult {
        let input: FileReadInput = match serde_json::from_value(input.clone()) {
            Ok(i) => i,
            Err(e) => return ValidationResult::err(format!("Invalid input: {}", e), 1),
        };

        let path = match context.resolve_access_path(&input.file_path) {
            Ok(path) => path,
            Err(error) => return ValidationResult::err(error, 9),
        };
        if let Some(pages) = input
            .pages
            .as_deref()
            .map(str::trim)
            .filter(|pages| !pages.is_empty())
        {
            let validation = validate_pdf_page_range(pages);
            if !validation.result {
                return validation;
            }

            if !Self::is_pdf_file(&path) {
                return ValidationResult::err(
                    "The pages parameter is only supported for PDF files.".to_string(),
                    3,
                );
            }
        }

        if !path.exists() {
            return ValidationResult::err(format!("File does not exist: {}", input.file_path), 2);
        }

        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: FileReadInput = serde_json::from_value(input.clone())?;
        let path = context
            .resolve_access_path(&input.file_path)
            .map_err(ToolError::PermissionDenied)?;

        let pages = input
            .pages
            .as_deref()
            .map(str::trim)
            .filter(|pages| !pages.is_empty());

        let file_content = if Self::is_image_file(&path) {
            self.read_image_file(&path).await?
        } else if Self::is_pdf_file(&path) {
            self.read_pdf_file(&path, pages, input.offset, input.limit)
                .await?
        } else {
            self.read_text_file(&path, input.offset, input.limit)
                .await?
        };

        let output_type = match &file_content {
            FileContent::Text { .. } => "text",
            FileContent::Image { .. } => "image",
        };

        let output = FileReadOutput {
            output_type: output_type.to_string(),
            file: file_content,
        };

        // Update read file state
        if let FileContent::Text { content, .. } = &output.file {
            context.read_file_state.insert(
                input.file_path.clone(),
                FileState {
                    content: content.clone(),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as i64,
                    offset: Some(input.offset),
                    limit: input.limit,
                },
            );
        }

        Ok(ToolOutput {
            data: serde_json::to_value(output)?,
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let Some(output_type) = output.data.get("type").and_then(Value::as_str) else {
            return json!({
                "tool_use_id": tool_use_id,
                "type": "tool_result",
                "content": output.data
            });
        };
        let file = output.data.get("file").unwrap_or(&Value::Null);

        match output_type {
            "text" => {
                let content = file.get("content").and_then(Value::as_str).unwrap_or("");
                let start_line =
                    file.get("start_line").and_then(Value::as_u64).unwrap_or(1) as usize;
                let total_lines = file.get("total_lines").and_then(Value::as_u64).unwrap_or(0);

                let content = if content.is_empty() {
                    if total_lines == 0 {
                        "<system-reminder>Warning: the file exists but the contents are empty.</system-reminder>".to_string()
                    } else {
                        format!(
                            "<system-reminder>Warning: the file exists but is shorter than the provided offset ({start_line}). The file has {total_lines} lines.</system-reminder>"
                        )
                    }
                } else {
                    format_file_lines(content, start_line)
                };

                json!({
                    "tool_use_id": tool_use_id,
                    "type": "tool_result",
                    "content": content
                })
            }
            "image" => json!({
                "tool_use_id": tool_use_id,
                "type": "tool_result",
                "content": [{
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "data": file.get("base64").and_then(Value::as_str).unwrap_or(""),
                        "media_type": file.get("type").and_then(Value::as_str).unwrap_or("image/png")
                    }
                }]
            }),
            _ => json!({
                "tool_use_id": tool_use_id,
                "type": "tool_result",
                "content": output.data
            }),
        }
    }
}

fn format_file_lines(content: &str, start_line: usize) -> String {
    content
        .split('\n')
        .enumerate()
        .map(|(index, line)| format!("{}\t{}", start_line + index, line))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ACCESS_ROOTS_APP_STATE_KEY;
    use std::collections::HashMap;
    use std::fmt::Write;

    fn test_context() -> ToolContext {
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd: ".".to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        }
    }

    fn write_temp_file(extension: &str, content: &[u8]) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("kiana-read-{}.{}", uuid::Uuid::new_v4(), extension));
        std::fs::write(&path, content).unwrap();
        path
    }

    fn poppler_available() -> bool {
        std::process::Command::new("pdfinfo")
            .arg("-v")
            .output()
            .is_ok()
            && std::process::Command::new("pdftotext")
                .arg("-v")
                .output()
                .is_ok()
    }

    fn minimal_two_page_pdf() -> Vec<u8> {
        let streams = [
            "BT /F1 24 Tf 72 720 Td (First page text) Tj ET",
            "BT /F1 24 Tf 72 720 Td (Second page text) Tj ET",
        ];
        let objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 7 0 R >> >> /Contents 4 0 R >>".to_string(),
            format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                streams[0].len(),
                streams[0]
            ),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 7 0 R >> >> /Contents 6 0 R >>".to_string(),
            format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                streams[1].len(),
                streams[1]
            ),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];

        let mut pdf = String::from("%PDF-1.4\n");
        let mut offsets = vec![0usize];
        for (idx, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            let _ = writeln!(pdf, "{} 0 obj", idx + 1);
            let _ = writeln!(pdf, "{object}");
            let _ = writeln!(pdf, "endobj");
        }

        let xref_start = pdf.len();
        let _ = writeln!(pdf, "xref");
        let _ = writeln!(pdf, "0 {}", offsets.len());
        let _ = writeln!(pdf, "0000000000 65535 f ");
        for offset in offsets.iter().skip(1) {
            let _ = writeln!(pdf, "{offset:010} 00000 n ");
        }
        let _ = writeln!(pdf, "trailer");
        let _ = writeln!(pdf, "<< /Size {} /Root 1 0 R >>", offsets.len());
        let _ = writeln!(pdf, "startxref");
        let _ = writeln!(pdf, "{xref_start}");
        let _ = writeln!(pdf, "%%EOF");

        pdf.into_bytes()
    }

    #[test]
    fn parse_pdf_page_ranges() {
        assert_eq!(
            parse_pdf_page_range("3").unwrap(),
            Some(PdfPageRange {
                first_page: 3,
                last_page: Some(3)
            })
        );
        assert_eq!(
            parse_pdf_page_range(" 1-5 ").unwrap(),
            Some(PdfPageRange {
                first_page: 1,
                last_page: Some(5)
            })
        );
        assert_eq!(
            parse_pdf_page_range("4-").unwrap(),
            Some(PdfPageRange {
                first_page: 4,
                last_page: None
            })
        );
        assert!(parse_pdf_page_range("0").is_err());
        assert!(parse_pdf_page_range("5-1").is_err());
        assert!(parse_pdf_page_range("abc").is_err());
    }

    #[tokio::test]
    async fn validate_rejects_invalid_page_ranges() {
        let path = write_temp_file("pdf", b"not a real pdf");

        let result = FileReadTool
            .validate_input(
                &json!({
                    "file_path": path.display().to_string(),
                    "pages": "1-25"
                }),
                &test_context(),
            )
            .await;

        assert!(!result.result);
        assert!(result.message.unwrap().contains("exceeds maximum"));

        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn validate_rejects_pages_for_non_pdf_files() {
        let path = write_temp_file("txt", b"hello");

        let result = FileReadTool
            .validate_input(
                &json!({
                    "file_path": path.display().to_string(),
                    "pages": "1-2"
                }),
                &test_context(),
            )
            .await;

        assert!(!result.result);
        assert!(result
            .message
            .unwrap()
            .contains("only supported for PDF files"));

        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn read_text_file_offset_beyond_eof_is_empty() {
        let path = write_temp_file("txt", b"one\ntwo\n");
        let mut context = test_context();

        let output = FileReadTool
            .call(
                &json!({
                    "file_path": path.display().to_string(),
                    "offset": 10
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["file"]["content"], "");
        assert_eq!(output.data["file"]["num_lines"], 0);
        assert_eq!(output.data["file"]["total_lines"], 2);

        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn text_read_maps_to_model_facing_line_numbered_content() {
        let path = write_temp_file("txt", b"one\ntwo\nthree\n");
        let mut context = test_context();

        let output = FileReadTool
            .call(
                &json!({
                    "file_path": path.display().to_string(),
                    "offset": 2,
                    "limit": 2
                }),
                &mut context,
            )
            .await
            .unwrap();

        let result = FileReadTool.map_to_api_result(&output, "toolu_read");

        assert_eq!(result["type"], "tool_result");
        assert_eq!(result["tool_use_id"], "toolu_read");
        assert_eq!(result["content"], "2\ttwo\n3\tthree");
        assert!(result["content"]["file"].is_null());

        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn access_roots_allow_added_directory_and_reject_other_paths() {
        let root = std::env::temp_dir().join(format!("kiana-read-root-{}", uuid::Uuid::new_v4()));
        let extra = std::env::temp_dir().join(format!("kiana-read-extra-{}", uuid::Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("kiana-read-outside-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&extra).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let extra_file = extra.join("allowed.txt");
        let outside_file = outside.join("blocked.txt");
        std::fs::write(&extra_file, "allowed").unwrap();
        std::fs::write(&outside_file, "blocked").unwrap();

        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: root.to_string_lossy().to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::from([(
                ACCESS_ROOTS_APP_STATE_KEY.to_string(),
                json!([extra.to_string_lossy()]),
            )]),
            abort_signal: abort_rx,
        };

        let output = FileReadTool
            .call(
                &json!({ "file_path": extra_file.to_string_lossy() }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(output.data["file"]["content"], "allowed");

        let validation = FileReadTool
            .validate_input(
                &json!({ "file_path": outside_file.to_string_lossy() }),
                &context,
            )
            .await;
        assert!(!validation.result);
        assert!(validation
            .message
            .unwrap()
            .contains("outside the allowed access roots"));

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(extra);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[tokio::test]
    async fn read_pdf_page_range_extracts_selected_text() {
        if !poppler_available() {
            eprintln!("skipping PDF extraction test because poppler-utils is unavailable");
            return;
        }

        let path = write_temp_file("pdf", &minimal_two_page_pdf());
        let mut context = test_context();

        let output = FileReadTool
            .call(
                &json!({
                    "file_path": path.display().to_string(),
                    "pages": "2"
                }),
                &mut context,
            )
            .await
            .unwrap();

        let content = output.data["file"]["content"].as_str().unwrap();
        assert!(content.contains("Second page text"));
        assert!(!content.contains("First page text"));
        assert!(context
            .read_file_state
            .get(&path.display().to_string())
            .unwrap()
            .content
            .contains("Second page text"));

        let _ = std::fs::remove_file(path);
    }
}
