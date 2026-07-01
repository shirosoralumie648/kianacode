# kiana-chrome-mcp

Browser automation MCP server using chromiumoxide.

## Features

- Navigate to URLs
- Read page content
- Click elements
- Type into forms
- Execute JavaScript
- Take screenshots
- Tab management

## Usage

```bash
cargo build --release
./target/release/kiana-chrome-mcp
```

## MCP Tools

- `navigate`: Navigate to a URL
- `get_content`: Get page HTML content
- `click`: Click an element by CSS selector
- `type`: Type text into an element
- `evaluate`: Execute JavaScript in page context
- `screenshot`: Capture page screenshot (base64)
- `new_tab`: Open a new browser tab
- `close_tab`: Close current tab

## Configuration

Add to MCP settings:

```json
{
  "mcpServers": {
    "chrome": {
      "command": "/path/to/kiana-chrome-mcp"
    }
  }
}
```
