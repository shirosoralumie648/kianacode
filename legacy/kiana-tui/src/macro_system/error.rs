use std::io;
use thiserror::Error;

/// Macro system errors
#[derive(Error, Debug)]
pub enum MacroError {
    #[error("Invalid register: {0} (must be a-z)")]
    InvalidRegister(char),

    #[error("Already recording to register {0}")]
    AlreadyRecording(char),

    #[error("Not currently recording")]
    NotRecording,

    #[error("Macro not found in register: {0}")]
    MacroNotFound(char),

    #[error("Empty macro in register: {0}")]
    EmptyMacro(char),

    #[error("No last macro to repeat")]
    NoLastMacro,

    #[error("Maximum actions exceeded (limit: {0})")]
    MaxActionsExceeded(usize),

    #[error("Home directory not found")]
    NoHomeDir,

    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] toml::ser::Error),

    #[error("Deserialization error: {0}")]
    Deserialization(#[from] toml::de::Error),

    #[error("Macro file too large (max 1MB)")]
    FileTooLarge,

    #[error("Cannot record macro command in macro")]
    RecursiveMacro,
}

pub type Result<T> = std::result::Result<T, MacroError>;
