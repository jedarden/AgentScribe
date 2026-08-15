# Skip Routing Test Plan

**Purpose:** Document all skip routing scenarios, catalog skip-type line patterns, and provide comprehensive test coverage for AgentScribe's envelope-based filtering system.

**Generated:** 2026-08-15
**Bead:** agentscr-7784ea15

---

## Overview

Skip routing is a feature in AgentScribe's JSONL parser that allows selective filtering of log lines based on their envelope type. When a JSONL line contains a "wrapped" event (an envelope structure with `{type_field, payload_field}`), the system can route different types of lines to different handlers: `"event"`, `"meta"`, or `"skip"`.

### Key Locations

- **Core Implementation:** `src/parser/jsonl.rs` (lines 44-129, 150-260)
- **Configuration:** `src/plugin.rs` (lines 150-195)
- **Error Strategy:** `src/error.rs` (lines 17-36, 72-81, 177-180)
- **Tests:** `src/parser/jsonl.rs` (lines 1146+)

---

## Skip Routing Scenarios

### 1. Explicit Skip Routing

**Description:** Lines with types explicitly mapped to `"skip"` in the TOML configuration are dropped immediately.

**Behavior:**
- Returns `Ok(Vec::new())` - no errors, no events produced
- Line is not counted in session metrics
- Processing continues to next line

**Real-World Examples:**
```json
{"type": "heartbeat", "timestamp": "2026-07-04T10:00:05Z", "payload": {"status": "ok"}}
{"type": "ping", "timestamp": "2026-07-04T10:00:10Z", "payload": {"seq": 123}}
{"type": "system_noise", "timestamp": "2026-07-04T10:00:15Z", "payload": {"level": "debug"}}
```

**Configuration:**
```toml
[source.envelope.type_routing]
"heartbeat" = "skip"
"ping" = "skip"
"system_noise" = "skip"
```

**Test Cases:**
- ✅ Basic skip routing returns empty Vec
- ✅ Multiple consecutive skip lines
- ✅ Skip routing bypasses event construction
- ✅ Skip routing does not affect session metrics

---

### 2. Implicit Skip (Unknown Types)

**Description:** Types not present in the routing map default to `"skip"` with a warning logged.

**Behavior:**
- Returns `Ok(Vec::new())` - no events produced
- Logs warning: `"Unknown envelope type value, routing to 'skip'"`
- Prevents crashes from unexpected/rogue data

**Real-World Examples:**
```json
{"type": "new_feature_not_yet_supported", "timestamp": "2026-07-04T10:00:00Z", "payload": {"data": "..."}}
{"type": "typo_in_type_field", "timestamp": "2026-07-04T10:00:00Z", "payload": {"msg": "..."}}
```

**Test Cases:**
- ✅ Unknown types default to skip
- ✅ Unknown type logs warning message
- ✅ Multiple unknown types in sequence
- ✅ Unknown type surrounded by known types

---

### 3. Meta Routing (Session Metadata)

**Description:** Types mapped to `"meta"` return empty Vec (no events) but preserve metadata for future accumulation.

**Current Behavior:**
- Returns `Ok(Vec::new())` - no events produced currently
- Lines are dropped but marked as metadata (TODO: future session context accumulation)
- No warning logged

**Future Enhancement:**
- Accumulate session-level metadata (project, model, version)
- Envelope JSON preserved for context extraction

**Real-World Examples:**
```json
{"type": "session_start", "timestamp": "2026-07-04T10:00:00Z", "payload": {"session_id": "abc123", "model": "claude-sonnet-5"}}
{"type": "session_metadata", "timestamp": "2026-07-04T10:00:01Z", "payload": {"cwd": "/home/user/project"}}
```

**Configuration:**
```toml
[source.envelope.type_routing]
"session_start" = "meta"
"session_metadata" = "meta"
```

**Test Cases:**
- ✅ Meta routing returns empty Vec
- ✅ Meta routing does not log warning
- ✅ Mixed meta/event/skip routing
- 🔄 Meta accumulation (future test when implemented)

---

### 4. Skip-and-Log Error Strategy

**Description:** Parser errors do not block scraping of other files/sessions. Errors are logged with file path and line number, then processing continues.

**Behavior:**
- Returns `Err(AgentScribeError::Parse)` for the specific line
- Error logged with context: file path, line number, specific message
- Scraper continues to next line/file/session
- No crashes from malformed data

**Real-World Examples:**
```jsonl
{"type": "message", "timestamp": "2026-07-04T10:00:00Z", "payload": {"role": "user"}}  // Missing content field
invalid json line here
{"type": "message", "timestamp": "malformed", "payload": {"role": "user", "content": "hi"}}
```

**Test Cases:**
- ✅ Invalid JSON line returns error with line number
- ✅ Malformed timestamp returns parse error
- ✅ Missing required field returns parse error
- ✅ Error includes file path in message
- ✅ Processing continues after error

---

### 5. Invalid JSON Lines (Companion Files)

**Description:** In companion index files (e.g., `~/.codex/session_index.jsonl`), invalid JSON lines are silently skipped.

**Behavior:**
- Invalid JSON lines produce no events
- No error logged (companion files are best-effort)
- Processing continues to next line

**Real-World Example:**
```jsonl
{"thread_id": "abc123", "cwd": "/home/user/project"}
corrupted line here
{"thread_id": "def456", "cwd": "/home/user/other"}
```

**Test Cases:**
- ✅ Companion index skips invalid lines silently
- ✅ Valid lines after invalid are processed
- ✅ Empty companion file handled gracefully

---

### 6. Fast Pre-Filter (Non-Assistant Events)

**Description:** In capacity.rs, lines that cannot be assistant events are skipped quickly before expensive processing.

**Behavior:**
- Pre-filter check: `model == "<synthetic>"` → skip
- Pre-filter check: all-zero token counts → skip
- Avoids unnecessary processing of non-conversational data

**Real-World Examples:**
```json
{"model": "<synthetic>", "tokens": {"input": 0, "output": 0}, "role": "assistant", "content": "..."}
{"model": "claude-sonnet-5", "tokens": {"input": 0, "output": 0}, "role": "assistant", "content": "..."}
```

**Test Cases:**
- ✅ Synthetic model lines are skipped
- ✅ All-zero token counts are skipped
- ✅ Valid events after pre-filter are processed

---

### 7. Type-Based Filtering (Include/Exclude)

**Description:** Plugins can define `include_types` and `exclude_types` filters that skip events based on their type field value.

**Configuration:**
```toml
[parser.include_types]
field = "type"
values = ["user", "assistant", "tool_call"]

[parser.exclude_types]
field = "type"
values = ["system", "debug"]
```

**Behavior:**
- `include_types`: Only process events with matching type values
- `exclude_types`: Skip events with matching type values
- Envelope-aware: `^` prefix reads from wrapper, otherwise from payload

**Test Cases:**
- ✅ Include types filter - only matching types processed
- ✅ Exclude types filter - matching types skipped
- ✅ Envelope-aware type filtering with `^` prefix
- ✅ Both include and exclude filters applied

---

## Skip-Type Line Patterns

### Pattern 1: Heartbeat/Keep-Alive Signals

**Structure:**
```json
{
  "type": "heartbeat",
  "timestamp": "ISO-8601",
  "payload": {
    "status": "ok",
    "seq": 123
  }
}
```

**Variants:**
- `heartbeat`, `keepalive`, `ping`, `pong`
- May include sequence numbers, timestamps, status codes

**Routing Action:** `"skip"`

**Test Fixtures:**
- `tests/fixtures/envelope/heartbeat.jsonl`

---

### Pattern 2: System/Debug Messages

**Structure:**
```json
{
  "type": "system_log",
  "timestamp": "ISO-8601",
  "payload": {
    "level": "debug",
    "message": "connection pool stats"
  }
}
```

**Variants:**
- `system_log`, `debug`, `trace`, `verbose`
- May include log levels, stack traces (non-error)

**Routing Action:** `"skip"`

**Test Fixtures:**
- `tests/fixtures/envelope/system-noise.jsonl`

---

### Pattern 3: Empty/Null Type Field

**Structure:**
```json
{
  "type": null,
  "timestamp": "ISO-8601",
  "payload": {"role": "user", "content": "hello"}
}
```

**Variants:**
- `type` field missing entirely
- `type` field is `null`
- `type` field is empty string `""`

**Routing Action:** Implicit skip (unknown type defaults to skip)

**Test Cases:**
- ✅ Null type field defaults to skip
- ✅ Missing type field defaults to skip
- ✅ Empty string type defaults to skip

---

### Pattern 4: Wrong Data Type for Type Field

**Structure:**
```json
{
  "type": 123,
  "timestamp": "ISO-8601",
  "payload": {"role": "user", "content": "hello"}
}
```

**Variants:**
- `type` field is a number
- `type` field is a boolean
- `type` field is an array or object

**Routing Action:** Implicit skip (converted to string, then unknown)

**Test Cases:**
- ✅ Number type field converted to string, then unknown
- ✅ Boolean type field converted to string, then unknown
- ✅ Array type field converted to string, then unknown

---

### Pattern 5: Missing or Invalid Payload Field

**Structure:**
```json
{
  "type": "message",
  "timestamp": "ISO-8601",
  "payload": "this is a string, not an object"
}
```

**Variants:**
- `payload_field` missing entirely
- `payload_field` is `null`
- `payload_field` is a string, number, boolean, or array

**Routing Action:** Skip with warning (specific message based on what was found)

**Warning Messages:**
- `"Envelope payload_field 'payload' missing for type 'message', skipping line"`
- `"Envelope payload_field 'payload' exists for type 'message' but is not an object (found: string '...'), skipping line"`

**Test Cases:**
- ✅ Missing payload field logs appropriate warning
- ✅ String payload logs appropriate warning with truncation
- ✅ Null payload logs appropriate warning
- ✅ Number/bool/array payload logs appropriate warning

---

### Pattern 6: Malformed JSON

**Structure:**
```jsonl
{"type": "message", "timestamp": "2026-07-04T10:00:00Z", "payload": {"role": "user"}}  // Missing comma
{"type": "message", "timestamp": "2026-07-04T10:00:00Z", "payload": {"role": "user", "content": "hello"}}
```

**Variants:**
- Missing commas, brackets, quotes
- Trailing commas
- Unescaped characters in strings

**Routing Action:** Parse error (skip-and-log strategy)

**Test Cases:**
- ✅ Malformed JSON returns parse error with line number
- ✅ Next valid line after malformed is processed
- ✅ Multiple malformed lines in sequence

---

## Edge Cases

### Edge Case 1: Empty Type Field Value

**Scenario:** Type field exists but is empty string `""`

**Expected Behavior:**
- Empty string is used as type value
- `get_routing("")` returns `"skip"` (unknown type)
- Warning logged: `"Unknown envelope type value, routing to 'skip'"`

**Test:** `test_empty_type_field_skips`

---

### Edge Case 2: Missing Type Field

**Scenario:** Type field not present in JSON object

**Expected Behavior:**
- `extract_string()` returns `None`
- Defaults to empty string `""`
- Handled as empty type field case above

**Test:** `test_missing_type_field_skips`

---

### Edge Case 3: Multiple Consecutive Skip Lines

**Scenario:** File contains 100+ consecutive skip-type lines

**Expected Behavior:**
- All skip lines return empty Vec
- No events produced
- No memory accumulation
- Processing completes without errors

**Test:** `test_many_consecutive_skip_lines`

---

### Edge Case 4: All Lines Are Skip-Type

**Scenario:** Entire file contains only skip-type lines

**Expected Behavior:**
- Session is created with zero events
- Session file is not written (empty sessions are skipped)
- No error raised

**Test:** `test_all_lines_are_skip_type`

---

### Edge Case 5: Mixed Routing in Single File

**Scenario:** File contains interleaved skip, meta, and event lines

**Example:**
```jsonl
{"type": "heartbeat", ...}  // skip
{"type": "message", ...}  // event
{"type": "session_start", ...}  // meta
{"type": "ping", ...}  // skip
{"type": "message", ...}  // event
```

**Expected Behavior:**
- Only event-type lines produce events
- Skip and meta lines return empty Vec
- Events maintain chronological order

**Test:** `test_mixed_skip_meta_event_routing`

---

### Edge Case 6: Type Routing Value Validation

**Scenario:** TOML configuration has invalid routing value (not `"event"`, `"meta"`, or `"skip"`)

**Example:**
```toml
[source.envelope.type_routing]
"message" = "invalid_routing_value"
```

**Expected Behavior:**
- Plugin validation fails at load time
- Error message: `"Invalid envelope routing action 'invalid_routing_value' for type 'message': must be one of 'event', 'meta', 'skip'"`
- Scraper does not start

**Test:** `test_invalid_routing_value_fails_validation`

---

### Edge Case 7: Envelope Field Extraction with `^` Prefix

**Scenario:** Parser field config uses `^` prefix to read from envelope instead of payload

**Example:**
```toml
[parser]
timestamp = "^timestamp"  # Read from envelope, not payload
role = "message.role"     # Read from payload
```

**Expected Behavior:**
- `^timestamp` reads from envelope_json
- `message.role` reads from payload_json
- Correct values extracted from each source

**Test:** `test_envelope_field_extraction_with_caret_prefix`

---

### Edge Case 8: Payload Field Shadowing

**Scenario:** Same field name exists in both envelope and payload with `^` prefix

**Example:**
```json
{
  "timestamp": "2026-07-04T10:00:00Z",
  "type": "message",
  "payload": {
    "timestamp": "2026-07-04T10:00:05Z",
    "role": "user",
    "content": "hello"
  }
}
```

**Config:**
```toml
[parser]
timestamp = "^timestamp"  # Should get envelope value: 10:00:00Z
```

**Expected Behavior:**
- `^timestamp` returns envelope value (`10:00:00Z`)
- Regular `timestamp` would return payload value (`10:00:05Z`)
- `^` prefix always wins for envelope fields

**Test:** `test_envelope_field_shadows_payload`

---

### Edge Case 9: Skip Routing Performance

**Scenario:** File with 10,000 lines, 50% are skip-type

**Expected Behavior:**
- Processing completes in reasonable time (<5 seconds)
- Memory use remains bounded
- No performance degradation from skip line density

**Test:** `test_skip_routing_performance`

---

### Edge Case 10: Unicode/Non-ASCII Type Values

**Scenario:** Type field contains non-ASCII characters

**Examples:**
```json
{"type": "状态", "timestamp": "...", "payload": {...}}
{"type": "статус", "timestamp": "...", "payload": {...}}
{"type": "🔄", "timestamp": "...", "payload": {...}}
```

**Expected Behavior:**
- Type values are compared as strings (UTF-8)
- Unicode type values route correctly
- Unknown Unicode types default to skip with warning

**Test:** `test_unicode_type_values`

---

## Test Coverage Checklist

### Core Functionality

- [x] Basic skip routing returns empty Vec
- [x] Unknown types default to skip with warning
- [x] Meta routing returns empty Vec (no warning)
- [x] Event routing produces events
- [x] Multiple consecutive skip lines
- [x] Skip routing bypasses event construction
- [x] Mixed skip/meta/event routing

### Error Handling

- [x] Invalid JSON returns parse error with line number
- [x] Malformed timestamp returns parse error
- [x] Missing required field returns parse error
- [x] Error includes file path in message
- [x] Processing continues after error
- [x] Companion index skips invalid lines silently

### Type Field Variations

- [x] Null type field defaults to skip
- [x] Missing type field defaults to skip
- [x] Empty string type defaults to skip
- [x] Number type field converted to string
- [x] Boolean type field converted to string
- [x] Array/object type field converted to string
- [x] Unicode type values handled correctly

### Payload Field Variations

- [x] Missing payload field logs warning
- [x] String payload logs warning with truncation
- [x] Null payload logs warning
- [x] Number/bool/array payload logs warning
- [x] Valid object payload produces events

### Envelope Field Extraction

- [x] `^` prefix reads from envelope
- [x] Without `^` reads from payload
- [x] Envelope field shadows payload field
- [x] Mixed envelope/priority field extraction

### Configuration Validation

- [x] Invalid routing value fails validation
- [x] Missing type_field configuration
- [x] Missing payload_field configuration
- [x] Empty type_routing map (all unknown)

### Integration Tests

- [x] Complete fixture with mixed routing types
- [x] Skip-only fixture produces no events
- [x] Meta-only fixture produces no events
- [x] Event-only fixture produces all events
- [x] Large file with skip routing (performance)

### Edge Cases

- [ ] All lines are skip-type (no session created)
- [ ] 100+ consecutive skip lines (memory test)
- [ ] Skip routing with envelope field extraction
- [ ] Unknown type surrounded by known types
- [ ] Multiple unknown types in sequence
- [ ] Type-based filtering (include/exclude)

---

## Test Fixtures

### Fixture 1: Mixed Routing Types

**File:** `tests/fixtures/envelope_test.jsonl`

**Content:**
```jsonl
{"type": "session_start", "timestamp": "2026-07-04T10:00:00Z", "payload": {"session_id": "abc123"}}
{"type": "heartbeat", "timestamp": "2026-07-04T10:00:05Z", "payload": {"status": "ok"}}
{"type": "message", "timestamp": "2026-07-04T10:00:10Z", "payload": {"role": "user", "content": "hello"}}
{"type": "ping", "timestamp": "2026-07-04T10:00:15Z", "payload": {"seq": 1}}
{"type": "message", "timestamp": "2026-07-04T10:00:20Z", "payload": {"role": "assistant", "content": "hi there"}}
{"type": "unknown_type", "timestamp": "2026-07-04T10:00:25Z", "payload": {"data": "..."}}
```

**Config:** `tests/fixtures/envelope_test.toml`

**Expected Results:**
- 2 events produced (message types)
- 4 lines skipped (session_start, heartbeat, ping, unknown_type)
- 1 warning logged (unknown_type)

---

### Fixture 2: Skip-Only File

**File:** `tests/fixtures/envelope/skip-only.jsonl`

**Content:**
```jsonl
{"type": "heartbeat", "timestamp": "2026-07-04T10:00:00Z", "payload": {"status": "ok"}}
{"type": "ping", "timestamp": "2026-07-04T10:00:05Z", "payload": {"seq": 1}}
{"type": "system", "timestamp": "2026-07-04T10:00:10Z", "payload": {"level": "debug"}}
```

**Expected Results:**
- 0 events produced
- 3 lines skipped
- No warnings (all are known skip types)

---

### Fixture 3: Invalid Payloads

**File:** `tests/fixtures/envelope/invalid-payloads.jsonl`

**Content:**
```jsonl
{"type": "message", "timestamp": "2026-07-04T10:00:00Z", "payload": null}
{"type": "message", "timestamp": "2026-07-04T10:00:05Z", "payload": "string payload"}
{"type": "message", "timestamp": "2026-07-04T10:00:10Z", "payload": 123}
{"type": "message", "timestamp": "2026-07-04T10:00:15Z", "payload": ["array"]}
{"type": "message", "timestamp": "2026-07-04T10:00:20Z", "payload": {"role": "user", "content": "valid"}}
```

**Expected Results:**
- 1 event produced (last line only)
- 4 warnings logged (specific to each payload type)
- All warnings include type value ("message")

---

## Implementation Notes

### Key Functions

1. **`unwrap_envelope()`** (`src/parser/jsonl.rs:44-129`)
   - Extracts payload based on routing action
   - Returns `(Value, Option<Value>)` tuple
   - Handles skip/meta/event routing

2. **`parse_line()`** (`src/parser/jsonl.rs:150-260`)
   - Main parser entry point
   - Applies envelope routing
   - Returns `Result<Vec<Event>>`

3. **`get_routing()`** (`src/plugin.rs:162-180`)
   - Looks up routing action for type value
   - Returns `"event"`, `"meta"`, `"skip"`, or defaults to `"skip"`
   - Logs warning for unknown types

### Performance Considerations

- Skip routing happens **before** expensive event construction
- Early return (`Ok(Vec::new())`) avoids unnecessary processing
- No memory allocation for skipped lines
- Suitable for high-frequency noise (heartbeats, pings)

### Future Enhancements

1. **Meta Accumulation:** Currently meta lines return empty Vec. Future: accumulate session metadata (project, model, version) for context.

2. **Skip Metrics:** Track skip counts per type for analytics (e.g., "1000 heartbeat lines skipped").

3. **Conditional Skip:** Skip based on content patterns (e.g., skip heartbeat if no payload changes).

---

## Related Documentation

- **Plugin Schema:** `plugins/BUILDING_PLUGINS.md` - Envelope configuration reference
- **CLI Reference:** `cli-reference.md` - Plugin validation commands
- **Implementation Plan:** `docs/plan.md` - Phase 9 envelope unwrapping design

---

## Summary

Skip routing is a critical performance and noise-reduction feature in AgentScribe's JSONL parser. By filtering out non-conversational data (heartbeats, pings, system noise) before expensive event construction, the system can efficiently process mixed-content log files from real-world coding agents.

The test plan above covers:
- **7 skip routing scenarios** (explicit, implicit, meta, error handling, etc.)
- **6 skip-type line patterns** (heartbeat, system noise, empty types, etc.)
- **10 edge cases** (empty types, missing fields, Unicode, performance, etc.)
- **25+ test coverage items** with status indicators

Existing tests in `src/parser/jsonl.rs` (lines 1146+) already cover most core scenarios. This plan documents the gaps and ensures comprehensive coverage for future enhancements.
