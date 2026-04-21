//! Canonical event schema
//!
//! All conversation events from all agents are normalized into this format.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Canonical role types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    System,
    ToolCall,
    ToolResult,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
            Role::ToolCall => "tool_call",
            Role::ToolResult => "tool_result",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "user" => Some(Role::User),
            "assistant" => Some(Role::Assistant),
            "system" => Some(Role::System),
            "tool_call" => Some(Role::ToolCall),
            "tool_result" => Some(Role::ToolResult),
            _ => None,
        }
    }
}

/// Token counts for an event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenCounts {
    pub input: u32,
    pub output: u32,
}

/// A single canonical event from any agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// ISO 8601 timestamp
    pub ts: DateTime<Utc>,

    /// Session ID (format: <agent>/<id>)
    pub session_id: String,

    /// Source agent name (plugin name)
    pub source_agent: String,

    /// Source agent version, if available
    pub source_version: Option<String>,

    /// Absolute path to the project directory
    pub project: Option<String>,

    /// Event role
    pub role: Role,

    /// Message/event text content
    pub content: String,

    /// Tool name for tool_call/tool_result roles
    pub tool: Option<String>,

    /// Token counts
    pub tokens: Option<TokenCounts>,

    /// LLM model name, if available
    pub model: Option<String>,

    /// File paths referenced in this event
    #[serde(default)]
    pub file_paths: Vec<String>,

    /// Error fingerprints found in this event
    #[serde(default)]
    pub error_fingerprints: Vec<String>,
}

impl Event {
    /// Create a new event with required fields
    pub fn new(
        ts: DateTime<Utc>,
        session_id: String,
        source_agent: String,
        role: Role,
        content: String,
    ) -> Self {
        Event {
            ts,
            session_id,
            source_agent,
            source_version: None,
            project: None,
            role,
            content,
            tool: None,
            tokens: None,
            model: None,
            file_paths: Vec::new(),
            error_fingerprints: Vec::new(),
        }
    }

    /// Set the source version
    #[allow(dead_code)]
    pub fn with_source_version(mut self, version: Option<String>) -> Self {
        self.source_version = version;
        self
    }

    /// Set the project path
    #[allow(dead_code)]
    pub fn with_project(mut self, project: Option<String>) -> Self {
        self.project = project;
        self
    }

    /// Set the tool name
    #[allow(dead_code)]
    pub fn with_tool(mut self, tool: Option<String>) -> Self {
        self.tool = tool;
        self
    }

    /// Set token counts
    #[allow(dead_code)]
    pub fn with_tokens(mut self, tokens: Option<TokenCounts>) -> Self {
        self.tokens = tokens;
        self
    }

    /// Set model name
    #[allow(dead_code)]
    pub fn with_model(mut self, model: Option<String>) -> Self {
        self.model = model;
        self
    }

    /// Add file paths
    #[allow(dead_code)]
    pub fn with_file_paths(mut self, paths: Vec<String>) -> Self {
        self.file_paths = paths;
        self
    }

    /// Add error fingerprints
    #[allow(dead_code)]
    pub fn with_error_fingerprints(mut self, fingerprints: Vec<String>) -> Self {
        self.error_fingerprints = fingerprints;
        self
    }

    /// Write event as JSONL
    pub fn to_jsonl(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Parse event from JSONL
    pub fn from_jsonl(line: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(line)
    }
}

/// Session metadata for indexing and search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionManifest {
    pub session_id: String,
    pub source_agent: String,
    pub project: Option<String>,
    pub started: DateTime<Utc>,
    pub ended: Option<DateTime<Utc>>,
    pub turns: u32,
    pub summary: Option<String>,
    pub outcome: Option<String>,
    pub tags: Vec<String>,
    pub files_touched: Vec<String>,
    pub model: Option<String>,
}

impl SessionManifest {
    /// Create a new manifest from the first event
    pub fn new(session_id: String, source_agent: String) -> Self {
        SessionManifest {
            session_id,
            source_agent,
            project: None,
            started: Utc::now(),
            ended: None,
            turns: 0,
            summary: None,
            outcome: None,
            tags: Vec::new(),
            files_touched: Vec::new(),
            model: None,
        }
    }
}

/// Scrape state for a single source file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFileState {
    /// Plugin name
    pub plugin: String,

    /// Last byte offset read (for JSONL/Markdown)
    pub last_byte_offset: u64,

    /// File modification time at last scrape
    pub last_modified: DateTime<Utc>,

    /// Last scrape timestamp
    pub last_scraped: DateTime<Utc>,

    /// Session IDs found in this file
    pub session_ids: Vec<String>,

    /// For delimiter-based formats: offset of last delimiter
    pub last_delimiter_offset: Option<u64>,
}

impl SourceFileState {
    /// Create new state for a file
    pub fn new(plugin: String) -> Self {
        let now = Utc::now();
        SourceFileState {
            plugin,
            last_byte_offset: 0,
            last_modified: now,
            last_scraped: now,
            session_ids: Vec::new(),
            last_delimiter_offset: None,
        }
    }
}

/// Global scrape state tracking all source files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeState {
    /// Per-source-file state, keyed by absolute file path
    pub sources: HashMap<String, SourceFileState>,
}

impl ScrapeState {
    /// Create empty scrape state
    pub fn new() -> Self {
        ScrapeState {
            sources: HashMap::new(),
        }
    }

    /// Get state for a file, or create new if not exists
    #[allow(dead_code)]
    pub fn get_or_create(&mut self, file_path: &str, plugin: &str) -> &mut SourceFileState {
        self.sources
            .entry(file_path.to_string())
            .or_insert_with(|| SourceFileState::new(plugin.to_string()))
    }

    /// Remove state for a file
    #[allow(dead_code)]
    pub fn remove(&mut self, file_path: &str) -> Option<SourceFileState> {
        self.sources.remove(file_path)
    }

    /// Get all files for a plugin
    #[allow(dead_code)]
    pub fn files_for_plugin(&self, plugin: &str) -> Vec<&str> {
        self.sources
            .iter()
            .filter(|(_, state)| state.plugin == plugin)
            .map(|(path, _)| path.as_str())
            .collect()
    }
}

impl Default for ScrapeState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_from_str() {
        assert_eq!(Role::from_str("user"), Some(Role::User));
        assert_eq!(Role::from_str("assistant"), Some(Role::Assistant));
        assert_eq!(Role::from_str("invalid"), None);
    }

    #[test]
    fn test_role_as_str() {
        assert_eq!(Role::User.as_str(), "user");
        assert_eq!(Role::ToolCall.as_str(), "tool_call");
    }

    #[test]
    fn test_event_jsonl_roundtrip() {
        let event = Event::new(
            Utc::now(),
            "test-agent/123".to_string(),
            "test-agent".to_string(),
            Role::User,
            "Hello, world!".to_string(),
        );

        let jsonl = event.to_jsonl().unwrap();
        let parsed = Event::from_jsonl(&jsonl).unwrap();

        assert_eq!(parsed.session_id, event.session_id);
        assert_eq!(parsed.role, event.role);
        assert_eq!(parsed.content, event.content);
    }
}
