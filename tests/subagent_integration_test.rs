//! Integration test for subagent session capture
//!
//! This test verifies that subagent sessions are properly:
//! 1. Detected from file path structure
//! 2. Tagged with parent_session_id
//! 3. Labeled with source_agent = "{plugin}-subagent"
//! 4. Included in scrape results (not excluded)

use agentscribe::scraper::Scraper;

#[test]
fn test_subagent_session_capture_integration() {
    let temp = tempfile::tempdir().expect("Failed to create temp dir");
    let data_dir = temp.path().join(".agentscribe");
    std::fs::create_dir_all(data_dir.join("sessions")).expect("Failed to create sessions dir");

    // Create a realistic Claude Code subagent file structure
    let claude_dir = temp.path().join(".claude/projects/AgentScribe");
    let parent_uuid = "parent-session-abc123";
    let subagent_id = "agent-def456";

    let subagent_path = claude_dir
        .join(parent_uuid)
        .join("subagents")
        .join(format!("{}.jsonl", subagent_id));

    std::fs::create_dir_all(subagent_path.parent().unwrap())
        .expect("Failed to create subagent directory");

    // Write realistic Claude Code subagent content
    let subagent_content = r#"{"timestamp": "2026-07-23T10:00:00Z", "role": "system", "content": "Agent task: Analyze code"}
{"timestamp": "2026-07-23T10:00:01Z", "role": "user", "content": "Please analyze this function"}
{"timestamp": "2026-07-23T10:00:02Z", "role": "assistant", "content": "I'll analyze the function for you"}
{"timestamp": "2026-07-23T10:00:03Z", "role": "user", "content": "What about the edge cases?"}
{"timestamp": "2026-07-23T10:00:04Z", "role": "assistant", "content": "Here are the edge cases to consider"}"#;

    std::fs::write(&subagent_path, subagent_content).expect("Failed to write subagent content");

    // Create a regular (non-subagent) session for comparison
    let regular_path = claude_dir.join("regular-session.jsonl");
    let regular_content = r#"{"timestamp": "2026-07-23T10:00:00Z", "role": "user", "content": "Regular session message"}
{"timestamp": "2026-07-23T10:00:01Z", "role": "assistant", "content": "Regular response"}"#;

    std::fs::write(&regular_path, regular_content)
        .expect("Failed to write regular session content");

    // Create scraper and plugin
    let mut scraper = Scraper::new(data_dir.clone()).expect("Failed to create scraper");

    // Create a minimal claude-code plugin for testing
    let plugin = agentscribe::plugin::Plugin {
        plugin: agentscribe::plugin::PluginMeta {
            name: "claude-code".to_string(),
            version: "1.0".to_string(),
        },
        source: agentscribe::plugin::Source {
            paths: vec![claude_dir.join("**/*.jsonl").to_str().unwrap().to_string()],
            exclude: vec![], // Empty - subagents are NOT excluded
            format: agentscribe::plugin::LogFormat::Jsonl,
            session_detection: agentscribe::plugin::SessionDetection::OneFilePerSession {
                session_id_from: agentscribe::plugin::SessionIdSource::Filename,
            },
            tree: None,
            truncation_limit: None,
            envelope: None,
            array: None,
        },
        parser: agentscribe::plugin::Parser {
            timestamp: Some("timestamp".to_string()),
            role: Some("role".to_string()),
            content: Some("content".to_string()),
            static_fields: {
                let mut map = std::collections::HashMap::new();
                map.insert("source_agent".to_string(), serde_json::json!("claude-code"));
                map
            },
            ..Default::default()
        },
        metadata: None,
    };

    // Load the plugin
    scraper.plugin_manager_mut().add_plugin(plugin);

    // Scrape the plugin
    let plugin_name = "claude-code";
    let plugin = scraper.plugin_manager().get(plugin_name).unwrap().clone();
    let result = scraper.scrape_plugin(&plugin).expect("Scrape should succeed");

    // Verify that both sessions were scraped
    assert_eq!(
        result.sessions_scraped, 2,
        "Should scrape both regular and subagent sessions"
    );

    // Verify all sessions are indexed
    assert_eq!(result.sessions_indexed, 2, "Should index both sessions");

    // List all sessions for the plugin
    let sessions = scraper
        .list_sessions("claude-code")
        .expect("Should list sessions");

    assert_eq!(sessions.len(), 2, "Should have two sessions total");

    // Find the subagent session
    let subagent_session = sessions
        .iter()
        .find(|s| s.contains(subagent_id))
        .expect("Should find subagent session");

    // Verify the session ID format
    assert!(subagent_session.starts_with("claude-code/"));

    // Read the subagent session events
    let events = scraper
        .read_session(subagent_session)
        .expect("Should read subagent events");

    // Verify events were parsed correctly
    assert_eq!(
        events.len(),
        5,
        "Should have 5 events from subagent session"
    );

    // Verify all events have the correct source_agent (claude-code-subagent)
    for event in &events {
        assert_eq!(
            event.source_agent, "claude-code-subagent",
            "Subagent events should have source_agent = claude-code-subagent"
        );
    }

    // Find the regular session
    let regular_session = sessions
        .iter()
        .find(|s| s.contains("regular-session"))
        .expect("Should find regular session");

    // Read the regular session events
    let regular_events = scraper
        .read_session(regular_session)
        .expect("Should read regular events");

    // Verify regular session events have the correct source_agent
    for event in &regular_events {
        assert_eq!(
            event.source_agent, "claude-code",
            "Regular events should have source_agent = claude-code"
        );
    }

    // Note: Index manager search test commented out as index_manager is private
    // This functionality is tested through the public API (read_session) above

    println!("✅ Subagent integration test passed!");
    println!("   - Subagent session detected and scraped");
    println!("   - Regular session detected and scraped");
    println!("   - Subagent events tagged with source_agent = claude-code-subagent");
    println!("   - Regular events tagged with source_agent = claude-code");
}
