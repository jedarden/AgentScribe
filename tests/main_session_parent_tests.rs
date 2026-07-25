//! Unit tests for main session parent_session_id behavior.
//!
//! This test suite specifically verifies that main sessions have parent_session_id
//! set to None/empty across various creation scenarios.
//!
//! These tests are focused and isolated (unit tests), testing the manifest creation
//! logic directly without full integration scraping flows.

use agentscribe::event::{Event, Role};
use agentscribe::index::build_manifest_from_events;
use chrono::Utc;

// ─── Test Helpers ─────────────────────────────────────────────────────────────

/// Create a minimal test event for testing.
fn create_test_event() -> Event {
    Event {
        ts: Utc::now(),
        session_id: "test-session".to_string(),
        source_agent: "claude-code".to_string(),
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
    (0..count).map(|_| create_test_event()).collect()
}

// ─── Core Unit Tests: Main Session parent_session_id ─────────────────────────

#[test]
fn test_main_session_empty_events_no_parent() {
    // Test that main sessions with empty events have parent_session_id = None
    let events = vec![];
    let session_id = "main-session-empty";
    let source_agent = "claude-code";

    let manifest = build_manifest_from_events(
        &events,
        session_id,
        source_agent,
        None::<&str>,  // project
        None::<&str>,  // model
        None::<&str>,  // parent_session_id - None for main sessions
    );

    assert!(
        manifest.parent_session_id.is_none(),
        "Main session should have parent_session_id = None"
    );
    assert_eq!(manifest.session_id, session_id);
    assert_eq!(manifest.source_agent, source_agent);
}

#[test]
fn test_main_session_with_events_no_parent() {
    // Test that main sessions with events have parent_session_id = None
    let events = create_test_events(3);
    let session_id = "main-session-with-events";
    let source_agent = "claude-code";

    let manifest = build_manifest_from_events(
        &events,
        session_id,
        source_agent,
        None::<&str>,
        None::<&str>,
        None::<&str>,
    );

    assert!(
        manifest.parent_session_id.is_none(),
        "Main session with events should have parent_session_id = None"
    );
    assert_eq!(manifest.turns, 3);
}

#[test]
fn test_main_session_with_project_no_parent() {
    // Test that main sessions with project specified have parent_session_id = None
    let events = create_test_events(2);
    let session_id = "main-session-with-project";
    let source_agent = "claude-code";
    let project = Some("test-project");

    let manifest = build_manifest_from_events(
        &events,
        session_id,
        source_agent,
        project,
        None::<&str>,
        None::<&str>,
    );

    assert!(
        manifest.parent_session_id.is_none(),
        "Main session with project should have parent_session_id = None"
    );
    assert_eq!(manifest.project, Some("test-project".to_string()));
}

#[test]
fn test_main_session_with_model_no_parent() {
    // Test that main sessions with model specified have parent_session_id = None
    let events = create_test_events(2);
    let session_id = "main-session-with-model";
    let source_agent = "claude-code";
    let model = Some("claude-sonnet-5");

    let manifest = build_manifest_from_events(
        &events,
        session_id,
        source_agent,
        None::<&str>,
        model,
        None::<&str>,
    );

    assert!(
        manifest.parent_session_id.is_none(),
        "Main session with model should have parent_session_id = None"
    );
    assert_eq!(manifest.model, Some("claude-sonnet-5".to_string()));
}

#[test]
fn test_main_session_with_project_and_model_no_parent() {
    // Test that main sessions with both project and model have parent_session_id = None
    let events = create_test_events(5);
    let session_id = "main-session-full-metadata";
    let source_agent = "claude-code";
    let project = Some("my-project");
    let model = Some("claude-opus-5");

    let manifest = build_manifest_from_events(
        &events,
        session_id,
        source_agent,
        project,
        model,
        None::<&str>,
    );

    assert!(
        manifest.parent_session_id.is_none(),
        "Main session with all metadata should have parent_session_id = None"
    );
    assert_eq!(manifest.project, Some("my-project".to_string()));
    assert_eq!(manifest.model, Some("claude-opus-5".to_string()));
    assert_eq!(manifest.turns, 5);
}

#[test]
fn test_main_session_different_source_agents_no_parent() {
    // Test that main sessions with different source_agent values all have parent_session_id = None
    let source_agents = vec![
        "claude-code",
        "aider",
        "codex",
        "opencode",
        "cursor",
    ];

    for source_agent in source_agents {
        let events = create_test_events(2);
        let session_id = "main-session-multi-agent";

        let manifest = build_manifest_from_events(
            &events,
            session_id,
            source_agent,
            None::<&str>,
            None::<&str>,
            None::<&str>,
        );

        assert!(
            manifest.parent_session_id.is_none(),
            "Main session with source_agent '{}' should have parent_session_id = None",
            source_agent
        );
        assert_eq!(manifest.source_agent, source_agent);
    }
}

#[test]
fn test_main_session_single_event_no_parent() {
    // Test edge case: main session with exactly one event
    let events = vec![create_test_event()];
    let session_id = "main-session-single-event";

    let manifest = build_manifest_from_events(
        &events,
        session_id,
        "claude-code",
        None::<&str>,
        None::<&str>,
        None::<&str>,
    );

    assert!(
        manifest.parent_session_id.is_none(),
        "Main session with single event should have parent_session_id = None"
    );
    assert_eq!(manifest.turns, 1);
}

#[test]
fn test_main_session_many_events_no_parent() {
    // Test edge case: main session with many events (100 turns)
    let events = create_test_events(100);
    let session_id = "main-session-many-events";

    let manifest = build_manifest_from_events(
        &events,
        session_id,
        "claude-code",
        None::<&str>,
        None::<&str>,
        None::<&str>,
    );

    assert!(
        manifest.parent_session_id.is_none(),
        "Main session with many events should have parent_session_id = None"
    );
    assert_eq!(manifest.turns, 100);
}

#[test]
fn test_main_session_with_file_paths_no_parent() {
    // Test that main sessions with file_paths have parent_session_id = None
    let mut event = create_test_event();
    event.file_paths = vec![
        "/path/to/file1.rs".to_string(),
        "/path/to/file2.rs".to_string(),
    ];
    let events = vec![event];
    let session_id = "main-session-with-files";

    let manifest = build_manifest_from_events(
        &events,
        session_id,
        "claude-code",
        None::<&str>,
        None::<&str>,
        None::<&str>,
    );

    assert!(
        manifest.parent_session_id.is_none(),
        "Main session with file_paths should have parent_session_id = None"
    );
    assert_eq!(manifest.files_touched.len(), 2);
}

#[test]
fn test_main_session_explicit_none_vs_no_parameter() {
    // Test that explicit None and no parameter behave the same for parent_session_id
    let events = create_test_events(2);
    let session_id = "main-session-none-test";

    // Test with explicit None
    let manifest1 = build_manifest_from_events(
        &events,
        session_id,
        "claude-code",
        None::<&str>,
        None::<&str>,
        None::<&str>,  // Explicit None
    );

    // Test with implicit None (using Option::None directly)
    let manifest2 = build_manifest_from_events(
        &events,
        session_id,
        "claude-code",
        None::<&str>,
        None::<&str>,
        None,  // Implicit None (type inference)
    );

    assert!(
        manifest1.parent_session_id.is_none(),
        "Explicit None should result in parent_session_id = None"
    );
    assert!(
        manifest2.parent_session_id.is_none(),
        "Implicit None should result in parent_session_id = None"
    );
}

#[test]
fn test_main_session_different_session_ids_no_parent() {
    // Test that various session_id formats all result in parent_session_id = None
    let session_ids = vec![
        "simple-session-id",
        "session-with-123-numbers",
        "session-with_special.chars",
        "uuid-like-session-abc123def456",
        "very-long-session-id-with-lots-of-characters",
    ];

    for session_id in session_ids {
        let events = create_test_events(2);

        let manifest = build_manifest_from_events(
            &events,
            session_id,
            "claude-code",
            None::<&str>,
            None::<&str>,
            None::<&str>,
        );

        assert!(
            manifest.parent_session_id.is_none(),
            "Main session with session_id '{}' should have parent_session_id = None",
            session_id
        );
        assert_eq!(manifest.session_id, session_id);
    }
}

#[test]
fn test_main_session_empty_session_id_no_parent() {
    // Test edge case: empty session_id string
    let events = create_test_events(2);
    let session_id = "";

    let manifest = build_manifest_from_events(
        &events,
        session_id,
        "claude-code",
        None::<&str>,
        None::<&str>,
        None::<&str>,
    );

    assert!(
        manifest.parent_session_id.is_none(),
        "Main session with empty session_id should have parent_session_id = None"
    );
    assert_eq!(manifest.session_id, "");
}

#[test]
fn test_main_session_whitespace_session_id_no_parent() {
    // Test edge case: whitespace-only session_id
    let events = create_test_events(2);
    let session_id = "   ";

    let manifest = build_manifest_from_events(
        &events,
        session_id,
        "claude-code",
        None::<&str>,
        None::<&str>,
        None::<&str>,
    );

    assert!(
        manifest.parent_session_id.is_none(),
        "Main session with whitespace session_id should have parent_session_id = None"
    );
    assert_eq!(manifest.session_id, "   ");
}

#[test]
fn test_main_session_consistency_across_multiple_calls() {
    // Test that calling build_manifest_from_events multiple times with same inputs
    // produces consistent parent_session_id = None results
    let events = create_test_events(3);
    let session_id = "consistency-test";

    let manifests: Vec<_> = (0..5)
        .map(|_| {
            build_manifest_from_events(
                &events,
                session_id,
                "claude-code",
                Some("test-project"),
                Some("test-model"),
                None::<&str>,
            )
        })
        .collect();

    // All manifests should have parent_session_id = None
    for (i, manifest) in manifests.iter().enumerate() {
        assert!(
            manifest.parent_session_id.is_none(),
            "Manifest {} should have parent_session_id = None",
            i
        );
    }

    // All manifests should be identical
    for manifest in manifests.iter().skip(1) {
        assert_eq!(manifest.parent_session_id, manifests[0].parent_session_id);
    }
}

#[test]
fn test_main_session_various_project_values_no_parent() {
    // Test that different project values all result in parent_session_id = None
    let projects = vec![
        Some("simple-project"),
        Some("project-with-numbers-123"),
        Some("nested/project/path"),
        Some("project_with_underscores"),
        Some("project-with.dots"),
        None,  // No project
    ];

    for project in projects {
        let events = create_test_events(2);

        let manifest = build_manifest_from_events(
            &events,
            "main-session",
            "claude-code",
            project,
            None::<&str>,
            None::<&str>,
        );

        assert!(
            manifest.parent_session_id.is_none(),
            "Main session with project {:?} should have parent_session_id = None",
            project
        );
    }
}

#[test]
fn test_main_session_various_model_values_no_parent() {
    // Test that different model values all result in parent_session_id = None
    let models = vec![
        Some("claude-sonnet-5"),
        Some("claude-opus-5"),
        Some("claude-haiku-4-5"),
        Some("gpt-4"),
        Some("custom-model-name"),
        None,  // No model
    ];

    for model in models {
        let events = create_test_events(2);

        let manifest = build_manifest_from_events(
            &events,
            "main-session",
            "claude-code",
            None::<&str>,
            model,
            None::<&str>,
        );

        assert!(
            manifest.parent_session_id.is_none(),
            "Main session with model {:?} should have parent_session_id = None",
            model
        );
    }
}
