/// 搜索历史管理模块
///
/// 实现基于 frecency（frequency + recency）算法的搜索历史管理，
/// 支持智能排序、去重和持久化存储。
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

/// 搜索历史条目
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchHistoryEntry {
    /// 搜索查询字符串
    pub query: String,

    /// 首次使用时间
    pub first_used: DateTime<Utc>,

    /// 最后使用时间
    pub last_used: DateTime<Utc>,

    /// 使用次数
    pub use_count: u32,

    /// 搜索上下文（例如："prompt_history", "file_search"）
    #[serde(default)]
    pub context: String,
}

impl SearchHistoryEntry {
    /// 创建新的搜索历史条目
    pub fn new(query: String, context: String) -> Self {
        let now = Utc::now();
        Self {
            query,
            first_used: now,
            last_used: now,
            use_count: 1,
            context,
        }
    }

    /// 计算 frecency 分数
    ///
    /// Frecency = frequency_score + recency_score
    /// - frequency_score = ln(use_count + 1) * 100
    /// - recency_score = 基于时间衰减（100 到 10）
    pub fn frecency_score(&self) -> f64 {
        let frequency_score = ((self.use_count as f64) + 1.0).ln() * 100.0;
        let recency_score = self.calculate_recency_score();
        frequency_score + recency_score
    }

    /// 计算基于时间衰减的新近度分数
    fn calculate_recency_score(&self) -> f64 {
        let duration = Utc::now() - self.last_used;

        if duration < Duration::hours(1) {
            100.0
        } else if duration < Duration::days(1) {
            80.0
        } else if duration < Duration::weeks(1) {
            60.0
        } else if duration < Duration::days(30) {
            40.0
        } else if duration < Duration::days(90) {
            20.0
        } else {
            10.0
        }
    }

    /// 更新使用统计
    pub fn record_use(&mut self) {
        self.last_used = Utc::now();
        self.use_count = self.use_count.saturating_add(1);
    }

    /// 获取人类可读的最后使用时间
    pub fn last_used_humanized(&self) -> String {
        let duration = Utc::now() - self.last_used;

        if duration < Duration::hours(1) {
            let mins = duration.num_minutes();
            if mins < 1 {
                "just now".to_string()
            } else if mins == 1 {
                "1 min ago".to_string()
            } else {
                format!("{} mins ago", mins)
            }
        } else if duration < Duration::days(1) {
            let hours = duration.num_hours();
            if hours == 1 {
                "1 hour ago".to_string()
            } else {
                format!("{} hours ago", hours)
            }
        } else if duration < Duration::weeks(1) {
            let days = duration.num_days();
            if days == 1 {
                "yesterday".to_string()
            } else {
                format!("{} days ago", days)
            }
        } else if duration < Duration::days(30) {
            let weeks = duration.num_weeks();
            if weeks == 1 {
                "1 week ago".to_string()
            } else {
                format!("{} weeks ago", weeks)
            }
        } else if duration < Duration::days(365) {
            let months = duration.num_days() / 30;
            if months == 1 {
                "1 month ago".to_string()
            } else {
                format!("{} months ago", months)
            }
        } else {
            self.last_used.format("%Y-%m-%d").to_string()
        }
    }
}

/// 搜索历史管理器
#[derive(Debug)]
pub struct SearchHistoryManager {
    /// 历史记录列表
    entries: Vec<SearchHistoryEntry>,

    /// 存储文件路径
    storage_path: PathBuf,

    /// 最大记录数
    max_entries: usize,

    /// 是否有未保存的更改
    dirty: bool,
}

impl SearchHistoryManager {
    /// 创建新的搜索历史管理器
    pub fn new(storage_path: PathBuf) -> Result<Self> {
        let entries = Self::load_from_disk(&storage_path).unwrap_or_else(|_| Vec::new());
        Ok(Self {
            entries,
            storage_path,
            max_entries: 1000,
            dirty: false,
        })
    }

    /// 使用默认路径创建管理器
    pub fn with_default_path() -> Result<Self> {
        let path = Self::default_storage_path()?;
        Self::new(path)
    }

    /// 获取默认存储路径
    fn default_storage_path() -> Result<PathBuf> {
        let data_dir = if cfg!(target_os = "macos") {
            dirs::home_dir()
                .context("无法获取 home 目录")?
                .join("Library")
                .join("Application Support")
                .join("kiana")
        } else if cfg!(target_os = "windows") {
            dirs::data_dir()
                .context("无法获取 AppData 目录")?
                .join("kiana")
        } else {
            // Linux 和其他 Unix-like 系统
            dirs::data_local_dir()
                .context("无法获取本地数据目录")?
                .join("kiana")
        };

        fs::create_dir_all(&data_dir).context("无法创建数据目录")?;

        Ok(data_dir.join("search_history.jsonl"))
    }

    /// 记录一次搜索（新增或更新）
    pub fn record_search(&mut self, query: &str, context: &str) {
        let normalized_query = Self::normalize_query(query);

        // 空查询不记录
        if normalized_query.is_empty() {
            return;
        }

        // 查找现有记录
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|e| e.query == normalized_query && e.context == context)
        {
            entry.record_use();
        } else {
            // 添加新记录
            self.entries.push(SearchHistoryEntry::new(
                normalized_query,
                context.to_string(),
            ));
        }

        self.dirty = true;
        self.trim_to_max();
    }

    /// 获取按 frecency 排序的历史记录
    #[allow(clippy::unnecessary_map_or)]
    pub fn get_sorted_history(&self, context: Option<&str>) -> Vec<SearchHistoryEntry> {
        let mut filtered: Vec<_> = self
            .entries
            .iter()
            .filter(|e| context.map_or(true, |ctx| e.context == ctx))
            .cloned()
            .collect();

        // 按 frecency 分数降序排序
        filtered.sort_by(|a, b| {
            b.frecency_score()
                .partial_cmp(&a.frecency_score())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        filtered
    }

    /// 搜索历史记录
    #[allow(clippy::unnecessary_map_or)]
    pub fn search(&self, query: &str, context: Option<&str>) -> Vec<SearchHistoryEntry> {
        let query_lower = query.to_lowercase();
        let mut results: Vec<_> = self
            .entries
            .iter()
            .filter(|e| {
                let context_matches = context.map_or(true, |ctx| e.context == ctx);
                let query_matches = e.query.to_lowercase().contains(&query_lower);
                context_matches && query_matches
            })
            .cloned()
            .collect();

        // 按 frecency 分数排序
        results.sort_by(|a, b| {
            b.frecency_score()
                .partial_cmp(&a.frecency_score())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        results
    }

    /// 清空历史记录
    pub fn clear(&mut self) {
        self.entries.clear();
        self.dirty = true;
    }

    /// 删除特定上下文的历史记录
    pub fn clear_context(&mut self, context: &str) {
        self.entries.retain(|e| e.context != context);
        self.dirty = true;
    }

    /// 保存到磁盘
    pub fn save(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }

        // 确保目录存在
        if let Some(parent) = self.storage_path.parent() {
            fs::create_dir_all(parent).context("无法创建存储目录")?;
        }

        let file = File::create(&self.storage_path).context("无法创建历史文件")?;
        let mut writer = BufWriter::new(file);

        for entry in &self.entries {
            serde_json::to_writer(&mut writer, entry).context("无法序列化历史条目")?;
            writeln!(writer).context("无法写入换行符")?;
        }

        writer.flush().context("无法刷新缓冲区")?;
        self.dirty = false;

        Ok(())
    }

    /// 从磁盘加载
    fn load_from_disk(path: &Path) -> Result<Vec<SearchHistoryEntry>> {
        if !path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(path).context("无法打开历史文件")?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();

        for line in reader.lines() {
            let line = line.context("读取行失败")?;
            if line.trim().is_empty() {
                continue;
            }

            match serde_json::from_str::<SearchHistoryEntry>(&line) {
                Ok(entry) => entries.push(entry),
                Err(e) => {
                    eprintln!("警告：跳过损坏的历史条目: {}", e);
                    continue;
                }
            }
        }

        Ok(entries)
    }

    /// 规范化查询字符串
    fn normalize_query(query: &str) -> String {
        // 去除首尾空格，压缩连续空格
        query.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// 限制记录数到最大值
    fn trim_to_max(&mut self) {
        if self.entries.len() > self.max_entries {
            // 按 frecency 排序
            self.entries.sort_by(|a, b| {
                b.frecency_score()
                    .partial_cmp(&a.frecency_score())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            // 保留前 max_entries 条
            self.entries.truncate(self.max_entries);
        }
    }

    /// 获取历史记录数量
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 检查是否有未保存的更改
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}

impl Drop for SearchHistoryManager {
    fn drop(&mut self) {
        // 析构时自动保存
        if self.dirty {
            let _ = self.save();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration as StdDuration;

    #[test]
    fn test_frecency_score_increases_with_frequency() {
        let mut entry1 = SearchHistoryEntry::new("test".to_string(), "ctx".to_string());
        let score1 = entry1.frecency_score();

        entry1.use_count = 10;
        let score2 = entry1.frecency_score();

        assert!(score2 > score1, "频率增加应该提高 frecency 分数");
    }

    #[test]
    fn test_recency_decay() {
        let mut entry = SearchHistoryEntry::new("test".to_string(), "ctx".to_string());

        // 最近使用
        let recent_score = entry.calculate_recency_score();
        assert_eq!(recent_score, 100.0);

        // 模拟 2 小时前
        entry.last_used = Utc::now() - Duration::hours(2);
        let hours_ago_score = entry.calculate_recency_score();
        assert_eq!(hours_ago_score, 80.0);

        // 模拟 3 天前
        entry.last_used = Utc::now() - Duration::days(3);
        let days_ago_score = entry.calculate_recency_score();
        assert_eq!(days_ago_score, 60.0);

        // 模拟 100 天前
        entry.last_used = Utc::now() - Duration::days(100);
        let old_score = entry.calculate_recency_score();
        assert_eq!(old_score, 10.0);
    }

    #[test]
    fn test_normalize_query() {
        assert_eq!(
            SearchHistoryManager::normalize_query("  hello   world  "),
            "hello world"
        );
        assert_eq!(SearchHistoryManager::normalize_query("test"), "test");
        assert_eq!(SearchHistoryManager::normalize_query("   "), "");
    }

    #[test]
    fn test_record_new_search() {
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(format!(
            "test_search_history_new_{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&temp_path); // 清理旧文件
        let mut manager = SearchHistoryManager::new(temp_path.clone()).unwrap();

        manager.record_search("test query", "test_context");

        assert_eq!(manager.len(), 1);
        let entries = manager.get_sorted_history(Some("test_context"));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].query, "test query");
        assert_eq!(entries[0].use_count, 1);

        // 清理
        let _ = std::fs::remove_file(temp_path);
    }

    #[test]
    fn test_record_existing_search() {
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(format!(
            "test_search_history_existing_{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&temp_path); // 清理旧文件
        let mut manager = SearchHistoryManager::new(temp_path.clone()).unwrap();

        manager.record_search("test query", "test_context");
        sleep(StdDuration::from_millis(10));
        manager.record_search("test query", "test_context");

        assert_eq!(manager.len(), 1);
        let entries = manager.get_sorted_history(Some("test_context"));
        assert_eq!(entries[0].use_count, 2);

        // 清理
        let _ = std::fs::remove_file(temp_path);
    }

    #[test]
    fn test_deduplication() {
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(format!(
            "test_search_history_dedup_{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&temp_path);
        let mut manager = SearchHistoryManager::new(temp_path.clone()).unwrap();

        manager.record_search("  test  query  ", "ctx");
        manager.record_search("test query", "ctx");

        assert_eq!(manager.len(), 1, "规范化后的重复查询应该合并");

        // 清理
        let _ = std::fs::remove_file(temp_path);
    }

    #[test]
    fn test_sorted_history() {
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(format!(
            "test_search_history_sorted_{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&temp_path);
        let mut manager = SearchHistoryManager::new(temp_path.clone()).unwrap();

        // 添加多个查询，不同频率和时间
        manager.record_search("frequent", "ctx");
        manager.record_search("frequent", "ctx");
        manager.record_search("frequent", "ctx");

        sleep(StdDuration::from_millis(10));
        manager.record_search("recent", "ctx");

        let sorted = manager.get_sorted_history(Some("ctx"));

        // 频繁使用的应该排在前面（因为 ln(4)*100 + recency > ln(2)*100 + recency）
        assert_eq!(sorted[0].query, "frequent");

        // 清理
        let _ = std::fs::remove_file(temp_path);
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(format!(
            "test_search_history_roundtrip_{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&temp_path);

        // 创建并保存
        {
            let mut manager = SearchHistoryManager::new(temp_path.clone()).unwrap();
            manager.record_search("query1", "ctx1");
            manager.record_search("query2", "ctx2");
            manager.save().unwrap();
        }

        // 加载并验证
        {
            let manager = SearchHistoryManager::new(temp_path.clone()).unwrap();
            assert_eq!(manager.len(), 2);

            let entries = manager.get_sorted_history(None);
            let queries: Vec<_> = entries.iter().map(|e| e.query.as_str()).collect();
            assert!(queries.contains(&"query1"));
            assert!(queries.contains(&"query2"));
        }

        // 清理
        let _ = std::fs::remove_file(temp_path);
    }

    #[test]
    fn test_empty_query_not_recorded() {
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(format!(
            "test_search_history_empty_{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&temp_path);
        let mut manager = SearchHistoryManager::new(temp_path.clone()).unwrap();

        manager.record_search("", "ctx");
        manager.record_search("   ", "ctx");

        assert_eq!(manager.len(), 0, "空查询不应该被记录");

        // 清理
        let _ = std::fs::remove_file(temp_path);
    }

    #[test]
    fn test_context_filtering() {
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(format!(
            "test_search_history_context_{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&temp_path);
        let mut manager = SearchHistoryManager::new(temp_path.clone()).unwrap();

        manager.record_search("query1", "ctx1");
        manager.record_search("query2", "ctx2");
        manager.record_search("query3", "ctx1");

        let ctx1_entries = manager.get_sorted_history(Some("ctx1"));
        assert_eq!(ctx1_entries.len(), 2);

        let ctx2_entries = manager.get_sorted_history(Some("ctx2"));
        assert_eq!(ctx2_entries.len(), 1);

        let all_entries = manager.get_sorted_history(None);
        assert_eq!(all_entries.len(), 3);

        // 清理
        let _ = std::fs::remove_file(temp_path);
    }

    #[test]
    fn test_search_history() {
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(format!(
            "test_search_history_search_{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&temp_path);
        let mut manager = SearchHistoryManager::new(temp_path.clone()).unwrap();

        manager.record_search("rust workspace", "ctx");
        manager.record_search("rust testing", "ctx");
        manager.record_search("python code", "ctx");

        let results = manager.search("rust", Some("ctx"));
        assert_eq!(results.len(), 2);

        let results = manager.search("python", Some("ctx"));
        assert_eq!(results.len(), 1);

        let results = manager.search("java", Some("ctx"));
        assert_eq!(results.len(), 0);

        // 清理
        let _ = std::fs::remove_file(temp_path);
    }
}
