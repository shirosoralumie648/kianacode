// Binary file previewer with hex dump

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// Binary file previewer that displays hex dumps
pub struct BinaryPreviewer {
    /// Number of bytes to show per line (typically 16)
    pub bytes_per_line: usize,
    /// Maximum bytes to read from file
    pub max_bytes: usize,
}

impl Default for BinaryPreviewer {
    fn default() -> Self {
        Self {
            bytes_per_line: 16,
            max_bytes: 16 * 1024, // 16KB
        }
    }
}

impl BinaryPreviewer {
    /// Create a new binary previewer with custom settings
    pub fn new(bytes_per_line: usize, max_bytes: usize) -> Self {
        Self {
            bytes_per_line,
            max_bytes,
        }
    }

    /// Preview a binary file as hex dump
    pub fn preview(&self, path: &Path) -> io::Result<Vec<Line<'static>>> {
        let mut file = File::open(path)?;
        let mut buffer = vec![0u8; self.max_bytes];
        let bytes_read = file.read(&mut buffer)?;
        buffer.truncate(bytes_read);

        let mut lines = Vec::new();

        // Add header
        lines.push(Line::from(vec![Span::styled(
            format!("Binary file: {} ({} bytes)", path.display(), bytes_read),
            Style::default().fg(Color::Cyan),
        )]));
        lines.push(Line::from(""));

        // Format hex dump
        for (i, chunk) in buffer.chunks(self.bytes_per_line).enumerate() {
            let offset = i * self.bytes_per_line;
            lines.push(self.format_hex_line(offset, chunk));
        }

        // Add truncation notice if needed
        let file_size = file.metadata()?.len() as usize;
        if bytes_read < file_size {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                format!("... truncated ({} / {} bytes shown)", bytes_read, file_size),
                Style::default().fg(Color::Yellow),
            )]));
        }

        Ok(lines)
    }

    /// Format a single line of hex dump
    /// Format: ADDRESS | HEX BYTES | ASCII
    fn format_hex_line(&self, offset: usize, bytes: &[u8]) -> Line<'static> {
        let mut spans = Vec::new();

        // Address (8 hex digits)
        spans.push(Span::styled(
            format!("{:08x}", offset),
            Style::default().fg(Color::DarkGray),
        ));
        spans.push(Span::raw("  "));

        // Hex bytes (2 digits each, space separated)
        let mut hex_parts = Vec::new();
        for (i, &byte) in bytes.iter().enumerate() {
            // Add extra space every 8 bytes for readability
            if i == 8 {
                hex_parts.push(format!(" {:02x}", byte));
            } else {
                hex_parts.push(format!("{:02x}", byte));
            }
        }

        // Pad with spaces if line is not full
        let padding_needed = self.bytes_per_line - bytes.len();
        for i in 0..padding_needed {
            if bytes.len() + i == 8 {
                hex_parts.push("   ".to_string());
            } else {
                hex_parts.push("  ".to_string());
            }
        }

        let hex_string = hex_parts.join(" ");
        spans.push(Span::styled(hex_string, Style::default().fg(Color::Green)));
        spans.push(Span::raw("  "));

        // ASCII representation
        spans.push(Span::styled("|", Style::default().fg(Color::DarkGray)));
        let ascii_string: String = bytes
            .iter()
            .map(|&b| {
                if (32..127).contains(&b) {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        spans.push(Span::styled(ascii_string, Style::default().fg(Color::Gray)));
        spans.push(Span::styled("|", Style::default().fg(Color::DarkGray)));

        Line::from(spans)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_format_hex_line_full() {
        let previewer = BinaryPreviewer::default();
        let bytes = b"Hello, World!!!!";
        let line = previewer.format_hex_line(0, bytes);

        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("00000000"));
        assert!(text.contains("Hello, World!!!!"));
    }

    #[test]
    fn test_format_hex_line_partial() {
        let previewer = BinaryPreviewer::default();
        let bytes = b"Short";
        let line = previewer.format_hex_line(0, bytes);

        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("00000000"));
        assert!(text.contains("Short"));
    }

    #[test]
    fn test_format_hex_line_non_printable() {
        let previewer = BinaryPreviewer::default();
        let bytes = &[0x00, 0x01, 0x02, 0x1F, 0x7F, 0xFF];
        let line = previewer.format_hex_line(0, bytes);

        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        // Non-printable should be displayed as dots
        assert!(text.contains("."));
    }

    #[test]
    fn test_preview_small_file() -> io::Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(b"Test binary content")?;

        let previewer = BinaryPreviewer::default();
        let lines = previewer.preview(temp_file.path())?;

        assert!(!lines.is_empty());
        // Should have header, blank line, and at least one data line
        assert!(lines.len() >= 3);

        Ok(())
    }

    #[test]
    fn test_preview_respects_max_bytes() -> io::Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        let large_content = vec![0u8; 1024]; // 1KB
        temp_file.write_all(&large_content)?;

        let previewer = BinaryPreviewer::new(16, 256); // Only read 256 bytes
        let lines = previewer.preview(temp_file.path())?;

        // Check for truncation message
        let last_line_text: String = lines
            .last()
            .unwrap()
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(last_line_text.contains("truncated"));

        Ok(())
    }

    #[test]
    fn test_bytes_per_line_custom() {
        let previewer = BinaryPreviewer::new(8, 1024); // 8 bytes per line
        let bytes = b"12345678";
        let line = previewer.format_hex_line(0, bytes);

        // Verify it formats correctly with 8 bytes
        assert!(!line.spans.is_empty());
    }
}
