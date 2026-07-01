use kiana_ink::{BoxStyle, Color, Ink, Node, TextStyle};
use taffy::prelude::*;

fn main() -> anyhow::Result<()> {
    let mut ink = Ink::new()?;

    // Create a simple UI tree
    let text_style = TextStyle {
        color: Some(Color::Green),
        bold: true,
        ..Default::default()
    };

    let mut box_style = BoxStyle::default();
    box_style.flex_direction = FlexDirection::Column;
    box_style.padding = Rect {
        left: LengthPercentage::Length(2.0),
        right: LengthPercentage::Length(2.0),
        top: LengthPercentage::Length(1.0),
        bottom: LengthPercentage::Length(1.0),
    };

    let root = Node::new_box(
        box_style,
        vec![
            Node::new_text("Hello from Kiana Ink!".to_string(), text_style.clone()),
            Node::new_text("Minimal TUI framework".to_string(), TextStyle::default()),
        ],
    );

    ink.render(root)?;

    // Wait for user input
    std::thread::sleep(std::time::Duration::from_secs(3));

    ink.shutdown()?;
    Ok(())
}
