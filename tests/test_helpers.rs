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

/// Create a basic test plugin for simple JSONL parsing
///
/// Returns a minimal `Plugin` configured for basic JSONL format testing.
/// This plugin has no envelope routing, no array handling, and uses
/// simple field mappings for timestamp, role, and content.
///
/// # Returns
///
/// A configured `Plugin` suitable for basic parsing tests.
///
/// # Example
///
/// ```ignore
/// let plugin = create_test_plugin();
/// assert_eq!(plugin.plugin.name, "test");
/// assert!(plugin.source.envelope.is_none());
/// ```
pub fn create_test_plugin() -> Plugin {
    let mut static_fields = HashMap::new();
    static_fields.insert("source_agent".to_string(), serde_json::json!("test"));

    Plugin {
        plugin: PluginMeta {
            name: "test".to_string(),
            version: "1.0".to_string(),
        },
        source: Source {
            paths: vec!["/tmp/test.jsonl".to_string()],
            exclude: vec![],
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
            static_fields,
            ..Default::default()
        },
        metadata: None,
    }
}

/// Create a plugin with envelope routing configured
///
/// Returns a `Plugin` configured with envelope routing for testing
/// envelope-based log formats where JSONL lines are wrapped in an
/// envelope structure with type-based routing.
///
/// The envelope configuration includes:
/// - `message` events → routed as "event" (extracted from payload_field)
/// - `session` events → routed as "skip" (dropped)
/// - `compaction` events → routed as "meta" (metadata preserved)
/// - `model_change` events → routed as "skip" (dropped)
///
/// # Returns
///
/// A configured `Plugin` with envelope routing enabled.
///
/// # Example
///
/// ```ignore
/// let plugin = create_envelope_plugin();
/// assert_eq!(plugin.plugin.name, "test-envelope");
/// assert!(plugin.source.envelope.is_some());
///
/// let envelope = plugin.source.envelope.unwrap();
/// assert_eq!(envelope.get_routing("message"), "event");
/// assert_eq!(envelope.get_routing("session"), "skip");
/// ```
pub fn create_envelope_plugin() -> Plugin {
    let mut type_routing = HashMap::new();
    type_routing.insert("message".to_string(), "event".to_string());
    type_routing.insert("session".to_string(), "skip".to_string());
    type_routing.insert("compaction".to_string(), "meta".to_string());
    type_routing.insert("model_change".to_string(), "skip".to_string());

    let mut role_map = HashMap::new();
    role_map.insert("toolResult".to_string(), "tool_result".to_string());

    let mut static_fields = HashMap::new();
    static_fields.insert(
        "source_agent".to_string(),
        serde_json::json!("test-envelope"),
    );

    Plugin {
        plugin: PluginMeta {
            name: "test-envelope".to_string(),
            version: "1.0".to_string(),
        },
        source: Source {
            paths: vec!["/tmp/test-envelope.jsonl".to_string()],
            exclude: vec![],
            format: LogFormat::Jsonl,
            session_detection: SessionDetection::OneFilePerSession {
                session_id_from: SessionIdSource::Filename,
            },
            tree: None,
            truncation_limit: None,
            envelope: Some(agentscribe::plugin::Envelope {
                payload_field: "message".to_string(),
                type_field: "type".to_string(),
                type_routing,
            }),
            array: None,
        },
        parser: Parser {
            timestamp: Some("timestamp".to_string()),
            role: Some("role".to_string()),
            content: Some("content".to_string()),
            role_map,
            static_fields,
            ..Default::default()
        },
        metadata: None,
    }
}

/// Create a plugin with meta routing for testing envelope types
///
/// Returns a `Plugin` configured with envelope routing specifically for testing
/// meta-type events (session_start, session_end, etc.) that should accumulate
/// metadata without producing canonical events.
///
/// The envelope configuration includes:
/// - `message` events → routed as "event" (produce canonical events)
/// - `heartbeat` events → routed as "skip" (dropped)
/// - `ping` events → routed as "skip" (dropped)
/// - `session_start` events → routed as "meta" (metadata preserved, no events)
/// - `session_end` events → routed as "meta" (metadata preserved, no events)
/// - `metrics` events → routed as "meta" (metadata preserved, no events)
/// - `compaction` events → routed as "meta" (metadata preserved, no events)
///
/// # Returns
///
/// A configured `Plugin` with meta routing enabled for testing.
///
/// # Example
///
/// ```ignore
/// let plugin = create_meta_routing_test_plugin();
/// assert_eq!(plugin.plugin.name, "test-meta-routing");
/// assert!(plugin.source.envelope.is_some());
///
/// let envelope = plugin.source.envelope.unwrap();
/// assert_eq!(envelope.get_routing("session_start"), "meta");
/// assert_eq!(envelope.get_routing("session_end"), "meta");
/// ```
pub fn create_meta_routing_test_plugin() -> Plugin {
    let mut type_routing = HashMap::new();
    type_routing.insert("message".to_string(), "event".to_string());
    type_routing.insert("heartbeat".to_string(), "skip".to_string());
    type_routing.insert("ping".to_string(), "skip".to_string());
    type_routing.insert("session_start".to_string(), "meta".to_string());
    type_routing.insert("session_end".to_string(), "meta".to_string());
    type_routing.insert("metrics".to_string(), "meta".to_string());
    type_routing.insert("compaction".to_string(), "meta".to_string());

    let mut static_fields = HashMap::new();
    static_fields.insert(
        "source_agent".to_string(),
        serde_json::json!("test-meta-routing"),
    );

    Plugin {
        plugin: PluginMeta {
            name: "test-meta-routing".to_string(),
            version: "1.0".to_string(),
        },
        source: Source {
            paths: vec!["/tmp/test-meta-routing.jsonl".to_string()],
            exclude: vec![],
            format: LogFormat::Jsonl,
            session_detection: SessionDetection::OneFilePerSession {
                session_id_from: SessionIdSource::Filename,
            },
            tree: None,
            truncation_limit: None,
            envelope: Some(agentscribe::plugin::Envelope {
                payload_field: "payload".to_string(),
                type_field: "type".to_string(),
                type_routing,
            }),
            array: None,
        },
        parser: Parser {
            timestamp: Some("timestamp".to_string()),
            role: Some("role".to_string()),
            content: Some("content".to_string()),
            static_fields,
            ..Default::default()
        },
        metadata: None,
    }
}

/// Helper function for testing meta routing fixture lines that should return Ok(Vec::new()).
///
/// This function encapsulates the common pattern for testing envelope lines that are
/// routed to "meta" types (such as session_start, session_end, metrics, compaction).
/// These meta-type lines should parse successfully but produce zero canonical events,
/// returning Ok(Vec::new()).
///
/// # Arguments
///
/// * `fixture_line` - The JSONL fixture line to parse
/// * `line_number` - The line number (for realistic error reporting in tests)
/// * `assertion_message` - Custom assertion message describing what should produce zero events
///
/// # Purpose
///
/// Meta-type routing is used for events that should accumulate session metadata but not
/// emit canonical events. Examples include:
/// - `session_start`: Marks session beginning with metadata (session_id, model, cwd)
/// - `session_end`: Marks session end with metadata (duration, final state)
/// - `compaction`: Metadata about storage operations
/// - `metrics`: Performance or operational metrics
///
/// # Examples
///
/// ```ignore
/// use agentscribe::parser::jsonl::JsonlParser;
/// use agentscribe::parser::ParseContext;
///
/// let session_start_line = r#"{"type":"session_start","timestamp":"2026-07-04T10:00:00Z","payload":{"session_id":"sess-001"}}"#;
/// assert_meta_routing_returns_empty(
///     session_start_line,
///     1,
///     "session_start should produce zero events"
/// );
///
/// let session_end_line = r#"{"type":"session_end","timestamp":"2026-07-04T10:30:00Z","payload":{"duration":1800}}"#;
/// assert_meta_routing_returns_empty(
///     session_end_line,
///     5,
///     "session_end should produce zero events"
/// );
/// ```
pub fn assert_meta_routing_returns_empty(
    fixture_line: &str,
    line_number: usize,
    assertion_message: &str,
) {
    use agentscribe::parser::{JsonlParser, ParseContext};

    let plugin = create_meta_routing_test_plugin();
    let context = ParseContext::new(
        "test-session".to_string(),
        "test-meta-routing".to_string(),
        "/tmp/test-meta-routing.jsonl".to_string(),
    );

    // Verify the line parses successfully
    let result = JsonlParser::parse_line(fixture_line, line_number, &context, &plugin);
    assert!(
        result.is_ok(),
        "Meta routing line should parse successfully: {}",
        assertion_message
    );

    // Verify it produces zero events (the expected behavior for meta-type routing)
    let events = result.unwrap();
    assert!(
        events.is_empty(),
        "{}: Meta-type routing should produce zero events, got {} events",
        assertion_message,
        events.len()
    );
}

/// Set up an empty Tantivy index for testing search behavior.
///
/// Creates a temporary directory with the standard AgentScribe layout and
/// initializes an empty Tantivy index at `<temp_dir>/.agentscribe/index/tantivy/`.
/// This is useful for tests that need to verify search behavior when the index
/// contains no documents, or for tests that want to start with a clean slate.
///
/// # Returns
///
/// A tuple containing:
/// * `tempfile::TempDir` - The temporary directory (cleaned up on drop)
/// * `agentscribe::index::IndexManager` - The index manager ready for use
///
/// # Index Location
///
/// The empty index is created at: `<temp_dir>/.agentscribe/index/tantivy/`
///
/// This path is significant because:
/// 1. It matches the production index structure
/// 2. It's where `IndexManager::open()` expects to find the index
/// 3. It persists for the lifetime of the `TempDir`
///
/// # Example
///
/// ```ignore
/// use agentscribe::index::IndexManager;
///
/// let (temp_dir, index_manager) = setup_empty_index();
/// let index_path = temp_dir.path().join(".agentscribe/index/tantivy");
///
/// // Verify the index exists and is empty
/// assert!(index_path.exists());
/// assert!(index_path.is_dir());
///
/// // Use the index manager for search tests
/// let reader = index_manager.index.reader().unwrap();
/// let searcher = reader.searcher();
/// assert_eq!(searcher.num_docs(), 0); // No documents indexed
/// ```
///
/// # Notes
///
/// - The index is created with the standard AgentScribe schema (see `agentscribe::index::build_schema()`)
/// - No documents are indexed — this is a truly empty index
/// - The `TempDir` is automatically cleaned up when dropped, so no manual cleanup is needed
/// - Multiple calls to this function create independent, isolated indices
pub fn setup_empty_index() -> (tempfile::TempDir, agentscribe::index::IndexManager) {
    use agentscribe::index::IndexManager;

    let temp_dir = setup_temp_directory();
    let data_dir = temp_dir.path().join(".agentscribe");

    // Create the index manager - this will create an empty index if one doesn't exist
    let index_manager =
        IndexManager::open(&data_dir).expect("Failed to create empty index manager");

    // Verify the index was created and is empty
    let index_path = data_dir.join("index").join("tantivy");
    assert!(
        index_path.exists(),
        "Index directory should exist at: {}",
        index_path.display()
    );

    // Begin and finish a write session to ensure the index is fully initialized
    let mut manager = index_manager;
    manager
        .begin_write()
        .expect("Failed to begin write on empty index");
    manager
        .finish()
        .expect("Failed to finish write on empty index");

    // Verify no documents are indexed
    let reader = manager.index().reader().unwrap();
    let searcher = reader.searcher();
    assert_eq!(
        searcher.num_docs(),
        0,
        "Empty index should have zero documents"
    );

    (temp_dir, manager)
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

    #[test]
    fn test_create_test_plugin() {
        let plugin = create_test_plugin();

        // Verify plugin metadata
        assert_eq!(plugin.plugin.name, "test");
        assert_eq!(plugin.plugin.version, "1.0");

        // Verify source configuration
        assert_eq!(plugin.source.format, LogFormat::Jsonl);
        assert_eq!(plugin.source.paths, vec!["/tmp/test.jsonl"]);
        assert!(plugin.source.envelope.is_none());
        assert!(plugin.source.array.is_none());

        // Verify parser configuration
        assert_eq!(plugin.parser.timestamp, Some("timestamp".to_string()));
        assert_eq!(plugin.parser.role, Some("role".to_string()));
        assert_eq!(plugin.parser.content, Some("content".to_string()));
    }

    #[test]
    fn test_create_envelope_plugin() {
        let plugin = create_envelope_plugin();

        // Verify plugin metadata
        assert_eq!(plugin.plugin.name, "test-envelope");
        assert_eq!(plugin.plugin.version, "1.0");

        // Verify source configuration
        assert_eq!(plugin.source.format, LogFormat::Jsonl);
        assert_eq!(plugin.source.paths, vec!["/tmp/test-envelope.jsonl"]);

        // Verify envelope is configured
        assert!(plugin.source.envelope.is_some());
        let envelope = plugin.source.envelope.as_ref().unwrap();
        assert_eq!(envelope.type_field, "type");
        assert_eq!(envelope.payload_field, "message");

        // Verify type routing
        assert_eq!(envelope.get_routing("message"), "event");
        assert_eq!(envelope.get_routing("session"), "skip");
        assert_eq!(envelope.get_routing("compaction"), "meta");
        assert_eq!(envelope.get_routing("model_change"), "skip");
        assert_eq!(envelope.get_routing("unknown"), "skip"); // Unknown types default to skip

        // Verify parser configuration
        assert_eq!(plugin.parser.timestamp, Some("timestamp".to_string()));
        assert_eq!(plugin.parser.role, Some("role".to_string()));
        assert_eq!(plugin.parser.content, Some("content".to_string()));

        // Verify role_map
        assert_eq!(
            plugin.parser.role_map.get("toolResult"),
            Some(&"tool_result".to_string())
        );
    }
}
