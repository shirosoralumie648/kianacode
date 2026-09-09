use enigo::{Button, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use serde::{Deserialize, Serialize};
use std::thread;
use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum InputError {
    #[error("Failed to initialize input controller")]
    InitError,
    #[error("Invalid coordinates: x={0}, y={1}")]
    InvalidCoordinates(i32, i32),
    #[error("Platform operation failed: {0}")]
    PlatformError(String),
}

pub type Result<T> = std::result::Result<T, InputError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

pub struct ComputerInput {
    enigo: Enigo,
}

impl ComputerInput {
    pub fn new() -> Result<Self> {
        Ok(Self {
            enigo: Enigo::new(&Settings::default()).map_err(|_| InputError::InitError)?,
        })
    }

    pub fn mouse_move(&mut self, x: i32, y: i32) -> Result<()> {
        self.enigo
            .move_mouse(x, y, enigo::Coordinate::Abs)
            .map_err(|e| InputError::PlatformError(e.to_string()))
    }

    pub fn mouse_click(&mut self, button: MouseButton) -> Result<()> {
        let btn = match button {
            MouseButton::Left => Button::Left,
            MouseButton::Right => Button::Right,
            MouseButton::Middle => Button::Middle,
        };
        self.enigo
            .button(btn, Direction::Click)
            .map_err(|e| InputError::PlatformError(e.to_string()))
    }

    pub fn mouse_down(&mut self, button: MouseButton) -> Result<()> {
        let btn = match button {
            MouseButton::Left => Button::Left,
            MouseButton::Right => Button::Right,
            MouseButton::Middle => Button::Middle,
        };
        self.enigo
            .button(btn, Direction::Press)
            .map_err(|e| InputError::PlatformError(e.to_string()))
    }

    pub fn mouse_up(&mut self, button: MouseButton) -> Result<()> {
        let btn = match button {
            MouseButton::Left => Button::Left,
            MouseButton::Right => Button::Right,
            MouseButton::Middle => Button::Middle,
        };
        self.enigo
            .button(btn, Direction::Release)
            .map_err(|e| InputError::PlatformError(e.to_string()))
    }

    pub fn drag(&mut self, from: Point, to: Point) -> Result<()> {
        self.mouse_move(from.x, from.y)?;
        thread::sleep(Duration::from_millis(10));
        self.mouse_down(MouseButton::Left)?;
        thread::sleep(Duration::from_millis(50));
        self.mouse_move(to.x, to.y)?;
        thread::sleep(Duration::from_millis(10));
        self.mouse_up(MouseButton::Left)
    }

    pub fn scroll(&mut self, delta_y: i32) -> Result<()> {
        self.enigo
            .scroll(delta_y, enigo::Axis::Vertical)
            .map_err(|e| InputError::PlatformError(e.to_string()))
    }

    pub fn key_press(&mut self, key: &str) -> Result<()> {
        if let Some(k) = parse_key(key) {
            self.enigo
                .key(k, Direction::Click)
                .map_err(|e| InputError::PlatformError(e.to_string()))
        } else {
            self.enigo
                .text(key)
                .map_err(|e| InputError::PlatformError(e.to_string()))
        }
    }

    pub fn key_down(&mut self, key: &str) -> Result<()> {
        if let Some(k) = parse_key(key) {
            self.enigo
                .key(k, Direction::Press)
                .map_err(|e| InputError::PlatformError(e.to_string()))
        } else {
            Err(InputError::PlatformError("Invalid key".to_string()))
        }
    }

    pub fn key_up(&mut self, key: &str) -> Result<()> {
        if let Some(k) = parse_key(key) {
            self.enigo
                .key(k, Direction::Release)
                .map_err(|e| InputError::PlatformError(e.to_string()))
        } else {
            Err(InputError::PlatformError("Invalid key".to_string()))
        }
    }

    pub fn type_text(&mut self, text: &str) -> Result<()> {
        self.enigo
            .text(text)
            .map_err(|e| InputError::PlatformError(e.to_string()))
    }

    pub fn get_foreground_app(&self) -> Result<String> {
        Err(InputError::PlatformError(
            "Foreground app detection requires platform-specific dependencies".to_string(),
        ))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

fn parse_key(key: &str) -> Option<Key> {
    match key.to_lowercase().as_str() {
        "return" | "enter" => Some(Key::Return),
        "tab" => Some(Key::Tab),
        "space" => Some(Key::Space),
        "backspace" => Some(Key::Backspace),
        "delete" => Some(Key::Delete),
        "escape" | "esc" => Some(Key::Escape),
        "up" | "uparrow" => Some(Key::UpArrow),
        "down" | "downarrow" => Some(Key::DownArrow),
        "left" | "leftarrow" => Some(Key::LeftArrow),
        "right" | "rightarrow" => Some(Key::RightArrow),
        "home" => Some(Key::Home),
        "end" => Some(Key::End),
        "pageup" => Some(Key::PageUp),
        "pagedown" => Some(Key::PageDown),
        "shift" => Some(Key::Shift),
        "control" | "ctrl" => Some(Key::Control),
        "alt" | "option" => Some(Key::Alt),
        "meta" | "command" | "cmd" => Some(Key::Meta),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init() {
        let input = ComputerInput::new();
        assert!(input.is_ok());
    }

    #[test]
    fn test_parse_key() {
        assert!(parse_key("enter").is_some());
        assert!(parse_key("tab").is_some());
        assert!(parse_key("shift").is_some());
        assert!(parse_key("xyz").is_none());
    }

    #[test]
    fn test_point_serialization() {
        let point = Point { x: 100, y: 200 };
        let json = serde_json::to_string(&point).unwrap();
        assert!(json.contains("100"));
        assert!(json.contains("200"));
    }
}
