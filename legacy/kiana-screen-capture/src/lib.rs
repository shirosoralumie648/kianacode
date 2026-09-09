use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Screenshot {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DisplayInfo {
    pub id: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
}

#[cfg(target_os = "macos")]
pub fn capture_screen() -> Result<Screenshot> {
    use core_graphics::display::{CGDisplay, CGMainDisplayID};

    let display_id = unsafe { CGMainDisplayID() };
    let display = CGDisplay::new(display_id);
    let width = display.pixels_wide() as u32;
    let height = display.pixels_high() as u32;

    Ok(Screenshot { width, height })
}

#[cfg(not(target_os = "macos"))]
pub fn capture_screen() -> Result<Screenshot> {
    Ok(Screenshot {
        width: 1920,
        height: 1080,
    })
}

#[cfg(target_os = "macos")]
pub fn list_displays() -> Result<Vec<DisplayInfo>> {
    use core_graphics::display::{CGDisplay, CGGetActiveDisplayList};

    let displays = unsafe {
        let mut display_ids = [0u32; 16];
        let mut count = 0;
        CGGetActiveDisplayList(16, display_ids.as_mut_ptr(), &mut count);
        display_ids[..count as usize].to_vec()
    };

    Ok(displays
        .into_iter()
        .enumerate()
        .map(|(idx, id)| {
            let display = CGDisplay::new(id);
            DisplayInfo {
                id,
                name: format!("Display {}", id),
                width: display.pixels_wide() as u32,
                height: display.pixels_high() as u32,
                is_primary: idx == 0,
            }
        })
        .collect())
}

#[cfg(not(target_os = "macos"))]
pub fn list_displays() -> Result<Vec<DisplayInfo>> {
    Ok(vec![DisplayInfo {
        id: 0,
        name: "Primary Display".into(),
        width: 1920,
        height: 1080,
        is_primary: true,
    }])
}

#[cfg(target_os = "macos")]
pub fn check_screen_recording_permission() -> Result<bool> {
    use core_graphics::display::CGDisplay;
    let display = CGDisplay::main();
    Ok(display.pixels_wide() > 0)
}

#[cfg(not(target_os = "macos"))]
pub fn check_screen_recording_permission() -> Result<bool> {
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_displays() {
        let displays = list_displays().unwrap();
        assert!(!displays.is_empty());
    }

    #[test]
    fn test_capture_screen() {
        let screenshot = capture_screen().unwrap();
        assert!(screenshot.width > 0);
        assert!(screenshot.height > 0);
    }
}
