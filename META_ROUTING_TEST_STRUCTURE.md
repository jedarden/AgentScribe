# Meta Routing Test Structure Documentation

## Overview
This document describes the current test setup for meta routing fixtures in AgentScribe, specifically how session_start and session_end tests verify `Ok(Vec::new())` return behavior.

## Test File Locations

### Primary Test Files
- **Main parser tests**: `src/parser/jsonl.rs` (lines 739-2074 contain test module)
- **Test helpers**: `tests/test_helpers.rs` (contains reusable test infrastructure)

### Fixture Files
- **Meta routing fixture**: `tests/fixtures/envelope/non-event-types.jsonl`
- **Mixed routing fixture**: `tests/fixtures/envelope/envelope-routing.jsonl`
- **Skip-only fixture**: `tests/fixtures/envelope/skip-only.jsonl`

## Current Test Structure

### 1. Helper Functions in `tests/test_helpers.rs`

#### `create_meta_routing_test_plugin()`
Creates a Plugin configured with envelope routing for meta-type events:
```rust
pub fn create_meta_routing_test_plugin() -> Plugin {
    // Configures routing for:
    // - "message" → "event" (produces canonical events)
    // - "heartbeat" → "skip" (dropped)
    // - "ping" → "skip" (dropped)
    // - "session_start" → "meta" (metadata preserved, no events)
    // - "session_end" → "meta" (metadata preserved, no events)
    // - "metrics" → "meta" (metadata preserved, no events)
    // - "compaction" → "meta" (metadata preserved, no events)
}
```

#### `assert_meta_routing_returns_empty()`
Helper function that tests the `Ok(Vec::new())` pattern for meta-type routing:
```rust
pub fn assert_meta_routing_returns_empty(
    fixture_line: &str,
    line_number: usize,
    assertion_message: &str,
) {
    let plugin = create_meta_routing_test_plugin();
    let context = ParseContext::new(...);
    
    // Verify the line parses successfully
    let result = JsonlParser::parse_line(fixture_line, line_number, &context, &plugin);
    assert!(result.is_ok(), "Meta routing line should parse successfully");
    
    // Verify it produces zero events (the expected behavior for meta-type routing)
    let events = result.unwrap();
    assert!(events.is_empty(), "Meta-type routing should produce zero events");
}
```

### 2. Test Functions in `src/parser/jsonl.rs`

#### Individual Line Tests
The `src/parser/jsonl.rs` file contains these specific test functions that verify `Ok(Vec::new())` behavior:

**`test_session_start_fixture_line_returns_empty_vec()`** (line ~1630)
```rust
#[test]
fn test_session_start_fixture_line_returns_empty_vec() {
    // Test case for session_start fixture line from non-event-types.jsonl
    // Verifies that Ok(Vec::new()) is returned for session_start meta routing
    // Exact fixture line from non-event-types.jsonl line 3
    let line = r#"{"type":"session_start","timestamp":"2026-07-04T10:00:00Z","payload":{"session_id":"sess-001"}}"#;
    
    assert_meta_routing_returns_empty(line, 3, "session_start should produce zero events");
}
```

**`test_session_end_fixture_line_returns_empty_vec()`** (line ~1640)
```rust
#[test]
fn test_session_end_fixture_line_returns_empty_vec() {
    // Test case for session_end fixture line from non-event-types.jsonl
    // Verifies that Ok(Vec::new()) is returned for session_end meta routing
    // Exact fixture line from non-event-types.jsonl line 4
    let line = r#"{"type":"session_end","timestamp":"2026-07-04T10:00:30Z","payload":{"duration":30}}"#;
    
    assert_meta_routing_returns_empty(line, 4, "session_end should produce zero events");
}
```

#### Comprehensive Fixture Tests
**`test_fixture_with_only_non_event_types_produces_zero_events()`** (line ~1614)
```rust
#[test]
fn test_fixture_with_only_non_event_types_produces_zero_events() {
    // Parse the fixture file that contains ONLY skip/meta/unknown lines
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/envelope/non-event-types.jsonl");
    
    let plugin = create_skip_meta_unknown_plugin();
    let all_events = JsonlParser.parse(&fixture_path, &plugin).unwrap();
    
    assert!(all_events.is_empty(), 
        "fixture with only skip/meta/unknown lines should produce zero events, got {}", 
        all_events.len());
}
```

### 3. Meta Routing Type Categories

The test structure covers three routing categories:

#### **Skip Types** (produce zero events, dropped)
- `heartbeat` - heartbeat events
- `ping` - ping events
- Any unknown type not in routing map

#### **Meta Types** (produce zero events, accumulate metadata)
- `session_start` - session beginning with metadata
- `session_end` - session end with metadata
- `metrics` - performance/operational metrics
- `compaction` - storage operation metadata

#### **Event Types** (produce canonical events)
- `message` - normal message events

## Current Verification Pattern

### How `Ok(Vec::new())` is Verified

The current tests use a two-step verification process:

1. **Parse Success Check**: 
   ```rust
   let result = JsonlParser::parse_line(...);
   assert!(result.is_ok(), "Meta routing line should parse successfully");
   ```

2. **Empty Result Check**:
   ```rust
   let events = result.unwrap();
   assert!(events.is_empty(), "Meta-type routing should produce zero events");
   ```

### What This Pattern Ensures

The `Ok(Vec::new())` verification ensures:
- The line is syntactically valid JSON and parses without errors
- The meta routing logic correctly identifies the type and applies meta routing
- No canonical events are produced (only metadata accumulation, which is future work)
- The line is not dropped due to parse errors or routing issues

## Fixture File Structure

### `tests/fixtures/envelope/non-event-types.jsonl`
```jsonl
{"type":"heartbeat","timestamp":"2026-07-04T10:00:05Z","payload":{"status":"ok"}}
{"type":"ping","timestamp":"2026-07-04T10:00:10Z","payload":{"seq":1}}
{"type":"session_start","timestamp":"2026-07-04T10:00:00Z","payload":{"session_id":"sess-001"}}
{"type":"session_end","timestamp":"2026-07-04T10:00:30Z","payload":{"duration":30}}
{"type":"metrics","timestamp":"2026-07-04T10:00:35Z","payload":{"events_processed":5}}
{"type":"compaction","timestamp":"2026-07-04T10:00:40Z","payload":{"files_compacted":3}}
{"type":"unknown_event","timestamp":"2026-07-04T10:00:35Z","payload":{"data":"something"}}
```

This fixture file is used to test that all non-event types (skip, meta, unknown) correctly return `Ok(Vec::new())`.

### `tests/fixtures/envelope/envelope-routing.jsonl`
```jsonl
{"type":"session","version":3,"id":"sess-env-001","timestamp":"2025-01-15T09:00:00.000Z","cwd":"/home/user/env-test"}
{"type":"session_info","id":"si-001","timestamp":"2025-01-15T09:00:00.100Z","message":{"info":"git_branch","value":"main"}}
{"type":"message","id":"m-001","parentId":null,"timestamp":"2025-01-15T09:00:01.000Z","message":{"role":"user","content":"What files are in this directory?","timestamp":1736920801000}}
{"type":"model_change","id":"mc-001","timestamp":"2025-01-15T09:00:02.000Z","message":{"model":"claude-sonnet-4-5","provider":"anthropic"}}
{"type":"message","id":"m-002","parentId":"m-001","timestamp":"2025-01-15T09:00:03.000Z","message":{"role":"assistant","content":[{"type":"text","text":"I'll list the files for you."},{"type":"toolCall","id":"call-001","name":"bash","arguments":{"command":"ls -la"}}],"model":"claude-sonnet-4-5","provider":"anthropic","timestamp":1736920803000}}
{"type":"message","id":"m-003","parentId":"m-002","timestamp":"2025-01-15T09:00:04.000Z","message":{"role":"toolResult","toolCallId":"call-001","toolName":"bash","content":[{"type":"text","text":"README.md\nsrc/\ntests/"}],"isError":false,"timestamp":1736920804000}}
{"type":"message","id":"m-004","parentId":"m-003","timestamp":"2025-01-15T09:00:05.000Z","message":{"role":"assistant","content":"The directory contains a README, a src folder, and a tests folder.","model":"claude-sonnet-4-5","provider":"anthropic","timestamp":1736920805000}}
{"type":"compaction","id":"cmp-001","timestamp":"2025-01-15T09:00:06.000Z","message":{"summary":"Earlier conversation compacted to save context tokens."}}
{"type":"custom","id":"cust-001","timestamp":"2025-01-15T09:00:07.000Z","message":{"extension":"metrics","data":{"event":"copy"}}}
```

This fixture tests mixed scenarios with both event-producing and non-event-producing lines.

## Related Test Infrastructure

### Plugin Creation Helpers
Multiple helper functions exist for creating test plugins with different routing configurations:
- `create_test_plugin()` - Basic plugin without envelope
- `create_envelope_test_plugin()` - Plugin with basic envelope routing
- `create_meta_routing_plugin()` - Plugin with custom type routing
- `create_skip_meta_unknown_plugin()` - Plugin with comprehensive routing

### Test Context Pattern
Tests use a consistent `ParseContext` creation pattern:
```rust
let context = ParseContext::new(
    "test-session".to_string(),
    "test".to_string(),
    "/tmp/test.jsonl".to_string(),
);
```

## Summary

The current meta routing test structure provides:

1. **Comprehensive fixture coverage** with dedicated files for different routing scenarios
2. **Helper functions** for common test patterns (plugin creation, assertion helpers)
3. **Individual line tests** for precise verification of specific fixture lines
4. **File-level integration tests** for end-to-end fixture parsing
5. **Clear verification pattern** that ensures both parse success and empty result

The `session_start` and `session_end` tests specifically verify `Ok(Vec::new())` behavior through:
- Direct fixture line testing with exact line numbers from fixtures
- Helper function `assert_meta_routing_returns_empty()` that encapsulates the verification logic
- File-level tests that ensure entire fixtures with only meta types produce zero events

All fixture files are located in `tests/fixtures/envelope/` and follow the envelope routing structure with `{type, timestamp, payload}` format.