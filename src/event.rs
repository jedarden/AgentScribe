//! Canonical event schema for AgentScribe.
//!
//! This module defines the core data structures that power AgentScribe's unified conversation log format.
//! All conversation events from all supported agents (Claude Code, Aider, OpenCode, Codex, Cursor, Windsurf)
//! are normalized into this canonical format during the scraping process.
//!
//! # Core Concepts
//!
//! ## Events
//! An [`Event`] represents a single conversational turn from any agent. Events are the atomic unit of
//! conversation data in AgentScribe. Each event has a role (user, assistant, system, tool_call, tool_result),
//! content, optional metadata (tokens, model, file paths), and a timestamp.
//!
//! ## Sessions
//! Events are grouped into sessions, identified by a `session_id` in the format `<agent>/<id>`.
//! A session represents one complete conversation between a user and an AI agent. Session metadata is
//! tracked separately in [`SessionManifest`].
//!
//! ## Canonical Format
//! The canonical format serves as AgentScribe's source of truth. Raw agent-specific log formats are
//! parsed, normalized, and written as JSONL files (one event per line) in the `sessions/` directory.
//! This format enables:
//! - Unified search across all agent types
//! - Agent-agnostic analytics and enrichment
//! - Simple, diff-friendly storage (append-only JSONL)
//!
//! # Data Flow
//!
//! 1. **Scraping**: Agent-specific log formats are read from their native locations
//! 2. **Normalization**: Events are converted to the canonical format via field mapping and event expansion
//! 3. **Storage**: Normalized events are written as JSONL to `sessions/<agent>/<session-id>.jsonl`
//! 4. **Enrichment**: Events are analyzed to extract outcomes, solutions, errors, and patterns
//! 5. **Indexing**: Session metadata and content are indexed in Tantivy for fast search
//!
//! # Examples
//!
//! ```no_run
//! use agentscribe::event::{Event, Role};
//! use chrono::Utc;
//!
//! // Create a new event (typically done by parsers, not users)
//! let event = Event::new(
//!     Utc::now(),
//!     "claude-code/abc123".to_string(),
//!     "claude-code".to_string(),
//!     Role::User,
//!     "How do I fix this error?".to_string(),
//! );
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Canonical role types for conversation events.
///
/// Roles define the origin and nature of each message in a conversation. These canonical roles
/// map to agent-specific roles during normalization, enabling cross-agent analysis.
///
/// # Variants
///
/// * **User** - Messages from the human user. This includes direct prompts, follow-up questions,
///   feedback, and corrections. User messages are the primary signal for intent and satisfaction.
///
/// * **Assistant** - Responses from the AI agent. These are the main conversational turns containing
///   explanations, code, and guidance. Assistant messages may contain embedded tool calls in some agents.
///
/// * **System** - System-level messages, prompts, or configuration. These are typically invisible to
///   the user and contain instructions, error context, or operational metadata.
///
/// * **ToolCall** - Represents the agent's intent to call a tool or function. Contains tool name,
///   parameters, and context. In agents with structured tool use (Claude Code, OpenCode), these are
///   extracted from assistant messages as separate events.
///
/// * **ToolResult** - The result of executing a tool call. Contains return values, error messages,
///   exit codes, and output. Tool results are critical for outcome detection and error fingerprinting.
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

    #[allow(clippy::should_implement_trait)]
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

/// Token usage counts for an event.
///
/// Tracks the number of tokens consumed by an LLM for a single event. This metadata is used for:
/// - Cost estimation and analytics
/// - Understanding token efficiency across agents
/// - Identifying expensive operations (e.g., large context reads)
///
/// # Fields
///
/// * **input** - Number of tokens in the prompt sent to the model (user message + conversation history)
/// * **output** - Number of tokens in the model's response (assistant message + tool calls)
///
/// # Availability
///
/// Token counts are only available when the source agent logs them. Claude Code and OpenCode
/// typically provide this metadata; Aider and some other agents do not. When unavailable, the
/// field is `None` on the parent [`Event`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenCounts {
    pub input: u32,
    pub output: u32,
}

/// A single canonical event from any agent.
///
/// Events are the atomic unit of conversation data in AgentScribe. Each event represents one
/// conversational turn (user message, assistant response, tool call, tool result, or system message)
/// and is normalized from agent-specific formats into this canonical structure.
///
/// # Event Lifecycle
///
/// 1. **Creation**: Events are created by parser plugins during scraping, each parser converts
///    agent-specific log formats into canonical Events
/// 2. **Storage**: Events are written as JSONL to `sessions/<agent>/<session-id>.jsonl`
/// 3. **Enrichment**: Events are analyzed to extract error fingerprints, file paths, and tags
/// 4. **Indexing**: Event content is indexed in Tantivy for search
/// 5. **Query**: Events are retrieved via `agentscribe search` and related commands
///
/// # Field Availability
///
/// Some fields are optional because not all agents log them:
/// - `source_version`: Only available when the agent reports its version
/// - `project`: May be missing for agents that don't track project directories
/// - `tool`/`tool_params`: Only present for [`Role::ToolCall`] and [`Role::ToolResult`]
/// - `tokens`: Only available when the agent logs token usage
/// - `model`: Only available when the agent reports which model was used
/// - `file_paths`: Populated during enrichment if not present in source data
/// - `error_fingerprints`: Added during enrichment, not present in source data
///
/// # Examples
///
/// Creating events is typically done by parser plugins, not by users:
///
/// ```no_run
/// use agentscribe::event::{Event, Role};
/// use chrono::Utc;
///
/// let event = Event::new(
///     Utc::now(),
///     "claude-code/abc123".to_string(),
///     "claude-code".to_string(),
///     Role::User,
///     "How do I fix this error?".to_string(),
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// ISO 8601 timestamp when this event occurred
    pub ts: DateTime<Utc>,

    /// Unique session identifier in format `<agent>/<id>`.
    ///
    /// Multiple events share the same `session_id` if they belong to the same conversation.
    /// The `<agent>` prefix comes from the plugin name, and `<id>` is unique within that agent.
    pub session_id: String,

    /// Name of the source agent (plugin name that produced this event).
    ///
    /// Examples: `"claude-code"`, `"aider"`, `"opencode"`, `"codex"`, `"cursor"`, `"windsurf"`.
    /// This field enables filtering analytics and search by agent type.
    pub source_agent: String,

    /// Version of the source agent, if available in the log data.
    ///
    /// Useful for tracking agent behavior changes over time and debugging agent-specific issues.
    /// Not all agents log their version, so this field is often `None`.
    pub source_version: Option<String>,

    /// Absolute path to the project directory where this conversation occurred.
    ///
    /// Enables project-scoped search and analytics. May be `None` for agents that don't track
    /// project directories or for conversations outside a project context.
    pub project: Option<String>,

    /// Role of the entity that produced this event.
    ///
    /// Defines whether this is a user message, assistant response, system message, tool call,
    /// or tool result. Critical for understanding conversation flow and for enrichment.
    pub role: Role,

    /// Text content of the message or event.
    ///
    /// For user/assistant roles, this is the conversational text. For tool calls, it contains
    /// the tool name and parameters. For tool results, it contains output or error messages.
    /// Content is indexed for full-text search but truncated at 500KB per session to manage
    /// index size (see ADR-2).
    pub content: String,

    /// Tool name for tool_call/tool_result events.
    ///
    /// Examples: `"Edit"`, `"Bash"`, `"Read"`, `"Write"`, `"grep"`. Only present for events
    /// with [`Role::ToolCall`] or [`Role::ToolResult`]. Used for tool usage analytics and
    /// tag extraction.
    pub tool: Option<String>,

    /// Structured tool call parameters (for tool_call events).
    ///
    /// Contains the arguments passed to the tool as structured JSON. Used by enrichment to
    /// extract file paths, command names, and other actionable metadata. Only present for
    /// [`Role::ToolCall`] events when the source agent logs structured parameters.
    pub tool_params: Option<serde_json::Value>,

    /// Token usage counts for this event, if available.
    ///
    /// Some agents (Claude Code, OpenCode) log token usage for each turn. Used for cost
    /// estimation and analytics. Many agents (Aider, Codex) do not log token usage, so this
    /// field is often `None`.
    pub tokens: Option<TokenCounts>,

    /// Name of the LLM model used for this event, if available.
    ///
    /// Examples: `"claude-sonnet-4-20250514"`, `"gpt-4o"`, `"deepseek-chat"`. Used for
    /// model-specific analytics and cost calculation. Not all agents log the model name.
    pub model: Option<String>,

    /// File paths referenced in this event.
    ///
    /// Extracted from tool call parameters (e.g., `Edit.file_path`) and content via regex.
    /// Used for the file knowledge map (`agentscribe file <path>`) and for project-wide
    /// file touch analytics. Populated during enrichment if not present in source data.
    #[serde(default)]
    pub file_paths: Vec<String>,

    /// Normalized error fingerprints found in this event.
    ///
    /// Error patterns are extracted from tool_result and assistant content, then normalized
    /// by stripping variable parts (paths, UUIDs, timestamps). Enables error-specific search
    /// via `agentscribe search --error`. Populated during enrichment, never in source data.
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
            tool_params: None,
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

    /// Set tool parameters
    #[allow(dead_code)]
    pub fn with_tool_params(mut self, params: Option<serde_json::Value>) -> Self {
        self.tool_params = params;
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

/// Session metadata for indexing and analytics.
///
/// A [`SessionManifest`] represents the high-level metadata about a conversation session,
/// distinct from the individual events that make up the conversation. Manifests are stored in
/// the Tantivy search index and used for:
/// - Fast search results without loading full event streams
/// - Analytics and aggregation (success rates, agent comparisons)
/// - Filtering and faceted search (by agent, project, outcome, tags)
///
/// # Relationship with Events
///
/// While [`Event`] represents individual conversational turns, `SessionManifest` represents the
/// entire conversation's metadata. One session has many events, but only one manifest. The manifest
/// is enriched with computed fields like `outcome`, `summary`, `tags`, and `files_touched` that
/// are derived from analyzing the full event stream.
///
/// # Field Availability
///
/// Some fields are computed during enrichment and may not be immediately available:
/// - `ended`: `None` if the session is still active or if the source doesn't log end times
/// - `summary`: Populated during enrichment, may be `None` for very short sessions
/// - `outcome`: Detected via signal scoring, may be `None` if signals are ambiguous
/// - `tags`: Extracted from content, tool names, and file types
/// - `files_touched`: Extracted from tool call parameters and content
/// - `model`: Only available if the source agent logs model names
/// - `parent_session_id`: Only set for subagent/sidechain sessions (e.g., Claude Code subagents)
///
/// # Examples
///
/// ```no_run
/// use agentscribe::event::SessionManifest;
/// use chrono::Utc;
///
/// // Create a new manifest (typically done by the scraper)
/// let manifest = SessionManifest::new(
///     "claude-code/abc123".to_string(),
///     "claude-code".to_string(),
/// );
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionManifest {
    /// Unique session identifier in format `<agent>/<id>`
    pub session_id: String,

    /// Name of the source agent that produced this session
    pub source_agent: String,

    /// Absolute path to the project directory, if available
    pub project: Option<String>,

    /// Session start timestamp (from the first event)
    pub started: DateTime<Utc>,

    /// Session end timestamp, if available
    ///
    /// `None` indicates the session is still active or the source doesn't log end times
    pub ended: Option<DateTime<Utc>>,

    /// Number of conversational turns in this session
    ///
    /// A "turn" is a user message plus all assistant/tool responses until the next user message
    pub turns: u32,

    /// One-line summary of what the session accomplished
    ///
    /// Generated during enrichment from the first user prompt and outcome. Used for search
    /// result previews and analytics.
    pub summary: Option<String>,

    /// Detected outcome: "success", "failure", "abandoned", or "unknown"
    ///
    /// Computed via signal scoring (see enrichment::outcome). Used for outcome filtering
    /// and success rate analytics.
    pub outcome: Option<String>,

    /// Tags extracted from content, tool names, and file types
    ///
    /// Examples: `["rust", "postgres", "migration", "auth"]`. Enables faceted search and
    /// technology trend analysis.
    pub tags: Vec<String>,

    /// Files referenced or modified during this session
    ///
    /// Extracted from tool call parameters and content analysis. Used for the file knowledge
    /// map (`agentscribe file <path>`) and impact analysis.
    pub files_touched: Vec<String>,

    /// LLM model used, if available in source data
    pub model: Option<String>,

    /// Parent session ID for subagent/sidechain sessions
    ///
    /// Set when this session was spawned as a sub-agent by another session (e.g., Claude Code's
    /// sidechain agents). Format is same as session_id: `<agent>/<id>`. Enables reconstructing
    /// the full conversation tree when main agents delegate to specialists.
    pub parent_session_id: Option<String>,
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
            parent_session_id: None,
        }
    }
}

/// Scrape state tracking for a single source file.
///
/// [`SourceFileState`] enables incremental scraping by tracking how far AgentScribe has read
/// in each source file. This prevents re-processing unchanged data and enables efficient
/// background scraping of active log files.
///
/// # Incremental Scraping Strategies
///
/// Different log formats use different incremental strategies:
///
/// * **JSONL** (append-only): Tracks `last_byte_offset`. On re-scrape, seeks to this offset
///   and reads only new lines. This is exact and efficient for Claude Code, Codex, etc.
///
/// * **Markdown** (delimiter-based): Tracks `last_delimiter_offset`. On re-scrape, seeks back
///   to the last delimiter boundary to pick up any appended content in the current session
///   plus new sessions. Used for Aider logs.
///
/// * **SQLite** (databases): Tracks `last_scraped` timestamp. On re-scrape, queries for records
///   with `time_updated > last_scraped`. Used for Cursor and Windsurf.
///
/// * **Rolling window** (truncating sources): When `truncation_limit` is set in the plugin,
///   state is cleared before each scrape because old conversations are overwritten. Used for
///   Windsurf (20-conversation limit).
///
/// # Crash Safety
///
/// Scrape state is crash-safe per ADR-1: state is written atomically via temp-file + rename,
/// and corrupted state files are quarantined rather than blocking scraping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFileState {
    /// Plugin name that owns this source file
    pub plugin: String,

    /// Last byte offset read from this file (for JSONL/Markdown incremental scraping)
    ///
    /// Used to seek directly to new data on re-scrape. For delimiter-based formats, see
    /// `last_delimiter_offset` for the session boundary tracking.
    pub last_byte_offset: u64,

    /// File modification time at last successful scrape
    ///
    /// Used to detect if the file was modified since last scrape. For truncation detection,
    /// if `last_byte_offset > current_file_size`, the file was rewritten and a full rescan
    /// is triggered.
    pub last_modified: DateTime<Utc>,

    /// Timestamp of the last successful scrape of this file
    ///
    /// Used for time-based incremental scraping (SQLite formats) and for diagnostics.
    pub last_scraped: DateTime<Utc>,

    /// Session IDs discovered in this file
    ///
    /// Tracks which sessions have been extracted from this file. Used for deduplication
    /// and for understanding the relationship between source files and sessions.
    pub session_ids: Vec<String>,

    /// For delimiter-based formats: offset of the last session delimiter seen
    ///
    /// Used by Markdown parsers (Aider) to resume from the last session boundary rather than
    /// the exact byte offset. This ensures that partially-written sessions are re-processed
    /// completely rather than creating partial sessions.
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

/// Global scrape state tracking all source files across all plugins.
///
/// [`ScrapeState`] is the top-level data structure that tracks incremental scraping progress
/// for every source file monitored by AgentScribe. It enables efficient background scraping
/// by allowing the scraper to skip unchanged files and resume reading partially-written files.
///
/// # Persistence
///
/// Scrape state is persisted to `~/.agentscribe/state/scrape-state.json` and loaded at startup.
/// The file is written atomically (temp-file + rename) for crash safety per ADR-1. If the file
/// is corrupted, it is quarantined and scraping starts from an empty state (one-time full rescan).
///
/// # Concurrency
///
/// Scrape state is protected by file locking (`flock`) to prevent concurrent writes when both
/// the daemon and a CLI `agentscribe scrape` run simultaneously. The second process waits for
/// the lock.
///
/// # File Key Format
///
/// Sources are keyed by their absolute file path. This ensures that the same file is tracked
/// consistently across re-scrapes, even if the plugin configuration changes. Paths are stored
/// as strings (not `PathBuf`) for JSON serialization.
///
/// # Examples
///
/// ```no_run
/// use agentscribe::event::ScrapeState;
/// use std::collections::HashMap;
///
/// // Create empty state (typically loaded from disk)
/// let mut state = ScrapeState::new();
///
/// // Get or create state for a specific file
/// let file_state = state.get_or_create(
///     "/home/user/.claude/projects/-home-user-myapp/abc123.jsonl",
///     "claude-code"
/// );
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeState {
    /// Per-source-file state, keyed by absolute file path
    ///
    /// Each value tracks the incremental scraping position for one source file. The map
    /// contains entries for all files that have ever been successfully scraped.
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
