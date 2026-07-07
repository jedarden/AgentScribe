//! Plugin system for scraper definitions
//!
//! Plugins are TOML files that define how to find, parse, and normalize
//! conversation logs from different agent types.

use crate::error::{AgentScribeError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::warn;

/// Plugin definition from TOML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plugin {
    pub plugin: PluginMeta,
    pub source: Source,
    #[serde(default)]
    pub parser: Parser,
    #[serde(default)]
    pub metadata: Option<Metadata>,
}

/// Plugin identity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMeta {
    pub name: String,
    pub version: String,
}

/// Configuration for JSON array sources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonArraySourceConfig {
    /// Dot-path to the array within the JSON document (e.g., "data.items")
    /// Empty string means the document root is the array
    #[serde(default)]
    pub items_path: String,
}

impl Default for JsonArraySourceConfig {
    fn default() -> Self {
        JsonArraySourceConfig {
            items_path: String::new(),
        }
    }
}

/// Envelope configuration for JSONL sources where lines are wrapped
/// in a {timestamp, type, payload} structure (e.g., Codex rollouts)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    /// Field name containing the actual event payload
    pub payload_field: String,
    /// Field name containing the event type for routing
    pub type_field: String,
    /// Maps type values to routing actions: "event", "meta", or "skip"
    #[serde(default)]
    pub type_routing: HashMap<String, String>,
}

impl Envelope {
    /// Get the routing action for a given type value
    /// Returns "skip" for unknown types (with warning logged at parse time)
    pub fn get_routing(&self, type_value: &str) -> &str {
        match self.type_routing.get(type_value) {
            Some(action) => {
                match action.as_str() {
                    "event" | "meta" | "skip" => action,
                    // Invalid routing values are treated as skip
                    _ => "skip",
                }
            }
            // Unknown types default to skip with a warning
            None => {
                warn!(
                    type_value = type_value,
                    "Unknown envelope type value, routing to 'skip'"
                );
                "skip"
            }
        }
    }

    /// Validate envelope configuration
    pub fn validate(&self) -> Result<()> {
        // Validate routing values
        for (type_val, action) in &self.type_routing {
            if !matches!(action.as_str(), "event" | "meta" | "skip") {
                return Err(AgentScribeError::InvalidPlugin(format!(
                    "Invalid envelope routing action '{}' for type '{}': must be one of 'event', 'meta', 'skip'",
                    action, type_val
                )));
            }
        }
        Ok(())
    }
}

/// Source configuration - where to find logs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub paths: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    pub format: LogFormat,
    #[serde(default)]
    pub session_detection: SessionDetection,
    #[serde(default)]
    pub tree: Option<TreeConfig>,
    /// Hard limit on the number of conversations the source retains (rolling window).
    /// When set the scraper clears per-file state before each scrape so that
    /// overwritten conversations do not leave stale data in the output.
    /// Example: Windsurf keeps at most 20 conversations; set this to 20.
    #[serde(default)]
    pub truncation_limit: Option<u32>,
    /// Optional envelope unwrapping for wrapped JSONL lines
    #[serde(default)]
    pub envelope: Option<Envelope>,
    /// Optional array configuration for JSON array sources
    #[serde(default)]
    pub array: Option<JsonArraySourceConfig>,
}

/// Supported log formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LogFormat {
    Jsonl,
    Markdown,
    JsonTree,
    Sqlite,
    JsonArray,
}

impl LogFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogFormat::Jsonl => "jsonl",
            LogFormat::Markdown => "markdown",
            LogFormat::JsonTree => "json-tree",
            LogFormat::Sqlite => "sqlite",
            LogFormat::JsonArray => "json-array",
        }
    }

    #[allow(dead_code, clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "jsonl" => Some(LogFormat::Jsonl),
            "markdown" => Some(LogFormat::Markdown),
            "json-tree" => Some(LogFormat::JsonTree),
            "sqlite" => Some(LogFormat::Sqlite),
            "json-array" => Some(LogFormat::JsonArray),
            _ => None,
        }
    }
}

/// Session detection strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "kebab-case")]
pub enum SessionDetection {
    #[serde(rename = "one-file-per-session")]
    OneFilePerSession { session_id_from: SessionIdSource },
    #[serde(rename = "timestamp-gap")]
    TimestampGap { gap_threshold: String },
    #[serde(rename = "delimiter")]
    Delimiter { delimiter_pattern: String },
}

impl Default for SessionDetection {
    fn default() -> Self {
        SessionDetection::OneFilePerSession {
            session_id_from: SessionIdSource::Filename,
        }
    }
}

/// Where to extract the session ID
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionIdSource {
    Filename,
    #[serde(rename = "field")]
    Field(String),
}

/// Configuration for json-tree format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeConfig {
    pub session_glob: String,
    pub message_glob: String,
    pub part_glob: String,
    pub session_id_field: String,
    pub message_session_field: String,
    pub part_message_field: String,
    pub ordering_field: String,
}

/// Parser configuration - field mapping
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Parser {
    // JSONL/JSON tree fields
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub type_field: Option<String>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_args: Option<String>,
    #[serde(default)]
    pub tokens_in: Option<String>,
    #[serde(default)]
    pub tokens_out: Option<String>,

    // Markdown-specific fields
    #[serde(default)]
    pub user_prefix: Option<String>,
    #[serde(default)]
    pub assistant_prefix: Option<String>,
    #[serde(default)]
    pub tool_prefix: Option<String>,
    #[serde(default)]
    pub system_prefix: Option<String>,
    #[serde(default)]
    pub timestamp_pattern: Option<String>,

    // SQLite-specific fields
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub key_filter: Option<String>,
    #[serde(default)]
    pub content_path: Option<String>,
    /// Regex applied to the key column to extract a per-row session ID (first
    /// capture group).  When set, `detect_sessions` queries distinct IDs from
    /// the DB and `parse` tags every event with its composerId so the scraper
    /// can route events to the correct session output file.
    /// Example: `"^bubbleId:([^:]+):"` extracts composerId from Cursor/Windsurf keys.
    #[serde(default)]
    pub key_session_id_regex: Option<String>,

    // Field filtering
    #[serde(default)]
    pub role_map: HashMap<String, String>,
    #[serde(default)]
    pub include_types: Option<TypeFilter>,
    #[serde(default)]
    pub exclude_types: Option<TypeFilter>,

    // Static metadata
    #[serde(default)]
    pub static_fields: HashMap<String, serde_json::Value>,

    // Project detection
    #[serde(default)]
    pub project: Option<ProjectDetection>,

    // Model detection
    #[serde(default)]
    pub model: Option<ModelDetection>,

    // File path extraction
    #[serde(default)]
    pub file_paths: Option<FilePathExtraction>,
}

/// Type filter for including/excluding events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeFilter {
    pub field: String,
    pub values: Vec<String>,
}

/// Project detection strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "kebab-case")]
pub enum ProjectDetection {
    #[serde(rename = "field")]
    Field { field: String },
    #[serde(rename = "parent_dir")]
    ParentDir,
    #[serde(rename = "git_root")]
    GitRoot,
}

#[allow(clippy::derivable_impls)]
impl Default for ProjectDetection {
    fn default() -> Self {
        ProjectDetection::ParentDir
    }
}

/// Model detection strategy
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "source", rename_all = "kebab-case")]
pub enum ModelDetection {
    #[serde(rename = "metadata")]
    Metadata { field: String },
    #[serde(rename = "event")]
    Event { field: String },
    #[serde(rename = "static")]
    Static { value: String },
    #[serde(rename = "none")]
    #[default]
    None,
}

/// File path extraction configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePathExtraction {
    /// Structured extraction from tool_call fields
    #[serde(default)]
    pub tool_call_field: Option<String>,
    /// Also extract paths from content via regex
    #[serde(default)]
    pub content_regex: Option<bool>,
}

/// Metadata sources
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metadata {
    /// Path to a companion index file (JSONL) that maps session IDs to metadata.
    /// Unlike session_meta/session_summary which are per-session file templates,
    /// this is a single file containing metadata for all sessions.
    /// Example: "~/.codex/session_index.jsonl" for Codex sessions.
    #[serde(default)]
    pub companion_index: Option<String>,
    #[serde(default)]
    pub session_meta: Option<String>,
    #[serde(default)]
    pub session_summary: Option<String>,
    #[serde(default)]
    pub session_facets: Option<String>,
}

/// Plugin manager - loads and validates plugins
pub struct PluginManager {
    plugins: HashMap<String, Plugin>,
    plugin_dir: PathBuf,
}

impl PluginManager {
    /// Create a new plugin manager
    pub fn new(plugin_dir: PathBuf) -> Self {
        PluginManager {
            plugins: HashMap::new(),
            plugin_dir,
        }
    }

    /// Load all plugins from the plugin directory
    pub fn load_all(&mut self) -> Result<Vec<String>> {
        if !self.plugin_dir.exists() {
            return Ok(Vec::new());
        }

        let mut loaded = Vec::new();
        let entries = std::fs::read_dir(&self.plugin_dir)
            .map_err(|e| AgentScribeError::DataDir(format!("Cannot read plugin dir: {}", e)))?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }

            match self.load_plugin(&path) {
                Ok(name) => loaded.push(name),
                Err(e) => {
                    eprintln!("Warning: Failed to load plugin {:?}: {}", path, e);
                }
            }
        }

        Ok(loaded)
    }

    /// Load a single plugin from a TOML file
    pub fn load_plugin(&mut self, path: &Path) -> Result<String> {
        let content = std::fs::read_to_string(path)?;
        let plugin: Plugin = toml::from_str(&content).map_err(|e| {
            AgentScribeError::plugin_error(path.display().to_string(), e.to_string())
        })?;

        let name = plugin.plugin.name.clone();
        self.validate_plugin(&plugin)?;
        self.add_plugin(plugin);
        Ok(name)
    }

    /// Validate a plugin definition
    pub fn validate_plugin(&self, plugin: &Plugin) -> Result<()> {
        // Check plugin name
        if plugin.plugin.name.is_empty() {
            return Err(AgentScribeError::InvalidPlugin(
                "Plugin name cannot be empty".to_string(),
            ));
        }

        // Check name format (lowercase, alphanumeric, hyphens)
        if !plugin
            .plugin
            .name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(AgentScribeError::InvalidPlugin(
                "Plugin name must be lowercase alphanumeric with hyphens only".to_string(),
            ));
        }

        // Check paths
        if plugin.source.paths.is_empty() {
            return Err(AgentScribeError::InvalidPlugin(
                "Source paths cannot be empty".to_string(),
            ));
        }

        // Validate format-specific fields
        match plugin.source.format {
            LogFormat::Jsonl => {
                if plugin.parser.timestamp.is_none()
                    || plugin.parser.role.is_none()
                    || plugin.parser.content.is_none()
                {
                    return Err(AgentScribeError::InvalidPlugin(
                        "JSONL format requires timestamp, role, and content fields".to_string(),
                    ));
                }
            }
            LogFormat::Markdown => {
                if plugin.parser.user_prefix.is_none() {
                    return Err(AgentScribeError::InvalidPlugin(
                        "Markdown format requires user_prefix".to_string(),
                    ));
                }
            }
            LogFormat::JsonTree => {
                if plugin.source.tree.is_none() {
                    return Err(AgentScribeError::InvalidPlugin(
                        "JSON tree format requires [source.tree] configuration".to_string(),
                    ));
                }
            }
            LogFormat::Sqlite => {
                if plugin.parser.query.is_none() {
                    return Err(AgentScribeError::InvalidPlugin(
                        "SQLite format requires query field".to_string(),
                    ));
                }
            }
            LogFormat::JsonArray => {
                if plugin.parser.timestamp.is_none()
                    || plugin.parser.role.is_none()
                    || plugin.parser.content.is_none()
                {
                    return Err(AgentScribeError::InvalidPlugin(
                        "JSON array format requires timestamp, role, and content fields"
                            .to_string(),
                    ));
                }
            }
        }

        // Validate role_map target values
        for to in plugin.parser.role_map.values() {
            if !matches!(
                to.as_str(),
                "user" | "assistant" | "system" | "tool_call" | "tool_result"
            ) {
                return Err(AgentScribeError::InvalidPlugin(format!(
                    "Invalid role_map target: {}. Must be one of: user, assistant, system, tool_call, tool_result",
                    to
                )));
            }
        }

        // Validate envelope configuration if present
        if let Some(ref envelope) = plugin.source.envelope {
            envelope.validate()?;
        }

        Ok(())
    }

    /// Add a plugin to the manager
    pub fn add_plugin(&mut self, plugin: Plugin) {
        let name = plugin.plugin.name.clone();
        self.plugins.insert(name, plugin);
    }

    /// Get a plugin by name
    pub fn get(&self, name: &str) -> Option<&Plugin> {
        self.plugins.get(name)
    }

    /// Get all plugins
    pub fn all(&self) -> &HashMap<String, Plugin> {
        &self.plugins
    }

    /// Get plugin names
    pub fn names(&self) -> Vec<&str> {
        self.plugins.keys().map(|k| k.as_str()).collect()
    }
}

/// Validate a plugin file without loading
pub fn validate_plugin_file(path: &Path) -> Result<Plugin> {
    let content = std::fs::read_to_string(path)?;
    let plugin: Plugin = toml::from_str(&content)
        .map_err(|e| AgentScribeError::plugin_error(path.display().to_string(), e.to_string()))?;

    let manager = PluginManager::new(PathBuf::from("/dummy"));
    manager.validate_plugin(&plugin)?;

    Ok(plugin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_format_from_str() {
        assert_eq!(LogFormat::from_str("jsonl"), Some(LogFormat::Jsonl));
        assert_eq!(LogFormat::from_str("markdown"), Some(LogFormat::Markdown));
        assert_eq!(LogFormat::from_str("json-tree"), Some(LogFormat::JsonTree));
        assert_eq!(LogFormat::from_str("invalid"), None);
    }

    #[test]
    fn test_session_detection_default() {
        let sd = SessionDetection::default();
        assert!(matches!(
            sd,
            SessionDetection::OneFilePerSession {
                session_id_from: SessionIdSource::Filename
            }
        ));
    }

    // -- Envelope tests --

    fn make_envelope(routing: Vec<(&str, &str)>) -> Envelope {
        let mut type_routing = HashMap::new();
        for (k, v) in routing {
            type_routing.insert(k.to_string(), v.to_string());
        }
        Envelope {
            payload_field: "payload".to_string(),
            type_field: "type".to_string(),
            type_routing,
        }
    }

    #[test]
    fn test_envelope_get_routing_known_types() {
        let env = make_envelope(vec![
            ("message", "event"),
            ("meta_update", "meta"),
            ("heartbeat", "skip"),
        ]);
        assert_eq!(env.get_routing("message"), "event");
        assert_eq!(env.get_routing("meta_update"), "meta");
        assert_eq!(env.get_routing("heartbeat"), "skip");
    }

    #[test]
    fn test_envelope_get_routing_unknown_type_defaults_to_skip() {
        let env = make_envelope(vec![("message", "event")]);
        assert_eq!(env.get_routing("unknown_type"), "skip");
        assert_eq!(env.get_routing(""), "skip");
    }

    #[test]
    fn test_envelope_get_routing_invalid_value_treated_as_skip() {
        let env = make_envelope(vec![("bad", "unknown")]);
        // Invalid routing values are treated as skip at runtime
        assert_eq!(env.get_routing("bad"), "skip");
    }

    #[test]
    fn test_envelope_validate_accepts_valid_routing() {
        let env = make_envelope(vec![
            ("message", "event"),
            ("meta_update", "meta"),
            ("heartbeat", "skip"),
        ]);
        assert!(env.validate().is_ok());
    }

    #[test]
    fn test_envelope_validate_rejects_invalid_routing() {
        let env = make_envelope(vec![("message", "event"), ("bad_type", "unknown")]);
        let err = env.validate().unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("Invalid envelope routing action"));
        assert!(msg.contains("'unknown'"));
        assert!(msg.contains("bad_type"));
    }

    #[test]
    fn test_envelope_validate_rejects_other_invalid_values() {
        let env = make_envelope(vec![("message", "delete")]);
        let err = env.validate().unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("'delete'"));
    }

    #[test]
    fn test_validate_plugin_rejects_invalid_envelope() {
        let plugin = Plugin {
            plugin: PluginMeta {
                name: "test-plugin".to_string(),
                version: "0.1.0".to_string(),
            },
            source: Source {
                paths: vec!["~/logs".to_string()],
                exclude: vec![],
                format: LogFormat::Jsonl,
                session_detection: SessionDetection::default(),
                tree: None,
                truncation_limit: None,
                envelope: Some(Envelope {
                    payload_field: "payload".to_string(),
                    type_field: "type".to_string(),
                    type_routing: {
                        let mut m = HashMap::new();
                        m.insert("msg".to_string(), "event".to_string());
                        m.insert("bad".to_string(), "garbage".to_string());
                        m
                    },
                }),
            },
            parser: Parser {
                timestamp: Some("ts".to_string()),
                role: Some("role".to_string()),
                content: Some("content".to_string()),
                ..Parser::default()
            },
            metadata: None,
        };
        let manager = PluginManager::new(PathBuf::from("/dummy"));
        let result = manager.validate_plugin(&plugin);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("garbage"));
    }
}
