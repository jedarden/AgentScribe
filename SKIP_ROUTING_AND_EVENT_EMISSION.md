# Skip Routing Logic and Event Emission Analysis

## Executive Summary

AgentScribe implements a sophisticated **skip routing system** that allows plugins to declaratively control which JSONL lines should be processed, skipped, or treated as metadata. This analysis documents the complete flow from envelope configuration through event emission.

---

## 1. Skip Routing Logic

### 1.1 Configuration Entry Point

**Location:** `src/plugin.rs:146-195`

The `Envelope` struct defines skip routing configuration:

```rust
pub struct Envelope {
    /// Field name containing the event payload
    pub payload_field: String,
    /// Field name containing the event type for routing
    pub type_field: String,
    /// Maps type values to routing actions: "event", "meta", or "skip"
    #[serde(default)]
    pub type_routing: HashMap<String, String>,
}
```

**Routing actions:**
- `"event"` - Extract payload and emit events
- `"meta"` - Extract metadata but don't emit events (TODO: future session metadata accumulation)
- `"skip"` - Skip the line entirely (no event emitted)

### 1.2 Routing Decision Logic

**Location:** `src/plugin.rs:159-180`

The `get_routing()` method determines routing action:

```rust
pub fn get_routing(&self, type_value: &str) -> &str {
    match self.type_routing.get(type_value) {
        Some(action) => {
            match action.as_str() {
                "event" | "meta" | "skip" => action,
                _ => "skip",  // Invalid routing values treated as skip
            }
        }
        None => {
            warn!("Unknown envelope type value, routing to 'skip'");
            "skip"  // Unknown types default to skip
        }
    }
}
```

**Key behaviors:**
1. Unknown type values → `"skip"` (with warning)
2. Invalid routing values → `"skip"` (no warning)
3. Only valid actions: `"event"`, `"meta"`, `"skip"`

---

## 2. Event Emission Points

### 2.1 Primary Entry Point

**Location:** `src/parser/jsonl.rs:151-300`

The `parse_line()` function is the main event emission point:

```rust
pub fn parse_line(
    line: &str,
    line_number: usize,
    context: &ParseContext,
    plugin: &Plugin,
) -> Result<Vec<Event>>
```

**Key emission logic (lines 186-256):**

```rust
match routing {
    "skip" => {
        // Skip this line - no event emitted
        return Ok(Vec::new());
    }
    "meta" => {
        // Metadata line - no event emitted
        return Ok(Vec::new());
    }
    "event" => {
        // Extract payload from payload_field for event body
        // ... validation and extraction logic ...
        (Some(&raw_json), payload)
    }
    _ => {
        // Unknown routing - return empty
        return Ok(Vec::new());
    }
}
```

### 2.2 Additional Skip Points

**Location:** `src/parser/jsonl.rs:262-283`

Type-based filtering (include/exclude):

```rust
// Check type filter (envelope-aware: ^ prefix reads from wrapper, otherwise from payload)
if let Some(ref filter) = plugin.parser.include_types {
    let type_field = &filter.field;
    if let Some(type_val) = extract_string_with_envelope(type_field, payload_json, envelope_json) {
        if !filter.values.contains(&type_val) {
            return Ok(Vec::new()); // Skip this event
        }
    }
}

if let Some(ref filter) = plugin.parser.exclude_types {
    let type_field = &filter.field;
    if let Some(type_val) = extract_string_with_envelope(type_field, payload_json, envelope_json) {
        if filter.values.contains(&type_val) {
            return Ok(Vec::new()); // Skip this event
        }
    }
}
```

---

## 3. Event Emission Flow

### 3.1 Call Chain

```
scraper::scrape_file()  (src/scraper/mod.rs:423)
    └─> parser::parse() (FormatParser trait)
        └─> JsonlParser::parse() (src/parser/jsonl.rs)
            └─> JsonlParser::parse_line() (line-by-line processing)
                └─> [Envelope routing decision]
                    ├─> skip → return Ok(Vec::new())
                    ├─> meta → return Ok(Vec::new()) 
                    └─> event → [field extraction] → Event creation
```

### 3.2 Event Creation

**Location:** `src/parser/jsonl.rs:300+`

After routing passes the `"event"` check, events are created by:

1. **Timestamp extraction** (line 286-296)
2. **Role extraction** (line 298+)
3. **Content extraction** (line +)
4. **Optional field extraction** (tool, tokens, file_paths, etc.)
5. **Event construction** with `Event::new()` or similar

---

## 4. Error Handling vs Skip Routing

### 4.1 Skippable Errors

**Location:** `src/error.rs:178-181`

```rust
pub fn is_skippable(&self) -> bool {
    matches!(self, AgentScribeError::Parse { .. })
}
```

**Usage in scraper:** `src/scraper/mod.rs:459-470`

```rust
let all_events: Vec<Event> = match parser.parse(file_path, plugin) {
    Ok(events) => events,
    Err(e) => {
        if e.is_skippable() {
            result.errors.push(ScrapeError {
                file: file_path.display().to_string(),
                line: None,
                message: e.to_string(),
            });
            Vec::new()  // Skip - return empty events
        } else {
            return Err(e);  // Fatal error
        }
    }
};
```

**Key distinction:**
- **Skip routing** = Declarative filtering based on envelope type (configured in plugin)
- **Skippable errors** = Imperative error recovery during parsing (malformed JSON, missing fields)

---

## 5. Test Coverage

### 5.1 Skip Routing Tests

**Locations:** `src/parser/jsonl.rs:3872-4019`

1. **`test_skip_routing_event_emitter_never_called()`** (lines 3872-3912)
   - Verifies that skip routing bypasses event creation entirely
   - Uses complex payload that would normally emit events
   - Asserts empty result

2. **`test_mixed_skip_and_event_routing_emits_only_events()`** (lines 3915-3984)
   - Tests mixing skip and event types
   - Verifies only event types emit
   - Counts total events emitted

3. **`test_skip_routing_empty_event_stream_after_processing()`** (lines 3987-4019)
   - Processes multiple skip lines
   - Verifies event stream remains empty

### 5.2 Envelope Tests

**Locations:** `src/parser/jsonl.rs:800+` (referenced in comments)

- Meta-type fixture tests (line 804)
- Envelope payload extraction tests

---

## 6. Configuration Example

### 6.1 Plugin TOML with Skip Routing

```toml
[source.envelope]
payload_field = "payload"
type_field = "type"
type_routing = { session_meta = "meta", response_item = "event", turn_context = "meta", event_msg = "skip" }
```

**Behavior:**
- `session_meta` → Accumulate metadata, don't emit
- `response_item` → Extract payload, emit events
- `turn_context` → Accumulate metadata, don't emit
- `event_msg` → Skip entirely (noise)
- Unknown types → Skip (with warning)

---

## 7. Implementation Notes

### 7.1 Envelope-Aware Field Extraction

**Location:** `src/parser/jsonl.rs:162-176`

Comments explain the dual-reference system:

```rust
// Set up envelope_json and payload_json references:
// - envelope_json: reference to the full parsed line (or None if no envelope)
// - payload_json: reference to the event data (from payload_field if envelope, else full line)
//
// Field extraction uses envelope-aware functions:
// - Fields starting with '^' read from envelope_json
// - Fields without '^' read from payload_json
```

### 7.2 Future Work

**Location:** `src/parser/jsonl.rs:194` (TODO comment)

```rust
// TODO: Future session metadata accumulation (project, model, version)
// These lines contain session-level metadata that should be extracted
// and accumulated into the session context. For now, we drop them.
```

Currently `"meta"` routing returns empty (no events), but metadata extraction is not yet implemented.

---

## 8. Summary

**Skip routing locations:**
- Configuration: `src/plugin.rs:146-195` (`Envelope` struct)
- Routing decision: `src/plugin.rs:159-180` (`get_routing()` method)
- Routing application: `src/parser/jsonl.rs:186-256` (envelope handling)

**Event emission points:**
- Primary: `src/parser/jsonl.rs:151-300` (`parse_line()`)
- Additional filtering: `src/parser/jsonl.rs:262-283` (include/exclude types)
- Error skip: `src/error.rs:178-181` + `src/scraper/mod.rs:459-470`

**What needs to be tested:**
1. Skip routing correctly skips lines based on envelope type
2. Meta routing doesn't emit events (future: should accumulate metadata)
3. Event routing extracts payload and emits events
4. Mixed skip/event routing emits only event types
5. Unknown/invalid routing defaults to skip
6. Type-based filtering (include/exclude) works independently
7. Parse errors are skippable vs fatal

**Test coverage exists for:**
- ✅ Skip routing bypasses event creation
- ✅ Mixed skip/event routing emits only events
- ✅ Empty event stream after skip processing
- ✅ Envelope payload extraction
- ⚠️  Meta routing (metadata accumulation not yet implemented)
