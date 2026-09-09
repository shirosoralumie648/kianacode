use crate::dom::{Node, NodeType};
use crate::output::Output;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, layout::Rect, widgets::Paragraph, Terminal};
use std::io::{self, Stdout};
use taffy::prelude::*;
use taffy::TaffyTree;

pub struct Renderer {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    taffy: TaffyTree,
}

impl Renderer {
    pub fn new() -> anyhow::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;

        Ok(Self {
            terminal,
            taffy: TaffyTree::new(),
        })
    }

    pub fn render(&mut self, root: Node) -> anyhow::Result<()> {
        let size = self.terminal.size()?;
        let available_space = Size {
            width: AvailableSpace::Definite(size.width as f32),
            height: AvailableSpace::Definite(size.height as f32),
        };

        self.taffy.clear();
        let root_id = root.build_layout(&mut self.taffy)?;
        self.taffy.compute_layout(root_id, available_space)?;

        let mut output = Output::new(size.width as usize, size.height as usize);
        self.render_node(&root, root_id, 0, 0, &mut output)?;

        self.paint_to_terminal(&output)?;
        Ok(())
    }

    fn render_node(
        &self,
        node: &Node,
        node_id: taffy::NodeId,
        offset_x: i32,
        offset_y: i32,
        output: &mut Output,
    ) -> anyhow::Result<()> {
        let layout = self.taffy.layout(node_id)?;
        let x = offset_x + layout.location.x as i32;
        let y = offset_y + layout.location.y as i32;

        match &node.node_type {
            NodeType::Text(text_node) => {
                if x >= 0 && y >= 0 {
                    output.write_text(x as usize, y as usize, &text_node.content, &text_node.style);
                }
            }
            NodeType::Box(_) => {
                let child_ids = self.taffy.children(node_id)?;
                for (child, child_id) in node.children.iter().zip(child_ids.iter()) {
                    self.render_node(child, *child_id, x, y, output)?;
                }
            }
        }

        Ok(())
    }

    fn paint_to_terminal(&mut self, output: &Output) -> anyhow::Result<()> {
        self.terminal.draw(|f| {
            let screen = output.get_screen();
            for (y, row) in screen.cells.iter().enumerate() {
                for (x, cell) in row.iter().enumerate() {
                    if !cell.content.is_empty() {
                        let style = output.get_style(cell.style_id).copied().unwrap_or_default();
                        let area = Rect::new(x as u16, y as u16, 1, 1);
                        let para = Paragraph::new(cell.content.clone()).style(style);
                        f.render_widget(para, area);
                    }
                }
            }
        })?;
        Ok(())
    }

    pub fn shutdown(&mut self) -> anyhow::Result<()> {
        disable_raw_mode()?;
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen)?;
        Ok(())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
