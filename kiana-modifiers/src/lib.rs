//! # 文件职责
//!
//! 本模块按编译目标读取当前键盘修饰键状态，只报告 Shift、Ctrl、Alt 和 Cmd 是否正按下；它不监听
//! 按键事件、不模拟按键，也不修改桌面状态。macOS 使用 Core Graphics，Linux 使用 X11，Windows
//! 使用 Win32；其他目标返回 unsupported 状态。
//!
//! `kiana-commands/src/doctor.rs` 是目前找到的生产调用者，用 `get_modifier_status()` 展示后端可用性。
//! 本 crate 属于旧命令依赖；当前产品主执行链没有它的直接调用证据。`prewarm()` 当前是空函数，
//! 不能因为其名称推断已预加载任何系统库。
//!
//! 对外有两种观察方式：`get_modifier_status()` 同时返回平台、后端、可用标记、键集合和错误消息；
//! `get_modifiers()` 只返回键集合，遇到后端不可用时也会返回空集合，因此它无法区分“没有键按下”和“读取失败”。

#[derive(Debug, Clone, PartialEq, Eq)]
/// 一次修饰键轮询的结果；`available=false` 时空 modifiers 不代表确认没有按键按下。
pub struct ModifierStatus {
    /// 本次编译/查询使用的平台名称。
    pub platform: &'static str,
    /// 实际轮询后端，例如 x11、core-graphics 或 win32。
    pub backend: &'static str,
    /// 是否成功读取到后端数据；false 时应查看 message。
    pub available: bool,
    /// 当前正按下的规范名称集合：shift、ctrl、alt、cmd。
    pub modifiers: Vec<String>,
    /// 后端不可用时的诊断原因；成功时为空。
    pub message: Option<String>,
}

impl ModifierStatus {
    /// 构造一次读取成功的结果；空 modifiers 表示查询成功但未检测到受支持的修饰键。
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
    /// 构造读取失败/平台不支持的结果；保留平台和错误消息，键集合固定为空。
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
/// macOS 轮询实现：从 Core Graphics event flags 映射当前修饰键状态。
mod macos {
    use super::ModifierStatus;
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventSource};

    /// 只返回修饰键列表；此便捷接口会丢弃 `status()` 中的 backend 可用性和错误消息。
    pub fn get_modifiers() -> Vec<String> {
        status().modifiers
    }

    /// 通过 Core Graphics 创建 event source/event，再读取 flags 并映射为规范化键名。
    ///
    /// 任一对象创建失败时不 panic，而是返回 `available=false` 的诊断结果；成功时每个受支持的 flag
    /// 生成一个键名。此函数查询当前状态，不建立常驻监听。
    pub fn status() -> ModifierStatus {
        let mut result = Vec::new();
        // Cargo.lock 锁定的 core-graphics 0.23.2 要求 CGEventSourceStateID，并返回 Result<Self, ()>；
        // 其 CombinedSessionState 判别值为 0。当前调用传整数 0 且用 Option 模式解构；下一处
        // CGEvent::new(source) 也在该版本返回 Result，却同样用 Some 解构。源码与锁定依赖的类型签名
        // 存在 macOS 目标编译风险；这里仅记录，不替换调用或改变错误处理。
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
/// Linux 轮询实现：通过 X11/XWayland 查询 X server 的当前 keymap。
mod linux {
    use super::ModifierStatus;
    use std::ptr;
    use x11_dl::keysym::*;
    use x11_dl::xlib::Xlib;

    /// 只返回键列表；display 不可用时会变成空列表并丢失 status 错误原因。
    pub fn get_modifiers() -> Vec<String> {
        status().modifiers
    }

    /// 将底层 X11 查询的 Ok/Err 映射为统一的 ModifierStatus。
    pub fn status() -> ModifierStatus {
        match query_modifiers() {
            Ok(modifiers) => ModifierStatus::available("linux", "x11", modifiers),
            Err(message) => ModifierStatus::unavailable("linux", "x11", message),
        }
    }

    /// 打开 X11 动态库和默认 display，读取 keymap 位图并转换左右修饰键。
    ///
    /// 【调用者/输出】`status()` 调用它；成功返回规范化名称，动态库或 display 不可用时返回诊断字符串。
    ///
    /// 【unsafe 与资源】Xlib 是 C FFI（外部函数接口），需要传递原始 display/缓冲区指针，因此整个
    /// 查询包在 unsafe block 中。display 经空指针检查后才使用，成功路径在返回前调用 XCloseDisplay；
    /// `keys` 的 32 个字节是传给 XQueryKeymap 的输出存储。
    ///
    /// 【Wayland 边界】这里只尝试 XOpenDisplay；即使 `WAYLAND_DISPLAY` 存在也不会调用 Wayland 协议。
    /// 只有 DISPLAY/XWayland 提供 X11 display 时，当前实现才可以轮询全局修饰键。
    fn query_modifiers() -> Result<Vec<String>, String> {
        let xlib = Xlib::open().map_err(|err| format!("failed to load X11 library: {err}"))?;

        // Xlib FFI 使用 C 指针；必须先确认 display 非空，且把 keymap 输出写入本地固定大小缓冲区。
        unsafe {
            let display = (xlib.XOpenDisplay)(ptr::null());
            if display.is_null() {
                return Err(linux_display_failure_message(
                    non_empty_env("DISPLAY"),
                    non_empty_env("WAYLAND_DISPLAY"),
                ));
            }

            // 32 字节位图可按每个 keycode 一个 bit 表示 256 个 keycode 的当前按下状态。
            let mut keys: [u8; 32] = [0; 32];
            (xlib.XQueryKeymap)(display, keys.as_mut_ptr() as *mut i8);

            // X11 同一类修饰键可能有左右两个物理键；两边任一按下，都映射成一个逻辑 modifier。
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

            // 关闭本次短暂打开的 display；每次轮询重新连接，不缓存 Xlib display 句柄。
            (xlib.XCloseDisplay)(display);
            Ok(result)
        }
    }

    /// 在 XQueryKeymap 返回的 32 字节位图中检查一个 8 位 keycode 对应的 bit。
    ///
    /// keycode 0 表示没有映射，直接视为未按下；其余值用除以 8 选字节、取模 8 选 bit。
    fn is_key_pressed(keys: &[u8; 32], keycode: u8) -> bool {
        if keycode == 0 {
            return false;
        }
        let byte = (keycode / 8) as usize;
        let bit = keycode % 8;
        (keys[byte] & (1 << bit)) != 0
    }

    /// 读取并 trim 环境变量；变量缺失或内容全为空白时统一返回 None。
    fn non_empty_env(name: &str) -> Option<String> {
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    /// 按 DISPLAY/WAYLAND_DISPLAY 的当前值生成更具体的 X11 连接失败说明。
    ///
    /// 两个变量都没有时提示 DISPLAY 未设置；仅有 Wayland 时说明本实现仍需 X11/XWayland；
    /// DISPLAY 有值但连接失败时保留该值供旧 doctor 命令诊断。
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
/// Windows 轮询实现：调用 Win32 GetAsyncKeyState 读取修饰键的当前按下状态。
mod windows {
    use super::ModifierStatus;
    use windows::Win32::UI::Input::KeyboardAndMouse::*;

    /// 只返回修饰键列表；调用 Win32 查询后立即返回，不订阅键盘事件。
    pub fn get_modifiers() -> Vec<String> {
        status().modifiers
    }

    /// 分别检查 Shift、Control、Alt 和左右 Windows 键，并映射成跨平台名称。
    ///
    /// `VK_SHIFT`/`VK_CONTROL`/`VK_MENU` 查询逻辑键状态；Win 左右键分别查询后合并成 `cmd`。
    /// `GetAsyncKeyState` 没有 Result 错误通道，本函数无论返回位图是什么都会标记 available=true；
    /// 因此 OS 不允许读取和“当前没有修饰键按下”无法从此状态 DTO 中区分。
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

    /// 调用 Win32 FFI 并只检查返回值最高位，判断调用瞬间该虚拟键是否处于按下状态。
    ///
    /// `0x8000` 是 16 位状态的最高位掩码；低位“自上次查询以来曾按下”信息不参与本函数判断。
    /// Win32 要求调用不安全 FFI，因此此 helper 的调用者需要位于 unsafe block 中。
    unsafe fn key_is_down(key: VIRTUAL_KEY) -> bool {
        (GetAsyncKeyState(key.0 as i32) as u16) & 0x8000 != 0
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
/// 没有实现原生键盘查询的其他目标平台 fallback。
mod unsupported {
    use super::ModifierStatus;

    /// 没有可用键状态；使用状态接口时会附带明确的平台不支持消息。
    pub fn get_modifiers() -> Vec<String> {
        status().modifiers
    }

    /// 保留实际 OS 名称，并明确报告 modifier polling 未实现。
    pub fn status() -> ModifierStatus {
        ModifierStatus::unavailable(
            std::env::consts::OS,
            "unsupported",
            "modifier polling is not implemented for this platform",
        )
    }
}

/// 兼容旧调用方的预热入口；当前函数体为空，不加载库也不提前建立桌面连接。
pub fn prewarm() {}

/// 查询当前修饰键集合并按目标平台选择实现。
///
/// 返回键名统一为 shift/ctrl/alt/cmd；后端失败时此函数只给空集合，不返回失败标记，
/// 需要区分错误与“无键按下”时应调用 `get_modifier_status()`。
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

/// 查询修饰键集合并保留 platform/backend/available/message 元信息。
///
/// 当前 `kiana-commands/src/doctor.rs` 使用此接口展示后端诊断；它每次调用都会重新轮询系统 API。
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

/// 判断指定名称的修饰键此刻是否按下；大小写不敏感，但不做 control/command 等别名归一化。
///
/// 每次都会重新调用 `get_modifiers()`，不是读取缓存或订阅事件；后端不可用时空集合会得到 false。
pub fn is_modifier_pressed(modifier: &str) -> bool {
    contains_modifier(&get_modifiers(), modifier)
}

/// 在本次规范键名列表中做 ASCII 风格的大小写不敏感匹配；不会翻译或扩展别名。
fn contains_modifier(modifiers: &[String], modifier: &str) -> bool {
    modifiers
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(modifier))
}

#[cfg(test)]
/// 当前状态查询 helper 的本地检查；这些测试不模拟物理键盘或跨平台桌面会话。
mod tests {
    use super::*;

    #[test]
    fn test_prewarm() {
        prewarm();
    }

    #[test]
    fn test_get_modifiers() {
        let mods = get_modifiers();
        // ⚠ 该断言对任何 Vec 都成立，只能证明调用没有在测试中 panic，不验证按键结果。
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
