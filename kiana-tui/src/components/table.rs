use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Row, Table as RatatuiTable, TableState},
    Frame,
};
use std::cmp::Ordering;

/// Column alignment options
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Alignment {
    Left,
    Center,
    Right,
}

/// Sort direction for columns
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SortDirection {
    Ascending,
    Descending,
    None,
}

impl SortDirection {
    pub fn toggle(&self) -> Self {
        match self {
            SortDirection::None | SortDirection::Descending => SortDirection::Ascending,
            SortDirection::Ascending => SortDirection::Descending,
        }
    }

    pub fn indicator(&self) -> &str {
        match self {
            SortDirection::Ascending => "↑",
            SortDirection::Descending => "↓",
            SortDirection::None => "",
        }
    }
}

/// Column definition with metadata
#[derive(Clone, Debug)]
pub struct Column {
    pub header: String,
    pub width: Constraint,
    pub alignment: Alignment,
    pub sortable: bool,
    pub sort_key: Option<String>, // Unique identifier for sorting
}

impl Column {
    pub fn new(header: impl Into<String>) -> Self {
        Self {
            header: header.into(),
            width: Constraint::Percentage(10),
            alignment: Alignment::Left,
            sortable: false,
            sort_key: None,
        }
    }

    pub fn width(mut self, width: Constraint) -> Self {
        self.width = width;
        self
    }

    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    pub fn sortable(mut self, sort_key: impl Into<String>) -> Self {
        self.sortable = true;
        self.sort_key = Some(sort_key.into());
        self
    }
}

/// A single row of data
pub type TableRow = Vec<String>;

/// Table view component with sorting, filtering, and selection
pub struct TableView {
    columns: Vec<Column>,
    rows: Vec<TableRow>,
    filtered_indices: Vec<usize>, // Indices into rows after filtering
    state: TableState,
    sort_column: Option<usize>,
    sort_direction: SortDirection,
    filter: Option<String>,
    title: Option<String>,
    show_header: bool,
    highlight_style: Style,
    header_style: Style,
}

impl TableView {
    pub fn new(columns: Vec<Column>) -> Self {
        let filtered_indices = Vec::new();
        Self {
            columns,
            rows: Vec::new(),
            filtered_indices,
            state: TableState::default(),
            sort_column: None,
            sort_direction: SortDirection::None,
            filter: None,
            title: None,
            show_header: true,
            highlight_style: Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
            header_style: Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = style;
        self
    }

    pub fn with_header_style(mut self, style: Style) -> Self {
        self.header_style = style;
        self
    }

    pub fn show_header(mut self, show: bool) -> Self {
        self.show_header = show;
        self
    }

    /// Set table data
    pub fn set_rows(&mut self, rows: Vec<TableRow>) {
        self.rows = rows;
        self.rebuild_filtered_indices();
        self.apply_sort();
    }

    /// Add a single row
    pub fn push_row(&mut self, row: TableRow) {
        let index = self.rows.len();
        self.rows.push(row);

        if self.matches_filter(index) {
            self.filtered_indices.push(index);
        }

        if self.sort_column.is_some() {
            self.apply_sort();
        }
    }

    /// Get current row count (after filtering)
    pub fn row_count(&self) -> usize {
        self.filtered_indices.len()
    }

    /// Get selected row index (in filtered view)
    pub fn selected(&self) -> Option<usize> {
        self.state.selected()
    }

    /// Get selected row data
    pub fn selected_row(&self) -> Option<&TableRow> {
        self.state
            .selected()
            .and_then(|i| self.filtered_indices.get(i))
            .and_then(|&row_idx| self.rows.get(row_idx))
    }

    /// Select row by index
    pub fn select(&mut self, index: Option<usize>) {
        self.state.select(index);
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }

        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.filtered_indices.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    /// Move selection up
    pub fn select_previous(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }

        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.filtered_indices.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    /// Select first row
    pub fn select_first(&mut self) {
        if !self.filtered_indices.is_empty() {
            self.state.select(Some(0));
        }
    }

    /// Select last row
    pub fn select_last(&mut self) {
        if !self.filtered_indices.is_empty() {
            self.state.select(Some(self.filtered_indices.len() - 1));
        }
    }

    /// Page down
    pub fn page_down(&mut self, page_size: usize) {
        if self.filtered_indices.is_empty() {
            return;
        }

        let i = self.state.selected().unwrap_or(0);
        let new_i = (i + page_size).min(self.filtered_indices.len() - 1);
        self.state.select(Some(new_i));
    }

    /// Page up
    pub fn page_up(&mut self, page_size: usize) {
        if self.filtered_indices.is_empty() {
            return;
        }

        let i = self.state.selected().unwrap_or(0);
        let new_i = i.saturating_sub(page_size);
        self.state.select(Some(new_i));
    }

    /// Sort by column index
    pub fn sort_by_column(&mut self, column_index: usize) {
        if column_index >= self.columns.len() {
            return;
        }

        if !self.columns[column_index].sortable {
            return;
        }

        // Toggle sort direction if same column
        if self.sort_column == Some(column_index) {
            self.sort_direction = self.sort_direction.toggle();
        } else {
            self.sort_column = Some(column_index);
            self.sort_direction = SortDirection::Ascending;
        }

        self.apply_sort();
    }

    /// Apply current sort settings
    fn apply_sort(&mut self) {
        let Some(col_idx) = self.sort_column else {
            return;
        };

        if col_idx >= self.columns.len() {
            return;
        }

        let direction = self.sort_direction;
        if direction == SortDirection::None {
            return;
        }

        let rows = &self.rows;
        self.filtered_indices.sort_by(|&a, &b| {
            let cmp = rows
                .get(a)
                .and_then(|row_a| row_a.get(col_idx))
                .zip(rows.get(b).and_then(|row_b| row_b.get(col_idx)))
                .map(|(val_a, val_b)| {
                    // Try numeric comparison first
                    match (val_a.parse::<f64>(), val_b.parse::<f64>()) {
                        (Ok(num_a), Ok(num_b)) => {
                            num_a.partial_cmp(&num_b).unwrap_or(Ordering::Equal)
                        }
                        _ => val_a.cmp(val_b),
                    }
                })
                .unwrap_or(Ordering::Equal);

            match direction {
                SortDirection::Ascending => cmp,
                SortDirection::Descending => cmp.reverse(),
                SortDirection::None => Ordering::Equal,
            }
        });
    }

    /// Set filter string
    pub fn set_filter(&mut self, filter: Option<String>) {
        self.filter = filter;
        self.rebuild_filtered_indices();
        self.apply_sort();

        // Reset selection if current selection is out of bounds
        if let Some(selected) = self.state.selected() {
            if selected >= self.filtered_indices.len() {
                self.select_first();
            }
        }
    }

    /// Check if a row matches current filter
    fn matches_filter(&self, row_index: usize) -> bool {
        let Some(filter) = &self.filter else {
            return true;
        };

        if filter.is_empty() {
            return true;
        }

        let Some(row) = self.rows.get(row_index) else {
            return false;
        };

        let filter_lower = filter.to_lowercase();
        row.iter()
            .any(|cell| cell.to_lowercase().contains(&filter_lower))
    }

    /// Rebuild filtered indices based on current filter
    fn rebuild_filtered_indices(&mut self) {
        self.filtered_indices.clear();
        for (idx, _) in self.rows.iter().enumerate() {
            if self.matches_filter(idx) {
                self.filtered_indices.push(idx);
            }
        }
    }

    /// Clear all data
    pub fn clear(&mut self) {
        self.rows.clear();
        self.filtered_indices.clear();
        self.state.select(None);
    }

    /// Get current sort state
    pub fn sort_state(&self) -> (Option<usize>, SortDirection) {
        (self.sort_column, self.sort_direction)
    }

    /// Render the table
    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        // Build header with sort indicators
        let header_cells = self.columns.iter().enumerate().map(|(i, col)| {
            let mut header_text = col.header.clone();

            if self.sort_column == Some(i) {
                header_text.push(' ');
                header_text.push_str(self.sort_direction.indicator());
            }

            header_text
        });

        let header = Row::new(header_cells).style(self.header_style);

        // Build rows from filtered data
        let rows: Vec<Row> = self
            .filtered_indices
            .iter()
            .filter_map(|&row_idx| self.rows.get(row_idx))
            .map(|row| Row::new(row.iter().map(|s| s.as_str())))
            .collect();

        // Create widths from column definitions
        let widths: Vec<Constraint> = self.columns.iter().map(|col| col.width).collect();

        let mut table = RatatuiTable::new(rows, widths).row_highlight_style(self.highlight_style);

        if self.show_header {
            table = table.header(header);
        }

        if let Some(title) = &self.title {
            let block = Block::default().borders(Borders::ALL).title(title.as_str());
            table = table.block(block);
        }

        f.render_stateful_widget(table, area, &mut self.state);
    }
}

impl Default for TableView {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_table() -> TableView {
        let columns = vec![
            Column::new("Name")
                .width(Constraint::Length(20))
                .sortable("name"),
            Column::new("Age")
                .width(Constraint::Length(10))
                .sortable("age"),
            Column::new("City")
                .width(Constraint::Length(15))
                .sortable("city"),
        ];

        TableView::new(columns)
    }

    #[test]
    fn test_column_creation() {
        let col = Column::new("Test")
            .width(Constraint::Length(20))
            .alignment(Alignment::Center)
            .sortable("test_key");

        assert_eq!(col.header, "Test");
        assert_eq!(col.width, Constraint::Length(20));
        assert_eq!(col.alignment, Alignment::Center);
        assert!(col.sortable);
        assert_eq!(col.sort_key, Some("test_key".to_string()));
    }

    #[test]
    fn test_sort_direction_toggle() {
        assert_eq!(SortDirection::None.toggle(), SortDirection::Ascending);
        assert_eq!(SortDirection::Ascending.toggle(), SortDirection::Descending);
        assert_eq!(SortDirection::Descending.toggle(), SortDirection::Ascending);
    }

    #[test]
    fn test_table_creation() {
        let table = create_test_table();
        assert_eq!(table.row_count(), 0);
        assert_eq!(table.selected(), None);
    }

    #[test]
    fn test_set_rows() {
        let mut table = create_test_table();

        let rows = vec![
            vec!["Alice".to_string(), "30".to_string(), "NYC".to_string()],
            vec!["Bob".to_string(), "25".to_string(), "LA".to_string()],
        ];

        table.set_rows(rows);
        assert_eq!(table.row_count(), 2);
    }

    #[test]
    fn test_push_row() {
        let mut table = create_test_table();

        table.push_row(vec![
            "Alice".to_string(),
            "30".to_string(),
            "NYC".to_string(),
        ]);
        assert_eq!(table.row_count(), 1);

        table.push_row(vec!["Bob".to_string(), "25".to_string(), "LA".to_string()]);
        assert_eq!(table.row_count(), 2);
    }

    #[test]
    fn test_selection() {
        let mut table = create_test_table();

        table.set_rows(vec![
            vec!["Alice".to_string(), "30".to_string(), "NYC".to_string()],
            vec!["Bob".to_string(), "25".to_string(), "LA".to_string()],
            vec!["Charlie".to_string(), "35".to_string(), "SF".to_string()],
        ]);

        assert_eq!(table.selected(), None);

        table.select(Some(1));
        assert_eq!(table.selected(), Some(1));

        table.select_next();
        assert_eq!(table.selected(), Some(2));

        table.select_next(); // Wraps to 0
        assert_eq!(table.selected(), Some(0));

        table.select_previous(); // Wraps to last
        assert_eq!(table.selected(), Some(2));
    }

    #[test]
    fn test_select_first_last() {
        let mut table = create_test_table();

        table.set_rows(vec![
            vec!["Alice".to_string(), "30".to_string(), "NYC".to_string()],
            vec!["Bob".to_string(), "25".to_string(), "LA".to_string()],
            vec!["Charlie".to_string(), "35".to_string(), "SF".to_string()],
        ]);

        table.select_first();
        assert_eq!(table.selected(), Some(0));

        table.select_last();
        assert_eq!(table.selected(), Some(2));
    }

    #[test]
    fn test_selected_row() {
        let mut table = create_test_table();

        table.set_rows(vec![
            vec!["Alice".to_string(), "30".to_string(), "NYC".to_string()],
            vec!["Bob".to_string(), "25".to_string(), "LA".to_string()],
        ]);

        table.select(Some(1));
        let row = table.selected_row();
        assert!(row.is_some());
        assert_eq!(row.unwrap()[0], "Bob");
    }

    #[test]
    fn test_sorting() {
        let mut table = create_test_table();

        table.set_rows(vec![
            vec!["Charlie".to_string(), "35".to_string(), "SF".to_string()],
            vec!["Alice".to_string(), "30".to_string(), "NYC".to_string()],
            vec!["Bob".to_string(), "25".to_string(), "LA".to_string()],
        ]);

        // Sort by name (column 0)
        table.sort_by_column(0);
        let (col, dir) = table.sort_state();
        assert_eq!(col, Some(0));
        assert_eq!(dir, SortDirection::Ascending);

        // First row should be Alice
        let first_idx = table.filtered_indices[0];
        assert_eq!(table.rows[first_idx][0], "Alice");

        // Toggle to descending
        table.sort_by_column(0);
        assert_eq!(table.sort_state().1, SortDirection::Descending);

        // First row should be Charlie
        let first_idx = table.filtered_indices[0];
        assert_eq!(table.rows[first_idx][0], "Charlie");
    }

    #[test]
    fn test_numeric_sorting() {
        let mut table = create_test_table();

        table.set_rows(vec![
            vec!["Alice".to_string(), "30".to_string(), "NYC".to_string()],
            vec!["Bob".to_string(), "25".to_string(), "LA".to_string()],
            vec!["Charlie".to_string(), "100".to_string(), "SF".to_string()],
        ]);

        // Sort by age (column 1) - should sort numerically
        table.sort_by_column(1);

        let ages: Vec<i32> = table
            .filtered_indices
            .iter()
            .map(|&idx| table.rows[idx][1].parse().unwrap())
            .collect();

        assert_eq!(ages, vec![25, 30, 100]);
    }

    #[test]
    fn test_filtering() {
        let mut table = create_test_table();

        table.set_rows(vec![
            vec!["Alice".to_string(), "30".to_string(), "NYC".to_string()],
            vec!["Bob".to_string(), "25".to_string(), "LA".to_string()],
            vec!["Charlie".to_string(), "35".to_string(), "SF".to_string()],
        ]);

        assert_eq!(table.row_count(), 3);

        // Filter for "alice"
        table.set_filter(Some("alice".to_string()));
        assert_eq!(table.row_count(), 1);

        // Filter for "a" - should match Alice, LA, and Charlie
        table.set_filter(Some("a".to_string()));
        assert_eq!(table.row_count(), 3);

        // Clear filter
        table.set_filter(None);
        assert_eq!(table.row_count(), 3);
    }

    #[test]
    fn test_filter_with_selection() {
        let mut table = create_test_table();

        table.set_rows(vec![
            vec!["Alice".to_string(), "30".to_string(), "NYC".to_string()],
            vec!["Bob".to_string(), "25".to_string(), "LA".to_string()],
            vec!["Charlie".to_string(), "35".to_string(), "SF".to_string()],
        ]);

        table.select(Some(2)); // Select Charlie
        assert_eq!(table.selected(), Some(2));

        // Filter to only show Alice
        table.set_filter(Some("alice".to_string()));
        assert_eq!(table.row_count(), 1);

        // Selection should reset to first since previous selection is out of bounds
        assert_eq!(table.selected(), Some(0));
    }

    #[test]
    fn test_clear() {
        let mut table = create_test_table();

        table.set_rows(vec![vec![
            "Alice".to_string(),
            "30".to_string(),
            "NYC".to_string(),
        ]]);

        table.select(Some(0));
        assert_eq!(table.row_count(), 1);

        table.clear();
        assert_eq!(table.row_count(), 0);
        assert_eq!(table.selected(), None);
    }

    #[test]
    fn test_page_navigation() {
        let mut table = create_test_table();

        let rows: Vec<TableRow> = (0..100)
            .map(|i| vec![format!("User{}", i), format!("{}", i), format!("City{}", i)])
            .collect();

        table.set_rows(rows);
        table.select_first();
        assert_eq!(table.selected(), Some(0));

        table.page_down(10);
        assert_eq!(table.selected(), Some(10));

        table.page_down(100); // Should clamp to last
        assert_eq!(table.selected(), Some(99));

        table.page_up(10);
        assert_eq!(table.selected(), Some(89));

        table.page_up(100); // Should clamp to first
        assert_eq!(table.selected(), Some(0));
    }
}
