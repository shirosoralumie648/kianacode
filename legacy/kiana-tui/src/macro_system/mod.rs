pub mod error;
pub mod integration;
pub mod manager;
pub mod storage;

#[cfg(test)]
mod tests;

pub use error::{MacroError, Result};
pub use integration::{
    execute_macro_action, MacroCommand, MacroInputHandler, MacroIntegration, MacroRegisterAction,
};
pub use manager::{Macro, MacroAction, MacroRecorder, RecordingStats};
pub use storage::MacroStorage;
