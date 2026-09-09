use ratatui::layout::{Constraint, Direction, Layout, Rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexDirection {
    Row,
    Column,
}

pub struct Box {
    pub direction: FlexDirection,
    pub gap: u16,
    pub padding: (u16, u16, u16, u16), // top, right, bottom, left
    pub margin: (u16, u16, u16, u16),
}

impl Box {
    pub fn new() -> Self {
        Self {
            direction: FlexDirection::Column,
            gap: 0,
            padding: (0, 0, 0, 0),
            margin: (0, 0, 0, 0),
        }
    }

    pub fn direction(mut self, direction: FlexDirection) -> Self {
        self.direction = direction;
        self
    }

    pub fn gap(mut self, gap: u16) -> Self {
        self.gap = gap;
        self
    }

    pub fn padding(mut self, top: u16, right: u16, bottom: u16, left: u16) -> Self {
        self.padding = (top, right, bottom, left);
        self
    }

    pub fn margin(mut self, top: u16, right: u16, bottom: u16, left: u16) -> Self {
        self.margin = (top, right, bottom, left);
        self
    }

    pub fn apply_constraints(&self, area: Rect, children_count: usize) -> Vec<Rect> {
        if children_count == 0 {
            return vec![];
        }

        // Apply margins
        let inner = Rect {
            x: area.x + self.margin.3,
            y: area.y + self.margin.0,
            width: area.width.saturating_sub(self.margin.1 + self.margin.3),
            height: area.height.saturating_sub(self.margin.0 + self.margin.2),
        };

        // Apply padding
        let content = Rect {
            x: inner.x + self.padding.3,
            y: inner.y + self.padding.0,
            width: inner.width.saturating_sub(self.padding.1 + self.padding.3),
            height: inner.height.saturating_sub(self.padding.0 + self.padding.2),
        };

        let direction = match self.direction {
            FlexDirection::Row => Direction::Horizontal,
            FlexDirection::Column => Direction::Vertical,
        };

        let constraints: Vec<Constraint> =
            (0..children_count).map(|_| Constraint::Length(1)).collect();

        Layout::default()
            .direction(direction)
            .constraints(constraints)
            .split(content)
            .to_vec()
    }
}

impl Default for Box {
    fn default() -> Self {
        Self::new()
    }
}
