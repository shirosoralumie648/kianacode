use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Widget},
};
use std::ops::Range;

/// Trait for items that can be rendered in a virtual list
pub trait VirtualListItem {
    fn render(&self, area: Rect, buf: &mut Buffer, selected: bool);
    fn height(&self) -> u16;
}

/// Height strategy for virtual list items
#[derive(Clone)]
pub enum ItemHeight<T> {
    /// All items have the same fixed height
    Fixed(u16),
    /// Items have variable heights calculated by this function
    Variable(fn(&T) -> u16),
}

impl<T> ItemHeight<T> {
    /// Calculate the height of an item
    pub fn calculate(&self, item: &T) -> u16 {
        match self {
            ItemHeight::Fixed(h) => *h,
            ItemHeight::Variable(f) => f(item),
        }
    }
}

/// Cache for item heights with prefix sum optimization for O(1) lookup
pub struct HeightCache {
    /// Cached heights for each item
    heights: Vec<u16>,
    /// Prefix sums for O(1) offset calculation
    prefix_sums: Vec<usize>,
    /// Whether the cache needs to be recalculated
    dirty: bool,
}

impl HeightCache {
    fn new() -> Self {
        Self {
            heights: Vec::new(),
            prefix_sums: Vec::new(),
            dirty: false,
        }
    }

    /// Initialize cache with item count
    fn initialize(&mut self, count: usize) {
        self.heights = vec![0; count];
        self.prefix_sums = vec![0; count + 1];
        self.dirty = true;
    }

    /// Set the height of an item at index
    fn set_height(&mut self, index: usize, height: u16) {
        if index < self.heights.len() && self.heights[index] != height {
            self.heights[index] = height;
            self.dirty = true;
        }
    }

    /// Get the height of an item at index
    fn get_height(&self, index: usize) -> u16 {
        self.heights.get(index).copied().unwrap_or(0)
    }

    /// Get the total offset (sum of heights) up to but not including index
    fn get_offset(&mut self, index: usize) -> usize {
        if self.dirty {
            self.rebuild_prefix_sums();
        }
        self.prefix_sums.get(index).copied().unwrap_or(0)
    }

    /// Get the total height of all items
    fn total_height(&mut self) -> usize {
        if self.dirty {
            self.rebuild_prefix_sums();
        }
        self.prefix_sums.last().copied().unwrap_or(0)
    }

    /// Rebuild prefix sums from heights
    fn rebuild_prefix_sums(&mut self) {
        if self.heights.is_empty() {
            self.prefix_sums.clear();
            self.prefix_sums.push(0);
        } else {
            self.prefix_sums.resize(self.heights.len() + 1, 0);
            let mut sum = 0;
            self.prefix_sums[0] = 0;
            for (i, &height) in self.heights.iter().enumerate() {
                sum += height as usize;
                self.prefix_sums[i + 1] = sum;
            }
        }
        self.dirty = false;
    }

    /// Invalidate a range of cached heights
    fn invalidate_range(&mut self, _range: Range<usize>) {
        self.dirty = true;
    }

    /// Clear all cached data
    fn clear(&mut self) {
        self.heights.clear();
        self.prefix_sums.clear();
        self.dirty = false;
    }
}

/// Scroll state management
#[derive(Clone, Debug)]
pub struct ScrollState {
    /// Current scroll offset (line number)
    offset: usize,
    /// Maximum scroll offset
    max_offset: usize,
    /// Size of the viewport
    viewport_size: u16,
}

impl ScrollState {
    fn new(viewport_size: u16) -> Self {
        Self {
            offset: 0,
            max_offset: 0,
            viewport_size,
        }
    }

    /// Scroll to a specific line
    pub fn scroll_to(&mut self, line: usize) {
        self.offset = line.min(self.max_offset);
    }

    /// Scroll by a delta (positive = down, negative = up)
    pub fn scroll_by(&mut self, delta: isize) {
        if delta < 0 {
            self.offset = self.offset.saturating_sub((-delta) as usize);
        } else {
            self.offset = (self.offset + delta as usize).min(self.max_offset);
        }
    }

    /// Scroll up one page
    pub fn page_up(&mut self) {
        let page_size = self.viewport_size.saturating_sub(1) as usize;
        self.offset = self.offset.saturating_sub(page_size);
    }

    /// Scroll down one page
    pub fn page_down(&mut self) {
        let page_size = self.viewport_size.saturating_sub(1) as usize;
        self.offset = (self.offset + page_size).min(self.max_offset);
    }

    /// Scroll to the top
    pub fn home(&mut self) {
        self.offset = 0;
    }

    /// Scroll to the bottom
    pub fn end(&mut self) {
        self.offset = self.max_offset;
    }

    /// Get current offset
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Update max offset based on total items and viewport
    fn update_max_offset(&mut self, total_items: usize) {
        if total_items > 0 {
            self.max_offset = total_items - 1;
        } else {
            self.max_offset = 0;
        }
    }
}

/// Virtual list that only renders visible items for performance
pub struct VirtualList<T> {
    /// All items in the list
    items: Vec<T>,
    /// Height calculation strategy
    item_height: ItemHeight<T>,
    /// Height cache for variable-height items
    cache: HeightCache,
    /// Current scroll state
    scroll_state: ScrollState,
    /// Currently selected item index
    selected: Option<usize>,
    /// Block style for rendering
    block: Option<Block<'static>>,
}

impl<T> VirtualList<T> {
    /// Create a new virtual list with fixed item height
    pub fn new(items: Vec<T>, viewport_height: u16) -> Self
    where
        T: VirtualListItem,
    {
        let item_count = items.len();
        let mut cache = HeightCache::new();
        cache.initialize(item_count);

        let mut scroll_state = ScrollState::new(viewport_height);
        scroll_state.update_max_offset(item_count);

        let mut list = Self {
            items,
            item_height: ItemHeight::Fixed(1),
            cache,
            scroll_state,
            selected: if item_count == 0 { None } else { Some(0) },
            block: None,
        };

        // Initialize cache with fixed height for all items
        for i in 0..item_count {
            list.cache.set_height(i, 1);
        }

        list
    }

    /// Create a new virtual list with variable item heights
    pub fn with_variable_height(
        items: Vec<T>,
        viewport_height: u16,
        height_fn: fn(&T) -> u16,
    ) -> Self
    where
        T: VirtualListItem,
    {
        let mut cache = HeightCache::new();
        cache.initialize(items.len());

        let mut scroll_state = ScrollState::new(viewport_height);
        scroll_state.update_max_offset(items.len());

        let mut list = Self {
            items,
            item_height: ItemHeight::Variable(height_fn),
            cache,
            scroll_state,
            selected: None,
            block: None,
        };

        // Pre-calculate all heights
        list.recalculate_all_heights();
        list
    }

    /// Set the block style
    pub fn block(mut self, block: Block<'static>) -> Self {
        self.block = Some(block);
        self
    }

    /// Set items and recalculate cache
    pub fn set_items(&mut self, items: Vec<T>)
    where
        T: VirtualListItem,
    {
        self.items = items;
        self.cache.initialize(self.items.len());
        self.recalculate_all_heights();
        self.scroll_state.update_max_offset(self.items.len());

        // Adjust selection if needed
        if let Some(sel) = self.selected {
            if sel >= self.items.len() {
                self.selected = if self.items.is_empty() {
                    None
                } else {
                    Some(self.items.len() - 1)
                };
            }
        }
    }

    /// Get the number of items
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Check if the list is empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Get the currently selected index
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Select an item by index
    pub fn select(&mut self, index: Option<usize>) {
        self.selected = index;
        if let Some(idx) = index {
            self.ensure_visible(idx);
        }
    }

    /// Select the next item
    pub fn select_next(&mut self) {
        if self.items.is_empty() {
            return;
        }

        let next = match self.selected {
            Some(i) => (i + 1).min(self.items.len() - 1),
            None => 0,
        };
        self.select(Some(next));
    }

    /// Select the previous item
    pub fn select_previous(&mut self) {
        if self.items.is_empty() {
            return;
        }

        let prev = match self.selected {
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        self.select(Some(prev));
    }

    /// Select the first item
    pub fn select_first(&mut self) {
        if !self.items.is_empty() {
            self.select(Some(0));
        }
    }

    /// Select the last item
    pub fn select_last(&mut self) {
        if !self.items.is_empty() {
            self.select(Some(self.items.len() - 1));
        }
    }

    /// Scroll down by a number of lines
    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_state.scroll_by(lines as isize);
    }

    /// Scroll up by a number of lines
    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_state.scroll_by(-(lines as isize));
    }

    /// Scroll down one page
    pub fn page_down(&mut self) {
        self.scroll_state.page_down();
    }

    /// Scroll up one page
    pub fn page_up(&mut self) {
        self.scroll_state.page_up();
    }

    /// Scroll to top
    pub fn scroll_to_top(&mut self) {
        self.scroll_state.home();
    }

    /// Scroll to bottom
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_state.end();
    }

    /// Ensure the given index is visible in the viewport
    fn ensure_visible(&mut self, index: usize) {
        if index < self.scroll_state.offset() {
            self.scroll_state.scroll_to(index);
        } else {
            let viewport_end =
                self.scroll_state.offset() + self.scroll_state.viewport_size as usize;
            if index >= viewport_end {
                self.scroll_state
                    .scroll_to(index.saturating_sub(self.scroll_state.viewport_size as usize - 1));
            }
        }
    }

    /// Recalculate heights for all items
    fn recalculate_all_heights(&mut self)
    where
        T: VirtualListItem,
    {
        for (i, item) in self.items.iter().enumerate() {
            let height = self.item_height.calculate(item);
            self.cache.set_height(i, height);
        }
    }

    /// Calculate which items are visible in the viewport
    fn calculate_visible_range(&mut self, viewport_height: u16) -> Range<usize> {
        if self.items.is_empty() {
            return 0..0;
        }

        let start = self.scroll_state.offset();
        let mut end = start;
        let mut accumulated_height = 0u16;

        while end < self.items.len() && accumulated_height < viewport_height {
            let item_height = self.cache.get_height(end);
            accumulated_height = accumulated_height.saturating_add(item_height);
            end += 1;
        }

        start..end
    }

    /// Render the virtual list
    pub fn render(&mut self, area: Rect, buf: &mut Buffer)
    where
        T: VirtualListItem,
    {
        // Apply block if present
        let inner_area = if let Some(ref block) = self.block {
            let inner = block.inner(area);
            block.clone().render(area, buf);
            inner
        } else {
            area
        };

        if inner_area.height == 0 || self.items.is_empty() {
            return;
        }

        // Update viewport size
        self.scroll_state.viewport_size = inner_area.height;

        // Calculate visible range
        let visible_range = self.calculate_visible_range(inner_area.height);

        // Render visible items
        let mut y_offset = 0;
        for i in visible_range {
            if let Some(item) = self.items.get(i) {
                let item_height = self.cache.get_height(i);
                if y_offset + item_height > inner_area.height {
                    break;
                }

                let item_area = Rect {
                    x: inner_area.x,
                    y: inner_area.y + y_offset,
                    width: inner_area.width,
                    height: item_height,
                };

                let is_selected = self.selected == Some(i);
                item.render(item_area, buf, is_selected);

                y_offset += item_height;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct TestItem {
        text: String,
        height: u16,
    }

    impl VirtualListItem for TestItem {
        fn render(&self, area: Rect, buf: &mut Buffer, selected: bool) {
            let style = if selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            buf.set_string(area.x, area.y, &self.text, style);
        }

        fn height(&self) -> u16 {
            self.height
        }
    }

    #[test]
    fn test_height_cache_fixed() {
        let mut cache = HeightCache::new();
        cache.initialize(5);

        for i in 0..5 {
            cache.set_height(i, 1);
        }

        assert_eq!(cache.get_height(0), 1);
        assert_eq!(cache.get_offset(0), 0);
        assert_eq!(cache.get_offset(1), 1);
        assert_eq!(cache.get_offset(5), 5);
        assert_eq!(cache.total_height(), 5);
    }

    #[test]
    fn test_height_cache_variable() {
        let mut cache = HeightCache::new();
        cache.initialize(3);

        cache.set_height(0, 2);
        cache.set_height(1, 3);
        cache.set_height(2, 1);

        assert_eq!(cache.get_offset(0), 0);
        assert_eq!(cache.get_offset(1), 2);
        assert_eq!(cache.get_offset(2), 5);
        assert_eq!(cache.get_offset(3), 6);
        assert_eq!(cache.total_height(), 6);
    }

    #[test]
    fn test_scroll_state_basic() {
        let mut state = ScrollState::new(10);
        state.update_max_offset(100);

        assert_eq!(state.offset(), 0);

        state.scroll_by(5);
        assert_eq!(state.offset(), 5);

        state.scroll_by(-3);
        assert_eq!(state.offset(), 2);
    }

    #[test]
    fn test_scroll_state_boundaries() {
        let mut state = ScrollState::new(10);
        state.update_max_offset(100);

        state.scroll_by(-10);
        assert_eq!(state.offset(), 0);

        state.scroll_by(1000);
        assert_eq!(state.offset(), 99); // max_offset is 99 for 100 items
    }

    #[test]
    fn test_scroll_state_page() {
        let mut state = ScrollState::new(10);
        state.update_max_offset(100);

        state.page_down();
        assert_eq!(state.offset(), 9);

        state.page_down();
        assert_eq!(state.offset(), 18);

        state.page_up();
        assert_eq!(state.offset(), 9);
    }

    #[test]
    fn test_scroll_state_home_end() {
        let mut state = ScrollState::new(10);
        state.update_max_offset(100);

        state.scroll_by(50);
        assert_eq!(state.offset(), 50);

        state.home();
        assert_eq!(state.offset(), 0);

        state.end();
        assert_eq!(state.offset(), 99); // max_offset is 99 for 100 items
    }

    #[test]
    fn test_virtual_list_creation() {
        let items: Vec<TestItem> = (0..100)
            .map(|i| TestItem {
                text: format!("Item {}", i),
                height: 1,
            })
            .collect();

        let list = VirtualList::new(items, 20);
        assert_eq!(list.len(), 100);
        assert_eq!(list.selected(), Some(0));
    }

    #[test]
    fn test_virtual_list_empty() {
        let items: Vec<TestItem> = vec![];
        let list = VirtualList::new(items, 20);

        assert_eq!(list.len(), 0);
        assert!(list.is_empty());
        assert_eq!(list.selected(), None);
    }

    #[test]
    fn test_virtual_list_selection() {
        let items: Vec<TestItem> = (0..10)
            .map(|i| TestItem {
                text: format!("Item {}", i),
                height: 1,
            })
            .collect();

        let mut list = VirtualList::new(items, 5);

        assert_eq!(list.selected(), Some(0));

        list.select_next();
        assert_eq!(list.selected(), Some(1));

        list.select_next();
        assert_eq!(list.selected(), Some(2));

        list.select_previous();
        assert_eq!(list.selected(), Some(1));

        list.select_first();
        assert_eq!(list.selected(), Some(0));

        list.select_last();
        assert_eq!(list.selected(), Some(9));
    }

    #[test]
    fn test_virtual_list_selection_boundaries() {
        let items: Vec<TestItem> = (0..5)
            .map(|i| TestItem {
                text: format!("Item {}", i),
                height: 1,
            })
            .collect();

        let mut list = VirtualList::new(items, 3);

        list.select_first();
        list.select_previous();
        assert_eq!(list.selected(), Some(0));

        list.select_last();
        list.select_next();
        assert_eq!(list.selected(), Some(4));
    }

    #[test]
    fn test_virtual_list_visible_range_fixed_height() {
        let items: Vec<TestItem> = (0..100)
            .map(|i| TestItem {
                text: format!("Item {}", i),
                height: 1,
            })
            .collect();

        let mut list = VirtualList::new(items, 10);

        let range = list.calculate_visible_range(10);
        assert_eq!(range, 0..10);

        list.scroll_down(5);
        let range = list.calculate_visible_range(10);
        assert_eq!(range, 5..15);
    }

    #[test]
    fn test_virtual_list_visible_range_variable_height() {
        let items: Vec<TestItem> = vec![
            TestItem {
                text: "Item 0".to_string(),
                height: 2,
            },
            TestItem {
                text: "Item 1".to_string(),
                height: 3,
            },
            TestItem {
                text: "Item 2".to_string(),
                height: 1,
            },
            TestItem {
                text: "Item 3".to_string(),
                height: 2,
            },
            TestItem {
                text: "Item 4".to_string(),
                height: 1,
            },
        ];

        let mut list = VirtualList::with_variable_height(items, 5, |item| item.height);

        let range = list.calculate_visible_range(5);
        // height: 2 + 3 = 5, so items 0 and 1
        assert_eq!(range, 0..2);
    }

    #[test]
    fn test_virtual_list_set_items() {
        let items: Vec<TestItem> = (0..5)
            .map(|i| TestItem {
                text: format!("Item {}", i),
                height: 1,
            })
            .collect();

        let mut list = VirtualList::new(items, 10);
        assert_eq!(list.len(), 5);

        let new_items: Vec<TestItem> = (0..10)
            .map(|i| TestItem {
                text: format!("New {}", i),
                height: 1,
            })
            .collect();

        list.set_items(new_items);
        assert_eq!(list.len(), 10);
    }

    #[test]
    fn test_virtual_list_large_dataset() {
        let items: Vec<TestItem> = (0..100_000)
            .map(|i| TestItem {
                text: format!("Item {}", i),
                height: 1,
            })
            .collect();

        let mut list = VirtualList::new(items, 20);
        assert_eq!(list.len(), 100_000);

        // Should only calculate visible range, not all items
        let range = list.calculate_visible_range(20);
        assert_eq!(range.len(), 20);

        list.scroll_to_bottom();
        let range = list.calculate_visible_range(20);
        assert!(range.end <= 100_000);
    }

    #[test]
    fn test_virtual_list_scroll_operations() {
        let items: Vec<TestItem> = (0..100)
            .map(|i| TestItem {
                text: format!("Item {}", i),
                height: 1,
            })
            .collect();

        let mut list = VirtualList::new(items, 10);

        list.scroll_down(5);
        assert_eq!(list.scroll_state.offset(), 5);

        list.scroll_up(3);
        assert_eq!(list.scroll_state.offset(), 2);

        list.page_down();
        assert_eq!(list.scroll_state.offset(), 11);

        list.page_up();
        assert_eq!(list.scroll_state.offset(), 2);

        list.scroll_to_top();
        assert_eq!(list.scroll_state.offset(), 0);

        list.scroll_to_bottom();
        assert_eq!(list.scroll_state.offset(), 99);
    }
}
