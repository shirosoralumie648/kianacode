use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use std::collections::HashSet;

/// 脏区域追踪器 - 只重绘变化的屏幕区域
#[derive(Debug, Clone)]
pub struct DirtyRegion {
    regions: Vec<Rect>,
    full_redraw: bool,
}

impl DirtyRegion {
    pub fn new() -> Self {
        Self {
            regions: Vec::new(),
            full_redraw: true,
        }
    }

    /// 标记整个屏幕需要重绘
    pub fn mark_all_dirty(&mut self) {
        self.full_redraw = true;
        self.regions.clear();
    }

    /// 标记特定区域为脏
    pub fn mark_dirty(&mut self, rect: Rect) {
        if self.full_redraw {
            return;
        }

        if rect.area() == 0 {
            return;
        }

        self.regions.push(rect);
    }

    /// 检查是否需要全屏重绘
    pub fn needs_full_redraw(&self) -> bool {
        self.full_redraw
    }

    /// 获取所有脏区域（合并优化后）
    pub fn get_regions(&self) -> Vec<Rect> {
        if self.full_redraw {
            return vec![];
        }

        self.merge_regions()
    }

    /// 清除脏标记
    pub fn clear(&mut self) {
        self.regions.clear();
        self.full_redraw = false;
    }

    /// 合并重叠或相邻的区域
    fn merge_regions(&self) -> Vec<Rect> {
        if self.regions.is_empty() {
            return vec![];
        }

        let mut merged = Vec::new();
        let mut to_merge: Vec<Rect> = self.regions.clone();

        // 简单的合并策略：合并重叠区域
        while let Some(rect) = to_merge.pop() {
            let mut current = rect;
            let mut changed = true;

            while changed {
                changed = false;
                to_merge.retain(|&other| {
                    if Self::should_merge(current, other) {
                        current = Self::merge_two_rects(current, other);
                        changed = true;
                        false
                    } else {
                        true
                    }
                });
            }

            merged.push(current);
        }

        merged
    }

    /// 判断两个矩形是否应该合并
    fn should_merge(a: Rect, b: Rect) -> bool {
        // 重叠检测
        let overlaps = a.intersects(b);

        // 相邻检测（带小间隙容忍）
        let adjacent = (a.x + a.width == b.x || b.x + b.width == a.x)
            && Self::ranges_overlap(a.y, a.height, b.y, b.height)
            || (a.y + a.height == b.y || b.y + b.height == a.y)
                && Self::ranges_overlap(a.x, a.width, b.x, b.width);

        // 小间隙合并（避免过多小区域）
        let small_gap = Self::gap_size(a, b) <= 2;

        overlaps || adjacent || small_gap
    }

    /// 合并两个矩形
    fn merge_two_rects(a: Rect, b: Rect) -> Rect {
        let x1 = a.x.min(b.x);
        let y1 = a.y.min(b.y);
        let x2 = (a.x + a.width).max(b.x + b.width);
        let y2 = (a.y + a.height).max(b.y + b.height);

        Rect {
            x: x1,
            y: y1,
            width: x2 - x1,
            height: y2 - y1,
        }
    }

    /// 计算两个矩形之间的间隙
    fn gap_size(a: Rect, b: Rect) -> u16 {
        let x_gap = if a.x + a.width < b.x {
            b.x - (a.x + a.width)
        } else if b.x + b.width < a.x {
            a.x - (b.x + b.width)
        } else {
            0
        };

        let y_gap = if a.y + a.height < b.y {
            b.y - (a.y + a.height)
        } else if b.y + b.height < a.y {
            a.y - (b.y + b.height)
        } else {
            0
        };

        x_gap.min(y_gap)
    }

    /// 检查两个范围是否重叠
    fn ranges_overlap(start1: u16, len1: u16, start2: u16, len2: u16) -> bool {
        let end1 = start1 + len1;
        let end2 = start2 + len2;
        start1 < end2 && start2 < end1
    }
}

impl Default for DirtyRegion {
    fn default() -> Self {
        Self::new()
    }
}

/// 渲染缓存 - 保存上一帧并检测变化
pub struct RenderCache {
    last_buffer: Option<Buffer>,
    dirty_tracker: DirtyRegion,
    frame_count: u64,
}

impl RenderCache {
    pub fn new() -> Self {
        Self {
            last_buffer: None,
            dirty_tracker: DirtyRegion::new(),
            frame_count: 0,
        }
    }

    /// 比较当前缓冲区和上一帧，标记脏区域
    pub fn detect_changes(&mut self, current: &Buffer) -> &DirtyRegion {
        self.frame_count += 1;

        // 第一帧总是全屏重绘
        if self.last_buffer.is_none() {
            self.dirty_tracker.mark_all_dirty();
            return &self.dirty_tracker;
        }

        let last = self.last_buffer.as_ref().unwrap();

        // 尺寸变化需要全屏重绘
        if current.area != last.area {
            self.dirty_tracker.mark_all_dirty();
            return &self.dirty_tracker;
        }

        // 重置脏区域
        self.dirty_tracker.clear();

        // 扫描变化的cell
        let area = current.area;
        let width = area.width as usize;
        let height = area.height as usize;

        // 使用行扫描检测变化块
        let mut dirty_rows = HashSet::new();

        for y in 0..height {
            for x in 0..width {
                let index = y * width + x;
                if index >= current.content.len() || index >= last.content.len() {
                    continue;
                }

                if current.content[index] != last.content[index] {
                    dirty_rows.insert(y as u16);
                    break;
                }
            }
        }

        // 将脏行转换为矩形区域
        if !dirty_rows.is_empty() {
            let mut rows: Vec<u16> = dirty_rows.into_iter().collect();
            rows.sort_unstable();

            // 合并连续的行
            let mut start_row = rows[0];
            let mut end_row = rows[0];

            for &row in &rows[1..] {
                if row == end_row + 1 {
                    end_row = row;
                } else {
                    // 创建区域
                    self.dirty_tracker.mark_dirty(Rect {
                        x: area.x,
                        y: area.y + start_row,
                        width: area.width,
                        height: end_row - start_row + 1,
                    });
                    start_row = row;
                    end_row = row;
                }
            }

            // 添加最后一个区域
            self.dirty_tracker.mark_dirty(Rect {
                x: area.x,
                y: area.y + start_row,
                width: area.width,
                height: end_row - start_row + 1,
            });
        }

        &self.dirty_tracker
    }

    /// 更新缓存的缓冲区
    pub fn update_cache(&mut self, buffer: Buffer) {
        self.last_buffer = Some(buffer);
    }

    /// 获取脏区域追踪器
    pub fn dirty_tracker(&self) -> &DirtyRegion {
        &self.dirty_tracker
    }

    /// 获取帧计数
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// 手动标记区域为脏
    pub fn mark_dirty(&mut self, rect: Rect) {
        self.dirty_tracker.mark_dirty(rect);
    }

    /// 强制全屏重绘
    pub fn invalidate_all(&mut self) {
        self.dirty_tracker.mark_all_dirty();
    }
}

impl Default for RenderCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dirty_region_basic() {
        let mut dirty = DirtyRegion::new();
        assert!(dirty.needs_full_redraw());

        dirty.clear();
        assert!(!dirty.needs_full_redraw());

        dirty.mark_dirty(Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 5,
        });
        assert_eq!(dirty.get_regions().len(), 1);
    }

    #[test]
    fn test_region_merging() {
        let mut dirty = DirtyRegion::new();
        dirty.clear();

        // 重叠区域
        dirty.mark_dirty(Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        });
        dirty.mark_dirty(Rect {
            x: 5,
            y: 5,
            width: 10,
            height: 10,
        });

        let merged = dirty.get_regions();
        assert_eq!(merged.len(), 1);
        assert_eq!(
            merged[0],
            Rect {
                x: 0,
                y: 0,
                width: 15,
                height: 15
            }
        );
    }

    #[test]
    fn test_adjacent_regions() {
        let mut dirty = DirtyRegion::new();
        dirty.clear();

        // 相邻区域
        dirty.mark_dirty(Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        });
        dirty.mark_dirty(Rect {
            x: 10,
            y: 0,
            width: 10,
            height: 10,
        });

        let merged = dirty.get_regions();
        assert_eq!(merged.len(), 1);
    }
}
