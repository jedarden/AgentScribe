# Skip Routing Implementation Review

**Date:** 2026-08-16  
**Scope:** Complete review of all skip mechanisms in AgentScribe codebase  
**Files Reviewed:** 27 files containing skip-related code

---

## Executive Summary

AgentScribe implements **seven distinct skip mechanisms** across its parsing, indexing, and enrichment pipeline. The primary skip mechanism is **envelope type-based routing** for JSONL formats, which filters unwanted log lines at parse time through declarative plugin configuration.

---

## Skip Mechanisms Identified

### 1. Envelope Type-Based Skip Routing (Primary)

**Location:** `src/plugin.rs:160-194`, `src/parser/jsonl.rs:200-291`

**Purpose:** Filter JSONL lines based on envelope type field for agents like Codex that wrap events in `{type, payload}` structures.

**Configuration:**
```toml
[source.envelope]
payload_field = "payload"
type_field = "type"
type_routing = {
    "message" = "event",           # Parse into canonical events
    "session_start" = "meta",      # Accumulate metadata (TODO)
    "heartbeat" = "skip",          # Drop the line
    "ping" = "skip",               # Drop the line
}
```

**Implementation:**
```rust
// src/parser/jsonl.rs:218-291
let type_value = extract_string(&raw_json, &envelope_cfg.type_field).unwrap_or_default();
let routing = envelope_cfg.get_routing(&type_value);

match routing {
    "skip" => {
        // Line dropped - no event emitted
        return Ok(Vec::new());
    }
    "meta" => {
        // Metadata line - no event emitted
        // TODO: Future session metadata accumulation
        return Ok(Vec::new());
    }
    "event" => {
        // Extract payload from payload_field and parse
    }
}
```

**Routing Actions:**
- **"event"**: Extract payload from `payload_field`, parse into canonical Event objects
- **"meta"**: Return empty Vec (future: accumulate session-level metadata)
- **"skip"**: Return empty Vec immediately

**Default Behavior:** Unknown types default to "skip" with logged warning.

**Code Flow for Skip-Type Lines:**
1. Plugin defines `[source.envelope]` with `type_field`, `payload_field`, `type_routing`
2. JSONL line parsed: `{type: "heartbeat", timestamp: "...", payload: {...}}`
3. `get_routing("heartbeat")` → looks up in HashMap → returns "skip"
4. `parse_line()` returns `Ok(Vec::new())` - line dropped, no event emitted
5. No errors, no warnings - line simply filtered out

**Test Coverage:** Comprehensive (20+ tests in jsonl.rs)
- `test_parse_line_envelope_skip_routing` - skip routing produces zero events
- `test_skip_type_routing_heartbeat_and_ping_produce_zero_events` - fixture-based validation
- `test_skip_only_fixture_routing_integration` - end-to-end skip-only fixture parsing
- `test_unwrap_envelope_skip_type_returns_empty_and_none` - unit tests for skip logic

---

### 2. Include/Exclude Type Filters

**Location:** `src/parser/jsonl.rs:298-318`

**Purpose:** Filter events by type field values after envelope routing.

**Configuration:**
```toml
[parser.include_types]
field = "type"
values = ["message", "tool_call"]

[parser.exclude_types]
field = "type"
values = ["heartbeat", "ping"]
```

**Implementation:**
```rust
// Include filter
if let Some(ref filter) = plugin.parser.include_types {
    let type_field = &filter.field;
    if let Some(type_val) = extract_string_with_envelope(type_field, payload_json, envelope_json) {
        if !filter.values.contains(&type_val) {
            return Ok(Vec::new()); // Skip this event
        }
    }
}

// Exclude filter
if let Some(ref filter) = plugin.parser.exclude_types {
    let type_field = &filter.field;
    if let Some(type_val) = extract_string_with_envelope(type_field, payload_json, envelope_json) {
        if filter.values.contains(&type_val) {
            return Ok(Vec::new()); // Skip this event
        }
    }
}
```

**Envelope-Aware:** Uses `extract_string_with_envelope()` to support `^` prefix for reading from wrapper vs payload.

---

### 3. Error-Based Skipping

**Location:** `src/error.rs`, used in `src/parser/jsonl.rs:575-580`

**Purpose:** Allow graceful handling of non-fatal parse errors.

**Implementation:**
```rust
// src/parser/jsonl.rs:575-580
match JsonlParser::parse_line(&line, line_num, &context, plugin) {
    Ok(mut line_events) => events.append(&mut line_events),
    Err(e) => {
        if e.is_skippable() {
            eprintln!("Warning: {}", e);
        } else {
            return Err(e); // Fatal error - abort parsing
        }
    }
}
```

**Skippable Errors:**
- Malformed JSON lines
- Missing required fields (with defaults configured)
- Invalid timestamps (with fallback)
- Invalid role values

**Fatal Errors:**
- File read errors
- Permission errors
- Corrupted file headers

---

### 4. Blank Line Skipping

**Location:** All parsers (jsonl.rs:568, markdown.rs, sqlite.rs)

**Purpose:** Skip empty lines in log files.

**Implementation:**
```rust
// src/parser/jsonl.rs:568-570
if line.trim().is_empty() {
    continue;
}
```

**Applied At:** JSONL, Markdown, SQLite, JSON-array parsers.

---

### 5. Tool Result Skipping (Vector Index)

**Location:** `src/vector.rs` (note: currently stubbed/non-functional)

**Purpose:** Exclude tool_result events from embedding generation to improve relevance.

**Implementation:**
```rust
// src/vector.rs
crate::event::Role::ToolResult => continue, // Skip tool results in embeddings
```

**Rationale:** Tool outputs (stderr, execution results) add noise for semantic search - focus on user/assistant/tool_call content instead.

---

### 6. Test Skips

**Location:** Test files throughout codebase

**Purpose:** Conditional test execution based on environment.

**Implementation:**
```rust
// src/enrichment/behavioral_signals.rs
// If scraper creation fails (due to missing dependencies), skip the test gracefully
let scraper = match scraper_result {
    Ok(s) => s,
    Err(_) => {
        // Test skipped - missing dependencies
        return;
    }
};
```

**Used For:** Tests requiring:
- Database connections (SQLite tests)
- File system access (permission tests)
- External dependencies (scraper tests)

---

### 7. Subagent File Skipping

**Location:** `src/parser/jsonl.rs:22-26`, `src/plugin.rs`

**Purpose:** Detect and mark subagent sessions for hierarchical tracking.

**Implementation:**
```rust
// src/parser/jsonl.rs:22-26
fn is_subagent_file(source_path: &Path) -> bool {
    source_path
        .components()
        .any(|c| c.as_os_str() == "subagents")
}
```

**Not a Skip:** Subagent files are PARSED (not skipped), but tagged with `-subagent` suffix:
```rust
let source_agent = if is_subagent_file(source_path) {
    format!("{}-subagent", plugin.plugin.name.clone())
} else {
    plugin.plugin.name.clone()
};
```

---

## Code Flow Diagrams

### Skip-Type Line Processing Flow

```
┌─────────────────────────────────────────────────────────────┐
│ JSONL File: {type: "heartbeat", timestamp: "...", payload} │
└────────────────────┬────────────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────────────┐
│ JsonlParser::parse_line()                                   │
├─────────────────────────────────────────────────────────────┤
│ 1. Parse JSON: serde_json::from_str(line)                 │
└────────────────────┬────────────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────────────┐
│ Envelope Routing (if envelope config exists)               │
├─────────────────────────────────────────────────────────────┤
│ type_value = extract_type(raw_json, "type") = "heartbeat"  │
│ routing = envelope.get_routing("heartbeat") = "skip"        │
└────────────────────┬────────────────────────────────────────┘
                     │
                     ▼
              ┌──────┴──────┐
              │  "skip"     │
              └──────┬──────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────────────┐
│ Return Ok(Vec::new())                                       │
│ - No event emitted                                          │
│ - No error raised                                           │
│ - Line silently dropped                                    │
└─────────────────────────────────────────────────────────────┘
```

### Event-Type Line Processing Flow

```
┌─────────────────────────────────────────────────────────────┐
│ JSONL File: {type: "message", timestamp: "...", message: {}}│
└────────────────────┬────────────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────────────┐
│ Envelope Routing                                            │
├─────────────────────────────────────────────────────────────┤
│ type_value = "message"                                      │
│ routing = envelope.get_routing("message") = "event"         │
└────────────────────┬────────────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────────────┐
│ Extract Payload from payload_field                          │
├─────────────────────────────────────────────────────────────┤
│ payload = raw_json.get("payload")                           │
│ - Validate payload is Object                                 │
│ - Skip with warning if missing/non-object                   │
└────────────────────┬────────────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────────────┐
│ Field Extraction (Envelope-Aware)                          │
├─────────────────────────────────────────────────────────────┤
│ timestamp = extract_with_envelope("^timestamp", payload,    │
│                                    envelope)                 │
│ - ^ prefix: read from envelope wrapper                      │
│ - No ^ prefix: read from payload                            │
│ role = extract_with_envelope("role", payload, envelope)     │
│ content = extract_with_envelope("content", payload,          │
│                                envelope)                     │
└────────────────────┬────────────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────────────┐
│ Include/Exclude Type Filters                                 │
├─────────────────────────────────────────────────────────────┤
│ if include_types and not in values → skip                   │
│ if exclude_types and in values → skip                       │
└────────────────────┬────────────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────────────┐
│ Build Event                                                  │
├─────────────────────────────────────────────────────────────┤
│ event = Event::new(ts, session_id, agent, role, content)    │
└────────────────────┬────────────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────────────┐
│ Return Ok(vec![event])                                       │
└─────────────────────────────────────────────────────────────┘
```

---

## File-by-File Breakdown

| File | Skip Mechanisms | Lines | Purpose |
|------|----------------|-------|---------|
| `src/plugin.rs` | Envelope type routing config | 149-194 | `Envelope::get_routing()` - type → action lookup |
| `src/parser/mod.rs` | Field extraction helpers | 423-547 | `extract_with_envelope()` - envelope-aware field reading |
| `src/parser/jsonl.rs` | Primary skip routing implementation | 200-291, 298-318, 568-570 | Envelope unwrapping, type filters, blank line skip |
| `src/parser/markdown.rs` | Delimiter skipping | 152-172 | Session delimiter detection |
| `src/vector.rs` | Tool result skip | (stubbed) | Embedding generation filter |
| `src/error.rs` | Skippable error classification | (all) | `is_skippable()` method |
| `src/scraper/mod.rs` | Error handling | (multiple) | Parse error propagation |
| Test files (20+) | Skip behavior validation | (various) | Unit tests for all skip mechanisms |

---

## Configuration Examples

### Complete Envelope Skip Configuration (Codex-style)

```toml
[plugin]
name = "codex"
version = "1.0"

[source]
paths = ["~/.codex/sessions/**/*.jsonl"]
format = "jsonl"

[source.envelope]
payload_field = "payload"
type_field = "type"
type_routing = {
    # Event types - parse into canonical events
    "message" = "event",
    "response_item" = "event",
    
    # Metadata types - accumulate session info (TODO)
    "session_meta" = "meta",
    "turn_context" = "meta",
    
    # Skip types - drop the line entirely
    "heartbeat" = "skip",
    "ping" = "skip",
    "event_msg" = "skip",
}

[parser]
timestamp = "^timestamp"
role = "role"
content = "content"
```

### Include/Exclude Filter Configuration

```toml
[parser.include_types]
field = "type"
values = ["user", "assistant", "tool_call"]

[parser.exclude_types]
field = "type"
values = ["system", "debug"]
```

---

## Test Coverage Analysis

**Total tests for skip routing:** 25+ tests in `src/parser/jsonl.rs`

**Key Test Categories:**
1. **Envelope routing tests** (15 tests)
   - Skip-type routing produces zero events
   - Meta-type routing produces zero events  
   - Unknown type defaults to skip
   - Event-type routing produces events

2. **Unwrap envelope unit tests** (7 tests)
   - `test_unwrap_envelope_skip_type_returns_empty_and_none`
   - `test_unwrap_envelope_unknown_type_returns_empty_and_none`
   - `test_unwrap_envelope_different_skip_types_all_return_empty_none`

3. **Integration tests** (3 tests)
   - `test_skip_only_fixture_routing_integration` - end-to-end skip-only fixture
   - `test_fixture_with_only_non_event_types_produces_zero_events`
   - `test_mixed_fixture_event_lines_still_parse` - verify events still parse

**Test Fixtures:**
- `tests/fixtures/envelope/skip-only.jsonl` - 4 lines, all skip-type
- `tests/fixtures/envelope/non-event-types.jsonl` - 6 lines, meta/skip/unknown
- `tests/fixtures/envelope/envelope-routing.jsonl` - mixed event/meta/skip lines

---

## Design Patterns

### 1. **Envelope-First Field Extraction**

Fields prefixed with `^` read from the envelope wrapper, not the payload:

```rust
// ^timestamp → reads from {type: "...", timestamp: "...", payload: {...}}
//          → "2026-03-16T12:00:00Z" (wrapper level)

// role      → reads from {type: "...", timestamp: "...", payload: {role: "..."}}
//          → "user" (payload level)
```

This enables reading wrapper-level metadata (timestamps, model names) alongside payload-level event data.

### 2. **Graceful Degradation**

All skip mechanisms return `Ok(Vec::new())` - never an error:

```rust
match routing {
    "skip" => return Ok(Vec::new()),  // Not an error - just filtered
    "event" => { /* parse */ },
}
```

This allows parsing to continue even when lines are malformed or missing expected fields.

### 3. **Warning-Logged Defaults**

Unknown/skipped items log warnings for debugging but don't fail parsing:

```rust
None => {
    warn!(type_value = type_value, "Unknown envelope type value, routing to 'skip'");
    "skip"
}
```

### 4. **Test Fixture Isolation**

Skip logic tested with dedicated fixtures containing only skip-type lines:

```
# tests/fixtures/envelope/skip-only.jsonl
{"type":"heartbeat","timestamp":"2026-07-04T10:00:05Z","payload":{"status":"ok"}}
{"type":"heartbeat","timestamp":"2026-07-04T10:00:10Z","payload":{"status":"ok"}}
{"type":"ping","timestamp":"2026-07-04T10:00:15Z","payload":{"seq":1}}
{"type":"unlisted_type","timestamp":"2026-07-04T10:00:20Z","payload":{"data":"test"}}
```

Expected: 0 events produced, no errors.

---

## Performance Considerations

1. **Envelope routing is O(1)** - HashMap lookup in `type_routing`
2. **Skip happens early** - before payload extraction and field parsing
3. **No error handling overhead** - returns `Ok(Vec::new())`, not `Err(...)`
4. **Minimal memory allocation** - empty Vec is reused

---

## Future Enhancements (TODOs in Code)

### Meta-Type Metadata Accumulation

**Location:** `src/parser/jsonl.rs:229-232`

```rust
"meta" => {
    // Metadata line - no event emitted
    // TODO: Future session metadata accumulation (project, model, version)
    return Ok(Vec::new());
}
```

**Planned:** Extract session-level metadata (model, version, project) from meta-type lines and accumulate into session context.

---

## Validation Checklist

✅ **Complete list of all skip routing code files reviewed** - 27 files  
✅ **Documentation of each skip mechanism found** - 7 mechanisms identified  
✅ **Code flow diagrams for skip-type lines** - 2 flowcharts  
✅ **Understanding of how skip-type lines are processed** - envelope routing pipeline documented  
✅ **Test coverage analyzed** - 25+ tests, 3 fixture files  
✅ **Configuration examples provided** - TOML configs for all mechanisms  
✅ **Design patterns documented** - 4 patterns identified  
✅ **Performance notes included** - O(1) routing, early skip  

---

## Acceptance Criteria Met

| Criterion | Status | Details |
|-----------|--------|---------|
| Complete list of all skip routing code files reviewed | ✅ | 27 files identified and analyzed |
| Documentation of each skip mechanism found | ✅ | 7 mechanisms documented with code locations |
| Code flow diagrams or descriptions | ✅ | 2 flowcharts (skip-type and event-type processing) |
| Understanding of how skip-type lines are processed | ✅ | Envelope routing pipeline fully documented |

---

## Conclusion

AgentScribe's skip routing implementation is **well-designed, thoroughly tested, and performance-optimized**. The envelope type-based routing mechanism provides a declarative, configuration-driven approach to filtering unwanted log lines at parse time, with graceful fallback behavior for unknown or malformed data.

The primary skip mechanism (envelope routing) is production-ready with comprehensive test coverage. Future work should focus on implementing the TODO for meta-type metadata accumulation to enable session-level enrichment.
