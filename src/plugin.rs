//! Plugin system for scraper definitions
//!
//! Plugins are TOML files that define how to find, parse, and normalize
//! conversation logs from different agent types.

use crate::error::{AgentScribeError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
}

/// Supported log formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LogFormat {
    Jsonl,
    Markdown,
    JsonTree,
    Sqlite,
}

impl LogFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogFormat::Jsonl => "jsonl",
            LogFormat::Markdown => "markdown",
            LogFormat::JsonTree => "json-tree",
            LogFormat::Sqlite => "sqlite",
        }
    }

    #[allow(dead_code, clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "jsonl" => Some(LogFormat::Jsonl),
            "markdown" => Some(LogFormat::Markdown),
            "json-tree" => Some(LogFormat::JsonTree),
            "sqlite" => Some(LogFormat::Sqlite),
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
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
}
