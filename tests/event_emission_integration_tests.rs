//! Integration tests demonstrating event emission testing infrastructure usage
//!
//! This test module provides comprehensive examples of using the event emission
//! test helpers to validate real-world parsing scenarios, envelope routing,
//! and event stream state management.

mod event_emission_test_helpers;

use agentscribe::event::{Event, Role};
use chrono::Utc;
use event_emission_test_helpers::*;
use std::collections::HashMap;

#[test]
fn test_mock_emitter_basic_conversation() {
    // Test basic conversation emission
    let mut emitter =
        MockEventEmitter::new("test-session/123".to_string(), "test-agent".to_string());

    emitter.emit_user_event("How do I fix this error?");
    emitter.emit_assistant_event("Here's the solution");

    assert_eq!(emitter.event_count(), 2);
    assert!(!emitter.is_empty());

    let events = emitter.events();
    assert_eq!(events[0].role, Role::User);
    assert_eq!(events[1].role, Role::Assistant);
}

#[test]
fn test_mock_emitter_tool_sequence() {
    // Test tool call/result pairing
    let mut emitter =
        MockEventEmitter::new("tool-session/456".to_string(), "claude-code".to_string());

    emitter.emit_user_event("Edit the file");
    emitter.emit_tool_call_event("Edit", "Editing src/main.rs");
    emitter.emit_tool_result_event("Edit", "Exit code 0");

    assert_eq!(emitter.event_count(), 3);

    let tool_events = emitter.events_by_tool("Edit");
    assert_eq!(tool_events.len(), 2);

    // Verify tool call/result pairing
    EventEmissionVerifier::verify_tool_call_result_pairing(emitter.events()).unwrap();
}

#[test]
fn test_mock_emitter_timestamp_increments() {
    // Test automatic timestamp management
    let start_time = Utc::now();
    let mut emitter =
        MockEventEmitter::new("timestamp-test/789".to_string(), "test-agent".to_string())
            .with_start_time(start_time)
            .with_timestamp_increment(1000); // 1 second increments

    emitter.emit_user_event("First message");
    emitter.emit_assistant_event("Response");

    let events = emitter.events();
    assert_eq!(events[0].ts, start_time);
    assert_eq!(events[1].ts, start_time + chrono::Duration::seconds(1));

    EventEmissionVerifier::verify_unique_timestamps(events).unwrap();
}

#[test]
fn test_stream_tracker_completeness() {
    // Test event stream tracking with expectations
    let mut tracker = EventStreamTracker::new()
        .with_expected_count(2)
        .with_expected_role_sequence(vec![Role::User, Role::Assistant]);

    let events = fixtures::simple_conversation("test-session");
    for event in events {
        tracker.track(event);
    }

    assert_eq!(tracker.count(), 2);
    assert!(tracker.is_complete());
    assert!(tracker.verify_role_sequence().is_ok());
}

#[test]
fn test_stream_tracker_consumption() {
    // Test event stream consumption patterns
    let mut tracker = EventStreamTracker::new();

    let events = fixtures::tool_use_conversation("test-session");
    for event in events {
        tracker.track(event);
    }

    // Peek at first event
    let first = tracker.peek().unwrap();
    assert_eq!(first.role, Role::User);

    // Consume events
    let _event_count = 0;
    while tracker.consume_next().is_some() {
        // Process events
    }

    assert!(tracker.is_empty());
}

#[test]
fn test_skip_routing_fixture_basic() {
    // Test envelope routing fixture for Codex format
    let fixture = SkipRoutingFixture::new("codex-envelope".to_string())
        .with_routing("session_meta", RoutingAction::Meta)
        .with_routing("response_item", RoutingAction::Emit)
        .with_routing("turn_context", RoutingAction::Meta)
        .with_routing("event_msg", RoutingAction::Skip);

    // Verify routing expectations
    assert!(fixture
        .assert_routing("session_meta", RoutingAction::Meta)
        .is_ok());
    assert!(fixture
        .assert_routing("response_item", RoutingAction::Emit)
        .is_ok());
    assert!(fixture
        .assert_routing("turn_context", RoutingAction::Meta)
        .is_ok());
    assert!(fixture
        .assert_routing("event_msg", RoutingAction::Skip)
        .is_ok());
}

#[test]
fn test_skip_routing_fixture_errors() {
    // Test routing assertion errors
    let fixture = SkipRoutingFixture::new("test-envelope".to_string())
        .with_routing("message", RoutingAction::Emit)
        .with_routing("heartbeat", RoutingAction::Skip);

    // This should fail - wrong action
    let result = fixture.assert_routing("message", RoutingAction::Skip);
    assert!(result.is_err());

    // This should fail - no routing defined
    let result = fixture.assert_routing("unknown_type", RoutingAction::Emit);
    assert!(result.is_err());
}

#[test]
fn test_emission_verifier_order() {
    // Test event order verification
    let events = fixtures::multi_turn_conversation("test-session");

    let expected_roles = vec![
        Role::User,
        Role::Assistant,
        Role::ToolCall,
        Role::ToolResult,
        Role::User,
    ];

    assert!(EventEmissionVerifier::verify_event_order(&events, &expected_roles).is_ok());
}

#[test]
fn test_emission_verifier_role_counts() {
    // Test role count verification
    let events = fixtures::tool_use_conversation("test-session");

    let mut expected_counts = HashMap::new();
    expected_counts.insert(Role::User, 1);
    expected_counts.insert(Role::ToolCall, 1);
    expected_counts.insert(Role::ToolResult, 1);

    assert!(EventEmissionVerifier::verify_role_counts(&events, &expected_counts).is_ok());
}

#[test]
fn test_emission_verifier_session_consistency() {
    // Test session ID consistency
    let events = fixtures::simple_conversation("test-session");

    assert!(EventEmissionVerifier::verify_single_session(&events, "test-session").is_ok());
}

#[test]
fn test_emission_verifier_tool_pairing() {
    // Test tool call/result pairing verification
    let events = fixtures::tool_use_conversation("test-session");

    assert!(EventEmissionVerifier::verify_tool_call_result_pairing(&events).is_ok());
}

#[test]
fn test_emission_verifier_unpaired_tool_calls() {
    // Test detection of unpaired tool calls
    let base_time = Utc::now();
    let events_with_unpaired = vec![
        Event {
            ts: base_time,
            session_id: "test-session".to_string(),
            source_agent: "test-agent".to_string(),
            source_version: None,
            project: None,
            role: Role::ToolCall,
            content: "Unpaired call".to_string(),
            tool: Some("Edit".to_string()),
            tool_params: None,
            tokens: None,
            model: None,
            file_paths: Vec::new(),
            error_fingerprints: Vec::new(),
        },
        // Missing tool result
    ];

    let result = EventEmissionVerifier::verify_tool_call_result_pairing(&events_with_unpaired);
    assert!(result.is_err());
}

#[test]
fn test_fixture_simple_conversation() {
    // Test simple conversation fixture
    let events = fixtures::simple_conversation("test-session");

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].role, Role::User);
    assert_eq!(events[1].role, Role::Assistant);
    assert_eq!(events[0].content, "How do I fix this error?");
}

#[test]
fn test_fixture_tool_use_conversation() {
    // Test tool use conversation fixture
    let events = fixtures::tool_use_conversation("test-session");

    assert_eq!(events.len(), 3);
    assert_eq!(events[1].role, Role::ToolCall);
    assert_eq!(events[2].role, Role::ToolResult);
    assert_eq!(events[1].tool, Some("Edit".to_string()));
    assert_eq!(events[2].tool, Some("Edit".to_string()));
}

#[test]
fn test_fixture_multi_turn_conversation() {
    // Test multi-turn conversation fixture
    let events = fixtures::multi_turn_conversation("test-session");

    assert_eq!(events.len(), 5);

    // Verify conversation flow
    assert_eq!(events[0].role, Role::User);
    assert_eq!(events[1].role, Role::Assistant);
    assert_eq!(events[2].role, Role::ToolCall);
    assert_eq!(events[3].role, Role::ToolResult);
    assert_eq!(events[4].role, Role::User);
}

#[test]
fn test_complex_parsing_scenario() {
    // Test complex scenario with multiple tool uses
    let mut emitter =
        MockEventEmitter::new("complex-session/abc".to_string(), "claude-code".to_string());

    // Simulate complex conversation
    emitter.emit_user_event("Fix the authentication bug");
    emitter.emit_assistant_event("I'll investigate the issue");
    emitter.emit_tool_call_event("Read", "Reading src/auth.rs");
    emitter.emit_tool_result_event("Read", "File content shown");
    emitter.emit_tool_call_event("Bash", "Running cargo test");
    emitter.emit_tool_result_event("Bash", "Test output");
    emitter.emit_tool_call_event("Edit", "Fixing auth.rs");
    emitter.emit_tool_result_event("Edit", "Exit code 0");
    emitter.emit_user_event("Thanks! That works.");

    let events = emitter.events();
    assert_eq!(events.len(), 9);

    // Verify all tool calls are paired
    EventEmissionVerifier::verify_tool_call_result_pairing(events).unwrap();

    // Verify role counts
    let mut expected_counts = HashMap::new();
    expected_counts.insert(Role::User, 2);
    expected_counts.insert(Role::Assistant, 1);
    expected_counts.insert(Role::ToolCall, 3);
    expected_counts.insert(Role::ToolResult, 3);

    EventEmissionVerifier::verify_role_counts(events, &expected_counts).unwrap();
}

#[test]
fn test_error_scenario_emission() {
    // Test scenario with errors and retries
    let mut emitter = MockEventEmitter::new("error-session/xyz".to_string(), "aider".to_string());

    emitter.emit_user_event("Deploy the application");
    emitter.emit_assistant_event("Running deployment");
    emitter.emit_tool_call_event("Bash", "Deploying to production");
    emitter.emit_tool_result_event("Bash", "Error: Connection failed");

    let events = emitter.events();
    assert_eq!(events.len(), 4);

    // Verify error is in tool result
    let last_event = &events[3];
    assert_eq!(last_event.role, Role::ToolResult);
    assert!(last_event.content.contains("Error"));
}

#[test]
fn test_multi_agent_scenario() {
    // Test scenario with sessions from different agents
    let agents = vec!["claude-code", "aider", "opencode"];

    for agent in agents {
        let mut emitter =
            MockEventEmitter::new(format!("{}/session-123", agent), agent.to_string());

        emitter.emit_user_event("Test message");
        emitter.emit_assistant_event("Test response");

        assert_eq!(emitter.event_count(), 2);
        assert_eq!(emitter.events()[0].source_agent, agent);
    }
}

#[test]
fn test_timestamp_uniqueness() {
    // Test timestamp uniqueness across many events
    let mut emitter =
        MockEventEmitter::new("timestamp-test/999".to_string(), "test-agent".to_string())
            .with_timestamp_increment(1); // 1ms increments

    // Emit 100 events
    for i in 0..100 {
        emitter.emit_user_event(&format!("Message {}", i));
    }

    assert_eq!(emitter.event_count(), 100);
    EventEmissionVerifier::verify_unique_timestamps(emitter.events()).unwrap();
}

#[test]
fn test_empty_session_handling() {
    // Test handling of empty sessions
    let emitter = MockEventEmitter::new("empty-session/000".to_string(), "test-agent".to_string());

    assert!(emitter.is_empty());
    assert_eq!(emitter.event_count(), 0);
}

#[test]
fn test_stream_tracker_with_incomplete_sequence() {
    // Test tracker with incomplete event sequence
    let mut tracker = EventStreamTracker::new()
        .with_expected_count(3)
        .with_expected_role_sequence(vec![Role::User, Role::Assistant, Role::User]);

    // Only track 2 events
    let events = fixtures::simple_conversation("test-session");
    for event in events {
        tracker.track(event);
    }

    assert_eq!(tracker.count(), 2);
    assert!(!tracker.is_complete());

    // Role sequence verification should fail
    let result = tracker.verify_role_sequence();
    assert!(result.is_err());
}

#[test]
fn test_custom_event_emission() {
    // Test custom event emission with full control
    let mut emitter =
        MockEventEmitter::new("custom-session/111".to_string(), "test-agent".to_string());

    let custom_event = Event {
        ts: Utc::now(),
        session_id: "custom-session/111".to_string(),
        source_agent: "test-agent".to_string(),
        source_version: Some("1.0.0".to_string()),
        project: Some("/home/user/project".to_string()),
        role: Role::System,
        content: "System initialization".to_string(),
        tool: None,
        tool_params: None,
        tokens: None,
        model: Some("gpt-4".to_string()),
        file_paths: vec!["src/main.rs".to_string()],
        error_fingerprints: vec!["ErrorPattern".to_string()],
    };

    emitter.emit_custom_event(custom_event.clone());

    assert_eq!(emitter.event_count(), 1);
    assert_eq!(emitter.events()[0].role, Role::System);
}

#[test]
fn test_role_filtering() {
    // Test filtering events by role
    let mut emitter =
        MockEventEmitter::new("filter-test/222".to_string(), "test-agent".to_string());

    // Emit various role types
    emitter.emit_user_event("User message");
    emitter.emit_assistant_event("Assistant message");
    emitter.emit_system_event("System message");
    emitter.emit_tool_call_event("Read", "Reading file");
    emitter.emit_tool_result_event("Read", "File content");

    // Filter by roles
    let user_events = emitter.events_by_role(Role::User);
    let assistant_events = emitter.events_by_role(Role::Assistant);
    let tool_events = emitter.events_by_role(Role::ToolCall);

    assert_eq!(user_events.len(), 1);
    assert_eq!(assistant_events.len(), 1);
    assert_eq!(tool_events.len(), 1);
}

#[test]
fn test_tool_filtering() {
    // Test filtering events by tool name
    let mut emitter =
        MockEventEmitter::new("tool-filter/333".to_string(), "test-agent".to_string());

    // Emit multiple tool uses
    emitter.emit_tool_call_event("Read", "Reading file A");
    emitter.emit_tool_result_event("Read", "Content A");
    emitter.emit_tool_call_event("Read", "Reading file B");
    emitter.emit_tool_result_event("Read", "Content B");
    emitter.emit_tool_call_event("Edit", "Editing file");
    emitter.emit_tool_result_event("Edit", "Exit code 0");

    // Filter by tool name
    let read_events = emitter.events_by_tool("Read");
    let edit_events = emitter.events_by_tool("Edit");

    assert_eq!(read_events.len(), 4); // 2 calls + 2 results
    assert_eq!(edit_events.len(), 2); // 1 call + 1 result
}

#[test]
fn test_session_id_consistency() {
    // Test session ID consistency across multiple events
    let session_id = "consistency-test/444";
    let mut emitter = MockEventEmitter::new(session_id.to_string(), "test-agent".to_string());

    emitter.emit_user_event("Message 1");
    emitter.emit_assistant_event("Response 1");
    emitter.emit_user_event("Message 2");

    let events = emitter.events();
    for event in events {
        assert_eq!(event.session_id, session_id);
    }

    EventEmissionVerifier::verify_single_session(events, session_id).unwrap();
}
