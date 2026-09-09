mod dom;
mod output;
mod renderer;
mod screen;
mod styles;

pub use dom::{BoxNode, Node, NodeType, TextNode};
pub use renderer::Renderer;
pub use styles::{BoxStyle, Color, TextStyle};

pub struct Ink {
    renderer: Renderer,
}

impl Ink {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            renderer: Renderer::new()?,
        })
    }

    pub fn render(&mut self, root: Node) -> anyhow::Result<()> {
        self.renderer.render(root)
    }

    pub fn shutdown(&mut self) -> anyhow::Result<()> {
        self.renderer.shutdown()
    }
}
