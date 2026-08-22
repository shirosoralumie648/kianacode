use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

impl LogLevel {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "TRACE" | "TRC" => Some(Self::Trace),
            "DEBUG" | "DBG" => Some(Self::Debug),
            "INFO" | "INF" => Some(Self::Info),
            "WARN" | "WRN" | "WARNING" => Some(Self::Warn),
            "ERROR" | "ERR" => Some(Self::Error),
            "FATAL" | "FTL" | "CRITICAL" | "CRIT" => Some(Self::Fatal),
            _ => None,
        }
    }

    pub fn color(&self) -> Color {
        match self {
            Self::Trace => Color::DarkGray,
            Self::Debug => Color::Gray,
            Self::Info => Color::Green,
            Self::Warn => Color::Yellow,
            Self::Error => Color::Red,
            Self::Fatal => Color::Magenta,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
            Self::Fatal => "FATAL",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: Option<String>,
    pub level: Option<LogLevel>,
    pub message: String,
    pub raw: String,
}

pub struct LogParser;

impl LogParser {
    /// 解析日志行，尝试提取时间戳、级别和消息
    /// 支持常见格式：
    /// - [2026-08-11 10:30:45] INFO: message
    /// - 2026-08-11T10:30:45Z ERROR message
    /// - INFO: message
    pub fn parse(line: &str) -> LogEntry {
        let mut timestamp = None;
        let mut level = None;
        let message = line.to_string();

        // 尝试提取时间戳（简化版本，匹配括号内或开头的时间格式）
        if let Some(ts_start) = line.find('[') {
            if let Some(ts_end) = line[ts_start..].find(']') {
                let ts = &line[ts_start + 1..ts_start + ts_end];
                timestamp = Some(ts.to_string());
            }
        } else if line.len() > 19 {
            // 尝试匹配开头的 ISO 格式时间戳
            let potential_ts = &line[..19.min(line.len())];
            if potential_ts.contains('-') && potential_ts.contains(':') {
                timestamp = Some(potential_ts.to_string());
            }
        }

        // 尝试提取日志级别
        for word in line.split_whitespace() {
            let cleaned = word.trim_matches(|c: char| !c.is_alphabetic());
            if let Some(lvl) = LogLevel::from_str(cleaned) {
                level = Some(lvl);
                break;
            }
        }

        LogEntry {
            timestamp,
            level,
            message,
            raw: line.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_with_level() {
        let entry = LogParser::parse("INFO: Hello world");
        assert_eq!(entry.level, Some(LogLevel::Info));
        assert!(entry.message.contains("Hello world"));
    }

    #[test]
    fn test_parse_error_level() {
        let entry = LogParser::parse("ERROR: Something went wrong");
        assert_eq!(entry.level, Some(LogLevel::Error));
    }

    #[test]
    fn test_parse_with_timestamp() {
        let entry = LogParser::parse("[2026-08-11 10:30:45] INFO: Starting");
        assert_eq!(entry.level, Some(LogLevel::Info));
        assert!(entry.timestamp.is_some());
    }

    #[test]
    fn test_parse_plain_text() {
        let entry = LogParser::parse("Just a plain message");
        assert!(entry.level.is_none());
        assert_eq!(entry.message, "Just a plain message");
    }

    #[test]
    fn test_level_colors() {
        assert_eq!(LogLevel::Error.color(), Color::Red);
        assert_eq!(LogLevel::Warn.color(), Color::Yellow);
        assert_eq!(LogLevel::Info.color(), Color::Green);
        assert_eq!(LogLevel::Debug.color(), Color::Gray);
    }
}
