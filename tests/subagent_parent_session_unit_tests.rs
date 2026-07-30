//! Unit tests for subagent session parent_session_id.
//!
//! This test suite focuses specifically on verifying that subagent sessions
//! correctly inherit their parent's session ID. These tests are isolated unit tests
//! that mock subagent creation without actual spawning, verifying:
//! 1. parent_session_id matches the parent session's ID
//! 2. The parent_session_id field is correctly stored in manifests
//! 3. Edge cases and various session creation scenarios

use agentscribe::event::{Event, Role};
use agentscribe::index::build_manifest_from_events;
use chrono::Utc;

// ─── Test Helpers ─────────────────────────────────────────────────────────────

/// Create a minimal test event for testing.
fn create_test_event() -> Event {
    Event {
        ts: Utc::now(),
        session_id: "test-session".to_string(),
        source_agent: "claude-code-subagent".to_string(),
        source_version: None,
        project: None,
        role: Role::User,
        content: "Test message".to_string(),
        tool: None,
        tool_params: None,
        tokens: None,
        model: None,
        file_paths: vec![],
        error_fingerprints: vec![],
    }
}

/// Create multiple test events.
fn create_test_events(count: usize) -> Vec<Event> {
    (0..count)
        .map(|i| {
            let mut event = create_test_event();
            event.session_id = format!("session-{}", i);
            event
        })
        .collect()
}

/// Create test events with specific source_agent.
fn create_test_events_with_source(source_agent: &str, count: usize) -> Vec<Event> {
    (0..count)
        .map(|i| {
            let mut event = create_test_event();
            event.session_id = format!("session-{}", i);
            event.source_agent = source_agent.to_string();
            event
        })
        .collect()
}

// ─── Core Unit Tests: Subagent Session parent_session_id ─────────────────────

#[test]
fn test_subagent_session_with_parent_id() {
    // Test that subagent sessions correctly store parent_session_id
    let events = create_test_events(3);
    let session_id = "subagent-session-abc";
    let source_agent = "claude-code-subagent";
    let parent_session_id = Some("parent-session-123");

    let manifest = build_manifest_from_events(
        &events,
        session_id,
        source_agent,
        None::<&str>, // project
        None::<&str>, // model
        parent_session_id,
    );

    assert_eq!(
        manifest.parent_session_id,
        Some("parent-session-123".to_string()),
        "Subagent session should have parent_session_id set to parent's ID"
    );
    assert_eq!(manifest.session_id, session_id);
    assert_eq!(manifest.source_agent, source_agent);
    assert_eq!(manifest.turns, 3);
}

#[test]
fn test_subagent_empty_events_with_parent() {
    // Test that subagent sessions with empty events still have parent_session_id
    let events = vec![];
    let session_id = "subagent-empty";
    let source_agent = "claude-code-subagent";
    let parent_session_id = Some("parent-uuid-xyz");

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
        Some("parent-uuid-xyz".to_string()),
        "Empty subagent session should still have parent_session_id"
    );
    assert_eq!(manifest.turns, 0);
}

#[test]
fn test_subagent_single_event_with_parent() {
    // Test subagent session with exactly one event
    let events = vec![create_test_event()];
    let session_id = "subagent-single";
    let parent_session_id = Some("parent-single");

    let manifest = build_manifest_from_events(
        &events,
        session_id,
        "claude-code-subagent",
        None::<&str>,
        None::<&str>,
        parent_session_id,
    );

    assert_eq!(
        manifest.parent_session_id,
        Some("parent-single".to_string()),
        "Single-event subagent should have parent_session_id"
    );
    assert_eq!(manifest.turns, 1);
}

#[test]
fn test_subagent_many_events_with_parent() {
    // Test subagent session with many events (100 turns)
    let events = create_test_events(100);
    let session_id = "subagent-many";
    let parent_session_id = Some("parent-many");

    let manifest = build_manifest_from_events(
        &events,
        session_id,
        "claude-code-subagent",
        None::<&str>,
        None::<&str>,
        parent_session_id,
    );

    assert_eq!(
        manifest.parent_session_id,
        Some("parent-many".to_string()),
        "Many-event subagent should have parent_session_id"
    );
    assert_eq!(manifest.turns, 100);
}

#[test]
fn test_subagent_with_project_and_parent() {
    // Test that subagent sessions can have both project and parent_session_id
    let events = create_test_events(2);
    let session_id = "subagent-with-project";
    let source_agent = "claude-code-subagent";
    let project = Some("test-project");
    let parent_session_id = Some("parent-with-project");

    let manifest = build_manifest_from_events(
        &events,
        session_id,
        source_agent,
        project,
        None::<&str>, // model
        parent_session_id,
    );

    assert_eq!(
        manifest.parent_session_id,
        Some("parent-with-project".to_string()),
        "Subagent with project should have parent_session_id"
    );
    assert_eq!(manifest.project, Some("test-project".to_string()));
}

#[test]
fn test_subagent_with_model_and_parent() {
    // Test that subagent sessions can have both model and parent_session_id
    let events = create_test_events(2);
    let session_id = "subagent-with-model";
    let source_agent = "claude-code-subagent";
    let model = Some("claude-opus-5");
    let parent_session_id = Some("parent-with-model");

    let manifest = build_manifest_from_events(
        &events,
        session_id,
        source_agent,
        None::<&str>, // project
        model,
        parent_session_id,
    );

    assert_eq!(
        manifest.parent_session_id,
        Some("parent-with-model".to_string()),
        "Subagent with model should have parent_session_id"
    );
    assert_eq!(manifest.model, Some("claude-opus-5".to_string()));
}

#[test]
fn test_subagent_with_all_metadata_and_parent() {
    // Test subagent session with project, model, and parent_session_id
    let events = create_test_events(5);
    let session_id = "subagent-full";
    let source_agent = "claude-code-subagent";
    let project = Some("my-project");
    let model = Some("claude-sonnet-5");
    let parent_session_id = Some("parent-full-metadata");

    let manifest = build_manifest_from_events(
        &events,
        session_id,
        source_agent,
        project,
        model,
        parent_session_id,
    );

    assert_eq!(
        manifest.parent_session_id,
        Some("parent-full-metadata".to_string()),
        "Subagent with all metadata should have parent_session_id"
    );
    assert_eq!(manifest.project, Some("my-project".to_string()));
    assert_eq!(manifest.model, Some("claude-sonnet-5".to_string()));
    assert_eq!(manifest.turns, 5);
}

// ─── Tests: Different Source Agents ───────────────────────────────────────────

#[test]
fn test_subagent_various_source_agents_with_parent() {
    // Test that different subagent source_agents all correctly store parent_session_id
    let source_agents = vec![
        "claude-code-subagent",
        "aider-subagent",
        "cursor-subagent",
        "custom-tool-subagent",
    ];

    for source_agent in source_agents {
        let events = create_test_events(2);
        let session_id = "subagent-multi-agent";
        let parent_id = format!("parent-for-{}", source_agent);

        let manifest = build_manifest_from_events(
            &events,
            session_id,
            source_agent,
            None::<&str>,
            None::<&str>,
            Some(parent_id.as_str()),
        );

        assert_eq!(
            manifest.parent_session_id,
            Some(parent_id),
            "Subagent with source_agent '{}' should have parent_session_id",
            source_agent
        );
        assert_eq!(manifest.source_agent, source_agent);
    }
}

// ─── Tests: Parent Session ID Formats ──────────────────────────────────────────

#[test]
fn test_subagent_various_parent_id_formats() {
    // Test that various parent_session_id formats are correctly stored
    let parent_ids = vec![
        "simple-parent-id",
        "parent-with-123-numbers",
        "parent-with_special.chars",
        "uuid-like-parent-abc123def456",
        "very-long-parent-id-with-lots-of-characters",
        "parent-with_underscores",
        "parent-with.dots",
        "parent-with/slashes",
    ];

    for parent_id in parent_ids {
        let events = create_test_events(2);
        let session_id = "subagent-parent-test";

        let manifest = build_manifest_from_events(
            &events,
            session_id,
            "claude-code-subagent",
            None::<&str>,
            None::<&str>,
            Some(parent_id),
        );

        assert_eq!(
            manifest.parent_session_id,
            Some(parent_id.to_string()),
            "Subagent should correctly store parent_session_id: '{}'",
            parent_id
        );
    }
}

#[test]
fn test_subagent_uuid_parent_id() {
    // Test UUID-like parent_session_id (common in Claude Code)
    let uuid_parent = "a0b1c2d3-e4f5-6789-0abc-def123456789";
    let events = create_test_events(3);

    let manifest = build_manifest_from_events(
        &events,
        "subagent-uuid-parent",
        "claude-code-subagent",
        None::<&str>,
        None::<&str>,
        Some(uuid_parent),
    );

    assert_eq!(
        manifest.parent_session_id,
        Some(uuid_parent.to_string()),
        "UUID parent_session_id should be correctly stored"
    );
}

#[test]
fn test_subagent_short_parent_id() {
    // Test very short parent_session_id
    let short_parent = "p1";
    let events = create_test_events(2);

    let manifest = build_manifest_from_events(
        &events,
        "subagent-short-parent",
        "claude-code-subagent",
        None::<&str>,
        None::<&str>,
        Some(short_parent),
    );

    assert_eq!(
        manifest.parent_session_id,
        Some(short_parent.to_string()),
        "Short parent_session_id should be correctly stored"
    );
}

#[test]
fn test_subagent_long_parent_id() {
    // Test very long parent_session_id
    let long_parent = "parent-session-id-that-is-very-long-and-contains-lots-of-information-and-characters-beyond-the-norm";
    let events = create_test_events(2);

    let manifest = build_manifest_from_events(
        &events,
        "subagent-long-parent",
        "claude-code-subagent",
        None::<&str>,
        None::<&str>,
        Some(long_parent),
    );

    assert_eq!(
        manifest.parent_session_id,
        Some(long_parent.to_string()),
        "Long parent_session_id should be correctly stored"
    );
}

// ─── Tests: Edge Cases ───────────────────────────────────────────────────────

#[test]
fn test_subagent_empty_parent_id() {
    // Test edge case: empty string parent_session_id
    let events = create_test_events(2);
    let session_id = "subagent-empty-parent";

    let manifest = build_manifest_from_events(
        &events,
        session_id,
        "claude-code-subagent",
        None::<&str>,
        None::<&str>,
        Some(""), // Empty parent ID
    );

    assert_eq!(
        manifest.parent_session_id,
        Some("".to_string()),
        "Empty parent_session_id should be stored as empty string"
    );
}

#[test]
fn test_subagent_whitespace_parent_id() {
    // Test edge case: whitespace-only parent_session_id
    let whitespace_parent = "   ";
    let events = create_test_events(2);

    let manifest = build_manifest_from_events(
        &events,
        "subagent-whitespace-parent",
        "claude-code-subagent",
        None::<&str>,
        None::<&str>,
        Some(whitespace_parent),
    );

    assert_eq!(
        manifest.parent_session_id,
        Some(whitespace_parent.to_string()),
        "Whitespace parent_session_id should be stored as-is"
    );
}

#[test]
fn test_subagent_with_file_paths_and_parent() {
    // Test that subagent sessions with file_paths correctly store parent_session_id
    let mut event = create_test_event();
    event.file_paths = vec![
        "/path/to/file1.rs".to_string(),
        "/path/to/file2.rs".to_string(),
        "/path/to/file3.rs".to_string(),
    ];
    let events = vec![event];
    let session_id = "subagent-with-files";
    let parent_session_id = Some("parent-with-files");

    let manifest = build_manifest_from_events(
        &events,
        session_id,
        "claude-code-subagent",
        None::<&str>,
        None::<&str>,
        parent_session_id,
    );

    assert_eq!(
        manifest.parent_session_id,
        Some("parent-with-files".to_string()),
        "Subagent with file_paths should have parent_session_id"
    );
    assert_eq!(manifest.files_touched.len(), 3);
}

#[test]
fn test_subagent_consistency_across_multiple_calls() {
    // Test that calling build_manifest_from_events multiple times with same inputs
    // produces consistent parent_session_id results
    let events = create_test_events(3);
    let session_id = "consistency-subagent";
    let parent_session_id = Some("parent-consistency");

    let manifests: Vec<_> = (0..5)
        .map(|_| {
            build_manifest_from_events(
                &events,
                session_id,
                "claude-code-subagent",
                Some("test-project"),
                Some("test-model"),
                parent_session_id,
            )
        })
        .collect();

    // All manifests should have the same parent_session_id
    for (i, manifest) in manifests.iter().enumerate() {
        assert_eq!(
            manifest.parent_session_id,
            Some("parent-consistency".to_string()),
            "Manifest {} should have consistent parent_session_id",
            i
        );
    }

    // All manifests should be identical
    for manifest in manifests.iter().skip(1) {
        assert_eq!(manifest.parent_session_id, manifests[0].parent_session_id);
    }
}

#[test]
fn test_subagent_different_session_ids_with_same_parent() {
    // Test that different subagent sessions can have the same parent_session_id
    let parent_session_id = Some("shared-parent-123");
    let session_ids = vec![
        "subagent-1",
        "subagent-2",
        "subagent-3",
        "agent-a",
        "agent-b",
    ];

    for session_id in session_ids {
        let events = create_test_events(2);

        let manifest = build_manifest_from_events(
            &events,
            session_id,
            "claude-code-subagent",
            None::<&str>,
            None::<&str>,
            parent_session_id,
        );

        assert_eq!(
            manifest.parent_session_id,
            Some("shared-parent-123".to_string()),
            "Subagent '{}' should have correct parent_session_id",
            session_id
        );
        assert_eq!(manifest.session_id, session_id);
    }
}

#[test]
fn test_subagent_same_session_id_different_parents() {
    // Test edge case: same session_id with different parent_session_id
    // (This shouldn't happen in practice but tests the field is correctly set)
    let session_id = "subagent-same-id";
    let parent_ids = vec!["parent-1", "parent-2", "parent-3"];

    for parent_id in parent_ids {
        let events = create_test_events(2);

        let manifest = build_manifest_from_events(
            &events,
            session_id,
            "claude-code-subagent",
            None::<&str>,
            None::<&str>,
            Some(parent_id),
        );

        assert_eq!(
            manifest.parent_session_id,
            Some(parent_id.to_string()),
            "Should correctly store different parent_session_id"
        );
    }
}

// ─── Tests: Subagent vs Main Session Distinction ───────────────────────────────

#[test]
fn test_subagent_vs_main_session_parent_id() {
    // Test that subagent sessions have parent_session_id while main sessions don't
    let events = create_test_events(2);
    let session_id = "test-session";
    let source_agent_subagent = "claude-code-subagent";
    let source_agent_main = "claude-code";

    // Subagent manifest
    let subagent_manifest = build_manifest_from_events(
        &events,
        session_id,
        source_agent_subagent,
        None::<&str>,
        None::<&str>,
        Some("parent-123"),
    );

    // Main session manifest
    let main_manifest = build_manifest_from_events(
        &events,
        session_id,
        source_agent_main,
        None::<&str>,
        None::<&str>,
        None::<&str>,
    );

    assert_eq!(
        subagent_manifest.parent_session_id,
        Some("parent-123".to_string()),
        "Subagent should have parent_session_id"
    );

    assert!(
        main_manifest.parent_session_id.is_none(),
        "Main session should NOT have parent_session_id"
    );
}

#[test]
fn test_subagent_source_agent_suffix_implies_parent() {
    // Test that sessions with source_agent ending in "-subagent" typically have parent_session_id
    let subagent_source_agents = vec!["claude-code-subagent", "aider-subagent", "cursor-subagent"];

    for source_agent in subagent_source_agents {
        let events = create_test_events(2);
        let parent_id = format!("parent-for-{}", source_agent);

        let manifest = build_manifest_from_events(
            &events,
            "subagent-test",
            source_agent,
            None::<&str>,
            None::<&str>,
            Some(parent_id.as_str()),
        );

        assert_eq!(
            manifest.parent_session_id,
            Some(parent_id),
            "Source agent '{}' should have parent_session_id",
            source_agent
        );
        assert_eq!(manifest.source_agent, source_agent);
    }
}

#[test]
fn test_subagent_with_various_project_values_with_parent() {
    // Test that different project values all result in correct parent_session_id
    let projects = vec![
        Some("simple-project"),
        Some("project-with-numbers-123"),
        Some("nested/project/path"),
        Some("project_with_underscores"),
        Some("project-with.dots"),
        None, // No project
    ];

    for project in projects {
        let events = create_test_events(2);

        let manifest = build_manifest_from_events(
            &events,
            "subagent-project-test",
            "claude-code-subagent",
            project,
            None::<&str>,
            Some("parent-with-various-projects"),
        );

        assert_eq!(
            manifest.parent_session_id,
            Some("parent-with-various-projects".to_string()),
            "Subagent with project {:?} should have parent_session_id",
            project
        );
        assert_eq!(manifest.project, project.map(|s| s.to_string()));
    }
}

#[test]
fn test_subagent_with_various_model_values_with_parent() {
    // Test that different model values all result in correct parent_session_id
    let models = vec![
        Some("claude-sonnet-5"),
        Some("claude-opus-5"),
        Some("claude-haiku-4-5"),
        Some("gpt-4"),
        Some("custom-model-name"),
        None, // No model
    ];

    for model in models {
        let events = create_test_events(2);
        let parent_session_id = Some("parent-with-various-models");

        let manifest = build_manifest_from_events(
            &events,
            "subagent-model-test",
            "claude-code-subagent",
            None::<&str>,
            model,
            parent_session_id,
        );

        assert_eq!(
            manifest.parent_session_id,
            Some("parent-with-various-models".to_string()),
            "Subagent with model {:?} should have parent_session_id",
            model
        );
        assert_eq!(manifest.model, model.map(|s| s.to_string()));
    }
}
