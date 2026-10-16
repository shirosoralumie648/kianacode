//! 旧 TUI 搜索查询词的统计与持久化模块。
//!
//! `HistoryState` 在 prompt-history 页面中创建管理器，并按 `prompt_history` context 查询/记录；
//! 管理器把相同“规范化查询词 + context”合并为一项，更新时间和次数后按 frecency 排序。
//! 数据落在用户数据目录下的 JSON Lines（JSONL，每行一条 JSON）文件；本模块不保存用户 prompt，
//! 也不参与模型请求、授权或 session 恢复。
//!
//! 这里的 frecency（frequency + recency，频率加新近度）是一个本地展示排序分数，不是安全策略，
//! 也不保证稳定的跨进程排序快照：分数会在读取时根据当前时间重新计算。
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

/// 同一个查询词在一个 context 中的汇总记录；查询词相同但 context 不同会分开统计。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchHistoryEntry {
    /// 压缩连续空白后的查询内容，用作去重和模糊匹配键。
    pub query: String,

    /// 第一次记录该查询的 UTC 时间；重复使用不会覆盖它。
    pub first_used: DateTime<Utc>,

    /// 最近一次记录该查询的 UTC 时间；参与新近度评分和相对时间显示。
    pub last_used: DateTime<Utc>,

    /// 累计使用次数；达到整数上限后饱和，不再递增。
    pub use_count: u32,

    /// 查询所属页面/功能名，用于隔离不同搜索场景的排序与展示。
    #[serde(default)]
    pub context: String,
}

impl SearchHistoryEntry {
    /// 创建首次查询记录：首用与末用时间设为同一时刻，计数从一次开始。
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

    /// 计算用于列表排序的 frecency 分数；分数越高，调用方越靠前显示。
    ///
    /// 频率项用自然对数压低极高次数的边际增长，新近度项根据最后使用时间分段衰减（100 到 10）。
    /// 这些权重只是当前实现的排序启发式，不是配置项或概率；修改会改变历史列表次序。
    pub fn frecency_score(&self) -> f64 {
        let frequency_score = ((self.use_count as f64) + 1.0).ln() * 100.0;
        let recency_score = self.calculate_recency_score();
        frequency_score + recency_score
    }

    /// 把距 `last_used` 的时间映射到固定档位；每档边界是排序规则的一部分。
    ///
    /// 这里使用读取时的 `Utc::now()`，所以同一条记录的分数会随时间变化。
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

    /// 重复使用现有条目时更新时间并饱和增加计数，避免整数溢出回绕成小值。
    pub fn record_use(&mut self) {
        self.last_used = Utc::now();
        self.use_count = self.use_count.saturating_add(1);
    }

    /// 将末次使用时间转换为列表展示用的英文相对时间；不参与排序或持久化。
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

/// 搜索词记录的内存集合和 JSONL 文件 adapter；调用方通过 `save()` 或析构时持久化。
#[derive(Debug)]
pub struct SearchHistoryManager {
    /// 当前进程持有的条目快照；磁盘内容只在构造时加载、保存时整体重写。
    entries: Vec<SearchHistoryEntry>,

    /// 用户数据目录下的搜索历史文件位置。
    storage_path: PathBuf,

    /// 超出该条目数后按当前 frecency 排序并只保留前 N 项。
    max_entries: usize,

    /// 是否发生过需要落盘的修改；成功写完并 flush 后才清除此标记。
    dirty: bool,
}

impl SearchHistoryManager {
    /// 从指定位置构造管理器，并尝试加载现有 JSONL。
    ///
    /// 【失败语义】路径加载错误会被 `unwrap_or_else` 转成空集合，因此该错误不会返回给调用方；
    /// 只有管理器本身仍会以 Ok 创建。若后续发生写入，这个空集合可能成为下一次保存的数据来源。
    pub fn new(storage_path: PathBuf) -> Result<Self> {
        let entries = Self::load_from_disk(&storage_path).unwrap_or_else(|_| Vec::new());
        Ok(Self {
            entries,
            storage_path,
            max_entries: 1000,
            dirty: false,
        })
    }

    /// 解析操作系统用户数据目录并在其下打开本产品的搜索历史文件。
    ///
    /// macOS、Windows 和其他平台使用不同的标准目录；目录创建失败会返回错误，由 `HistoryState::default()`
    /// 选择禁用搜索词历史的降级行为。
    pub fn with_default_path() -> Result<Self> {
        let path = Self::default_storage_path()?;
        Self::new(path)
    }

    /// 只负责解析/创建数据目录并返回文件路径，不在这里写入历史内容。
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

    /// 记录一次查询：按规范化文本和 context 查找已有条目，存在则累计，不存在则新建。
    ///
    /// 空白查询会直接忽略；成功记录会置 dirty 并执行条目数上限裁剪，但不会立刻写磁盘。
    /// 持久化要等调用方显式 `save()` 或管理器析构，因此进程异常退出可能丢失尚未保存的最近变更。
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

    /// 按可选 context 筛选并返回拥有所有权的排序副本，避免把内部可变集合借给 UI。
    ///
    /// `None` 表示跨 context 返回全部条目；`Some(name)` 只返回完全匹配 context 的记录。
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

    /// 对查询词做不区分大小写的子串匹配，再按 frecency 排序返回副本。
    ///
    /// 这是管理器提供的查询 API；当前 `HistoryState` 的屏幕过滤另有实现，未调用此方法。
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

    /// 清除所有内存条目并标记待保存；真正覆盖磁盘文件发生在后续 save/drop。
    pub fn clear(&mut self) {
        self.entries.clear();
        self.dirty = true;
    }

    /// 仅删除指定 context 的内存记录并标记待保存；其他 context 的条目保留。
    pub fn clear_context(&mut self, context: &str) {
        self.entries.retain(|e| e.context != context);
        self.dirty = true;
    }

    /// 将整份集合序列化为 JSONL 并覆盖目标文件。
    ///
    /// 写入包含建目录、创建/截断文件、逐条 JSON 编码、写换行和 flush。任何一步失败都会返回带上下文错误，
    /// `dirty` 只在全部步骤成功后清除。此实现没有临时文件 + 原子 rename；写入中断可能留下不完整文件。
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

    /// 逐行读取 JSONL；损坏 JSON 行记录警告并跳过，文件打开/行 I/O 错误则返回 Err。
    ///
    /// 调用方 `new()` 会吞掉这个 Err 并改用空集合，因此这里的错误并不一定会向 UI 暴露。
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

    /// 将连续空白压成一个 ASCII 空格，并同时去除首尾空白，用于稳定去重键。
    fn normalize_query(query: &str) -> String {
        // 去除首尾空格，压缩连续空格
        query.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// 超过容量时先按当前分数排序，再截断尾部；只有真正超限时才改变内部顺序。
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

    /// 返回当前内存集合大小，不会触发磁盘读取。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 检查当前内存集合是否为空，不代表目标文件一定不存在。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 查询内存是否有未成功保存的修改；该标记不是磁盘完整性证明。
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}

impl Drop for SearchHistoryManager {
    fn drop(&mut self) {
        // 这是调用方忘记显式 save() 时的最后一次尽力保存；析构函数不能把失败返回给调用方，
        // 所以错误被丢弃。不要把对象离开作用域本身当成“数据已成功落盘”的回执。
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
