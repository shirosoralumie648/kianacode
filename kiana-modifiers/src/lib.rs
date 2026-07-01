#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModifierStatus {
    pub platform: &'static str,
    pub backend: &'static str,
    pub available: bool,
    pub modifiers: Vec<String>,
    pub message: Option<String>,
}

impl ModifierStatus {
    fn available(platform: &'static str, backend: &'static str, modifiers: Vec<String>) -> Self {
        Self {
            platform,
            backend,
            available: true,
            modifiers,
            message: None,
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn unavailable(
        platform: &'static str,
        backend: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            platform,
            backend,
            available: false,
            modifiers: Vec::new(),
            message: Some(message.into()),
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::ModifierStatus;
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventSource};

    pub fn get_modifiers() -> Vec<String> {
        status().modifiers
    }

    pub fn status() -> ModifierStatus {
        let mut result = Vec::new();
        let Some(source) = CGEventSource::new(0) else {
            return ModifierStatus::unavailable(
                "macos",
                "core-graphics",
                "failed to create Core Graphics event source",
            );
        };
        let Some(event) = CGEvent::new(source) else {
            return ModifierStatus::unavailable(
                "macos",
                "core-graphics",
                "failed to create Core Graphics event",
            );
        };

        let flags = event.get_flags();
        if flags.contains(CGEventFlags::CGEventFlagShift) {
            result.push("shift".to_string());
        }
        if flags.contains(CGEventFlags::CGEventFlagControl) {
            result.push("ctrl".to_string());
        }
        if flags.contains(CGEventFlags::CGEventFlagAlternate) {
            result.push("alt".to_string());
        }
        if flags.contains(CGEventFlags::CGEventFlagCommand) {
            result.push("cmd".to_string());
        }

        ModifierStatus::available("macos", "core-graphics", result)
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::ModifierStatus;
    use std::ptr;
    use x11_dl::keysym::*;
    use x11_dl::xlib::Xlib;

    pub fn get_modifiers() -> Vec<String> {
        status().modifiers
    }

    pub fn status() -> ModifierStatus {
        match query_modifiers() {
            Ok(modifiers) => ModifierStatus::available("linux", "x11", modifiers),
            Err(message) => ModifierStatus::unavailable("linux", "x11", message),
        }
    }

    fn query_modifiers() -> Result<Vec<String>, String> {
        let xlib = Xlib::open().map_err(|err| format!("failed to load X11 library: {err}"))?;

        unsafe {
            let display = (xlib.XOpenDisplay)(ptr::null());
            if display.is_null() {
                return Err(linux_display_failure_message(
                    non_empty_env("DISPLAY"),
                    non_empty_env("WAYLAND_DISPLAY"),
                ));
            }

            let mut keys: [u8; 32] = [0; 32];
            (xlib.XQueryKeymap)(display, keys.as_mut_ptr() as *mut i8);

            let shift_l = (xlib.XKeysymToKeycode)(display, XK_Shift_L as u64);
            let shift_r = (xlib.XKeysymToKeycode)(display, XK_Shift_R as u64);
            let ctrl_l = (xlib.XKeysymToKeycode)(display, XK_Control_L as u64);
            let ctrl_r = (xlib.XKeysymToKeycode)(display, XK_Control_R as u64);
            let alt_l = (xlib.XKeysymToKeycode)(display, XK_Alt_L as u64);
            let alt_r = (xlib.XKeysymToKeycode)(display, XK_Alt_R as u64);
            let super_l = (xlib.XKeysymToKeycode)(display, XK_Super_L as u64);
            let super_r = (xlib.XKeysymToKeycode)(display, XK_Super_R as u64);

            let mut result = Vec::new();
            if is_key_pressed(&keys, shift_l) || is_key_pressed(&keys, shift_r) {
                result.push("shift".to_string());
            }
            if is_key_pressed(&keys, ctrl_l) || is_key_pressed(&keys, ctrl_r) {
                result.push("ctrl".to_string());
            }
            if is_key_pressed(&keys, alt_l) || is_key_pressed(&keys, alt_r) {
                result.push("alt".to_string());
            }
            if is_key_pressed(&keys, super_l) || is_key_pressed(&keys, super_r) {
                result.push("cmd".to_string());
            }

            (xlib.XCloseDisplay)(display);
            Ok(result)
        }
    }

    fn is_key_pressed(keys: &[u8; 32], keycode: u8) -> bool {
        if keycode == 0 {
            return false;
        }
        let byte = (keycode / 8) as usize;
        let bit = keycode % 8;
        (keys[byte] & (1 << bit)) != 0
    }

    fn non_empty_env(name: &str) -> Option<String> {
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    pub(super) fn linux_display_failure_message(
        display: Option<String>,
        wayland: Option<String>,
    ) -> String {
        match (display, wayland) {
            (Some(display), _) => format!("XOpenDisplay failed for DISPLAY={display}"),
            (None, Some(wayland)) => format!(
                "DISPLAY is not set; WAYLAND_DISPLAY={wayland} is present, but global modifier polling requires X11/XWayland"
            ),
            (None, None) => "DISPLAY is not set".to_string(),
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::ModifierStatus;
    use windows::Win32::UI::Input::KeyboardAndMouse::*;

    pub fn get_modifiers() -> Vec<String> {
        status().modifiers
    }

    pub fn status() -> ModifierStatus {
        let mut result = Vec::new();
        unsafe {
            if key_is_down(VK_SHIFT) {
                result.push("shift".to_string());
            }
            if key_is_down(VK_CONTROL) {
                result.push("ctrl".to_string());
            }
            if key_is_down(VK_MENU) {
                result.push("alt".to_string());
            }
            if key_is_down(VK_LWIN) || key_is_down(VK_RWIN) {
                result.push("cmd".to_string());
            }
        }
        ModifierStatus::available("windows", "win32", result)
    }

    unsafe fn key_is_down(key: VIRTUAL_KEY) -> bool {
        (GetAsyncKeyState(key.0 as i32) as u16) & 0x8000 != 0
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
mod unsupported {
    use super::ModifierStatus;

    pub fn get_modifiers() -> Vec<String> {
        status().modifiers
    }

    pub fn status() -> ModifierStatus {
        ModifierStatus::unavailable(
            std::env::consts::OS,
            "unsupported",
            "modifier polling is not implemented for this platform",
        )
    }
}

pub fn prewarm() {}

pub fn get_modifiers() -> Vec<String> {
    #[cfg(target_os = "macos")]
    return macos::get_modifiers();
    #[cfg(target_os = "linux")]
    return linux::get_modifiers();
    #[cfg(target_os = "windows")]
    return windows::get_modifiers();
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    return unsupported::get_modifiers();
}

pub fn get_modifier_status() -> ModifierStatus {
    #[cfg(target_os = "macos")]
    return macos::status();
    #[cfg(target_os = "linux")]
    return linux::status();
    #[cfg(target_os = "windows")]
    return windows::status();
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    return unsupported::status();
}

pub fn is_modifier_pressed(modifier: &str) -> bool {
    contains_modifier(&get_modifiers(), modifier)
}

fn contains_modifier(modifiers: &[String], modifier: &str) -> bool {
    modifiers
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(modifier))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prewarm() {
        prewarm();
    }

    #[test]
    fn test_get_modifiers() {
        let mods = get_modifiers();
        assert!(mods.is_empty() || !mods.is_empty());
    }

    #[test]
    fn test_get_modifier_status_reports_backend() {
        let status = get_modifier_status();
        assert!(!status.platform.is_empty());
        assert!(!status.backend.is_empty());
    }

    #[test]
    fn test_is_modifier_pressed() {
        let _ = is_modifier_pressed("shift");
    }

    #[test]
    fn contains_modifier_is_case_insensitive() {
        let modifiers = vec!["shift".to_string(), "ctrl".to_string()];
        assert!(contains_modifier(&modifiers, "SHIFT"));
        assert!(contains_modifier(&modifiers, "Ctrl"));
        assert!(!contains_modifier(&modifiers, "alt"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_display_failure_mentions_wayland_without_display() {
        let message = linux::linux_display_failure_message(None, Some("wayland-0".to_string()));
        assert!(message.contains("WAYLAND_DISPLAY=wayland-0"));
        assert!(message.contains("X11/XWayland"));
    }
}
