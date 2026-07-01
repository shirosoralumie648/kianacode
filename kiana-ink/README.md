# Kiana Ink - Minimal Terminal UI Framework

A simplified Rust implementation of the ink terminal UI framework, using ratatui for rendering and taffy for flexbox layout.

## Architecture

The original TypeScript ink framework (100 files) has been distilled into core functionality:

### Core Modules

- **dom.rs** - DOM-like node tree (Box and Text nodes)
- **styles.rs** - Style definitions (colors, text styling, flexbox properties)
- **screen.rs** - Virtual screen buffer with cell grid
- **output.rs** - Output manager for writing styled text to screen
- **renderer.rs** - Main renderer using ratatui + taffy layout engine

### Key Differences from Original

**Original (TypeScript):**
- React reconciler for component tree
- Yoga layout engine (C++ binding)
- Custom ANSI terminal handling
- 100+ files with full event system, selection, scrolling

**Kiana (Rust):**
- Simple node tree (no React)
- Taffy layout engine (pure Rust)
- Ratatui for terminal rendering
- ~6 files with core rendering only

## Usage

```rust
use kiana_ink::{BoxStyle, Color, Ink, Node, TextStyle};
use taffy::prelude::*;

fn main() -> anyhow::Result<()> {
    let mut ink = Ink::new()?;

    let text_style = TextStyle {
        color: Some(Color::Green),
        bold: true,
        ..Default::default()
    };

    let mut box_style = BoxStyle::default();
    box_style.flex_direction = FlexDirection::Column;
    box_style.padding = Rect::all(LengthPercentage::Length(2.0));

    let root = Node::new_box(
        box_style,
        vec![
            Node::new_text("Hello World".to_string(), text_style),
            Node::new_text("Line 2".to_string(), TextStyle::default()),
        ],
    );

    ink.render(root)?;
    std::thread::sleep(std::time::Duration::from_secs(2));
    ink.shutdown()?;
    Ok(())
}
```

## Features

- ✅ Flexbox layout (via taffy)
- ✅ Text styling (color, bold, italic, underline)
- ✅ Box model (margin, padding, borders via taffy)
- ✅ Nested components
- ❌ Event handling (mouse, keyboard)
- ❌ Text selection
- ❌ Scrolling
- ❌ Focus management

## Run Example

```bash
cd rust-rewrite/kiana-ink
cargo run --example hello
```
