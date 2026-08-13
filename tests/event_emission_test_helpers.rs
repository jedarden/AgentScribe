//! Test infrastructure for event emission testing
//!
//! This module provides comprehensive test helpers, mock objects, and utilities
//! for testing event emission behavior, skip routing scenarios, and event stream
//! state verification in AgentScribe.
//!
//! # Overview
//!
//! Event emission testing covers:
//! - **Skip routing**: Verifying events are properly skipped based on envelope routing rules
//! - **Event stream state**: Tracking emitted events and their order
//! - **Mock event emitters**: Simulating various agent log formats
//! - **State verification**: Asserting expected vs actual event emission patterns
//!
//! # Examples
//!
//! ```ignore
//! use event_emission_test_helpers::*;
//!
//! // Create a mock event stream
//! let mut stream = MockEventStream::new();
//!
//! // Emit test events
//! stream.emit_user_event("test-session", "Hello, world!");
//! stream.emit_tool_call_event("test-session", "Edit", "src/main.rs");
//! stream.emit_tool_result_event("test-session", "Exit code 0");
//!
//! // Verify emission
//! assert_eq!(stream.event_count(), 3);
//! assert!(!stream.is_empty());
//! ```

use agentscribe::event::{Event, Role};
use chrono::Utc;
use serde_json::json;
use std::collections::{HashMap, VecDeque};

/// Mock event emitter for testing event emission scenarios
///
/// `MockEventEmitter` simulates the behavior of real agent log parsers by
/// generating events according to configurable patterns. It supports:
///
/// - Sequential event emission with automatic timestamp management
/// - Role-based emission (user, assistant, tool_call, tool_result)
/// - Session tracking and validation
/// - Content customization for each event type
#[derive(Debug, Clone)]
pub struct MockEventEmitter {
    /// Session ID for events
    session_id: String,
    /// Source agent name
    source_agent: String,
    /// Current timestamp (auto-increments on each emission)
    current_timestamp: chrono::DateTime<chrono::Utc>,
    /// Millisecond increment between events
    timestamp_increment_ms: i64,
    /// Emitted events
    events: Vec<Event>,
}

impl MockEventEmitter {
    /// Create a new mock event emitter
    ///
    /// # Arguments
    ///
    /// * `session_id` - Session identifier in format `<agent>/<id>`
    /// * `source_agent` - Name of the source agent (e.g., "claude-code")
    pub fn new(session_id: String, source_agent: String) -> Self {
        MockEventEmitter {
            session_id,
            source_agent,
            current_timestamp: Utc::now(),
            timestamp_increment_ms: 1000, // 1 second between events
            events: Vec::new(),
        }
    }

    /// Set the timestamp increment between events
    #[allow(dead_code)]
    pub fn with_timestamp_increment(mut self, increment_ms: i64) -> Self {
        self.timestamp_increment_ms = increment_ms;
        self
    }

    /// Set the starting timestamp
    #[allow(dead_code)]
    pub fn with_start_time(mut self, start_time: chrono::DateTime<chrono::Utc>) -> Self {
        self.current_timestamp = start_time;
        self
    }

    /// Emit a user event with the given content
    pub fn emit_user_event(&mut self, content: &str) -> &Event {
        let event = Event::new(
            self.current_timestamp,
            self.session_id.clone(),
            self.source_agent.clone(),
            Role::User,
            content.to_string(),
        );
        self.events.push(event.clone());
        self.advance_timestamp();
        self.events.last().unwrap()
    }

    /// Emit an assistant event with the given content
    pub fn emit_assistant_event(&mut self, content: &str) -> &Event {
        let event = Event::new(
            self.current_timestamp,
            self.session_id.clone(),
            self.source_agent.clone(),
            Role::Assistant,
            content.to_string(),
        );
        self.events.push(event.clone());
        self.advance_timestamp();
        self.events.last().unwrap()
    }

    /// Emit a tool_call event
    ///
    /// # Arguments
    ///
    /// * `tool_name` - Name of the tool being called
    /// * `content` - Content describing the tool call
    pub fn emit_tool_call_event(&mut self, tool_name: &str, content: &str) -> &Event {
        let mut event = Event::new(
            self.current_timestamp,
            self.session_id.clone(),
            self.source_agent.clone(),
            Role::ToolCall,
            content.to_string(),
        );
        event.tool = Some(tool_name.to_string());
        self.events.push(event.clone());
        self.advance_timestamp();
        self.events.last().unwrap()
    }

    /// Emit a tool_result event
    ///
    /// # Arguments
    ///
    /// * `tool_name` - Name of the tool that produced the result
    /// * `content` - Tool output content
    pub fn emit_tool_result_event(&mut self, tool_name: &str, content: &str) -> &Event {
        let mut event = Event::new(
            self.current_timestamp,
            self.session_id.clone(),
            self.source_agent.clone(),
            Role::ToolResult,
            content.to_string(),
        );
        event.tool = Some(tool_name.to_string());
        self.events.push(event.clone());
        self.advance_timestamp();
        self.events.last().unwrap()
    }

    /// Emit a system event
    #[allow(dead_code)]
    pub fn emit_system_event(&mut self, content: &str) -> &Event {
        let event = Event::new(
            self.current_timestamp,
            self.session_id.clone(),
            self.source_agent.clone(),
            Role::System,
            content.to_string(),
        );
        self.events.push(event.clone());
        self.advance_timestamp();
        self.events.last().unwrap()
    }

    /// Emit a custom event with full control over all fields
    #[allow(dead_code)]
    pub fn emit_custom_event(&mut self, event: Event) {
        self.events.push(event);
        self.advance_timestamp();
    }

    /// Advance the internal timestamp
    fn advance_timestamp(&mut self) {
        self.current_timestamp += chrono::Duration::milliseconds(self.timestamp_increment_ms);
    }

    /// Get all emitted events
    #[allow(dead_code)]
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Get the number of emitted events
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Check if no events have been emitted
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Clear all emitted events
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Get events by role
    pub fn events_by_role(&self, role: Role) -> Vec<&Event> {
        self.events.iter().filter(|e| e.role == role).collect()
    }

    /// Get events by tool name
    pub fn events_by_tool(&self, tool_name: &str) -> Vec<&Event> {
        self.events
            .iter()
            .filter(|e| e.tool.as_deref() == Some(tool_name))
            .collect()
    }
}

/// Event stream state tracker for verifying emission patterns
///
/// `EventStreamTracker` tracks the state of an event stream to enable
/// assertions about emission patterns, ordering, and completeness.
#[derive(Debug, Clone)]
pub struct EventStreamTracker {
    /// Tracked events in order
    events: VecDeque<Event>,
    /// Expected event count
    expected_count: Option<usize>,
    /// Role sequence expectations
    expected_role_sequence: Vec<Role>,
}

impl EventStreamTracker {
    /// Create a new event stream tracker
    pub fn new() -> Self {
        EventStreamTracker {
            events: VecDeque::new(),
            expected_count: None,
            expected_role_sequence: Vec::new(),
        }
    }

    /// Set the expected event count
    pub fn with_expected_count(mut self, count: usize) -> Self {
        self.expected_count = Some(count);
        self
    }

    /// Set the expected role sequence
    pub fn with_expected_role_sequence(mut self, roles: Vec<Role>) -> Self {
        self.expected_role_sequence = roles;
        self
    }

    /// Track an event
    pub fn track(&mut self, event: Event) {
        self.events.push_back(event);
    }

    /// Get the current event count
    pub fn count(&self) -> usize {
        self.events.len()
    }

    /// Check if the stream is empty
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Check if the expected count has been reached
    pub fn is_complete(&self) -> bool {
        match self.expected_count {
            Some(expected) => self.events.len() >= expected,
            None => false,
        }
    }

    /// Verify the role sequence matches expectations
    pub fn verify_role_sequence(&self) -> Result<(), String> {
        if self.expected_role_sequence.is_empty() {
            return Ok(());
        }

        if self.events.len() != self.expected_role_sequence.len() {
            return Err(format!(
                "Role sequence length mismatch: expected {}, got {}",
                self.expected_role_sequence.len(),
                self.events.len()
            ));
        }

        for (i, (event, expected_role)) in self
            .events
            .iter()
            .zip(self.expected_role_sequence.iter())
            .enumerate()
        {
            if event.role != *expected_role {
                return Err(format!(
                    "Role mismatch at position {}: expected {:?}, got {:?}",
                    i, expected_role, event.role
                ));
            }
        }

        Ok(())
    }

    /// Get the next event (removes it from the stream)
    #[allow(dead_code)]
    pub fn consume_next(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    /// Peek at the next event without removing it
    #[allow(dead_code)]
    pub fn peek(&self) -> Option<&Event> {
        self.events.front()
    }

    /// Get all remaining events
    #[allow(dead_code)]
    pub fn remaining(&self) -> Vec<Event> {
        self.events.iter().cloned().collect()
    }

    /// Clear all tracked events
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.events.clear();
    }
}

impl Default for EventStreamTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Skip routing test fixture for envelope routing scenarios
///
/// `SkipRoutingFixture` provides test fixtures and assertion helpers for
/// testing envelope routing rules that determine which events should be
/// emitted, skipped, or treated as metadata.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SkipRoutingFixture {
    /// Fixture name
    name: String,
    /// Expected routing for each event type
    expected_routing: HashMap<String, RoutingAction>,
}

/// Expected routing action for an event type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingAction {
    /// Event should be emitted as a canonical event
    Emit,
    /// Event should be skipped (no output)
    Skip,
    /// Event should be treated as metadata (no canonical event, but metadata preserved)
    Meta,
}

impl SkipRoutingFixture {
    /// Create a new skip routing fixture
    pub fn new(name: String) -> Self {
        SkipRoutingFixture {
            name,
            expected_routing: HashMap::new(),
        }
    }

    /// Get the fixture name
    #[allow(dead_code)]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Add expected routing for an event type
    pub fn with_routing(mut self, event_type: &str, action: RoutingAction) -> Self {
        self.expected_routing.insert(event_type.to_string(), action);
        self
    }

    /// Get expected routing for an event type
    pub fn get_routing(&self, event_type: &str) -> Option<RoutingAction> {
        self.expected_routing.get(event_type).copied()
    }

    /// Assert that a specific event type has the expected routing
    pub fn assert_routing(
        &self,
        event_type: &str,
        actual_action: RoutingAction,
    ) -> Result<(), String> {
        match self.get_routing(event_type) {
            Some(expected) if expected == actual_action => Ok(()),
            Some(expected) => Err(format!(
                "Routing action mismatch for '{}': expected {:?}, got {:?}",
                event_type, expected, actual_action
            )),
            None => Err(format!(
                "No expected routing defined for event type '{}'",
                event_type
            )),
        }
    }
}

/// Event emission verifier for comprehensive emission testing
///
/// `EventEmissionVerifier` provides high-level assertion helpers for verifying
/// complete event emission scenarios.
pub struct EventEmissionVerifier;

impl EventEmissionVerifier {
    /// Verify that a stream contains events in the expected order
    pub fn verify_event_order(events: &[Event], expected_roles: &[Role]) -> Result<(), String> {
        if events.len() != expected_roles.len() {
            return Err(format!(
                "Event count mismatch: expected {}, got {}",
                expected_roles.len(),
                events.len()
            ));
        }

        for (i, (event, expected_role)) in events.iter().zip(expected_roles.iter()).enumerate() {
            if event.role != *expected_role {
                return Err(format!(
                    "Event role mismatch at position {}: expected {:?}, got {:?}",
                    i, expected_role, event.role
                ));
            }
        }

        Ok(())
    }

    /// Verify that a stream contains the expected number of events per role
    pub fn verify_role_counts(
        events: &[Event],
        expected_counts: &HashMap<Role, usize>,
    ) -> Result<(), String> {
        let mut actual_counts: HashMap<Role, usize> = HashMap::new();

        for event in events {
            *actual_counts.entry(event.role).or_insert(0) += 1;
        }

        for (role, expected_count) in expected_counts.iter() {
            let actual_count = actual_counts.get(role).copied().unwrap_or(0);
            if actual_count != *expected_count {
                return Err(format!(
                    "Role count mismatch for {:?}: expected {}, got {}",
                    role, expected_count, actual_count
                ));
            }
        }

        Ok(())
    }

    /// Verify that all events have unique timestamps
    pub fn verify_unique_timestamps(events: &[Event]) -> Result<(), String> {
        let mut seen_timestamps = std::collections::HashSet::new();

        for (i, event) in events.iter().enumerate() {
            if !seen_timestamps.insert(event.ts) {
                return Err(format!("Duplicate timestamp at event {}: {}", i, event.ts));
            }
        }

        Ok(())
    }

    /// Verify that all events belong to the same session
    pub fn verify_single_session(
        events: &[Event],
        expected_session_id: &str,
    ) -> Result<(), String> {
        for (i, event) in events.iter().enumerate() {
            if event.session_id != expected_session_id {
                return Err(format!(
                    "Session ID mismatch at event {}: expected {}, got {}",
                    i, expected_session_id, event.session_id
                ));
            }
        }

        Ok(())
    }

    /// Verify that tool_call events are followed by tool_result events
    pub fn verify_tool_call_result_pairing(events: &[Event]) -> Result<(), String> {
        let mut tool_call_stack = Vec::new();

        for (i, event) in events.iter().enumerate() {
            match event.role {
                Role::ToolCall => {
                    if let Some(tool_name) = &event.tool {
                        tool_call_stack.push((i, tool_name.clone()));
                    }
                }
                Role::ToolResult => {
                    if tool_call_stack.is_empty() {
                        return Err(format!(
                            "Tool result at event {} has no matching tool call",
                            i
                        ));
                    }
                    tool_call_stack.pop();
                }
                _ => {}
            }
        }

        if !tool_call_stack.is_empty() {
            return Err(format!(
                "Unclosed tool calls at positions: {:?}",
                tool_call_stack.iter().map(|(i, _)| i).collect::<Vec<_>>()
            ));
        }

        Ok(())
    }
}

/// Create standard test fixtures for common event emission scenarios
pub mod fixtures {
    use super::*;

    /// Create a simple conversation fixture (user → assistant)
    pub fn simple_conversation(session_id: &str) -> Vec<Event> {
        let base_time = Utc::now();
        vec![
            Event {
                ts: base_time,
                session_id: session_id.to_string(),
                source_agent: "test-agent".to_string(),
                source_version: None,
                project: None,
                role: Role::User,
                content: "How do I fix this error?".to_string(),
                tool: None,
                tool_params: None,
                tokens: None,
                model: None,
                file_paths: Vec::new(),
                error_fingerprints: Vec::new(),
            },
            Event {
                ts: base_time + chrono::Duration::seconds(1),
                session_id: session_id.to_string(),
                source_agent: "test-agent".to_string(),
                source_version: None,
                project: None,
                role: Role::Assistant,
                content: "Here's how to fix it...".to_string(),
                tool: None,
                tool_params: None,
                tokens: None,
                model: None,
                file_paths: Vec::new(),
                error_fingerprints: Vec::new(),
            },
        ]
    }

    /// Create a tool use fixture (user → assistant with tool_call → tool_result)
    pub fn tool_use_conversation(session_id: &str) -> Vec<Event> {
        let base_time = Utc::now();
        vec![
            Event {
                ts: base_time,
                session_id: session_id.to_string(),
                source_agent: "test-agent".to_string(),
                source_version: None,
                project: None,
                role: Role::User,
                content: "Edit the file".to_string(),
                tool: None,
                tool_params: None,
                tokens: None,
                model: None,
                file_paths: Vec::new(),
                error_fingerprints: Vec::new(),
            },
            Event {
                ts: base_time + chrono::Duration::seconds(1),
                session_id: session_id.to_string(),
                source_agent: "test-agent".to_string(),
                source_version: None,
                project: None,
                role: Role::ToolCall,
                content: "Editing src/main.rs".to_string(),
                tool: Some("Edit".to_string()),
                tool_params: Some(
                    json!({"file_path": "src/main.rs", "diff": "- Line 1\n+ Line 2"}),
                ),
                tokens: None,
                model: None,
                file_paths: vec!["src/main.rs".to_string()],
                error_fingerprints: Vec::new(),
            },
            Event {
                ts: base_time + chrono::Duration::seconds(2),
                session_id: session_id.to_string(),
                source_agent: "test-agent".to_string(),
                source_version: None,
                project: None,
                role: Role::ToolResult,
                content: "Exit code 0".to_string(),
                tool: Some("Edit".to_string()),
                tool_params: None,
                tokens: None,
                model: None,
                file_paths: Vec::new(),
                error_fingerprints: Vec::new(),
            },
        ]
    }

    /// Create a multi-turn conversation fixture
    pub fn multi_turn_conversation(session_id: &str) -> Vec<Event> {
        let base_time = Utc::now();
        vec![
            Event {
                ts: base_time,
                session_id: session_id.to_string(),
                source_agent: "test-agent".to_string(),
                source_version: None,
                project: None,
                role: Role::User,
                content: "Fix the bug".to_string(),
                tool: None,
                tool_params: None,
                tokens: None,
                model: None,
                file_paths: Vec::new(),
                error_fingerprints: Vec::new(),
            },
            Event {
                ts: base_time + chrono::Duration::seconds(1),
                session_id: session_id.to_string(),
                source_agent: "test-agent".to_string(),
                source_version: None,
                project: None,
                role: Role::Assistant,
                content: "I'll help with that".to_string(),
                tool: None,
                tool_params: None,
                tokens: None,
                model: None,
                file_paths: Vec::new(),
                error_fingerprints: Vec::new(),
            },
            Event {
                ts: base_time + chrono::Duration::seconds(2),
                session_id: session_id.to_string(),
                source_agent: "test-agent".to_string(),
                source_version: None,
                project: None,
                role: Role::ToolCall,
                content: "Reading file".to_string(),
                tool: Some("Read".to_string()),
                tool_params: Some(json!({"file_path": "src/main.rs"})),
                tokens: None,
                model: None,
                file_paths: vec!["src/main.rs".to_string()],
                error_fingerprints: Vec::new(),
            },
            Event {
                ts: base_time + chrono::Duration::seconds(3),
                session_id: session_id.to_string(),
                source_agent: "test-agent".to_string(),
                source_version: None,
                project: None,
                role: Role::ToolResult,
                content: "File content here".to_string(),
                tool: Some("Read".to_string()),
                tool_params: None,
                tokens: None,
                model: None,
                file_paths: Vec::new(),
                error_fingerprints: Vec::new(),
            },
            Event {
                ts: base_time + chrono::Duration::seconds(4),
                session_id: session_id.to_string(),
                source_agent: "test-agent".to_string(),
                source_version: None,
                project: None,
                role: Role::User,
                content: "Thanks!".to_string(),
                tool: None,
                tool_params: None,
                tokens: None,
                model: None,
                file_paths: Vec::new(),
                error_fingerprints: Vec::new(),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_event_emitter_basic() {
        let mut emitter =
            MockEventEmitter::new("test-session/123".to_string(), "test-agent".to_string());

        emitter.emit_user_event("Hello");
        emitter.emit_assistant_event("Hi there");
        emitter.emit_tool_call_event("Edit", "Editing file");

        assert_eq!(emitter.event_count(), 3);
        assert!(!emitter.is_empty());

        let user_events = emitter.events_by_role(Role::User);
        assert_eq!(user_events.len(), 1);
        assert_eq!(user_events[0].content, "Hello");
    }

    #[test]
    fn test_mock_event_emitter_tool_events() {
        let mut emitter =
            MockEventEmitter::new("test-session/456".to_string(), "test-agent".to_string());

        emitter.emit_tool_call_event("Read", "Reading file");
        emitter.emit_tool_result_event("Read", "File content");

        let tool_events = emitter.events_by_tool("Read");
        assert_eq!(tool_events.len(), 2);
    }

    #[test]
    fn test_event_stream_tracker() {
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
    fn test_skip_routing_fixture() {
        let mut fixture = SkipRoutingFixture::new("test-fixture".to_string());
        fixture = fixture
            .with_routing("message", RoutingAction::Emit)
            .with_routing("heartbeat", RoutingAction::Skip)
            .with_routing("session_start", RoutingAction::Meta);

        assert_eq!(fixture.get_routing("message"), Some(RoutingAction::Emit));
        assert_eq!(fixture.get_routing("heartbeat"), Some(RoutingAction::Skip));
        assert_eq!(
            fixture.get_routing("session_start"),
            Some(RoutingAction::Meta)
        );
        assert!(fixture
            .assert_routing("message", RoutingAction::Emit)
            .is_ok());
    }

    #[test]
    fn test_event_emission_verifier() {
        let events = fixtures::simple_conversation("test-session");

        assert!(
            EventEmissionVerifier::verify_event_order(&events, &[Role::User, Role::Assistant])
                .is_ok()
        );

        assert!(EventEmissionVerifier::verify_single_session(&events, "test-session").is_ok());
    }

    #[test]
    fn test_verify_tool_call_result_pairing() {
        let events = fixtures::tool_use_conversation("test-session");
        assert!(EventEmissionVerifier::verify_tool_call_result_pairing(&events).is_ok());
    }

    #[test]
    fn test_verify_unique_timestamps() {
        let events = fixtures::simple_conversation("test-session");
        assert!(EventEmissionVerifier::verify_unique_timestamps(&events).is_ok());
    }

    #[test]
    fn test_verify_role_counts() {
        let events = fixtures::tool_use_conversation("test-session");
        let mut expected_counts = HashMap::new();
        expected_counts.insert(Role::User, 1);
        expected_counts.insert(Role::ToolCall, 1);
        expected_counts.insert(Role::ToolResult, 1);

        assert!(EventEmissionVerifier::verify_role_counts(&events, &expected_counts).is_ok());
    }

    #[test]
    fn test_fixture_simple_conversation() {
        let events = fixtures::simple_conversation("test-session");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].role, Role::User);
        assert_eq!(events[1].role, Role::Assistant);
    }

    #[test]
    fn test_fixture_tool_use_conversation() {
        let events = fixtures::tool_use_conversation("test-session");
        assert_eq!(events.len(), 3);
        assert_eq!(events[1].role, Role::ToolCall);
        assert_eq!(events[2].role, Role::ToolResult);
    }

    #[test]
    fn test_fixture_multi_turn_conversation() {
        let events = fixtures::multi_turn_conversation("test-session");
        assert_eq!(events.len(), 5);
        assert_eq!(events[0].role, Role::User);
        assert_eq!(events[1].role, Role::Assistant);
        assert_eq!(events[2].role, Role::ToolCall);
        assert_eq!(events[3].role, Role::ToolResult);
        assert_eq!(events[4].role, Role::User);
    }
}
