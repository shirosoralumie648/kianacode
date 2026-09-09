use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::time::Instant;

/// 缓存条目
#[derive(Debug, Clone)]
pub struct CacheEntry<T> {
    pub value: T,
    pub hash: u64,
    pub size: usize,
    pub last_accessed: Instant,
    pub hit_count: u64,
}

impl<T> CacheEntry<T> {
    pub fn new(value: T, hash: u64, size: usize) -> Self {
        Self {
            value,
            hash,
            size,
            last_accessed: Instant::now(),
            hit_count: 0,
        }
    }

    pub fn touch(&mut self) {
        self.last_accessed = Instant::now();
        self.hit_count += 1;
    }
}

/// LRU缓存实现
pub struct LruCache<K, V> {
    cache: HashMap<K, CacheEntry<V>>,
    access_order: VecDeque<K>,
    capacity_bytes: usize,
    current_size: usize,
    max_entries: usize,

    // 统计信息
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl<K: Clone + Eq + Hash, V: Clone> LruCache<K, V> {
    pub fn new(capacity_bytes: usize, max_entries: usize) -> Self {
        Self {
            cache: HashMap::new(),
            access_order: VecDeque::new(),
            capacity_bytes,
            current_size: 0,
            max_entries,
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }

    /// 获取缓存值
    pub fn get(&mut self, key: &K) -> Option<V> {
        if let Some(entry) = self.cache.get_mut(key) {
            entry.touch();
            self.hits += 1;
            let value = entry.value.clone();
            self.update_access_order(key);
            Some(value)
        } else {
            self.misses += 1;
            None
        }
    }

    /// 插入缓存
    pub fn insert(&mut self, key: K, value: V, hash: u64, size: usize) {
        // 如果已存在，先删除旧的
        if self.cache.contains_key(&key) {
            self.remove(&key);
        }

        // 腾出空间
        while self.should_evict(size) {
            self.evict_lru();
        }

        // 插入新条目
        let entry = CacheEntry::new(value, hash, size);
        self.current_size += size;
        self.cache.insert(key.clone(), entry);
        self.access_order.push_back(key);
    }

    /// 删除缓存条目
    pub fn remove(&mut self, key: &K) -> Option<V> {
        if let Some(entry) = self.cache.remove(key) {
            self.current_size = self.current_size.saturating_sub(entry.size);
            self.access_order.retain(|k| k != key);
            Some(entry.value)
        } else {
            None
        }
    }

    /// 检查是否需要驱逐
    fn should_evict(&self, new_size: usize) -> bool {
        (self.current_size + new_size > self.capacity_bytes)
            || (self.cache.len() >= self.max_entries)
    }

    /// 驱逐最久未使用的条目
    fn evict_lru(&mut self) {
        if let Some(key) = self.access_order.pop_front() {
            if let Some(entry) = self.cache.remove(&key) {
                self.current_size = self.current_size.saturating_sub(entry.size);
                self.evictions += 1;
            }
        }
    }

    /// 更新访问顺序
    fn update_access_order(&mut self, key: &K) {
        // 移到队尾
        self.access_order.retain(|k| k != key);
        self.access_order.push_back(key.clone());
    }

    /// 清空缓存
    pub fn clear(&mut self) {
        self.cache.clear();
        self.access_order.clear();
        self.current_size = 0;
    }

    /// 获取统计信息
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits,
            misses: self.misses,
            evictions: self.evictions,
            entries: self.cache.len(),
            size_bytes: self.current_size,
            capacity_bytes: self.capacity_bytes,
            hit_rate: if self.hits + self.misses > 0 {
                self.hits as f64 / (self.hits + self.misses) as f64
            } else {
                0.0
            },
        }
    }

    /// 当前大小
    pub fn size(&self) -> usize {
        self.current_size
    }

    /// 条目数量
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

/// 缓存统计信息
#[derive(Debug, Clone, Copy)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub entries: usize,
    pub size_bytes: usize,
    pub capacity_bytes: usize,
    pub hit_rate: f64,
}

/// 可缓存trait
pub trait Cacheable {
    type Key: Clone + Eq + Hash;
    type Value: Clone;

    fn cache_key(&self) -> Self::Key;
    fn compute_value(&self) -> Self::Value;
    fn cache_size(&self) -> usize;
    fn content_hash(&self) -> u64;
}

/// 特定用途的缓存管理器
pub struct CacheManager {
    // Widget渲染缓存
    widget_cache: LruCache<String, Vec<u8>>,

    // 语法高亮缓存
    syntax_cache: LruCache<u64, Vec<(String, String)>>,

    // Markdown解析缓存
    markdown_cache: LruCache<u64, String>,

    // 布局计算缓存
    layout_cache: LruCache<u64, ratatui::layout::Rect>,
}

impl CacheManager {
    pub fn new(config: CacheConfig) -> Self {
        Self {
            widget_cache: LruCache::new(config.widget_cache_size, 1000),
            syntax_cache: LruCache::new(config.syntax_cache_size, 500),
            markdown_cache: LruCache::new(config.markdown_cache_size, 200),
            layout_cache: LruCache::new(10 * 1024 * 1024, 10000), // 10MB for layouts
        }
    }

    /// 获取widget渲染结果
    pub fn get_widget_render(&mut self, key: &str) -> Option<Vec<u8>> {
        self.widget_cache.get(&key.to_string())
    }

    /// 缓存widget渲染结果
    pub fn cache_widget_render(&mut self, key: String, data: Vec<u8>) {
        let size = data.len();
        let hash = crate::performance::incremental::hash_string(&key);
        self.widget_cache.insert(key, data, hash, size);
    }

    /// 获取语法高亮结果
    pub fn get_syntax_highlight(&mut self, hash: u64) -> Option<Vec<(String, String)>> {
        self.syntax_cache.get(&hash)
    }

    /// 缓存语法高亮结果
    pub fn cache_syntax_highlight(&mut self, hash: u64, tokens: Vec<(String, String)>) {
        let size = tokens.iter().map(|(t, s)| t.len() + s.len()).sum();
        self.syntax_cache.insert(hash, tokens, hash, size);
    }

    /// 获取markdown渲染结果
    pub fn get_markdown_render(&mut self, hash: u64) -> Option<String> {
        self.markdown_cache.get(&hash)
    }

    /// 缓存markdown渲染结果
    pub fn cache_markdown_render(&mut self, hash: u64, rendered: String) {
        let size = rendered.len();
        self.markdown_cache.insert(hash, rendered, hash, size);
    }

    /// 获取布局计算结果
    pub fn get_layout(&mut self, hash: u64) -> Option<ratatui::layout::Rect> {
        self.layout_cache.get(&hash)
    }

    /// 缓存布局计算结果
    pub fn cache_layout(&mut self, hash: u64, rect: ratatui::layout::Rect) {
        self.layout_cache.insert(
            hash,
            rect,
            hash,
            std::mem::size_of::<ratatui::layout::Rect>(),
        );
    }

    /// 清空所有缓存
    pub fn clear_all(&mut self) {
        self.widget_cache.clear();
        self.syntax_cache.clear();
        self.markdown_cache.clear();
        self.layout_cache.clear();
    }

    /// 获取总体统计信息
    pub fn total_stats(&self) -> TotalCacheStats {
        TotalCacheStats {
            widget: self.widget_cache.stats(),
            syntax: self.syntax_cache.stats(),
            markdown: self.markdown_cache.stats(),
            layout: self.layout_cache.stats(),
        }
    }
}

impl Default for CacheManager {
    fn default() -> Self {
        Self::new(CacheConfig::default())
    }
}

/// 缓存配置
#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub widget_cache_size: usize,
    pub syntax_cache_size: usize,
    pub markdown_cache_size: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            widget_cache_size: 50 * 1024 * 1024,   // 50 MB
            syntax_cache_size: 30 * 1024 * 1024,   // 30 MB
            markdown_cache_size: 20 * 1024 * 1024, // 20 MB
        }
    }
}

/// 总体缓存统计
#[derive(Debug, Clone, Copy)]
pub struct TotalCacheStats {
    pub widget: CacheStats,
    pub syntax: CacheStats,
    pub markdown: CacheStats,
    pub layout: CacheStats,
}

impl TotalCacheStats {
    pub fn total_hit_rate(&self) -> f64 {
        let total_hits =
            self.widget.hits + self.syntax.hits + self.markdown.hits + self.layout.hits;
        let total_requests = total_hits
            + self.widget.misses
            + self.syntax.misses
            + self.markdown.misses
            + self.layout.misses;

        if total_requests > 0 {
            total_hits as f64 / total_requests as f64
        } else {
            0.0
        }
    }

    pub fn total_size(&self) -> usize {
        self.widget.size_bytes
            + self.syntax.size_bytes
            + self.markdown.size_bytes
            + self.layout.size_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lru_cache_basic() {
        let mut cache: LruCache<String, String> = LruCache::new(1000, 10);

        cache.insert("key1".to_string(), "value1".to_string(), 1, 10);
        assert_eq!(cache.get(&"key1".to_string()), Some("value1".to_string()));

        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 0);
    }

    #[test]
    fn test_lru_eviction() {
        let mut cache: LruCache<i32, String> = LruCache::new(30, 10);

        // 插入3个条目，每个10字节
        cache.insert(1, "value1".to_string(), 1, 10);
        cache.insert(2, "value2".to_string(), 2, 10);
        cache.insert(3, "value3".to_string(), 3, 10);

        assert_eq!(cache.len(), 3);

        // 插入第4个，应该驱逐最久未使用的
        cache.insert(4, "value4".to_string(), 4, 10);
        assert_eq!(cache.len(), 3);
        assert!(cache.get(&1).is_none()); // 第一个应该被驱逐
    }

    #[test]
    fn test_lru_access_order() {
        let mut cache: LruCache<i32, String> = LruCache::new(30, 10);

        cache.insert(1, "value1".to_string(), 1, 10);
        cache.insert(2, "value2".to_string(), 2, 10);
        cache.insert(3, "value3".to_string(), 3, 10);

        // 访问第1个，使其变为最近使用
        cache.get(&1);

        // 插入第4个，应该驱逐2而不是1
        cache.insert(4, "value4".to_string(), 4, 10);
        assert!(cache.get(&1).is_some());
        assert!(cache.get(&2).is_none());
    }

    #[test]
    fn test_cache_stats() {
        let mut cache: LruCache<i32, String> = LruCache::new(1000, 10);

        cache.insert(1, "value1".to_string(), 1, 10);
        cache.get(&1); // hit
        cache.get(&2); // miss

        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hit_rate, 0.5);
    }
}
