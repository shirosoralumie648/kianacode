use super::{LogEntry, LogLevel};
use std::collections::VecDeque;

#[derive(Default, Clone)]
pub struct LogStats {
    pub total: usize,
    pub trace: usize,
    pub debug: usize,
    pub info: usize,
    pub warn: usize,
    pub error: usize,
    pub fatal: usize,
}

pub struct LogBuffer {
    entries: VecDeque<LogEntry>,
    max_capacity: usize,
    stats: LogStats,
}

impl LogBuffer {
    pub fn new(max_capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(max_capacity),
            max_capacity,
            stats: LogStats::default(),
        }
    }

    pub fn push(&mut self, entry: LogEntry) {
        // 更新统计
        self.stats.total += 1;
        if let Some(level) = entry.level {
            match level {
                LogLevel::Trace => self.stats.trace += 1,
                LogLevel::Debug => self.stats.debug += 1,
                LogLevel::Info => self.stats.info += 1,
                LogLevel::Warn => self.stats.warn += 1,
                LogLevel::Error => self.stats.error += 1,
                LogLevel::Fatal => self.stats.fatal += 1,
            }
        }

        // 如果超过容量，移除最旧的
        if self.entries.len() >= self.max_capacity {
            self.entries.pop_front();
        }

        self.entries.push_back(entry);
    }

    pub fn entries(&self) -> &VecDeque<LogEntry> {
        &self.entries
    }

    pub fn stats(&self) -> &LogStats {
        &self.stats
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.stats = LogStats::default();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log_viewer::LogParser;

    #[test]
    fn test_buffer_capacity() {
        let mut buffer = LogBuffer::new(5);
        for i in 0..10 {
            let entry = LogParser::parse(&format!("INFO: Message {}", i));
            buffer.push(entry);
        }
        assert_eq!(buffer.len(), 5);
    }

    #[test]
    fn test_buffer_stats() {
        let mut buffer = LogBuffer::new(100);
        buffer.push(LogParser::parse("INFO: info message"));
        buffer.push(LogParser::parse("ERROR: error message"));
        buffer.push(LogParser::parse("WARN: warning message"));

        let stats = buffer.stats();
        assert_eq!(stats.info, 1);
        assert_eq!(stats.error, 1);
        assert_eq!(stats.warn, 1);
        assert_eq!(stats.total, 3);
    }

    #[test]
    fn test_buffer_clear() {
        let mut buffer = LogBuffer::new(100);
        buffer.push(LogParser::parse("INFO: test"));
        assert_eq!(buffer.len(), 1);

        buffer.clear();
        assert_eq!(buffer.len(), 0);
        assert_eq!(buffer.stats().total, 0);
    }
}
