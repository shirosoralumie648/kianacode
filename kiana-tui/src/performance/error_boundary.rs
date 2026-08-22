use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use std::error::Error as StdError;
use std::fmt;
use std::panic::{self, AssertUnwindSafe};
use std::time::Instant;

/// 错误信息
#[derive(Debug, Clone)]
pub struct ErrorInfo {
    pub error: String,
    pub timestamp: Instant,
    pub component_path: String,
    pub stack_trace: Option<String>,
    pub recovery_attempts: u32,
}

impl ErrorInfo {
    pub fn new(error: String, component_path: String) -> Self {
        Self {
            error,
            timestamp: Instant::now(),
            component_path,
            stack_trace: None,
            recovery_attempts: 0,
        }
    }

    pub fn with_stack_trace(mut self, trace: String) -> Self {
        self.stack_trace = Some(trace);
        self
    }

    pub fn increment_attempts(&mut self) {
        self.recovery_attempts += 1;
    }

    pub fn age(&self) -> std::time::Duration {
        self.timestamp.elapsed()
    }
}

/// 恢复动作
#[derive(Debug, Clone)]
pub enum RecoveryAction {
    /// 重试操作
    Retry,
    /// 重置到默认状态
    Reset,
    /// 恢复到上一个良好状态
    Restore,
    /// 移除组件
    Remove,
    /// 显示错误UI
    ShowError,
}

/// 恢复策略
pub enum RecoveryStrategy {
    /// 重置到默认状态
    ResetToDefault,
    /// 恢复上一个良好状态
    RestoreLastGood,
    /// 显示错误UI
    ShowError,
    /// 移除失败组件
    Remove,
    /// 自定义恢复函数
    Custom(Box<dyn Fn(&ErrorInfo) -> RecoveryAction + Send>),
}

impl fmt::Debug for RecoveryStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecoveryStrategy::ResetToDefault => write!(f, "ResetToDefault"),
            RecoveryStrategy::RestoreLastGood => write!(f, "RestoreLastGood"),
            RecoveryStrategy::ShowError => write!(f, "ShowError"),
            RecoveryStrategy::Remove => write!(f, "Remove"),
            RecoveryStrategy::Custom(_) => write!(f, "Custom"),
        }
    }
}

/// 错误操作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorAction {
    Retry,
    Reset,
    Dismiss,
    ReportBug,
}

/// 错误边界状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoundaryState {
    Normal,
    Error,
    Recovering,
    Removed,
}

/// 错误边界
pub struct ErrorBoundary<W> {
    id: String,
    child: Option<W>,
    error_state: Option<ErrorInfo>,
    recovery_strategy: RecoveryStrategy,
    state: BoundaryState,
    last_good_snapshot: Option<Vec<u8>>,
    config: ErrorRecoveryConfig,
}

impl<W> ErrorBoundary<W> {
    pub fn new(id: String, child: W) -> Self {
        Self {
            id,
            child: Some(child),
            error_state: None,
            recovery_strategy: RecoveryStrategy::ShowError,
            state: BoundaryState::Normal,
            last_good_snapshot: None,
            config: ErrorRecoveryConfig::default(),
        }
    }

    pub fn with_strategy(mut self, strategy: RecoveryStrategy) -> Self {
        self.recovery_strategy = strategy;
        self
    }

    pub fn with_config(mut self, config: ErrorRecoveryConfig) -> Self {
        self.config = config;
        self
    }

    /// 捕获并处理错误
    pub fn catch_error<F, R>(&mut self, operation: F) -> Result<R, ErrorInfo>
    where
        F: FnOnce(&mut W) -> Result<R, Box<dyn StdError>>,
    {
        if self.state == BoundaryState::Removed {
            return Err(ErrorInfo::new(
                "Component removed".to_string(),
                self.id.clone(),
            ));
        }

        let child = match self.child.as_mut() {
            Some(c) => c,
            None => {
                return Err(ErrorInfo::new(
                    "No child component".to_string(),
                    self.id.clone(),
                ));
            }
        };

        // 尝试执行操作
        match panic::catch_unwind(AssertUnwindSafe(|| operation(child))) {
            Ok(Ok(result)) => {
                // 成功 - 保存快照
                self.state = BoundaryState::Normal;
                self.error_state = None;
                Ok(result)
            }
            Ok(Err(error)) => {
                // 可控错误
                self.handle_error(error.to_string())
            }
            Err(panic_info) => {
                // Panic
                let error_msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_info.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown panic".to_string()
                };
                self.handle_error(error_msg)
            }
        }
    }

    /// 处理错误
    fn handle_error<R>(&mut self, error_msg: String) -> Result<R, ErrorInfo> {
        let mut error_info = ErrorInfo::new(error_msg, self.id.clone());

        if self.config.collect_stack_traces {
            // 收集堆栈跟踪（简化版）
            error_info.stack_trace = Some(format!("Error in component: {}", self.id));
        }

        // 检查是否应该尝试恢复
        if let Some(existing_error) = &mut self.error_state {
            existing_error.increment_attempts();
            error_info.recovery_attempts = existing_error.recovery_attempts;

            if error_info.recovery_attempts >= self.config.max_recovery_attempts {
                // 超过最大重试次数
                self.state = BoundaryState::Removed;
                self.error_state = Some(error_info.clone());
                tracing::error!(
                    "Component {} failed after {} recovery attempts",
                    self.id,
                    error_info.recovery_attempts
                );
                return Err(error_info);
            }
        }

        self.error_state = Some(error_info.clone());
        self.state = BoundaryState::Error;

        // 尝试恢复
        self.try_recover();

        Err(error_info)
    }

    /// 尝试恢复
    fn try_recover(&mut self) {
        if self.state == BoundaryState::Removed {
            return;
        }

        self.state = BoundaryState::Recovering;

        let action = match &self.recovery_strategy {
            RecoveryStrategy::ResetToDefault => RecoveryAction::Reset,
            RecoveryStrategy::RestoreLastGood => RecoveryAction::Restore,
            RecoveryStrategy::ShowError => RecoveryAction::ShowError,
            RecoveryStrategy::Remove => RecoveryAction::Remove,
            RecoveryStrategy::Custom(f) => {
                if let Some(ref error) = self.error_state {
                    f(error)
                } else {
                    RecoveryAction::ShowError
                }
            }
        };

        match action {
            RecoveryAction::Reset => {
                // 重置会在下次使用时由外部处理
                tracing::info!("Scheduling reset for component {}", self.id);
            }
            RecoveryAction::Restore => {
                // 恢复快照（如果有）
                if self.last_good_snapshot.is_some() {
                    tracing::info!("Restoring last good state for {}", self.id);
                    self.state = BoundaryState::Normal;
                    self.error_state = None;
                } else {
                    self.state = BoundaryState::Error;
                }
            }
            RecoveryAction::Remove => {
                self.state = BoundaryState::Removed;
                self.child = None;
                tracing::warn!("Removing failed component {}", self.id);
            }
            RecoveryAction::ShowError => {
                self.state = BoundaryState::Error;
            }
            RecoveryAction::Retry => {
                self.state = BoundaryState::Normal;
            }
        }
    }

    /// 渲染错误UI
    pub fn render_error(&self, area: Rect, buf: &mut Buffer) {
        if let Some(ref error) = self.error_state {
            let block = Block::default()
                .title(format!(" Error in {} ", self.id))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red));

            let error_text = vec![
                Line::from(Span::styled(
                    "⚠ Component Error",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::raw(&error.error)),
                Line::from(""),
                Line::from(Span::styled(
                    format!("Attempts: {}", error.recovery_attempts),
                    Style::default().fg(Color::Yellow),
                )),
            ];

            let paragraph = Paragraph::new(error_text).block(block);
            Widget::render(paragraph, area, buf);
        }
    }

    /// 获取当前状态
    pub fn state(&self) -> &str {
        match self.state {
            BoundaryState::Normal => "normal",
            BoundaryState::Error => "error",
            BoundaryState::Recovering => "recovering",
            BoundaryState::Removed => "removed",
        }
    }

    /// 是否有错误
    pub fn has_error(&self) -> bool {
        self.error_state.is_some()
    }

    /// 获取错误信息
    pub fn error_info(&self) -> Option<&ErrorInfo> {
        self.error_state.as_ref()
    }

    /// 手动恢复
    pub fn recover(&mut self) {
        self.error_state = None;
        self.state = BoundaryState::Normal;
    }

    /// 保存快照
    pub fn save_snapshot(&mut self, data: Vec<u8>) {
        self.last_good_snapshot = Some(data);
    }
}

/// 错误恢复配置
#[derive(Debug, Clone)]
pub struct ErrorRecoveryConfig {
    pub max_recovery_attempts: u32,
    pub recovery_backoff_ms: u64,
    pub show_error_details: bool,
    pub collect_stack_traces: bool,
    pub auto_report_errors: bool,
}

impl Default for ErrorRecoveryConfig {
    fn default() -> Self {
        Self {
            max_recovery_attempts: 3,
            recovery_backoff_ms: 1000,
            show_error_details: cfg!(debug_assertions),
            collect_stack_traces: true,
            auto_report_errors: false,
        }
    }
}

/// 全局错误处理器
pub struct GlobalErrorHandler {
    boundaries: Vec<String>,
    error_log: Vec<ErrorInfo>,
    max_log_size: usize,
}

impl GlobalErrorHandler {
    pub fn new() -> Self {
        Self {
            boundaries: Vec::new(),
            error_log: Vec::new(),
            max_log_size: 1000,
        }
    }

    pub fn register_boundary(&mut self, id: String) {
        self.boundaries.push(id);
    }

    pub fn log_error(&mut self, error: ErrorInfo) {
        tracing::error!(
            "Error in {}: {} (attempts: {})",
            error.component_path,
            error.error,
            error.recovery_attempts
        );

        self.error_log.push(error);

        // 限制日志大小
        if self.error_log.len() > self.max_log_size {
            self.error_log.remove(0);
        }
    }

    pub fn recent_errors(&self, count: usize) -> &[ErrorInfo] {
        let start = self.error_log.len().saturating_sub(count);
        &self.error_log[start..]
    }

    pub fn error_count(&self) -> usize {
        self.error_log.len()
    }

    pub fn clear_log(&mut self) {
        self.error_log.clear();
    }
}

impl Default for GlobalErrorHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestWidget {
        should_fail: bool,
    }

    #[test]
    fn test_error_boundary_success() {
        let widget = TestWidget { should_fail: false };
        let mut boundary = ErrorBoundary::new("test".to_string(), widget);

        let result = boundary.catch_error(|w| {
            if w.should_fail {
                Err("test error".into())
            } else {
                Ok(42)
            }
        });

        assert!(result.is_ok());
        assert!(!boundary.has_error());
    }

    #[test]
    fn test_error_boundary_failure() {
        let widget = TestWidget { should_fail: true };
        let mut boundary = ErrorBoundary::new("test".to_string(), widget);

        let result = boundary.catch_error(|w| {
            if w.should_fail {
                Err("test error".into())
            } else {
                Ok(42)
            }
        });

        assert!(result.is_err());
        assert!(boundary.has_error());
    }

    #[test]
    fn test_recovery_attempts() {
        let widget = TestWidget { should_fail: true };
        let mut boundary =
            ErrorBoundary::new("test".to_string(), widget).with_config(ErrorRecoveryConfig {
                max_recovery_attempts: 2,
                ..Default::default()
            });

        // 第一次失败
        let _: Result<(), _> = boundary.catch_error(|_| Err("error 1".into()));
        assert_eq!(boundary.error_info().unwrap().recovery_attempts, 0);
        assert_eq!(boundary.state(), "error");

        // 第二次失败
        let _: Result<(), _> = boundary.catch_error(|_| Err("error 2".into()));
        assert_eq!(boundary.error_info().unwrap().recovery_attempts, 1);
        assert_eq!(boundary.state(), "error");

        // 第三次失败 - 应该被移除（达到max_recovery_attempts=2）
        let _: Result<(), _> = boundary.catch_error(|_| Err("error 3".into()));
        assert_eq!(boundary.state(), "removed");
    }
}
