//! Test helper functions for AgentScribe tests
//!
//! This module provides reusable test infrastructure for setting up
//! temporary directories, configured plugins, and common test scenarios.

use agentscribe::plugin::{
    LogFormat, Parser, Plugin, PluginMeta, SessionDetection, SessionIdSource, Source,
};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Set up a temporary directory with the standard AgentScribe layout
///
/// Creates a temporary directory with the following structure:
/// - `.agentscribe/plugins/` - For plugin definitions
/// - `.agentscribe/sessions/` - For normalized session files
/// - `.agentscribe/index/` - For search indices
/// - `.agentscribe/state/` - For scrape state
///
/// # Returns
///
/// A `tempfile::TempDir` that will be automatically cleaned up when dropped.
///
/// # Example
///
/// ```ignore
/// let temp_dir = setup_temp_directory();
/// let data_dir = temp_dir.path().join(".agentscribe");
/// // Use data_dir for testing
/// ```
pub fn setup_temp_directory() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let data_dir = dir.path().join(".agentscribe");

    fs::create_dir_all(data_dir.join("plugins")).expect("Failed to create plugins dir");
    fs::create_dir_all(data_dir.join("sessions")).expect("Failed to create sessions dir");
    fs::create_dir_all(data_dir.join("index")).expect("Failed to create index dir");
    fs::create_dir_all(data_dir.join("state")).expect("Failed to create state dir");

    dir
}

/// Create a configured claude-code plugin for testing
///
/// Returns a minimal `Plugin` configured for Claude Code format with
/// subagent detection enabled. This is useful for integration tests
/// that need to scrape Claude Code sessions.
///
/// # Arguments
///
/// * `base_path` - The base directory path where `.claude/projects/` exists
///
/// # Returns
///
/// A configured `Plugin` ready to be loaded into a scraper.
///
/// # Example
///
/// ```ignore
/// let temp = setup_temp_directory();
/// let claude_dir = temp.path().join(".claude/projects/test");
/// let plugin = create_claude_code_plugin(&claude_dir);
/// scraper.plugin_manager_mut().add_plugin(plugin);
/// ```
pub fn create_claude_code_plugin(base_path: &Path) -> Plugin {
    // Convert path to string for the glob pattern
    let base_str = base_path
        .to_str()
        .expect("Path contains invalid characters");
    let glob_pattern = format!("{}/**/*.jsonl", base_str);

    let mut static_fields = HashMap::new();
    static_fields.insert("source_agent".to_string(), serde_json::json!("claude-code"));

    Plugin {
        plugin: PluginMeta {
            name: "claude-code".to_string(),
            version: "1.0".to_string(),
        },
        source: Source {
            paths: vec![glob_pattern],
            exclude: vec![], // Empty - subagents are NOT excluded by default
            format: LogFormat::Jsonl,
            session_detection: SessionDetection::OneFilePerSession {
                session_id_from: SessionIdSource::Filename,
            },
            tree: None,
            truncation_limit: None,
            envelope: None,
            array: None,
        },
        parser: Parser {
            timestamp: Some("timestamp".to_string()),
            role: Some("role".to_string()),
            content: Some("content".to_string()),
            type_field: Some("type".to_string()),
            static_fields,
            ..Default::default()
        },
        metadata: None,
    }
}

/// Create a minimal parser for simple JSONL tests
///
/// Returns a `Parser` with basic field mappings for simple JSONL formats
/// that have `timestamp`, `role`, and `content` fields.
pub fn create_simple_parser() -> Parser {
    let mut static_fields = HashMap::new();
    static_fields.insert("source_agent".to_string(), serde_json::json!("test-agent"));

    Parser {
        timestamp: Some("timestamp".to_string()),
        role: Some("role".to_string()),
        content: Some("content".to_string()),
        static_fields,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_setup_temp_directory_creates_required_structure() {
        let temp_dir = setup_temp_directory();
        let data_dir = temp_dir.path().join(".agentscribe");

        // Verify all required directories exist
        assert!(data_dir.join("plugins").is_dir());
        assert!(data_dir.join("sessions").is_dir());
        assert!(data_dir.join("index").is_dir());
        assert!(data_dir.join("state").is_dir());
    }

    #[test]
    fn test_setup_temp_directory_is_unique() {
        let temp1 = setup_temp_directory();
        let temp2 = setup_temp_directory();

        // Each call should create a unique directory
        assert_ne!(temp1.path(), temp2.path());
    }

    #[test]
    fn test_create_claude_code_plugin_structure() {
        let temp_dir = setup_temp_directory();
        let base_path = temp_dir.path().join(".claude/projects");
        let plugin = create_claude_code_plugin(&base_path);

        // Verify plugin metadata
        assert_eq!(plugin.plugin.name, "claude-code");
        assert_eq!(plugin.plugin.version, "1.0");

        // Verify source configuration
        assert_eq!(plugin.source.format, LogFormat::Jsonl);
        assert!(!plugin.source.paths.is_empty());
        assert_eq!(plugin.source.exclude.len(), 0); // Subagents NOT excluded

        // Verify parser configuration
        assert_eq!(plugin.parser.timestamp, Some("timestamp".to_string()));
        assert_eq!(plugin.parser.role, Some("role".to_string()));
        assert_eq!(plugin.parser.content, Some("content".to_string()));
    }

    #[test]
    fn test_create_claude_code_plugin_includes_subagents() {
        let temp_dir = setup_temp_directory();
        let base_path = temp_dir.path().join(".claude/projects");
        let plugin = create_claude_code_plugin(&base_path);

        // Verify exclude list is empty - subagents should be included
        assert_eq!(
            plugin.source.exclude.len(),
            0,
            "Subagents should NOT be excluded by default"
        );
    }

    #[test]
    fn test_create_simple_parser() {
        let parser = create_simple_parser();

        assert_eq!(parser.timestamp, Some("timestamp".to_string()));
        assert_eq!(parser.role, Some("role".to_string()));
        assert_eq!(parser.content, Some("content".to_string()));

        // Verify static field was set
        assert_eq!(
            parser.static_fields.get("source_agent"),
            Some(&serde_json::json!("test-agent"))
        );
    }
}
