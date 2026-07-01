# kiana-modifiers

Cross-platform Rust library for querying currently pressed modifier keys (Shift, Ctrl, Alt, Cmd).

## Original Implementation

This is a rewrite of the private `modifiers-napi` shim using open-source Rust libraries.

Original API reference: `reference/shims/modifiers-napi`

## API

```rust
pub fn get_modifiers() -> Vec<String>
```
Returns array of currently pressed modifier keys: `["shift", "ctrl", "alt", "cmd"]`

```rust
pub fn is_modifier_pressed(modifier: &str) -> bool
```
Checks if a specific modifier key is currently pressed.

```rust
pub fn get_modifier_status() -> ModifierStatus
```
Returns platform/backend diagnostics, availability, the currently pressed
modifiers, and a failure message when polling cannot be initialized. `/doctor`
uses this so Linux users can tell the difference between "no modifier is
pressed" and "X11/XWayland polling is unavailable".

```rust
pub fn prewarm()
```
No-op in this implementation (original was for native module preloading).

## Platform Support

- **macOS**: Uses Core Graphics to query event flags
- **Linux**: Uses X11 to query keyboard state via `XQueryKeymap`
- **Windows**: Uses Win32 `GetAsyncKeyState` API
- **Other**: Returns empty results (graceful degradation)

## Dependencies

- `x11-dl` (Linux): Dynamic X11 library loading, no pkg-config needed
- `core-graphics` (macOS): Native event flag queries
- `windows` (Windows): Win32 keyboard state APIs

## Testing

```bash
cargo test -p kiana-modifiers
```
