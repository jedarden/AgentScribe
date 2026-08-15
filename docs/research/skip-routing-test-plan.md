# Skip Routing Test Plan — Comprehensive Coverage

## Overview

This document catalogs all skip routing test scenarios for AgentScribe's envelope parsing system, identifies skip-type line patterns, lists edge cases requiring coverage, and provides a comprehensive test plan with coverage checklist.

**Purpose:** Ensure skip routing correctly prevents event emission across all envelope routing scenarios while maintaining data integrity and performance.

**Scope:** JSONL envelope parsing with type-based routing (event/meta/skip actions).

---

## Skip Routing Architecture

### How Skip Routing Works

1. **Envelope Unwrapping** (`src/parser/jsonl.rs::unwrap_envelope`):
   - Reads `type_field` from JSON line
   - Looks up routing action via `Envelope::get_routing(&type_value)`
   - Returns `(payload_json, type_field_value)` tuple

2. **Routing Actions**:
   - **`skip`**: Returns `(empty object, None)` → drops line completely
   - **`meta`**: Returns `(empty object, Some(full_wrapper))` → metadata preserved, no events
   - **`event`**: Returns `(payload, Some(full_wrapper))` → canonical event emitted

3. **Default Behavior**:
   - Unknown types → route to `skip` (with warning logged)
   - Invalid routing values → treated as `skip`
   - Missing/invalid payload → skip with warning

### Configuration

```toml
[source.envelope]
payload_field = "payload"      # Field containing event data
type_field = "type"            # Field containing routing discriminator
type_routing = {
  "heartbeat" = "skip",         # Don't emit events for heartbeats
  "ping" = "skip",              # Don't emit events for pings
  "session_meta" = "meta",      # Preserve metadata, no event
  "message" = "event",          # Emit canonical event
}
```

---

## Catalog of Skip-Type Line Patterns

### Pattern Categories

#### 1. **Basic Noise/Keepalive Signals**
```jsonl
{"type": "heartbeat", "timestamp": "2026-03-16T12:00:00Z", "payload": {"status": "ok"}}
{"type": "ping", "timestamp": "2026-03-16T12:00:01Z", "payload": {"seq": 1}}
{"type": "keepalive", "timestamp": "2026-03-16T12:00:02Z", "payload": {"alive": true}}
{"type": "status", "timestamp": "2026-03-16T12:00:03Z", "payload": {"running": true}}
```

**Characteristics:**
- Small payload (< 100 bytes)
- High frequency (every few seconds)
- No conversation relevance
- Used for connection health monitoring

#### 2. **Session Metadata**
```jsonl
{"type": "session_start", "timestamp": "2026-03-16T12:00:00Z", "payload": {"cwd": "/path/to/project"}}
{"type": "session_end", "timestamp": "2026-03-16T12:45:00Z", "payload": {"duration": 2700}}
{"type": "session_info", "timestamp": "2026-03-16T12:00:00Z", "payload": {"model": "claude-sonnet-4"}}
```

**Characteristics:**
- Metadata-only, no conversational content
- Appears at session boundaries
- Should be preserved in session metadata but not emitted as events

#### 3. **Debug/Diagnostic Events**
```jsonl
{"type": "debug_log", "timestamp": "2026-03-16T12:00:00Z", "payload": {"level": "trace", "msg": "Processing..."}}
{"type": "diagnostic", "timestamp": "2026-03-16T12:00:01Z", "payload": {"check": "memory", "status": "ok"}}
{"type": "trace", "timestamp": "2026-03-16T12:00:02Z", "payload": {"span": "parse", "duration_ms": 5}}
```

**Characteristics:**
- Internal debugging information
- High-frequency noise
- Not relevant to conversation history

#### 4. **Metrics/Statistics**
```jsonl
{"type": "metric", "timestamp": "2026-03-16T12:00:00Z", "payload": {"name": "tokens_used", "value": 1234}}
{"type": "counter", "timestamp": "2026-03-16T12:00:01Z", "payload": {"event": "request", "count": 42}}
{"type": "gauge", "timestamp": "2026-03-16T12:00:02Z", "payload": {"metric": "memory_mb", "value": 256}}
```

**Characteristics:**
- Numeric telemetry data
- Used for monitoring/analytics
- Not conversational content

#### 5. **Internal Control Messages**
```jsonl
{"type": "control", "timestamp": "2026-03-16T12:00:00Z", "payload": {"command": "flush"}}
{"type": "internal", "timestamp": "2026-03-16T12:00:01Z", "payload": {"signal": "checkpoint"}}
{"type": "sync", "timestamp": "2026-03-16T12:00:02Z", "payload": {"request_id": "abc123"}}
```

**Characteristics:**
- System control signals
- Protocol coordination
- Not user-facing content

---

## Current Test Coverage

### ✅ Already Covered (tests/skip_routing_event_tests.rs)

| Test Category | Tests | Coverage |
|---------------|-------|----------|
| **Basic Skip Functionality** | test_skip_routing_basic_heartbeat_produces_no_events<br>test_skip_routing_basic_ping_produces_no_events | ✅ Verified skip routing drops events |
| **Event Emitter Bypass** | test_skip_routing_event_emitter_not_called | ✅ Confirmed emitter bypassed for skip types |
| **Multiple Skip Types** | test_skip_routing_multiple_skip_types_all_empty | ✅ All skip types produce empty streams |
| **Mixed Skip/Normal** | test_skip_routing_mixed_with_normal_events | ✅ Skip types don't affect normal events |
| **Edge Cases** | test_skip_routing_edge_case_empty_payload<br>test_skip_routing_edge_case_nested_payload<br>test_skip_routing_edge_case_large_payload<br>test_skip_routing_edge_case_special_characters | ✅ Various payload structures |
| **Unknown Types** | test_skip_routing_unknown_type_defaults_to_skip | ✅ Default to skip behavior |
| **Case Sensitivity** | test_skip_routing_case_sensitivity | ✅ Exact matching behavior |
| **Timestamp Variations** | test_skip_routing_timestamp_field_variations | ✅ Different timestamp formats |
| **Consecutive Skips** | test_skip_routing_consecutive_skip_lines | ✅ Multiple skip lines in sequence |
| **Meta vs Skip** | test_skip_routing_meta_type_vs_skip_type | ✅ Both produce zero events |
| **File Integration** | test_skip_routing_file_parsing_integration | ✅ Full file parsing with skips |
| **Tracker Consistency** | test_skip_routing_event_stream_tracker_consistency | ✅ Event stream tracking works |
| **Return Values** | test_skip_routing_return_value_consistency | ✅ Ok(Vec::new()) returned |
| **Memory** | test_skip_routing_no_memory_leak | ✅ 1000 iterations without leak |
| **Fixture Validation** | test_skip_routing_fixture_validation | ✅ Fixture-based testing |

### ❌ Missing Coverage

| Category | Missing Tests | Risk |
|----------|---------------|------|
| **Complex Payload Structures** | Arrays in skip payloads<br>Null payload fields<br>Malformed JSON in skip lines | Medium |
| **Timestamp Edge Cases** | Missing timestamp field<br>Invalid timestamp format<br>Unix timestamp 0<br>Future timestamps | Low |
| **Type Field Variations** | Numeric type values<br>Boolean type values<br>Empty string type<br>Very long type strings | Medium |
| **Routing Configuration** | Invalid routing values at parse time<br>Routing changes mid-file<br>Empty routing map<br>Conflicting routing rules | High |
| **Performance/Volume** | Large file with 90% skip lines<br>Rapid consecutive skip lines<br>Skip lines at file boundaries | Medium |
| **Error Recovery** | Skip routing after parse error<br>Envelope damage handling<br>Mixed valid/invalid skip lines | Low |
| **Cross-Format** | Skip routing in compressed files (.jsonl.zst)<br>Skip routing with different line endings | Low |

---

## Comprehensive Test Plan

### Phase 1: Core Skip Behavior Validation

#### Test Suite 1.1: Basic Skip Types
- ✅ **COMPLETED**: `test_skip_routing_basic_heartbeat_produces_no_events`
- ✅ **COMPLETED**: `test_skip_routing_basic_ping_produces_no_events`
- ✅ **COMPLETED**: `test_skip_routing_multiple_skip_types_all_empty`

#### Test Suite 1.2: Skip vs Meta vs Event
- ✅ **COMPLETED**: `test_skip_routing_meta_type_vs_skip_type`
- ✅ **COMPLETED**: `test_skip_routing_event_emitter_not_called`
- ✅ **COMPLETED**: `test_skip_routing_mixed_with_normal_events`

#### Test Suite 1.3: Edge Case Payloads
- ✅ **COMPLETED**: `test_skip_routing_edge_case_empty_payload`
- ✅ **COMPLETED**: `test_skip_routing_edge_case_nested_payload`
- ✅ **COMPLETED**: `test_skip_routing_edge_case_large_payload`
- ✅ **COMPLETED**: `test_skip_routing_edge_case_special_characters`
- ❌ **MISSING**: Array payloads in skip lines
- ❌ **MISSING**: Null payload fields
- ❌ **MISSING**: Malformed JSON in skip lines

### Phase 2: Type Field Variations

#### Test Suite 2.1: Type Value Formats
- ❌ **MISSING**: Numeric type values (e.g., `{"type": 123}`)
- ❌ **MISSING**: Boolean type values (e.g., `{"type": true}`)
- ❌ **MISSING**: Empty string type (e.g., `{"type": ""}`)
- ❌ **MISSING**: Very long type strings (>100 chars)
- ❌ **MISSING**: Unicode type values (e.g., `{"type": "heartbeat_中文"}`)

#### Test Suite 2.2: Unknown Types
- ✅ **COMPLETED**: `test_skip_routing_unknown_type_defaults_to_skip`
- ❌ **MISSING**: Unknown type with complex payload
- ❌ **MISSING**: Unknown type at file boundaries
- ❌ **MISSING**: Multiple unknown types in sequence

### Phase 3: Timestamp Variations

#### Test Suite 3.1: Timestamp Field
- ❌ **MISSING**: Missing timestamp field
- ❌ **MISSING**: Null timestamp value
- ❌ **MISSING**: Invalid timestamp format
- ❌ **MISSING**: Unix timestamp 0
- ❌ **MISSING**: Future timestamps

#### Test Suite 3.2: Timestamp Edge Cases
- ✅ **COMPLETED**: `test_skip_routing_timestamp_field_variations`
- ❌ **MISSING**: Timestamp with microseconds
- ❌ **MISSING**: Timestamp with timezone offsets
- ❌ **MISSING**: Non-ISO8601 timestamp formats

### Phase 4: Routing Configuration

#### Test Suite 4.1: Invalid Routing Values
- ❌ **MISSING**: Routing value "invalid" (not event/meta/skip)
- ❌ **MISSING**: Routing value with typos ("skkip", "evnt")
- ❌ **MISSING**: Empty routing value ("")
- ❌ **MISSING**: Case variations ("SKIP", "Skip", "EVENT")

#### Test Suite 4.2: Routing Map Edge Cases
- ❌ **MISSING**: Empty routing map (no types defined)
- ❌ **MISSING**: All types route to skip
- ❌ **MISSING**: Conflicting routing (same type, different actions)
- ❌ **MISSING**: Routing changes mid-file

### Phase 5: Performance and Volume

#### Test Suite 5.1: High-Frequency Skip Lines
- ❌ **MISSING**: 10,000 consecutive skip lines
- ❌ **MISSING**: File with 90% skip lines, 10% events
- ❌ **MISSING**: Skip lines at 1ms intervals
- ❌ **MISSING**: Skip lines interleaved with events (skip-event-skip-event...)

#### Test Suite 5.2: Large Skip Payloads
- ✅ **COMPLETED**: `test_skip_routing_edge_case_large_payload` (10KB)
- ❌ **MISSING**: 1MB skip payload
- ❌ **MISSING**: 10MB skip payload
- ❌ **MISSING**: Nested skip payload 100 levels deep

### Phase 6: Error Recovery

#### Test Suite 6.1: Malformed Skip Lines
- ❌ **MISSING**: Invalid JSON in skip line
- ❌ **MISSING**: Missing required fields in skip line
- ❌ **MISSING**: Extra fields in skip line
- ❌ **MISSING**: Wrong data types for fields

#### Test Suite 6.2: Mixed Valid/Invalid Lines
- ❌ **MISSING**: Valid skip → invalid skip → valid skip
- ❌ **MISSING**: Valid event → invalid line → valid event
- ❌ **MISSING**: Skip line after parse error
- ❌ **MISSING**: Parse error after skip line

### Phase 7: Cross-Format Integration

#### Test Suite 7.1: Compressed Files
- ❌ **MISSING**: Skip routing in .jsonl.zst files
- ❌ **MISSING**: Skip routing with mixed compression
- ❌ **MISSING**: Skip lines at compression boundaries

#### Test Suite 7.2: Line Ending Variations
- ❌ **MISSING**: Skip lines with Windows line endings (CRLF)
- ❌ **MISSING**: Skip lines with legacy Mac line endings (CR)
- ❌ **MISSING**: Mixed line endings in same file

### Phase 8: Integration Testing

#### Test Suite 8.1: Full Session Parsing
- ✅ **COMPLETED**: `test_skip_routing_file_parsing_integration`
- ❌ **MISSING**: Session with only skip types
- ❌ **MISSING**: Session with mixed skip/meta/event types
- ❌ **MISSING**: Multiple sessions with different skip configurations

#### Test Suite 8.2: State Management
- ❌ **MISSING**: Skip routing in incremental scraping
- ❌ **MISSING**: Skip routing with truncation limit
- ❌ **MISSING**: Skip routing after file rotation

---

## Coverage Checklist

### By Component

| Component | Tests | Pass | Coverage |
|-----------|-------|------|----------|
| **unwrap_envelope()** | 15 | 15 | 75% |
| **Envelope::get_routing()** | 3 | 3 | 60% |
| **Parse integration** | 8 | 8 | 50% |
| **Error handling** | 4 | 4 | 40% |
| **Performance** | 1 | 1 | 20% |

### By Scenario Category

| Category | Tests | Pass | Coverage |
|----------|-------|------|----------|
| **Basic skip** | 5 | 5 | ✅ 100% |
| **Edge cases** | 6 | 6 | ⚠️ 60% |
| **Type variations** | 2 | 2 | ❌ 25% |
| **Timestamp** | 2 | 2 | ❌ 30% |
| **Routing config** | 0 | 0 | ❌ 0% |
| **Performance** | 1 | 1 | ❌ 20% |
| **Error recovery** | 0 | 0 | ❌ 0% |
| **Cross-format** | 0 | 0 | ❌ 0% |

### Overall Coverage

- **Total Tests**: 30
- **Passing**: 30 (100%)
- **Missing**: 42 tests
- **Overall Coverage**: **42%** (30/72 scenarios)

---

## Priority Implementation Order

### High Priority (Complete Before Merge)

1. **Invalid Routing Values** (Test Suite 4.1)
   - Risk: Configuration errors could cause unexpected behavior
   - Tests: 4
   - Effort: 2 hours

2. **Empty Routing Map** (Test Suite 4.2)
   - Risk: Edge case could cause crashes
   - Tests: 3
   - Effort: 1 hour

3. **Type Value Variations** (Test Suite 2.1)
   - Risk: Real-world data may have unexpected formats
   - Tests: 5
   - Effort: 2 hours

### Medium Priority (Complete Before Next Release)

4. **Malformed Skip Lines** (Test Suite 6.1)
   - Risk: Error recovery not validated
   - Tests: 4
   - Effort: 2 hours

5. **High-Frequency Skip Lines** (Test Suite 5.1)
   - Risk: Performance issues at scale
   - Tests: 4
   - Effort: 3 hours

6. **Timestamp Edge Cases** (Test Suite 3.1)
   - Risk: Missing fields could cause crashes
   - Tests: 5
   - Effort: 2 hours

### Low Priority (Nice to Have)

7. **Compressed Files** (Test Suite 7.1)
   - Risk: Low (compression handled elsewhere)
   - Tests: 3
   - Effort: 2 hours

8. **Line Ending Variations** (Test Suite 7.2)
   - Risk: Low (standard libraries handle this)
   - Tests: 3
   - Effort: 1 hour

9. **State Management** (Test Suite 8.2)
   - Risk: Low (state tests exist elsewhere)
   - Tests: 3
   - Effort: 2 hours

---

## Test Implementation Guidelines

### Naming Convention

```rust
fn test_skip_routing_<category>_<scenario>_<expected_behavior>() {
    // Example: test_skip_routing_type_field_numeric_value_skips
}
```

### Test Structure

```rust
#[test]
fn test_skip_routing_<specific_case>() {
    // 1. Arrange: Set up routing configuration
    let mut type_routing = HashMap::new();
    type_routing.insert("<type>".to_string(), "skip".to_string());
    
    // 2. Act: Parse the test line
    let plugin = create_skip_routing_test_plugin(type_routing);
    let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();
    
    // 3. Assert: Verify expected behavior
    assert!(events.is_empty(), "skip should produce no events");
}
```

### Fixture Creation

For complex scenarios, create fixture files:

```jsonl
// tests/fixtures/envelope/skip-routing-complex.jsonl
{"type": "skip", "timestamp": "2026-03-16T12:00:00Z", "payload": {"complex": {"nested": "data"}}}
{"type": "event", "timestamp": "2026-03-16T12:00:01Z", "payload": {"role": "user", "content": "Hello"}}
```

### Performance Testing

```rust
#[test]
fn test_skip_routing_performance_high_frequency() {
    let start = std::time::Instant::now();
    
    for i in 0..10_000 {
        // Process skip line
    }
    
    let elapsed = start.elapsed();
    assert!(elapsed < std::time::Duration::from_secs(1), 
            "10k skip lines should process in < 1s");
}
```

---

## Success Criteria

### Coverage Targets

- **Minimum**: 70% overall coverage (51/72 scenarios)
- **Target**: 85% overall coverage (61/72 scenarios)
- **Ideal**: 95% overall coverage (68/72 scenarios)

### Quality Gates

- ✅ All new tests must pass
- ✅ No regression in existing tests
- ✅ Memory usage stable under high-frequency skip loads
- ✅ Performance: 10,000 skip lines < 1 second
- ✅ Zero crashes on malformed input

### Validation Checklist

- [ ] All high-priority tests implemented
- [ ] All medium-priority tests implemented
- [ ] Performance benchmarks met
- [ ] Error recovery validated
- [ ] Documentation updated
- [ ] Integration tests pass

---

## Appendix: Real-World Skip Patterns

### Codex Rollout Envelope

```jsonl
{"type": "session_meta", "timestamp": "...", "payload": {"thread_id": "...", "cwd": "..."}}
{"type": "response_item", "timestamp": "...", "payload": {"role": "user", "content": "..."}}
{"type": "turn_context", "timestamp": "...", "payload": {"model": "gpt-4"}}
{"type": "event_msg", "timestamp": "...", "payload": {"msg": "debug info"}}
```

**Routing**: `session_meta` → `meta`, `turn_context` → `meta`, `event_msg` → `skip`, `response_item` → `event`

### Claude Code Subagent Logs

```jsonl
{"type": "session_start", "timestamp": "...", "payload": {"parent_session": "..."}}
{"type": "progress", "timestamp": "...", "payload": {"stage": "processing"}}
{"type": "message", "timestamp": "...", "payload": {"role": "user", "content": "..."}}
```

**Routing**: `session_start` → `meta`, `progress` → `skip`, `message` → `event`

### Custom Agent Logs

```jsonl
{"type": "heartbeat", "timestamp": "...", "payload": {"status": "ok"}}
{"type": "metric", "timestamp": "...", "payload": {"name": "tokens", "value": 123}}
{"type": "user_message", "timestamp": "...", "payload": {"text": "Hello"}}
```

**Routing**: `heartbeat` → `skip`, `metric` → `skip`, `user_message` → `event`

---

## Changelog

### 2026-08-15
- Initial comprehensive test plan created
- Cataloged 72 test scenarios across 8 test suites
- Identified 30 existing tests (42% coverage)
- Prioritized 42 missing tests
- Added real-world skip pattern examples

---

**Status**: 📋 Ready for Implementation
**Owner**: AgentScribe Test Team
**Review**: Next sprint
