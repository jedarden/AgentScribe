# Event Emission Testing Guide

## Overview

AgentScribe provides comprehensive test infrastructure for event emission testing in `tests/event_emission_test_helpers.rs`. This infrastructure enables testing of:

- **Event emission patterns** - Verify events are emitted in the correct order and format
- **Skip routing scenarios** - Test envelope routing rules that determine which events should be emitted, skipped, or treated as metadata
- **Event stream state** - Track and verify the state of event streams during parsing
- **Mock event generation** - Simulate various agent log formats for testing

## Core Components

### 1. MockEventEmitter

The `MockEventEmitter` simulates real agent log parsers by generating test events with automatic timestamp management and role-based emission.

```rust
use event_emission_test_helpers::MockEventEmitter;

// Create a mock emitter for a test session
let mut emitter = MockEventEmitter::new(
    "test-session/123".to_string(),
    "claude-code".to_string()
);

// Configure timing
emitter = emitter
    .with_timestamp_increment(1000) // 1 second between events
    .with_start_time(Utc::now());

// Emit events
emitter.emit_user_event("How do I fix this error?");
emitter.emit_assistant_event("Here's the solution...");
emitter.emit_tool_call_event("Edit", "Editing src/main.rs");
emitter.emit_tool_result_event("Edit", "Exit code 0");

// Verify emission
assert_eq!(emitter.event_count(), 4);
assert!(!emitter.is_empty());

// Query events by role or tool
let user_events = emitter.events_by_role(Role::User);
let edit_events = emitter.events_by_tool("Edit");
```

**Methods:**
- `new(session_id, source_agent)` - Create emitter with session and agent names
- `with_timestamp_increment(ms)` - Set time between events
- `with_start_time(timestamp)` - Set starting timestamp
- `emit_user_event(content)` - Emit a user message
- `emit_assistant_event(content)` - Emit an assistant response
- `emit_tool_call_event(tool, content)` - Emit a tool call
- `emit_tool_result_event(tool, content)` - Emit a tool result
- `emit_system_event(content)` - Emit a system message
- `emit_custom_event(event)` - Emit a fully customized event
- `events()` - Get all emitted events
- `event_count()` - Get number of events
- `is_empty()` - Check if no events emitted
- `clear()` - Clear all events
- `events_by_role(role)` - Filter events by role
- `events_by_tool(tool_name)` - Filter events by tool name

### 2. EventStreamTracker

The `EventStreamTracker` tracks event stream state for verifying emission patterns and completeness.

```rust
use event_emission_test_helpers::EventStreamTracker;

// Create tracker with expectations
let mut tracker = EventStreamTracker::new()
    .with_expected_count(3)
    .with_expected_role_sequence(vec![
        Role::User,
        Role::Assistant,
        Role::ToolCall
    ]);

// Track events
for event in events {
    tracker.track(event);
}

// Verify state
assert_eq!(tracker.count(), 3);
assert!(tracker.is_complete());
assert!(tracker.verify_role_sequence().is_ok());

// Consume events
while let Some(event) = tracker.consume_next() {
    // Process event
}
```

**Methods:**
- `new()` - Create new tracker
- `with_expected_count(n)` - Set expected event count
- `with_expected_role_sequence(roles)` - Set expected role sequence
- `track(event)` - Track an event
- `count()` - Get current event count
- `is_empty()` - Check if stream is empty
- `is_complete()` - Check if expected count reached
- `verify_role_sequence()` - Verify roles match expectations
- `consume_next()` - Get and remove next event
- `peek()` - Look at next event without removing
- `remaining()` - Get all remaining events
- `clear()` - Clear all tracked events

### 3. SkipRoutingFixture

The `SkipRoutingFixture` tests envelope routing rules that determine event handling.

```rust
use event_emission_test_helpers::{SkipRoutingFixture, RoutingAction};

// Create fixture with routing expectations
let fixture = SkipRoutingFixture::new("codex-envelope".to_string())
    .with_routing("session_meta", RoutingAction::Meta)
    .with_routing("response_item", RoutingAction::Emit)
    .with_routing("event_msg", RoutingAction::Skip);

// Assert routing behavior
assert!(fixture.assert_routing("session_meta", RoutingAction::Meta).is_ok());
assert!(fixture.assert_routing("response_item", RoutingAction::Emit).is_ok());
assert!(fixture.assert_routing("event_msg", RoutingAction::Skip).is_ok());
```

**Routing Actions:**
- `Emit` - Event should be emitted as a canonical event
- `Skip` - Event should be skipped (no output)
- `Meta` - Event should be treated as metadata (preserved but not emitted as canonical event)

**Methods:**
- `new(name)` - Create fixture with name
- `with_routing(event_type, action)` - Add expected routing
- `get_routing(event_type)` - Get routing for event type
- `assert_routing(event_type, actual_action)` - Assert routing matches expectation

### 4. EventEmissionVerifier

The `EventEmissionVerifier` provides high-level assertion helpers for comprehensive emission testing.

```rust
use event_emission_test_helpers::EventEmissionVerifier;

// Verify event order
EventEmissionVerifier::verify_event_order(
    &events,
    &[Role::User, Role::Assistant, Role::ToolCall]
)?;

// Verify role counts
let mut expected_counts = HashMap::new();
expected_counts.insert(Role::User, 2);
expected_counts.insert(Role::Assistant, 1);
EventEmissionVerifier::verify_role_counts(&events, &expected_counts)?;

// Verify tool call/result pairing
EventEmissionVerifier::verify_tool_call_result_pairing(&events)?;

// Verify all events have unique timestamps
EventEmissionVerifier::verify_unique_timestamps(&events)?;

// Verify all events belong to same session
EventEmissionVerifier::verify_single_session(&events, "test-session")?;
```

**Methods:**
- `verify_event_order(events, expected_roles)` - Verify events are in expected order
- `verify_role_counts(events, expected_counts)` - Verify expected count per role
- `verify_tool_call_result_pairing(events)` - Verify tool calls have matching results
- `verify_unique_timestamps(events)` - Verify all timestamps are unique
- `verify_single_session(events, session_id)` - Verify all events are from same session

### 5. Pre-built Fixtures

The `fixtures` module provides pre-built test scenarios:

```rust
use event_emission_test_helpers::fixtures;

// Simple user-assistant conversation
let events = fixtures::simple_conversation("session-123");

// Tool use scenario (user → tool_call → tool_result)
let events = fixtures::tool_use_conversation("session-456");

// Multi-turn conversation with multiple exchanges
let events = fixtures::multi_turn_conversation("session-789");
```

**Available Fixtures:**
- `simple_conversation(session_id)` - Basic user-assistant exchange
- `tool_use_conversation(session_id)` - User → tool_call → tool_result pattern
- `multi_turn_conversation(session_id)` - Complex conversation with multiple turns

## Usage Examples

### Example 1: Testing Event Emission Order

```rust
#[test]
fn test_conversation_order() {
    use event_emission_test_helpers::*;
    
    let mut emitter = MockEventEmitter::new(
        "test-session".to_string(),
        "test-agent".to_string()
    );
    
    // Simulate conversation
    emitter.emit_user_event("Fix the bug");
    emitter.emit_assistant_event("I'll help");
    emitter.emit_tool_call_event("Read", "Reading file");
    emitter.emit_tool_result_event("Read", "File content");
    
    // Verify order
    let events = emitter.events();
    EventEmissionVerifier::verify_event_order(
        events,
        &[Role::User, Role::Assistant, Role::ToolCall, Role::ToolResult]
    ).unwrap();
}
```

### Example 2: Testing Skip Routing

```rust
#[test]
fn test_envelope_routing() {
    use event_emission_test_helpers::*;
    
    // Define expected routing for Codex envelope types
    let fixture = SkipRoutingFixture::new("codex-test".to_string())
        .with_routing("session_meta", RoutingAction::Meta)
        .with_routing("response_item", RoutingAction::Emit)
        .with_routing("turn_context", RoutingAction::Meta)
        .with_routing("event_msg", RoutingAction::Skip);
    
    // Test actual routing matches expectations
    let session_meta_action = parse_envelope_type("session_meta");
    fixture.assert_routing("session_meta", session_meta_action).unwrap();
}
```

### Example 3: Testing Event Stream State

```rust
#[test]
fn test_stream_tracking() {
    use event_emission_test_helpers::*;
    
    let mut tracker = EventStreamTracker::new()
        .with_expected_count(3)
        .with_expected_role_sequence(vec![Role::User, Role::Assistant, Role::User]);
    
    // Track events from a parser
    for event in parsed_events {
        tracker.track(event);
    }
    
    // Verify completeness
    assert!(tracker.is_complete());
    assert!(tracker.verify_role_sequence().is_ok());
}
```

### Example 4: Testing Tool Call/Result Pairing

```rust
#[test]
fn test_tool_pairing() {
    use event_emission_test_helpers::*;
    
    let events = fixtures::tool_use_conversation("test-session");
    
    // Verify proper tool call/result pairing
    EventEmissionVerifier::verify_tool_call_result_pairing(&events).unwrap();
    
    // Additional verification
    let tool_calls = events.iter()
        .filter(|e| e.role == Role::ToolCall)
        .count();
    let tool_results = events.iter()
        .filter(|e| e.role == Role::ToolResult)
        .count();
    
    assert_eq!(tool_calls, tool_results);
}
```

## Integration Testing

The test infrastructure integrates seamlessly with AgentScribe's parsing system. Here's how to test a real parser:

```rust
#[test]
fn test_claude_code_parser_emission() {
    use event_emission_test_helpers::*;
    use agentscribe::parser::claude_code;
    
    // Parse real fixture
    let fixture_path = "tests/fixtures/claude-code/complex-session.jsonl";
    let events = claude_code::parse_file(fixture_path).unwrap();
    
    // Verify emission patterns
    EventEmissionVerifier::verify_unique_timestamps(&events).unwrap();
    EventEmissionVerifier::verify_single_session(&events, "claude-code/abc123").unwrap();
    EventEmissionVerifier::verify_tool_call_result_pairing(&events).unwrap();
}
```

## Best Practices

1. **Start with fixtures** - Use pre-built fixtures when possible, then customize
2. **Verify invariants** - Always verify key invariants (timestamps, session IDs, tool pairing)
3. **Test edge cases** - Test empty sessions, single events, malformed data
4. **Use descriptive names** - Name fixtures and tests clearly to indicate what they test
5. **Combine verifiers** - Use multiple verifiers together for comprehensive testing
6. **Mock real scenarios** - Base test scenarios on actual agent behavior patterns

## Running Tests

Run all event emission tests:

```bash
cargo test event_emission
```

Run specific test categories:

```bash
cargo test mock_event_emitter
cargo test event_stream_tracker
cargo test skip_routing
cargo test event_emission_verifier
```

## Extending the Infrastructure

To add new functionality to the test infrastructure:

1. Add new types/traits to `event_emission_test_helpers.rs`
2. Implement methods following existing patterns
3. Add comprehensive documentation with examples
4. Include unit tests for new functionality
5. Update this guide with usage examples

## Related Documentation

- [Testing Framework Overview](../docs/testing-framework.md)
- [Parser Development Guide](../plugins/BUILDING_PLUGINS.md)
- [Event Schema Documentation](../src/event.rs)
