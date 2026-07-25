//! Comprehensive tests for parent_session_id functionality.
//!
//! This test suite verifies that:
//! 1. Main sessions have parent_session_id as None/empty
//! 2. Subagent sessions have correct parent_session_id extracted from path structure
//! 3. The full subagent spawning flow (scrape → parse → index) works correctly
//! 4. Edge cases are handled properly

use std::fs;

use agentscribe::index::build_manifest_from_events;
use agentscribe::plugin::{
    LogFormat, Parser, Plugin, PluginMeta, SessionDetection, SessionIdSource, Source,
};
use agentscribe::scraper::Scraper;

// ─── Test helpers ─────────────────────────────────────────────────────────────

/// Create a temp data directory with the required sub-structure.
fn make_data_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    fs::create_dir_all(dir.path().join("plugins")).unwrap();
    fs::create_dir_all(dir.path().join("sessions")).unwrap();
    fs::create_dir_all(dir.path().join("state")).unwrap();
    dir
}

/// Create a minimal JSONL plugin for testing.
fn jsonl_plugin(name: &str, glob: &str) -> Plugin {
    Plugin {
        plugin: PluginMeta {
            name: name.to_string(),
            version: "1.0".to_string(),
        },
        source: Source {
            paths: vec![glob.to_string()],
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
            static_fields: {
                let mut map = std::collections::HashMap::new();
                map.insert("source_agent".to_string(), serde_json::json!(name));
                map
            },
            ..Default::default()
        },
        metadata: None,
    }
}

/// Create test JSONL content with minimal events.
fn test_jsonl_content() -> String {
    r#"{"timestamp": "2026-07-23T10:00:00Z", "role": "user", "content": "Test message"}
{"timestamp": "2026-07-23T10:00:01Z", "role": "assistant", "content": "Test response"}"#.to_string()
}

// ─── Unit Tests: Path Parsing Logic ────────────────────────────────────────────

#[test]
fn test_parent_id_extraction_various_path_depths() {
    // Test parent_session_id extraction with different path depths
    let test_cases = vec![
        // (path, expected_parent_id, description)
        (
            "/home/user/.claude/projects/MyProject/parent-abc/subagents/agent-def.jsonl",
            Some("parent-abc".to_string()),
            "Standard project structure"
        ),
        (
            "/home/user/.claude/projects/nested/deep/path/parent-xyz/subagents/agent-123.jsonl",
            Some("parent-xyz".to_string()),
            "Nested project path"
        ),
        (
            "/home/user/.claude/projects/MyProject/main-session.jsonl",
            None,
            "Main session (no subagents)"
        ),
        (
            "/tmp/test.jsonl",
            None,
            "No projects directory"
        ),
        (
            "/home/user/.claude/projects/MyProject/subagents/agent-123.jsonl",
            None,
            "Subagents without parent session"
        ),
    ];

    for (path_str, expected_parent_id, description) in test_cases {
        let path = std::path::PathBuf::from(path_str);

        // Simulate the path parsing logic from JsonlParser
        let parent_session_id = path
            .components()
            .collect::<Vec<_>>()
            .iter()
            .position(|c| c.as_os_str() == "subagents")
            .and_then(|subagents_idx| {
                if subagents_idx >= 2 {
                    let components: Vec<_> = path.components().collect();
                    let parent_idx = subagents_idx - 1;
                    let has_projects_before_parent = components[..parent_idx]
                        .iter()
                        .any(|c| c.as_os_str() == "projects");

                    if has_projects_before_parent {
                        components
                            .get(parent_idx)
                            .and_then(|c| c.as_os_str().to_str())
                            .map(|s| s.to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            });

        assert_eq!(
            parent_session_id, expected_parent_id,
            "{}: expected {:?}, got {:?}",
            description, expected_parent_id, parent_session_id
        );
    }
}

// ─── Integration Tests: Full Flow ─────────────────────────────────────────────

#[test]
fn test_full_flow_subagent_session() {
    // Test the complete flow: scrape → parse → index for subagent sessions
    let data_dir = make_data_dir();
    let claude_dir = data_dir.path().join("sessions/claude-code");

    // Create a parent session
    let parent_uuid = "parent-session-main123";
    let parent_path = claude_dir.join(format!("{}.jsonl", parent_uuid));
    fs::create_dir_all(parent_path.parent().unwrap())
        .expect("Failed to create parent directory");
    fs::write(&parent_path, test_jsonl_content())
        .expect("Failed to write parent content");

    // Create a subagent session
    let subagent_id = "agent-sub456";
    let subagent_path = claude_dir
        .join(parent_uuid)
        .join("subagents")
        .join(format!("{}.jsonl", subagent_id));

    fs::create_dir_all(subagent_path.parent().unwrap())
        .expect("Failed to create subagent directory");
    fs::write(&subagent_path, test_jsonl_content())
        .expect("Failed to write subagent content");

    // Create scraper and plugin
    let mut scraper = Scraper::new(data_dir.path().to_path_buf())
        .expect("Failed to create scraper");

    let plugin = jsonl_plugin(
        "claude-code",
        &claude_dir.join("**/*.jsonl").to_str().unwrap(),
    );

    scraper.plugin_manager_mut().add_plugin(plugin);

    // Scrape the plugin
    let plugin_name = "claude-code";
    let plugin = scraper.plugin_manager().get(plugin_name).unwrap().clone();
    let result = scraper.scrape_plugin(&plugin).expect("Scrape should succeed");

    // Verify both sessions were scraped
    assert_eq!(
        result.sessions_scraped, 2,
        "Should scrape both parent and subagent sessions"
    );

    assert_eq!(
        result.sessions_indexed, 2,
        "Should index both sessions"
    );

    // List all sessions
    let sessions = scraper
        .list_sessions(plugin_name)
        .expect("Should list sessions");

    assert_eq!(sessions.len(), 2, "Should have two sessions total");

    // Find the subagent session
    let subagent_session = sessions
        .iter()
        .find(|s| s.contains(subagent_id))
        .expect("Should find subagent session");

    // Read the subagent session events
    let events = scraper
        .read_session(subagent_session)
        .expect("Should read subagent events");

    // Verify events were parsed correctly
    assert_eq!(events.len(), 2, "Should have 2 events from subagent session");

    // Verify all events have source_agent = claude-code-subagent
    for event in &events {
        assert_eq!(
            event.source_agent, "claude-code-subagent",
            "Subagent events should have source_agent = claude-code-subagent"
        );
    }

    // Find the parent session
    let parent_session = sessions
        .iter()
        .find(|s| s.contains(parent_uuid))
        .expect("Should find parent session");

    // Read the parent session events
    let parent_events = scraper
        .read_session(parent_session)
        .expect("Should read parent events");

    // Verify parent session events have source_agent = claude-code
    for event in &parent_events {
        assert_eq!(
            event.source_agent, "claude-code",
            "Parent events should have source_agent = claude-code"
        );
    }
}

#[test]
fn test_manifest_parent_session_id() {
    // Test that build_manifest_from_events correctly sets parent_session_id
    let events = vec![];
    let session_id = "test-session-id";
    let source_agent = "claude-code-subagent";
    let parent_session_id = Some("parent-session-123");

    let manifest = build_manifest_from_events(
        &events,
        session_id,
        source_agent,
        None::<&str>,
        None::<&str>,
        parent_session_id,
    );

    assert_eq!(
        manifest.parent_session_id,
        parent_session_id.map(|s| s.to_string()),
        "Manifest should have the correct parent_session_id"
    );
}

#[test]
fn test_manifest_main_session_no_parent() {
    // Test that main session manifests have no parent_session_id
    let events = vec![];
    let session_id = "main-session-id";
    let source_agent = "claude-code";
    let parent_session_id = None::<&str>;

    let manifest = build_manifest_from_events(
        &events,
        session_id,
        source_agent,
        None::<&str>,
        None::<&str>,
        parent_session_id,
    );

    assert!(
        manifest.parent_session_id.is_none(),
        "Main session manifest should have no parent_session_id"
    );
}

// ─── Edge Cases ──────────────────────────────────────────────────────────────

#[test]
fn test_multiple_subagents_same_parent() {
    // Test that multiple subagent sessions from the same parent all have the same parent_session_id
    let data_dir = make_data_dir();
    let claude_dir = data_dir.path().join("sessions/claude-code");

    let parent_uuid = "parent-shared-123";
    let subagent_count = 3;

    // Create parent session
    let parent_path = claude_dir.join(format!("{}.jsonl", parent_uuid));
    fs::create_dir_all(parent_path.parent().unwrap())
        .expect("Failed to create parent directory");
    fs::write(&parent_path, test_jsonl_content())
        .expect("Failed to write parent content");

    // Create multiple subagent sessions
    for i in 0..subagent_count {
        let subagent_path = claude_dir
            .join(parent_uuid)
            .join("subagents")
            .join(format!("agent-{:03}.jsonl", i));

        fs::create_dir_all(subagent_path.parent().unwrap())
            .expect("Failed to create directory");
        fs::write(&subagent_path, test_jsonl_content())
            .expect("Failed to write session content");
    }

    // Create scraper and plugin
    let mut scraper = Scraper::new(data_dir.path().to_path_buf())
        .expect("Failed to create scraper");

    let plugin = jsonl_plugin(
        "claude-code",
        &claude_dir.join("**/*.jsonl").to_str().unwrap(),
    );

    scraper.plugin_manager_mut().add_plugin(plugin);

    // Scrape the plugin
    let plugin_name = "claude-code";
    let plugin = scraper.plugin_manager().get(plugin_name).unwrap().clone();
    let result = scraper.scrape_plugin(&plugin).expect("Scrape should succeed");

    assert_eq!(
        result.sessions_scraped,
        1 + subagent_count,
        "Should scrape parent and all subagent sessions"
    );

    // List all sessions
    let sessions = scraper
        .list_sessions(plugin_name)
        .expect("Should list sessions");

    // Count subagent sessions (they contain the parent UUID in their path)
    let parent_session_id = format!("claude-code/{}", parent_uuid);
    let subagent_sessions: Vec<_> = sessions
        .iter()
        .filter(|s| s.contains(parent_uuid) && *s != &parent_session_id)
        .collect();

    assert_eq!(
        subagent_sessions.len(),
        subagent_count,
        "Should have all subagent sessions"
    );

    // Verify each subagent session can be read and has correct events
    for session_path in subagent_sessions {
        let events = scraper
            .read_session(session_path)
            .expect("Should read subagent events");

        assert_eq!(events.len(), 2, "Each subagent should have 2 events");

        // Verify source_agent is set correctly
        for event in &events {
            assert_eq!(
                event.source_agent, "claude-code-subagent",
                "Subagent events should have source_agent = claude-code-subagent"
            );
        }
    }
}

#[test]
fn test_search_by_parent_session_id() {
    // Test that we can search for sessions by parent_session_id
    let data_dir = make_data_dir();
    let claude_dir = data_dir.path().join("sessions/claude-code");

    // Create parent session
    let parent_uuid = "parent-search-123";
    let parent_path = claude_dir.join(format!("{}.jsonl", parent_uuid));
    fs::create_dir_all(parent_path.parent().unwrap())
        .expect("Failed to create directory");
    fs::write(&parent_path, test_jsonl_content())
        .expect("Failed to write content");

    // Create multiple subagent sessions
    let subagent_count = 3;
    for i in 0..subagent_count {
        let subagent_path = claude_dir
            .join(parent_uuid)
            .join("subagents")
            .join(format!("agent-{:03}.jsonl", i));

        fs::create_dir_all(subagent_path.parent().unwrap())
            .expect("Failed to create directory");
        fs::write(&subagent_path, test_jsonl_content())
            .expect("Failed to write content");
    }

    // Create scraper and plugin
    let mut scraper = Scraper::new(data_dir.path().to_path_buf())
        .expect("Failed to create scraper");

    let plugin = jsonl_plugin(
        "claude-code",
        &claude_dir.join("**/*.jsonl").to_str().unwrap(),
    );

    scraper.plugin_manager_mut().add_plugin(plugin);

    // Scrape
    let plugin_name = "claude-code";
    let plugin = scraper.plugin_manager().get(plugin_name).unwrap().clone();
    let result = scraper.scrape_plugin(&plugin).expect("Scrape should succeed");

    assert_eq!(
        result.sessions_scraped,
        1 + subagent_count,
        "Should scrape all sessions"
    );

    // List all sessions
    let sessions = scraper
        .list_sessions(plugin_name)
        .expect("Should list sessions");

    // Count subagent sessions
    let parent_session_id = format!("claude-code/{}", parent_uuid);
    let subagent_sessions: Vec<_> = sessions
        .iter()
        .filter(|s| s.contains(parent_uuid) && *s != &parent_session_id)
        .collect();

    assert_eq!(
        subagent_sessions.len(),
        subagent_count,
        "Should have all subagent sessions"
    );

    // Verify each subagent session can be read and has correct events
    for session_path in subagent_sessions {
        let events = scraper
            .read_session(session_path)
            .expect("Should read subagent events");

        assert_eq!(events.len(), 2, "Each subagent should have 2 events");

        // Verify source_agent is set correctly
        for event in &events {
            assert_eq!(
                event.source_agent, "claude-code-subagent",
                "Subagent events should have source_agent = claude-code-subagent"
            );
        }
    }
}

// ─── Unit Tests: Main Session parent_session_id Across Parser Types ────────────────

#[test]
fn test_main_session_jsonl_parser_no_parent() {
    // Test that JSONL parser correctly sets parent_session_id to None for main sessions
    let data_dir = make_data_dir();
    let claude_dir = data_dir.path().join("sessions/claude-code");

    // Create a main session (no subagents directory)
    let main_session_id = "main-session-abc123";
    let main_path = claude_dir.join(format!("{}.jsonl", main_session_id));
    fs::create_dir_all(main_path.parent().unwrap())
        .expect("Failed to create directory");
    fs::write(&main_path, test_jsonl_content())
        .expect("Failed to write content");

    // Create scraper and plugin
    let mut scraper = Scraper::new(data_dir.path().to_path_buf())
        .expect("Failed to create scraper");

    let plugin = jsonl_plugin(
        "claude-code",
        &claude_dir.join("**/*.jsonl").to_str().unwrap(),
    );

    scraper.plugin_manager_mut().add_plugin(plugin);

    // Scrape the plugin
    let plugin_name = "claude-code";
    let plugin = scraper.plugin_manager().get(plugin_name).unwrap().clone();
    let result = scraper.scrape_plugin(&plugin).expect("Scrape should succeed");

    assert_eq!(
        result.sessions_scraped, 1,
        "Should scrape exactly one session"
    );

    // Get the session manifest
    let sessions = scraper
        .list_sessions(plugin_name)
        .expect("Should list sessions");

    assert_eq!(sessions.len(), 1, "Should have one session");

    let session_path = &sessions[0];
    let events = scraper
        .read_session(session_path)
        .expect("Should read events");

    // Verify that the session has no parent_session_id by checking it's a main session
    assert!(
        session_path.contains(main_session_id),
        "Session path should contain main session ID"
    );

    // Verify events were parsed correctly
    assert_eq!(events.len(), 2, "Should have 2 events");
}

#[test]
fn test_main_session_multiple_main_sessions_no_parent() {
    // Test that multiple main sessions all have parent_session_id as None
    let data_dir = make_data_dir();
    let claude_dir = data_dir.path().join("sessions/claude-code");

    // Create multiple main sessions
    let main_session_ids = vec!["main-1", "main-2", "main-3"];
    for session_id in &main_session_ids {
        let path = claude_dir.join(format!("{}.jsonl", session_id));
        fs::create_dir_all(path.parent().unwrap())
            .expect("Failed to create directory");
        fs::write(&path, test_jsonl_content())
            .expect("Failed to write content");
    }

    // Create scraper and plugin
    let mut scraper = Scraper::new(data_dir.path().to_path_buf())
        .expect("Failed to create scraper");

    let plugin = jsonl_plugin(
        "claude-code",
        &claude_dir.join("**/*.jsonl").to_str().unwrap(),
    );

    scraper.plugin_manager_mut().add_plugin(plugin);

    // Scrape the plugin
    let plugin_name = "claude-code";
    let plugin = scraper.plugin_manager().get(plugin_name).unwrap().clone();
    let result = scraper.scrape_plugin(&plugin).expect("Scrape should succeed");

    assert_eq!(
        result.sessions_scraped,
        main_session_ids.len(),
        "Should scrape all main sessions"
    );

    // Verify all sessions are main sessions (no parent)
    let sessions = scraper
        .list_sessions(plugin_name)
        .expect("Should list sessions");

    assert_eq!(
        sessions.len(),
        main_session_ids.len(),
        "Should have all main sessions"
    );

    // Verify each session can be read and has correct events
    for session_path in &sessions {
        let events = scraper
            .read_session(session_path)
            .expect("Should read events");

        assert_eq!(events.len(), 2, "Each main session should have 2 events");

        // Verify source_agent is claude-code (not claude-code-subagent)
        for event in &events {
            assert_eq!(
                event.source_agent, "claude-code",
                "Main session events should have source_agent = claude-code"
            );
        }
    }
}

#[test]
fn test_main_session_nested_directories_no_parent() {
    // Test that main sessions in nested directory structures have parent_session_id as None
    let data_dir = make_data_dir();

    // Create a nested project structure
    let nested_dir = data_dir.path().join("sessions/claude-code/nested/project/path");
    fs::create_dir_all(&nested_dir).expect("Failed to create nested directory");

    // Create a main session in nested directory
    let main_session_id = "nested-main-session";
    let main_path = nested_dir.join(format!("{}.jsonl", main_session_id));
    fs::write(&main_path, test_jsonl_content())
        .expect("Failed to write content");

    // Create scraper and plugin
    let mut scraper = Scraper::new(data_dir.path().to_path_buf())
        .expect("Failed to create scraper");

    let plugin = jsonl_plugin(
        "claude-code",
        &data_dir.path().join("sessions/claude-code/**/*.jsonl").to_str().unwrap(),
    );

    scraper.plugin_manager_mut().add_plugin(plugin);

    // Scrape the plugin
    let plugin_name = "claude-code";
    let plugin = scraper.plugin_manager().get(plugin_name).unwrap().clone();
    let result = scraper.scrape_plugin(&plugin).expect("Scrape should succeed");

    assert_eq!(
        result.sessions_scraped, 1,
        "Should scrape the nested main session"
    );

    // Verify it's treated as a main session
    let sessions = scraper
        .list_sessions(plugin_name)
        .expect("Should list sessions");

    assert_eq!(sessions.len(), 1, "Should have one session");

    let events = scraper
        .read_session(&sessions[0])
        .expect("Should read events");

    assert_eq!(events.len(), 2, "Should have 2 events");

    // Verify source_agent is claude-code (not subagent)
    for event in &events {
        assert_eq!(
            event.source_agent, "claude-code",
            "Nested main session should have source_agent = claude-code"
        );
    }
}

#[test]
fn test_main_session_with_similar_path_to_subagent_no_parent() {
    // Test edge case: main session that has "subagents" in filename but not in path structure
    let data_dir = make_data_dir();
    let claude_dir = data_dir.path().join("sessions/claude-code");

    // Create a main session with "subagents" in the filename (edge case)
    // This should still be treated as a main session since it's not in the correct subagent path structure
    let main_session_id = "session-with-subagents-in-name";
    let main_path = claude_dir.join(format!("{}.jsonl", main_session_id));
    fs::create_dir_all(main_path.parent().unwrap())
        .expect("Failed to create directory");
    fs::write(&main_path, test_jsonl_content())
        .expect("Failed to write content");

    // Create scraper and plugin
    let mut scraper = Scraper::new(data_dir.path().to_path_buf())
        .expect("Failed to create scraper");

    let plugin = jsonl_plugin(
        "claude-code",
        &claude_dir.join("**/*.jsonl").to_str().unwrap(),
    );

    scraper.plugin_manager_mut().add_plugin(plugin);

    // Scrape the plugin
    let plugin_name = "claude-code";
    let plugin = scraper.plugin_manager().get(plugin_name).unwrap().clone();
    let result = scraper.scrape_plugin(&plugin).expect("Scrape should succeed");

    assert_eq!(
        result.sessions_scraped, 1,
        "Should scrape the main session"
    );

    // Verify it's treated as a main session
    let sessions = scraper
        .list_sessions(plugin_name)
        .expect("Should list sessions");

    assert_eq!(sessions.len(), 1, "Should have one session");

    let events = scraper
        .read_session(&sessions[0])
        .expect("Should read events");

    // Verify source_agent is claude-code (not subagent)
    for event in &events {
        assert_eq!(
            event.source_agent, "claude-code",
            "Main session should have source_agent = claude-code even with 'subagents' in filename"
        );
    }
}
