//! AgentScribe error types
//!
//! All errors are categorized into specific types for proper handling.
//! Parser errors use a skip-and-log strategy.

use std::path::PathBuf;
use thiserror::Error;

/// Main error type for AgentScribe
#[derive(Error, Debug)]
pub enum AgentScribeError {
    /// Configuration errors
    #[error("Configuration error: {0}")]
    Config(String),

    /// Plugin errors
    #[error("Plugin error in '{name}': {message}")]
    Plugin { name: String, message: String },

    /// Parser errors - these are logged but skipped during parsing
    #[error("{message}")]
    Parse {
        file: String,
        line: Option<usize>,
        message: String,
    },

    /// I/O errors
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON parsing errors
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// TOML parsing errors
    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),

    /// Glob pattern errors
    #[error("Glob pattern error: {0}")]
    Glob(String),

    /// Timestamp parsing errors
    #[error("Timestamp parse error: {0}")]
    Timestamp(String),

    /// Invalid plugin definition
    #[error("Invalid plugin: {0}")]
    InvalidPlugin(String),

    /// File not found
    #[error("File not found: {0}")]
    FileNotFound(PathBuf),

    /// Data directory error
    #[error("Data directory error: {0}")]
    DataDir(String),

    /// State file errors
    #[error("State file error: {0}")]
    State(String),

    /// Rules extraction errors
    #[error("Rules error: {0}")]
    Rules(String),

    /// Project registry errors
    #[error("Projects error: {0}")]
    Projects(String),

    /// Transcription errors
    #[error("Transcription error: {0}")]
    Transcription(String),

    /// Redaction errors
    #[error("Redaction error: {0}")]
    Redaction(String),
}

impl AgentScribeError {
    /// Create a parser error with skip-and-log semantics
    #[allow(dead_code)]
    pub fn parse_error(file: impl Into<String>, message: impl Into<String>) -> Self {
        let file = file.into();
        let message = message.into();
        AgentScribeError::Parse {
            file: file.clone(),
            line: None,
            message: format!("{}: {}", file, message),
        }
    }

    /// Create a parser error with line number
    pub fn parse_error_with_line(
        file: impl Into<String>,
        line: usize,
        message: impl Into<String>,
    ) -> Self {
        let file = file.into();
        let message = message.into();
        AgentScribeError::Parse {
            file: file.clone(),
            line: Some(line),
            message: format!("{}:{}: {}", file, line, message),
        }
    }

    /// Create a plugin error
    pub fn plugin_error(name: impl Into<String>, message: impl Into<String>) -> Self {
        AgentScribeError::Plugin {
            name: name.into(),
            message: message.into(),
        }
    }

    /// Check if this error should be skipped (logged but not fatal)
    pub fn is_skippable(&self) -> bool {
        matches!(self, AgentScribeError::Parse { .. })
    }
}

/// Result type for AgentScribe operations
pub type Result<T> = std::result::Result<T, AgentScribeError>;
