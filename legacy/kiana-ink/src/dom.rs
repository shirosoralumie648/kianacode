use crate::styles::{BoxStyle, TextStyle};
use taffy::prelude::*;
use taffy::TaffyTree;

pub type NodeId = taffy::NodeId;

#[derive(Debug)]
pub enum NodeType {
    Box(BoxNode),
    Text(TextNode),
}

#[derive(Debug)]
pub struct Node {
    pub node_type: NodeType,
    pub children: Vec<Node>,
}

#[derive(Debug)]
pub struct BoxNode {
    pub style: BoxStyle,
}

#[derive(Debug)]
pub struct TextNode {
    pub content: String,
    pub style: TextStyle,
}

impl Node {
    pub fn new_box(style: BoxStyle, children: Vec<Node>) -> Self {
        Self {
            node_type: NodeType::Box(BoxNode { style }),
            children,
        }
    }

    pub fn new_text(content: String, style: TextStyle) -> Self {
        Self {
            node_type: NodeType::Text(TextNode { content, style }),
            children: Vec::new(),
        }
    }

    pub fn build_layout(&self, taffy: &mut TaffyTree) -> anyhow::Result<NodeId> {
        match &self.node_type {
            NodeType::Box(box_node) => {
                let child_ids: Vec<NodeId> = self
                    .children
                    .iter()
                    .map(|child| child.build_layout(taffy))
                    .collect::<Result<_, _>>()?;

                let node = taffy.new_with_children(box_node.style.to_taffy_style(), &child_ids)?;
                Ok(node)
            }
            NodeType::Text(text_node) => {
                let width = unicode_width::UnicodeWidthStr::width(text_node.content.as_str());
                let lines = text_node.content.lines().count().max(1);

                let style = Style {
                    size: Size {
                        width: Dimension::Length(width as f32),
                        height: Dimension::Length(lines as f32),
                    },
                    ..Default::default()
                };

                let node = taffy.new_leaf(style)?;
                Ok(node)
            }
        }
    }
}
