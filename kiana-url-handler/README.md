# kiana-url-handler

Cross-platform URL handler for custom URL schemes and deep linking, implemented as an MCP server using the `rmcp` crate.

## Features

- **Cross-platform support**: macOS, Windows, and Linux
- **MCP server**: Exposes URL handling capabilities via Model Context Protocol
- **Two MCP tools**:
  - `register_url_scheme`: Register a custom URL scheme handler
  - `handle_url`: Process incoming URLs with validation

## Dependencies

- `rmcp`: Official Rust MCP implementation with stdio transport
- `url`: URL parsing and validation
- Platform-specific:
  - macOS: `cocoa`, `objc` for NSApplication integration
  - Windows: `windows` crate for COM initialization
  - Linux: XDG desktop entry registration via `xdg-mime`

## Usage

### As MCP Server

Run the binary to start the MCP server on stdio:

```bash
cargo run --release
```

The server exposes two tools that can be called by MCP clients.

### Direct CLI

Register a Linux URL scheme:

```bash
kiana-url-handler register kiana
```

Check the current Linux handler:

```bash
kiana-url-handler status kiana
```

Handle a URL directly:

```bash
kiana-url-handler handle 'kiana://open?session=123'
```

### As Library

```rust
use kiana_url_handler::UrlHandler;

#[tokio::main]
async fn main() {
    let handler = UrlHandler::new();

    // Register a callback
    handler.register_callback(|url| {
        println!("Received URL: {}", url);
    }).await;

    // Handle URLs programmatically
    handler.handle_url(Parameters(HandleUrlRequest {
        url: "myapp://action".to_string(),
    })).await.unwrap();
}
```

## Testing

```bash
cargo test
```

## Notes

- This is a minimal implementation focused on URL validation and callback dispatch
- Linux registration writes a user-level `.desktop` file under `$XDG_DATA_HOME/applications` or `~/.local/share/applications`, then runs `xdg-mime default`
- macOS and Windows registration still need production-grade native registration beyond the current initialization shims
