# Kiana Components

Rust TUI component library converted from TypeScript/React/Ink components.

## Architecture

**Original (TypeScript)**: 406 React/Ink components
**Converted (Rust)**: 7 core ratatui modules

### Core Components

- **Theme** - Color theming system (maps Ink theme keys to ratatui colors)
- **Text** - Styled text rendering with theme support
- **Box** - Flexbox-like layout container (row/column, gap, padding, margin)
- **Spinner** - Animated loading indicators with customizable frames
- **ListItem** - Interactive list items (focused/selected/disabled states)
- **Dialog** - Modal dialogs with title, subtitle, content, borders
- **ProgressBar** - Progress indicators (gauge and line styles)

## Design Decisions

1. **Minimal Core**: Simplified 406 files to 7 essential modules
2. **No Direct Port**: Converted UI patterns, not literal translation
3. **Ratatui Native**: Uses ratatui widgets directly instead of custom rendering
4. **Stateless Components**: Builder pattern, no React-style state management

## Usage

```rust
use kiana_components::{Theme, Spinner, ListItem, Dialog};

let theme = Theme::default();
let mut spinner = Spinner::new();
spinner.tick();

let item = ListItem::new("Option 1")
    .focused(true)
    .selected(false);

let dialog = Dialog::new("Confirmation")
    .subtitle("Are you sure?")
    .color(theme.permission);
```

## Original Component Categories

- **Design System**: ThemedText, ThemedBox, ListItem, Dialog, Pane, ProgressBar → Core modules
- **Messages**: 30+ message types → Application layer (not UI components)
- **Permissions**: Permission dialogs → Application layer
- **Settings**: Config UI → Application layer
- **Tasks/Agents**: Task management UI → Application layer
- **Spinner**: Multiple spinner variants → Single `Spinner` + `SpinnerWithVerb`

## Not Converted

Complex application-specific components were intentionally excluded:
- Message rendering system (25+ message types)
- Permission system (15+ dialog types)
- Settings panels (5+ config screens)
- Task/Agent management (10+ views)
- MCP/Skills/Teams interfaces

These belong in the application layer, not UI components.
