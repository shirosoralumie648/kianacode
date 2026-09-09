use crossterm::event::{KeyModifiers, MouseButton as CtMouseButton, MouseEventKind};
use ratatui::layout::Rect;

/// 鼠标位置
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Position {
    pub x: u16,
    pub y: u16,
}

impl Position {
    pub fn new(x: u16, y: u16) -> Self {
        Self { x, y }
    }

    pub fn is_within(&self, rect: Rect) -> bool {
        self.x >= rect.x
            && self.x < rect.x + rect.width
            && self.y >= rect.y
            && self.y < rect.y + rect.height
    }

    pub fn relative_to(&self, rect: Rect) -> Position {
        Position {
            x: self.x.saturating_sub(rect.x),
            y: self.y.saturating_sub(rect.y),
        }
    }
}

/// 鼠标按钮
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

impl From<CtMouseButton> for MouseButton {
    fn from(button: CtMouseButton) -> Self {
        match button {
            CtMouseButton::Left => MouseButton::Left,
            CtMouseButton::Right => MouseButton::Right,
            CtMouseButton::Middle => MouseButton::Middle,
        }
    }
}

/// 滚动方向
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

/// 光标样式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorStyle {
    Default,
    Pointer,    // 可点击
    Text,       // 文本选择
    ResizeH,    // 水平调整大小
    ResizeV,    // 垂直调整大小
    Move,       // 移动
    NotAllowed, // 不允许
}

/// 鼠标事件类型
#[derive(Debug, Clone)]
pub enum MouseEventType {
    /// 单击
    Click {
        button: MouseButton,
        modifiers: KeyModifiers,
    },
    /// 双击
    DoubleClick { button: MouseButton },
    /// 拖拽
    Drag {
        button: MouseButton,
        from: Position,
        to: Position,
    },
    /// 滚动
    Scroll {
        direction: ScrollDirection,
        amount: i16,
    },
    /// 悬停
    Hover { position: Position },
    /// 按下
    Press { button: MouseButton },
    /// 释放
    Release { button: MouseButton },
}

/// 鼠标事件
#[derive(Debug, Clone)]
pub struct MouseEvent {
    pub event_type: MouseEventType,
    pub position: Position,
    pub timestamp: std::time::Instant,
}

impl MouseEvent {
    pub fn new(event_type: MouseEventType, position: Position) -> Self {
        Self {
            event_type,
            position,
            timestamp: std::time::Instant::now(),
        }
    }
}

/// 鼠标处理器trait
pub trait MouseHandler {
    /// 处理鼠标事件，返回true表示事件已处理
    fn handle_mouse_event(&mut self, _event: &MouseEvent) -> bool {
        false
    }

    /// 检查位置是否在组件内
    fn contains_point(&self, position: Position) -> bool;

    /// 获取指定位置的光标样式
    fn get_cursor_style(&self, _position: Position) -> CursorStyle {
        CursorStyle::Default
    }

    /// 组件是否接受鼠标事件
    fn accepts_mouse_events(&self) -> bool {
        true
    }
}

/// 鼠标状态追踪器
pub struct MouseTracker {
    last_position: Option<Position>,
    last_press: Option<(MouseButton, Position, std::time::Instant)>,
    last_click: Option<(MouseButton, std::time::Instant)>,
    is_dragging: bool,
    drag_start: Option<Position>,
    config: MouseConfig,
}

impl MouseTracker {
    pub fn new(config: MouseConfig) -> Self {
        Self {
            last_position: None,
            last_press: None,
            last_click: None,
            is_dragging: false,
            drag_start: None,
            config,
        }
    }

    /// 处理原始鼠标事件
    pub fn process_event(
        &mut self,
        kind: MouseEventKind,
        position: Position,
        modifiers: KeyModifiers,
    ) -> Option<MouseEvent> {
        let now = std::time::Instant::now();

        match kind {
            MouseEventKind::Down(button) => {
                let mouse_button = button.into();
                self.last_press = Some((mouse_button, position, now));
                self.drag_start = Some(position);

                Some(MouseEvent::new(
                    MouseEventType::Press {
                        button: mouse_button,
                    },
                    position,
                ))
            }

            MouseEventKind::Up(button) => {
                let mouse_button = button.into();
                let event = if let Some((press_button, press_pos, _press_time)) = self.last_press {
                    if press_button == mouse_button {
                        if self.is_dragging {
                            // 结束拖拽
                            self.is_dragging = false;
                            let drag_event = MouseEvent::new(
                                MouseEventType::Drag {
                                    button: mouse_button,
                                    from: self.drag_start.unwrap_or(press_pos),
                                    to: position,
                                },
                                position,
                            );
                            self.drag_start = None;
                            Some(drag_event)
                        } else {
                            // 检测双击
                            let is_double = if let Some((last_button, last_time)) = self.last_click
                            {
                                last_button == mouse_button
                                    && now.duration_since(last_time).as_millis()
                                        < self.config.double_click_ms as u128
                            } else {
                                false
                            };

                            self.last_click = Some((mouse_button, now));

                            if is_double {
                                Some(MouseEvent::new(
                                    MouseEventType::DoubleClick {
                                        button: mouse_button,
                                    },
                                    position,
                                ))
                            } else {
                                Some(MouseEvent::new(
                                    MouseEventType::Click {
                                        button: mouse_button,
                                        modifiers,
                                    },
                                    position,
                                ))
                            }
                        }
                    } else {
                        Some(MouseEvent::new(
                            MouseEventType::Release {
                                button: mouse_button,
                            },
                            position,
                        ))
                    }
                } else {
                    Some(MouseEvent::new(
                        MouseEventType::Release {
                            button: mouse_button,
                        },
                        position,
                    ))
                };

                self.last_press = None;
                event
            }

            MouseEventKind::Drag(button) => {
                let mouse_button = button.into();

                if let Some((_, start_pos, _)) = self.last_press {
                    if !self.is_dragging {
                        // 检查是否超过拖拽阈值
                        let distance = self.distance(start_pos, position);
                        if distance >= self.config.drag_threshold {
                            self.is_dragging = true;
                        }
                    }

                    if self.is_dragging {
                        return Some(MouseEvent::new(
                            MouseEventType::Drag {
                                button: mouse_button,
                                from: self.drag_start.unwrap_or(start_pos),
                                to: position,
                            },
                            position,
                        ));
                    }
                }

                None
            }

            MouseEventKind::Moved => {
                self.last_position = Some(position);

                // 如果按住按钮移动，可能开始拖拽
                if let Some((button, start_pos, _)) = self.last_press {
                    let distance = self.distance(start_pos, position);
                    if distance >= self.config.drag_threshold && !self.is_dragging {
                        self.is_dragging = true;
                        return Some(MouseEvent::new(
                            MouseEventType::Drag {
                                button,
                                from: start_pos,
                                to: position,
                            },
                            position,
                        ));
                    }
                }

                Some(MouseEvent::new(
                    MouseEventType::Hover { position },
                    position,
                ))
            }

            MouseEventKind::ScrollUp => Some(MouseEvent::new(
                MouseEventType::Scroll {
                    direction: ScrollDirection::Up,
                    amount: self.config.scroll_speed as i16,
                },
                position,
            )),

            MouseEventKind::ScrollDown => Some(MouseEvent::new(
                MouseEventType::Scroll {
                    direction: ScrollDirection::Down,
                    amount: self.config.scroll_speed as i16,
                },
                position,
            )),

            MouseEventKind::ScrollLeft => Some(MouseEvent::new(
                MouseEventType::Scroll {
                    direction: ScrollDirection::Left,
                    amount: self.config.scroll_speed as i16,
                },
                position,
            )),

            MouseEventKind::ScrollRight => Some(MouseEvent::new(
                MouseEventType::Scroll {
                    direction: ScrollDirection::Right,
                    amount: self.config.scroll_speed as i16,
                },
                position,
            )),
        }
    }

    /// 计算两点距离
    fn distance(&self, p1: Position, p2: Position) -> u16 {
        let dx = (p1.x as i32 - p2.x as i32).abs();
        let dy = (p1.y as i32 - p2.y as i32).abs();
        ((dx * dx + dy * dy) as f64).sqrt() as u16
    }

    /// 获取最后的鼠标位置
    pub fn last_position(&self) -> Option<Position> {
        self.last_position
    }

    /// 是否正在拖拽
    pub fn is_dragging(&self) -> bool {
        self.is_dragging
    }

    /// 重置状态
    pub fn reset(&mut self) {
        self.last_position = None;
        self.last_press = None;
        self.last_click = None;
        self.is_dragging = false;
        self.drag_start = None;
    }
}

/// 鼠标配置
#[derive(Debug, Clone)]
pub struct MouseConfig {
    pub enabled: bool,
    pub double_click_ms: u64,
    pub drag_threshold: u16,
    pub scroll_speed: u16,
    pub hover_delay_ms: u64,
}

impl Default for MouseConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            double_click_ms: 500,
            drag_threshold: 3,
            scroll_speed: 3,
            hover_delay_ms: 500,
        }
    }
}

/// 鼠标事件路由器
pub struct MouseRouter {
    handlers: Vec<Box<dyn MouseHandler>>,
    current_cursor: CursorStyle,
}

impl MouseRouter {
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
            current_cursor: CursorStyle::Default,
        }
    }

    /// 注册处理器
    pub fn register(&mut self, handler: Box<dyn MouseHandler>) {
        self.handlers.push(handler);
    }

    /// 路由鼠标事件到合适的处理器
    pub fn route_event(&mut self, event: &MouseEvent) -> bool {
        // 从上到下查找能处理事件的组件
        for handler in self.handlers.iter_mut().rev() {
            if !handler.accepts_mouse_events() {
                continue;
            }

            if handler.contains_point(event.position) {
                if handler.handle_mouse_event(event) {
                    return true;
                }
            }
        }

        false
    }

    /// 更新光标样式
    pub fn update_cursor(&mut self, position: Position) -> CursorStyle {
        for handler in self.handlers.iter().rev() {
            if handler.contains_point(position) {
                let style = handler.get_cursor_style(position);
                if style != CursorStyle::Default {
                    self.current_cursor = style;
                    return style;
                }
            }
        }

        self.current_cursor = CursorStyle::Default;
        CursorStyle::Default
    }

    /// 获取当前光标样式
    pub fn current_cursor(&self) -> CursorStyle {
        self.current_cursor
    }

    /// 清除所有处理器
    pub fn clear(&mut self) {
        self.handlers.clear();
    }
}

impl Default for MouseRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position_within_rect() {
        let pos = Position::new(5, 5);
        let rect = Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        };

        assert!(pos.is_within(rect));

        let pos2 = Position::new(15, 5);
        assert!(!pos2.is_within(rect));
    }

    #[test]
    fn test_position_relative() {
        let pos = Position::new(15, 20);
        let rect = Rect {
            x: 10,
            y: 15,
            width: 20,
            height: 20,
        };

        let relative = pos.relative_to(rect);
        assert_eq!(relative, Position::new(5, 5));
    }

    #[test]
    fn test_mouse_tracker_click() {
        let mut tracker = MouseTracker::new(MouseConfig::default());

        // 按下
        let event1 = tracker.process_event(
            MouseEventKind::Down(CtMouseButton::Left),
            Position::new(10, 10),
            KeyModifiers::empty(),
        );
        assert!(matches!(
            event1,
            Some(MouseEvent {
                event_type: MouseEventType::Press { .. },
                ..
            })
        ));

        // 释放（点击）
        let event2 = tracker.process_event(
            MouseEventKind::Up(CtMouseButton::Left),
            Position::new(10, 10),
            KeyModifiers::empty(),
        );
        assert!(matches!(
            event2,
            Some(MouseEvent {
                event_type: MouseEventType::Click { .. },
                ..
            })
        ));
    }

    #[test]
    fn test_mouse_tracker_drag() {
        let mut tracker = MouseTracker::new(MouseConfig::default());

        // 按下
        tracker.process_event(
            MouseEventKind::Down(CtMouseButton::Left),
            Position::new(10, 10),
            KeyModifiers::empty(),
        );

        // 拖拽
        let event = tracker.process_event(
            MouseEventKind::Drag(CtMouseButton::Left),
            Position::new(20, 20),
            KeyModifiers::empty(),
        );

        assert!(tracker.is_dragging());
        assert!(matches!(
            event,
            Some(MouseEvent {
                event_type: MouseEventType::Drag { .. },
                ..
            })
        ));
    }
}
