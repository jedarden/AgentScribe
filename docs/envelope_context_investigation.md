# Envelope Context Availability Investigation

## Overview
This document verifies the availability and flow of envelope context (`envelope_json`) through the `parse_line()` function in the JSONL parser, and documents which helper functions currently receive envelope context.

## Investigation Summary

**✓ Confirmed:** `envelope_json` is available in `parse_line()` and correctly passed to all envelope-aware helper functions.

---

## 1. Envelope Context Availability in parse_line()

### Function Signature
```rust
pub fn parse_line(
    line: &str,
    line_number: usize,
    context: &ParseContext,
    plugin: &Plugin,
) -> Result<Vec<Event>>
```
Location: `src/parser/jsonl.rs:185-190`

### Envelope Context Setup

**Location:** `src/parser/jsonl.rs:212-298`

The envelope context is established early in `parse_line()` through envelope routing logic:

```rust
let (envelope_json, payload_json): (Option<&Value>, &Value) = if let Some(
    ref envelope_cfg,
) = plugin.source.envelope
{
    // Envelope mode: extract type and apply routing
    let type_value = extract_string_with_envelope(
        &envelope_cfg.type_field,
        &raw_json,
        Some(&raw_json),
    ).unwrap_or_default();
    let routing = envelope_cfg.get_routing(&type_value);

    match routing {
        "skip" => return Ok(Vec::new()),  // Skip this line
        "meta" => return Ok(Vec::new()),  // Metadata line (TODO: accumulate)
        "event" => {
            // Extract payload from payload_field
            let extracted = raw_json.get(&envelope_cfg.payload_field).and_then(|v| {
                match v {
                    Value::Object(_) => Some(v),
                    _ => None,
                }
            });

            match extracted {
                Some(payload) => (Some(&raw_json), payload),
                None => return Ok(Vec::new()),  // Skip with warning
            }
        }
        _ => return Ok(Vec::new()),  // Unknown routing
    }
} else {
    // No envelope: both envelope_json and payload_json point to the full line
    (None, &raw_json)
};
```

### Key Points

1. **`envelope_json` is `Option<&Value>`**: Contains reference to wrapper JSON when envelope is configured and routing is "event", otherwise `None`

2. **`payload_json` is `&Value`**: Always contains the event data (from `payload_field` if envelope exists, otherwise the full line)

3. **Routing determines availability**:
   - `"skip"` → returns early, envelope unused
   - `"meta"` → returns early, envelope unused (future: accumulate metadata)
   - `"event"` → envelope available as `Some(&raw_json)`
   - No envelope config → `None`

---

## 2. Call Chain to Helper Functions

### Helper Functions in src/parser/mod.rs

#### A. `extract_string_with_envelope()`
**Signature:**
```rust
pub fn extract_string_with_envelope(
    path: &str,
    payload: &Value,
    envelope: Option<&Value>,
) -> Option<String>
```
**Location:** `src/parser/mod.rs:553-590`

**Behavior:**
- Envelope-first lookup with payload fallback for `^`-prefixed paths
- `^field` → try envelope first, fallback to payload
- `field` → extract from payload only
- Supports dot notation and array indexing
- Coerces String/Number/Bool/Null to string
- Special handling for content arrays (text blocks)

#### B. `parse_timestamp_with_envelope()`
**Signature:**
```rust
pub fn parse_timestamp_with_envelope(
    path: &str,
    payload: &Value,
    envelope: Option<&Value>,
) -> Result<DateTime<Utc>>
```
**Location:** `src/parser/mod.rs:597-631`

**Behavior:**
- Wrapper around `extract_string_with_envelope()` with timestamp parsing
- Supports ISO 8601, Unix epoch (seconds/milliseconds), UTC-naive formats
- Returns `Result<DateTime<Utc>>` or `Timestamp` error

#### C. `extract_with_envelope()`
**Signature:**
```rust
pub fn extract_with_envelope(
    path: &str,
    payload: &Value,
    envelope: Option<&Value>,
) -> Option<Value>
```
**Location:** `src/parser/mod.rs:523-547`

**Behavior:**
- Lower-level function used by `extract_string_with_envelope()`
- Returns raw `Value` instead of string
- Implements envelope-first with payload fallback logic
- Supports dot notation and array indexing

---

## 3. Helper Functions Currently Receiving envelope_json

### In parse_line() at src/parser/jsonl.rs

| Line | Helper Call | Field | Purpose |
|------|-------------|-------|---------|
| 217 | `extract_string_with_envelope()` | `type_field` | Get type value for routing |
| 304 | `extract_string_with_envelope()` | `include_types.field` | Type filter (include) |
| 314 | `extract_string_with_envelope()` | `exclude_types.field` | Type filter (exclude) |
| 325 | `parse_timestamp_with_envelope()` | `timestamp` | Parse event timestamp |
| 338 | `extract_string_with_envelope()` | `role` | Extract role field |
| 376 | `extract_string_with_envelope()` | `content` | Extract content field |
| 401 | `extract_string_with_envelope()` | `tool_name` | Extract tool name |
| 409 | `extract_string_with_envelope()` | `tokens_in` | Extract input tokens |
| 413 | `extract_string_with_envelope()` | `tokens_out` | Extract output tokens |

### Pattern Summary

All field extraction in `parse_line()` follows this pattern:

```rust
// ^ prefix: envelope-first lookup with payload fallback
let field_value = extract_string_with_envelope(
    field_path,        // e.g., "^timestamp" or "role"
    payload_json,      // Event data
    envelope_json      // Wrapper JSON (Option<&Value>)
);

// Specialized timestamp parsing
let ts = parse_timestamp_with_envelope(
    ts_field,          // e.g., "^timestamp"
    payload_json,
    envelope_json
);
```

---

## 4. Envelope-Aware Helper Function Behavior

### Field Resolution Order

For **`^field`** (caret-prefixed):
1. Try `envelope.field` (if envelope exists)
2. Fallback to `payload.field` (if envelope missing or field not found)
3. Return `None` (if both fail)

For **`field`** (no caret):
- Extract from `payload.field` only (envelope ignored)

### Supported Path Syntax

- **Simple:** `"role"`, `"^timestamp"`
- **Nested:** `"user.role"`, `"^outer.inner.ts"`
- **Array:** `"items[0].name"`, `"^list[1].field"`

### Type Coercion (extract_string_with_envelope)

- `Value::String(s)` → `Some(s)`
- `Value::Number(n)` → `Some(n.to_string())`
- `Value::Bool(b)` → `Some(b.to_string())`
- `Value::Null` → `Some(String::new())`
- `Value::Array(arr)` → Text block extraction or `None`
- `Value::Object(_)` → `None`

---

## 5. Usage Examples from Test Suite

### Example 1: Extract timestamp from envelope
```rust
let payload = json!({"role": "user"});
let envelope = json!({"timestamp": "2026-03-16T12:00:00Z"});

// Extract from envelope using ^ prefix
let result = extract_string_with_envelope("^timestamp", &payload, Some(&envelope));
assert_eq!(result, Some("2026-03-16T12:00:00Z".to_string()));
```

### Example 2: Extract role from payload (no caret)
```rust
let payload = json!({"role": "user", "content": "hello"});
let envelope = json!({"timestamp": "2026-03-16T12:00:00Z"});

// Extract from payload (no ^ prefix)
let result = extract_string_with_envelope("role", &payload, Some(&envelope));
assert_eq!(result, Some("user".to_string()));
```

### Example 3: Nested field with envelope fallback
```rust
let payload = json!({"model": "gpt-4"});
let envelope = json!({"timestamp": "2026-03-16T12:00:00Z"});

// Field not in envelope, fallback to payload
let result = extract_string_with_envelope("^model", &payload, Some(&envelope));
assert_eq!(result, Some("gpt-4".to_string()));
```

### Example 4: Parse timestamp from envelope
```rust
let payload = json!({"role": "user"});
let envelope = json!({"timestamp": "2026-03-16T12:00:00Z"});

// Parse timestamp from envelope using ^ prefix
let result = parse_timestamp_with_envelope("^timestamp", &payload, Some(&envelope));
assert!(result.is_ok());
```

---

## 6. List of Helper Functions That Should Be Envelope-Aware

### Currently Envelope-Aware ✓
- `extract_string_with_envelope()` — USED
- `parse_timestamp_with_envelope()` — USED
- `extract_with_envelope()` — USED (internally by above)

### Potentially Future Helpers
The following helpers could be made envelope-aware if needed:

1. **Custom field extractors** — If new field types are added (e.g., complex objects, custom arrays)
2. **Metadata accumulators** — For "meta" routing (currently skipped, TODO in code)
3. **Array field handlers** — For extracting array elements with envelope awareness

However, the current implementation already covers all common use cases through the three envelope-aware helpers above.

---

## 7. Architecture Diagram

```
parse_line()
    │
    ├── [1] Parse JSON line → raw_json: Value
    │
    ├── [2] Envelope Routing
    │     ├── If plugin.source.envelope exists:
    │     │   ├── Extract type_field → routing
    │     │   ├── match routing {
    │     │   │   "skip" → return Ok(Vec::new())
    │     │   │   "meta" → return Ok(Vec::new())  [TODO: accumulate]
    │     │   │   "event" → (envelope_json, payload_json) = (Some(&raw_json), payload)
    │     │   │   }
    │     │   └── else: (envelope_json, payload_json) = (None, &raw_json)
    │     │
    │     └── envelope_json: Option<&Value> is now available
    │
    ├── [3] Type Filtering
    │     ├── include_types: extract_string_with_envelope(field, payload, envelope)
    │     └── exclude_types: extract_string_with_envelope(field, payload, envelope)
    │
    ├── [4] Field Extraction (all use envelope_json)
    │     ├── timestamp: parse_timestamp_with_envelope(field, payload, envelope)
    │     ├── role: extract_string_with_envelope(field, payload, envelope)
    │     ├── content: extract_string_with_envelope(field, payload, envelope)
    │     ├── tool_name: extract_string_with_envelope(field, payload, envelope)
    │     ├── tokens_in: extract_string_with_envelope(field, payload, envelope)
    │     └── tokens_out: extract_string_with_envelope(field, payload, envelope)
    │
    └── [5] Build Event → Ok(vec![event])
```

---

## 8. Key Findings

### Availability
✓ **`envelope_json` is available throughout `parse_line()`** after the envelope routing block (lines 212-298)

### Call Chain
✓ **All field extraction helpers receive `envelope_json`** via the `envelope` parameter

### Current Implementation
✓ **Three envelope-aware helpers cover all current use cases:**
- `extract_string_with_envelope()` — 9 call sites
- `parse_timestamp_with_envelope()` — 1 call site  
- `extract_with_envelope()` — internal use

### Design Pattern
✓ **Envelope-first with payload fallback** is consistently implemented across all helpers:
- `^field` → envelope first, payload fallback
- `field` → payload only

### Future Extensibility
✓ **The pattern is extensible** — new field types can use the same helpers without modification

---

## 9. Recommendations

### Current State
✓ **No changes needed** — envelope context flows correctly through all helpers

### Future Enhancements (Optional)
1. **Meta routing implementation** — Lines 232-235 have TODO for accumulating metadata from "meta" routing
2. **Additional helper types** — Only if new field extraction patterns emerge
3. **Performance optimization** — Consider caching parsed field paths if profiling shows need

### Testing Coverage
✓ **Comprehensive test suite** — 50+ tests covering envelope scenarios in `src/parser/mod.rs:682-1587`

---

## 10. Conclusion

**envelope_json is correctly available in parse_line()** and flows through the entire call chain to all helper functions that need it. The envelope-aware helper design (`extract_string_with_envelope`, `parse_timestamp_with_envelope`, `extract_with_envelope`) provides a consistent pattern for field extraction that:

1. Supports envelope-first lookup with payload fallback
2. Handles caret-prefixed paths for envelope fields
3. Maintains backward compatibility (non-caret fields read payload only)
4. Is extensible for future field types

**No code changes are required** — the implementation is correct and complete.