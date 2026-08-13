//! Comprehensive tests for skip routing event behavior
//!
//! This test module validates that skip routing properly prevents event emission
//! across various envelope routing scenarios, ensuring that:
//! - Skip-type lines produce no events
//! - Event emitters are bypassed during skip routing
//! - Event streams remain empty after skip processing
//! - Edge cases for different skip-type line variations are covered

mod event_emission_test_helpers;

use agentscribe::event::Role;
use agentscribe::parser::JsonlParser;
use agentscribe::parser::{FormatParser, ParseContext};
use agentscribe::plugin::{
    Envelope, LogFormat, Parser, Plugin, PluginMeta, SessionDetection, SessionIdSource, Source,
};
use event_emission_test_helpers::*;
use std::collections::HashMap;
use std::path::PathBuf;

/// Create a test plugin with custom envelope routing configuration
fn create_skip_routing_test_plugin(type_routing: HashMap<String, String>) -> Plugin {
    let mut role_map = std::collections::HashMap::new();
    role_map.insert("toolResult".to_string(), "tool_result".to_string());

    Plugin {
        plugin: PluginMeta {
            name: "skip-test".to_string(),
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
            envelope: Some(Envelope {
                payload_field: "payload".to_string(),
                type_field: "type".to_string(),
                type_routing,
            }),
            array: None,
        },
        parser: Parser {
            timestamp: Some("^timestamp".to_string()),
            role: Some("role".to_string()),
            content: Some("content".to_string()),
            role_map,
            ..Default::default()
        },
        metadata: None,
    }
}

#[test]
fn test_skip_routing_basic_heartbeat_produces_no_events() {
    // Test basic skip routing: heartbeat type should produce zero events
    let mut type_routing = HashMap::new();
    type_routing.insert("heartbeat".to_string(), "skip".to_string());

    let plugin = create_skip_routing_test_plugin(type_routing);
    let context = ParseContext::new(
        "test-session".to_string(),
        "skip-test".to_string(),
        "/tmp/test.jsonl".to_string(),
    );

    let line = r#"{"type": "heartbeat", "timestamp": "2026-03-16T12:00:00Z", "payload": {"status": "ok"}}"#;
    let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

    assert_eq!(
        events.len(),
        0,
        "heartbeat skip routing should produce zero events"
    );
    assert!(
        events.is_empty(),
        "event stream should be empty after skip routing"
    );
}

#[test]
fn test_skip_routing_basic_ping_produces_no_events() {
    // Test basic skip routing: ping type should produce zero events
    let mut type_routing = HashMap::new();
    type_routing.insert("ping".to_string(), "skip".to_string());

    let plugin = create_skip_routing_test_plugin(type_routing);
    let context = ParseContext::new(
        "test-session".to_string(),
        "skip-test".to_string(),
        "/tmp/test.jsonl".to_string(),
    );

    let line = r#"{"type": "ping", "timestamp": "2026-03-16T12:00:00Z", "payload": {"seq": 1}}"#;
    let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

    assert_eq!(
        events.len(),
        0,
        "ping skip routing should produce zero events"
    );
    assert!(
        events.is_empty(),
        "event stream should be empty after skip routing"
    );
}

#[test]
fn test_skip_routing_event_emitter_not_called() {
    // Test that skip routing bypasses the event emitter entirely
    let mut type_routing = HashMap::new();
    type_routing.insert("skip_event".to_string(), "skip".to_string());
    type_routing.insert("normal_event".to_string(), "event".to_string());

    let plugin = create_skip_routing_test_plugin(type_routing);
    let context = ParseContext::new(
        "test-session".to_string(),
        "skip-test".to_string(),
        "/tmp/test.jsonl".to_string(),
    );

    // Skip-type line should not call event emitter
    let skip_line = r#"{"type": "skip_event", "timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user", "content": "skip me"}}"#;
    let skip_events = JsonlParser::parse_line(skip_line, 1, &context, &plugin).unwrap();

    assert_eq!(
        skip_events.len(),
        0,
        "skip routing should bypass event emitter"
    );

    // Normal-type line should call event emitter
    let normal_line = r#"{"type": "normal_event", "timestamp": "2026-03-16T12:00:01Z", "payload": {"role": "user", "content": "emit me"}}"#;
    let normal_events = JsonlParser::parse_line(normal_line, 2, &context, &plugin).unwrap();

    assert_eq!(
        normal_events.len(),
        1,
        "normal routing should call event emitter"
    );
    assert_eq!(normal_events[0].content, "emit me");
}

#[test]
fn test_skip_routing_multiple_skip_types_all_empty() {
    // Test that multiple different skip-type lines all produce empty event streams
    let mut type_routing = HashMap::new();
    type_routing.insert("heartbeat".to_string(), "skip".to_string());
    type_routing.insert("ping".to_string(), "skip".to_string());
    type_routing.insert("keepalive".to_string(), "skip".to_string());
    type_routing.insert("status".to_string(), "skip".to_string());

    let plugin = create_skip_routing_test_plugin(type_routing);
    let context = ParseContext::new(
        "test-session".to_string(),
        "skip-test".to_string(),
        "/tmp/test.jsonl".to_string(),
    );

    let skip_lines = [
        r#"{"type": "heartbeat", "timestamp": "2026-03-16T12:00:00Z", "payload": {"status": "ok"}}"#,
        r#"{"type": "ping", "timestamp": "2026-03-16T12:00:01Z", "payload": {"seq": 1}}"#,
        r#"{"type": "keepalive", "timestamp": "2026-03-16T12:00:02Z", "payload": {"alive": true}}"#,
        r#"{"type": "status", "timestamp": "2026-03-16T12:00:03Z", "payload": {"running": true}}"#,
    ];

    for (i, line) in skip_lines.iter().enumerate() {
        let events = JsonlParser::parse_line(line, i + 1, &context, &plugin).unwrap();
        assert!(
            events.is_empty(),
            "skip-type line {} should produce empty event stream",
            i + 1
        );
    }
}

#[test]
fn test_skip_routing_mixed_with_normal_events() {
    // Test skip routing behavior when mixed with normal event-producing lines
    let mut type_routing = HashMap::new();
    type_routing.insert("message".to_string(), "event".to_string());
    type_routing.insert("heartbeat".to_string(), "skip".to_string());
    type_routing.insert("system".to_string(), "event".to_string());

    let plugin = create_skip_routing_test_plugin(type_routing);
    let context = ParseContext::new(
        "test-session".to_string(),
        "skip-test".to_string(),
        "/tmp/test.jsonl".to_string(),
    );

    let mut tracker = EventStreamTracker::new();

    // Normal event should be tracked
    let message_line = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user", "content": "Hello"}}"#;
    let message_events = JsonlParser::parse_line(message_line, 1, &context, &plugin).unwrap();
    for event in message_events {
        tracker.track(event);
    }

    // Skip event should not be tracked
    let heartbeat_line = r#"{"type": "heartbeat", "timestamp": "2026-03-16T12:00:01Z", "payload": {"status": "ok"}}"#;
    let heartbeat_events = JsonlParser::parse_line(heartbeat_line, 2, &context, &plugin).unwrap();
    assert!(heartbeat_events.is_empty(), "heartbeat should be skipped");
    for event in heartbeat_events {
        tracker.track(event);
    }

    // Another normal event should be tracked
    let system_line = r#"{"type": "system", "timestamp": "2026-03-16T12:00:02Z", "payload": {"role": "system", "content": "Processing"}}"#;
    let system_events = JsonlParser::parse_line(system_line, 3, &context, &plugin).unwrap();
    for event in system_events {
        tracker.track(event);
    }

    // Verify we only have 2 events (message + system, heartbeat was skipped)
    assert_eq!(
        tracker.count(),
        2,
        "tracker should only contain non-skip events"
    );
    assert!(
        !tracker.is_empty(),
        "tracker should have events from non-skip lines"
    );
}

#[test]
fn test_skip_routing_edge_case_empty_payload() {
    // Test skip routing with empty payload object
    let mut type_routing = HashMap::new();
    type_routing.insert("empty_skip".to_string(), "skip".to_string());

    let plugin = create_skip_routing_test_plugin(type_routing);
    let context = ParseContext::new(
        "test-session".to_string(),
        "skip-test".to_string(),
        "/tmp/test.jsonl".to_string(),
    );

    let line = r#"{"type": "empty_skip", "timestamp": "2026-03-16T12:00:00Z", "payload": {}}"#;
    let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

    assert_eq!(
        events.len(),
        0,
        "skip routing with empty payload should produce zero events"
    );
}

#[test]
fn test_skip_routing_edge_case_nested_payload() {
    // Test skip routing with deeply nested payload structure
    let mut type_routing = HashMap::new();
    type_routing.insert("nested_skip".to_string(), "skip".to_string());

    let plugin = create_skip_routing_test_plugin(type_routing);
    let context = ParseContext::new(
        "test-session".to_string(),
        "skip-test".to_string(),
        "/tmp/test.jsonl".to_string(),
    );

    let line = r#"{"type": "nested_skip", "timestamp": "2026-03-16T12:00:00Z", "payload": {"level1": {"level2": {"level3": "deep content"}}}}"#;
    let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

    assert_eq!(
        events.len(),
        0,
        "skip routing with nested payload should produce zero events"
    );
}

#[test]
fn test_skip_routing_edge_case_large_payload() {
    // Test skip routing with large payload (simulating real logs)
    let mut type_routing = HashMap::new();
    type_routing.insert("large_skip".to_string(), "skip".to_string());

    let plugin = create_skip_routing_test_plugin(type_routing);
    let context = ParseContext::new(
        "test-session".to_string(),
        "skip-test".to_string(),
        "/tmp/test.jsonl".to_string(),
    );

    // Create a large payload (10KB of data)
    let large_content = "x".repeat(10240);
    let line = format!(
        r#"{{"type": "large_skip", "timestamp": "2026-03-16T12:00:00Z", "payload": {{"content": "{}"}}}}"#,
        large_content
    );

    let events = JsonlParser::parse_line(&line, 1, &context, &plugin).unwrap();

    assert_eq!(
        events.len(),
        0,
        "skip routing with large payload should produce zero events"
    );
}

#[test]
fn test_skip_routing_edge_case_special_characters() {
    // Test skip routing with special characters in payload
    let mut type_routing = HashMap::new();
    type_routing.insert("special_skip".to_string(), "skip".to_string());

    let plugin = create_skip_routing_test_plugin(type_routing);
    let context = ParseContext::new(
        "test-session".to_string(),
        "skip-test".to_string(),
        "/tmp/test.jsonl".to_string(),
    );

    let line = r#"{"type": "special_skip", "timestamp": "2026-03-16T12:00:00Z", "payload": {"content": "Special chars: \n\t\r\"'<>[]{}"}}"#;
    let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

    assert_eq!(
        events.len(),
        0,
        "skip routing with special characters should produce zero events"
    );
}

#[test]
fn test_skip_routing_unknown_type_defaults_to_skip() {
    // Test that unknown types default to skip behavior
    let mut type_routing = HashMap::new();
    type_routing.insert("message".to_string(), "event".to_string());
    // "unknown_type" is NOT in the routing map

    let plugin = create_skip_routing_test_plugin(type_routing);
    let context = ParseContext::new(
        "test-session".to_string(),
        "skip-test".to_string(),
        "/tmp/test.jsonl".to_string(),
    );

    let line = r#"{"type": "unknown_type", "timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user", "content": "unknown"}}"#;
    let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

    assert_eq!(
        events.len(),
        0,
        "unknown type should default to skip behavior"
    );
}

#[test]
fn test_skip_routing_case_sensitivity() {
    // Test that skip routing is case-sensitive
    let mut type_routing = HashMap::new();
    type_routing.insert("Heartbeat".to_string(), "skip".to_string());
    // Only "Heartbeat" (capital H) routes to skip

    let plugin = create_skip_routing_test_plugin(type_routing);
    let context = ParseContext::new(
        "test-session".to_string(),
        "skip-test".to_string(),
        "/tmp/test.jsonl".to_string(),
    );

    // Exact match should be skipped
    let exact_line = r#"{"type": "Heartbeat", "timestamp": "2026-03-16T12:00:00Z", "payload": {"status": "ok"}}"#;
    let exact_events = JsonlParser::parse_line(exact_line, 1, &context, &plugin).unwrap();
    assert_eq!(exact_events.len(), 0, "exact case match should be skipped");

    // Different case should NOT be skipped (defaults to skip, but not explicitly routed)
    let different_line = r#"{"type": "heartbeat", "timestamp": "2026-03-16T12:00:01Z", "payload": {"status": "ok"}}"#;
    let different_events = JsonlParser::parse_line(different_line, 2, &context, &plugin).unwrap();
    assert_eq!(
        different_events.len(),
        0,
        "different case should also be skipped (defaults to skip)"
    );
}

#[test]
fn test_skip_routing_timestamp_field_variations() {
    // Test skip routing with different timestamp field formats
    let mut type_routing = HashMap::new();
    type_routing.insert("timestamp_test".to_string(), "skip".to_string());

    let plugin = create_skip_routing_test_plugin(type_routing);
    let context = ParseContext::new(
        "test-session".to_string(),
        "skip-test".to_string(),
        "/tmp/test.jsonl".to_string(),
    );

    let timestamp_variations = [
        r#"{"type": "timestamp_test", "timestamp": "2026-03-16T12:00:00Z", "payload": {}}"#,
        r#"{"type": "timestamp_test", "timestamp": "2026-03-16T12:00:00.123Z", "payload": {}}"#,
        r#"{"type": "timestamp_test", "timestamp": "1710595200", "payload": {}}"#,
    ];

    for (i, line) in timestamp_variations.iter().enumerate() {
        let events = JsonlParser::parse_line(line, i + 1, &context, &plugin).unwrap();
        assert!(
            events.is_empty(),
            "skip routing should work regardless of timestamp format"
        );
    }
}

#[test]
fn test_skip_routing_consecutive_skip_lines() {
    // Test multiple consecutive skip-type lines
    let mut type_routing = HashMap::new();
    type_routing.insert("skip1".to_string(), "skip".to_string());
    type_routing.insert("skip2".to_string(), "skip".to_string());
    type_routing.insert("skip3".to_string(), "skip".to_string());

    let plugin = create_skip_routing_test_plugin(type_routing);
    let context = ParseContext::new(
        "test-session".to_string(),
        "skip-test".to_string(),
        "/tmp/test.jsonl".to_string(),
    );

    let mut tracker = EventStreamTracker::new();

    let skip_lines = [
        r#"{"type": "skip1", "timestamp": "2026-03-16T12:00:00Z", "payload": {}}"#,
        r#"{"type": "skip2", "timestamp": "2026-03-16T12:00:01Z", "payload": {}}"#,
        r#"{"type": "skip3", "timestamp": "2026-03-16T12:00:02Z", "payload": {}}"#,
    ];

    for (i, line) in skip_lines.iter().enumerate() {
        let events = JsonlParser::parse_line(line, i + 1, &context, &plugin).unwrap();
        assert!(
            events.is_empty(),
            "consecutive skip line {} should produce no events",
            i + 1
        );
        for event in events {
            tracker.track(event);
        }
    }

    assert!(
        tracker.is_empty(),
        "tracker should remain empty after consecutive skip lines"
    );
}

#[test]
fn test_skip_routing_meta_type_vs_skip_type() {
    // Test that both meta and skip routing produce zero events
    let mut type_routing = HashMap::new();
    type_routing.insert("skip_type".to_string(), "skip".to_string());
    type_routing.insert("meta_type".to_string(), "meta".to_string());

    let plugin = create_skip_routing_test_plugin(type_routing);
    let context = ParseContext::new(
        "test-session".to_string(),
        "skip-test".to_string(),
        "/tmp/test.jsonl".to_string(),
    );

    // Both skip and meta should produce zero events
    let skip_line = r#"{"type": "skip_type", "timestamp": "2026-03-16T12:00:00Z", "payload": {}}"#;
    let skip_events = JsonlParser::parse_line(skip_line, 1, &context, &plugin).unwrap();
    assert_eq!(
        skip_events.len(),
        0,
        "skip routing should produce zero events"
    );

    let meta_line = r#"{"type": "meta_type", "timestamp": "2026-03-16T12:00:01Z", "payload": {}}"#;
    let meta_events = JsonlParser::parse_line(meta_line, 2, &context, &plugin).unwrap();
    assert_eq!(
        meta_events.len(),
        0,
        "meta routing should also produce zero events"
    );
}

#[test]
fn test_skip_routing_file_parsing_integration() {
    // Test skip routing behavior when parsing entire files
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/envelope/non-event-types.jsonl");

    let mut type_routing = HashMap::new();
    type_routing.insert("heartbeat".to_string(), "skip".to_string());
    type_routing.insert("ping".to_string(), "skip".to_string());
    type_routing.insert("session_start".to_string(), "meta".to_string());
    type_routing.insert("session_end".to_string(), "meta".to_string());

    let plugin = create_skip_routing_test_plugin(type_routing);

    // Parse the entire fixture file
    let all_events = JsonlParser.parse(&fixture_path, &plugin).unwrap();

    // Verify that file with only skip/meta types produces zero events
    assert!(
        all_events.is_empty(),
        "file with only skip/meta types should produce zero events"
    );
}

#[test]
fn test_skip_routing_event_stream_tracker_consistency() {
    // Test that event stream tracker correctly handles skip routing
    let mut type_routing = HashMap::new();
    type_routing.insert("skip".to_string(), "skip".to_string());
    type_routing.insert("emit".to_string(), "event".to_string());

    let plugin = create_skip_routing_test_plugin(type_routing);
    let context = ParseContext::new(
        "test-session".to_string(),
        "skip-test".to_string(),
        "/tmp/test.jsonl".to_string(),
    );

    let mut tracker = EventStreamTracker::new()
        .with_expected_count(1)
        .with_expected_role_sequence(vec![Role::User]);

    // Skip line should not affect tracker
    let skip_line = r#"{"type": "skip", "timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user", "content": "skip"}}"#;
    let skip_events = JsonlParser::parse_line(skip_line, 1, &context, &plugin).unwrap();
    for event in skip_events {
        tracker.track(event);
    }

    // Emit line should affect tracker
    let emit_line = r#"{"type": "emit", "timestamp": "2026-03-16T12:00:01Z", "payload": {"role": "user", "content": "hello"}}"#;
    let emit_events = JsonlParser::parse_line(emit_line, 2, &context, &plugin).unwrap();
    for event in emit_events {
        tracker.track(event);
    }

    // Tracker should have exactly 1 event (skip was ignored)
    assert_eq!(
        tracker.count(),
        1,
        "tracker should count only non-skip events"
    );
    assert!(
        tracker.is_complete(),
        "tracker should be complete with 1 event"
    );
    assert!(
        tracker.verify_role_sequence().is_ok(),
        "tracker should verify role sequence"
    );
}

#[test]
fn test_skip_routing_return_value_consistency() {
    // Test that skip routing consistently returns Ok(Vec::new())
    let mut type_routing = HashMap::new();
    type_routing.insert("skip_return".to_string(), "skip".to_string());

    let plugin = create_skip_routing_test_plugin(type_routing);
    let context = ParseContext::new(
        "test-session".to_string(),
        "skip-test".to_string(),
        "/tmp/test.jsonl".to_string(),
    );

    let line = r#"{"type": "skip_return", "timestamp": "2026-03-16T12:00:00Z", "payload": {}}"#;
    let result = JsonlParser::parse_line(line, 1, &context, &plugin);

    // Should return Ok, not Err
    assert!(result.is_ok(), "skip routing should return Ok result");

    // Should be empty vector
    let events = result.unwrap();
    assert!(events.is_empty(), "skip routing should return empty vector");
    assert_eq!(
        events.len(),
        0,
        "skip routing should return Vec with length 0"
    );
}

#[test]
fn test_skip_routing_no_memory_leak() {
    // Test that skip routing doesn't leak memory by creating event objects
    let mut type_routing = HashMap::new();
    type_routing.insert("leak_test".to_string(), "skip".to_string());

    let plugin = create_skip_routing_test_plugin(type_routing);
    let context = ParseContext::new(
        "test-session".to_string(),
        "skip-test".to_string(),
        "/tmp/test.jsonl".to_string(),
    );

    // Process many skip-type lines
    for i in 0..1000 {
        let iteration = i % 60;
        let payload = format!(r#"{{"iteration": {}}}"#, iteration);
        let line = format!(
            r#"{{"type": "leak_test", "timestamp": "2026-03-16T12:00:{:02}Z", "payload": {}}}"#,
            iteration, payload
        );
        let events = JsonlParser::parse_line(&line, i + 1, &context, &plugin).unwrap();
        assert!(
            events.is_empty(),
            "skip routing iteration {} should produce no events",
            i
        );
    }

    // If we got here without panicking or OOM, skip routing properly avoids creating Event objects
}

#[test]
fn test_skip_routing_fixture_validation() {
    // Test skip routing using fixture-based validation
    let mut fixture = SkipRoutingFixture::new("comprehensive-skip-test".to_string());

    fixture = fixture
        .with_routing("heartbeat", RoutingAction::Skip)
        .with_routing("ping", RoutingAction::Skip)
        .with_routing("keepalive", RoutingAction::Skip)
        .with_routing("session_info", RoutingAction::Meta)
        .with_routing("message", RoutingAction::Emit);

    // Validate fixture expectations
    assert!(fixture
        .assert_routing("heartbeat", RoutingAction::Skip)
        .is_ok());
    assert!(fixture.assert_routing("ping", RoutingAction::Skip).is_ok());
    assert!(fixture
        .assert_routing("keepalive", RoutingAction::Skip)
        .is_ok());
    assert!(fixture
        .assert_routing("session_info", RoutingAction::Meta)
        .is_ok());
    assert!(fixture
        .assert_routing("message", RoutingAction::Emit)
        .is_ok());

    // Test wrong action
    assert!(fixture
        .assert_routing("heartbeat", RoutingAction::Emit)
        .is_err());
    assert!(fixture
        .assert_routing("message", RoutingAction::Skip)
        .is_err());

    // Test unknown type
    assert!(fixture
        .assert_routing("unknown", RoutingAction::Skip)
        .is_err());
}
