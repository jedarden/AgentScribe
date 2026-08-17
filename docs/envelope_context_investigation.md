# Envelope Context Availability in parse_line()

## Investigation Summary

**Date:** 2026-08-17  
**Task:** Verify envelope context availability in parse_line() and understand how envelope_json is passed through the call stack  
**Status:** ✅ **CONFIRMED** - envelope_json is available in parse_line() and properly passed to all helper functions

---

## Key Findings

### 1. envelope_json Parameter Availability

**Location:** `src/parser/jsonl.rs:185-456` - `parse_line()` function

The `parse_line()` function receives:
- `line: &str` - The raw JSONL line
- `line_number: usize` - Line number for error reporting
- `context: &ParseContext` - Session context
- `plugin: &Plugin` - Plugin configuration with envelope settings

**envelope_json is NOT a direct parameter** but is **extracted within** `parse_line()` based on plugin configuration.

---

### 2. Envelope Extraction Logic

**Lines 212-298** contain the core envelope unwrapping logic:

```rust
let (envelope_json, payload_json): (Option<&Value>, &Value) = if let Some(ref envelope_cfg) = plugin.source.envelope {
    // Envelope mode: extract type and apply routing
    let type_value = extract_string_with_envelope(
        &envelope_cfg.type_field,
        &raw_json,
        Some(&raw_json),  // envelope_json = raw_json
    )
    .unwrap_or_default();
    let routing = envelope_cfg.get_routing(&type_value);

    match routing {
        "skip" => {
            return Ok(Vec::new());  // Drop line
        }
        "meta" => {
            return Ok(Vec::new());  // TODO: accumulate metadata
        }
        "event" => {
            let extracted = raw_json.get(&envelope_cfg.payload_field)
                .and_then(|v| match v {
                    Value::Object(_) => Some(v),
                    _ => None,
                });

            match extracted {
                Some(payload) => {
                    (Some(&raw_json), payload)  // envelope_json + payload
                }
                None => {
                    // Skip with warning
                    return Ok(Vec::new());
                }
            }
        }
        _ => return Ok(Vec::new()),
    }
} else {
    // No envelope: both envelope_json and payload_json point to the full line
    (None, &raw_json)
};
```

---

### 3. Helper Functions Currently Receiving envelope_json

All field extraction helpers receive **both** `payload_json` AND `envelope_json`:

| Helper Function | Line(s) | Purpose | Receives envelope_json? |
|----------------|---------|---------|--------------------------|
| `extract_string_with_envelope` | 304, 315 | Type filtering (include/exclude) | ✅ Yes |
| `parse_timestamp_with_envelope` | 325 | Timestamp extraction | ✅ Yes |
| `extract_string_with_envelope` | 338 | Role extraction | ✅ Yes |
| `extract_string_with_envelope` | 376 | Content extraction | ✅ Yes |
| `extract_string_with_envelope` | 401 | Tool name extraction | ✅ Yes |
| `extract_string_with_envelope` | 409, 413 | Token count extraction | ✅ Yes |

---

### 4. Call Chain Diagram

```
parse_line(line, line_number, context, plugin)
    │
    ├─> Parse JSON line → raw_json: Value
    │
    ├─> Extract envelope_json & payload_json (lines 212-298)
    │   │
    │   ├─> IF plugin.source.envelope = Some(envelope_cfg):
    │   │   │
    │   │   ├─> Extract type field → routing
    │   │   ├─> routing = "skip" → return Ok(Vec::new())
    │   │   ├─> routing = "meta" → return Ok(Vec::new())
    │   │   └─> routing = "event" → (Some(&raw_json), payload)
    │   │
    │   └─> ELSE (no envelope): (None, &raw_json)
    │
    ├─> Type filtering checks
    │   └─> extract_string_with_envelope(field, payload_json, envelope_json)
    │       │
    │       └─> ^prefix → reads from envelope_json
    │       └─> no prefix → reads from payload_json
    │
    ├─> Timestamp extraction
    │   └─> parse_timestamp_with_envelope(field, payload_json, envelope_json)
    │
    ├─> Role extraction
    │   └─> extract_string_with_envelope(field, payload_json, envelope_json)
    │
    ├─> Content extraction
    │   └─> extract_string_with_envelope(field, payload_json, envelope_json)
    │
    ├─> Tool name extraction
    │   └─> extract_string_with_envelope(field, payload_json, envelope_json)
    │
    └─> Token count extraction
        └─> extract_string_with_envelope(field, payload_json, envelope_json)
```

---

### 5. Envelope-Aware Field Extraction

The `^` prefix syntax controls data source selection:

- **`^field`** → Read from `envelope_json` (wrapper level)
- **`field`** → Read from `payload_json` (payload level)

**Example from envelope_test.toml:**

```toml
[parser]
timestamp = "^timestamp"     # Read from wrapper: {"type": "message", "timestamp": "...", "payload": {...}}
role = "role"                 # Read from payload: {"role": "user", "content": "..."}
content = "content"           # Read from payload
```

**Result:** Wrapper-level fields (like `timestamp`) are extracted from the envelope, while event fields (like `role` and `content`) are extracted from the payload.

---

### 6. Current Implementation Status

| Component | Status | Notes |
|-----------|--------|-------|
| Envelope unwrapping in parse_line() | ✅ Complete | Lines 212-298 |
| Field extraction with envelope context | ✅ Complete | All helpers receive envelope_json |
| Type filtering with envelope support | ✅ Complete | Lines 301-321 |
| Meta-type routing | ⚠️ Partial | Routes to `Ok(Vec::new())` with TODO comment for future metadata accumulation |
| Skip-type routing | ✅ Complete | Returns `Ok(Vec::new())` to drop lines |
| Event-type routing | ✅ Complete | Extracts payload and continues to event processing |

---

### 7. Helper Function Signatures

**From `src/parser/mod.rs`:**

```rust
/// Extract a string field from either envelope or payload
pub fn extract_string_with_envelope(
    field: &str,
    payload_json: &Value,
    envelope_json: Option<&Value>,
) -> Option<String>

/// Parse timestamp from either envelope or payload
pub fn parse_timestamp_with_envelope(
    field: &str,
    payload_json: &Value,
    envelope_json: Option<&Value>,
) -> Result<DateTime<Utc>>
```

Both functions:
- Accept `envelope_json` as `Option<&Value>` (None when no envelope)
- Parse the field path (e.g., "timestamp", "payload.role")
- Handle `^` prefix to select envelope vs payload
- Return `None` / `Err` when field is missing

---

### 8. Test Coverage

**Envelope routing tests** (all passing):

- ✅ `test_parse_line_simple` - Basic parsing without envelope
- ✅ `test_parse_line_envelope_skip_routing` - Skip-type routing drops lines
- ✅ `test_parse_line_envelope_meta_routing` - Meta-type routing (currently drops)
- ✅ `test_parse_line_envelope_unknown_type_defaults_to_skip` - Unknown types skip
- ✅ `test_parse_line_event_type` - Event-type routing extracts payload
- ✅ `test_parse_line_envelope_field_extraction` - Field extraction from both levels
- ✅ `test_fixture_with_only_non_event_types_produces_zero_events` - Integration test
- ✅ `test_skip_only_fixture_routing_integration` - Skip-type integration test

**Test verification:** ✅ All envelope tests pass, confirming correct envelope_json flow

---

### 9. Key Code Locations

| Concern | File | Lines | Description |
|---------|------|-------|-------------|
| Envelope extraction | `src/parser/jsonl.rs` | 212-298 | Core unwrapping logic |
| Type filtering | `src/parser/jsonl.rs` | 301-321 | Include/exclude with envelope support |
| Field extraction | `src/parser/jsonl.rs` | 324-423 | All field extraction with envelope |
| Helper functions | `src/parser/mod.rs` | - | `extract_string_with_envelope`, `parse_timestamp_with_envelope` |
| Plugin envelope config | `src/plugin.rs` | - | `Envelope` struct definition |

---

### 10. Future Work Areas

1. **Meta-type metadata accumulation** (line 232)
   - Currently returns `Ok(Vec::new())` 
   - TODO comment indicates future session-level metadata extraction
   - Should accumulate project, model, version from meta-type lines

2. **Performance optimization**
   - Current implementation extracts envelope_json for every line
   - Could be cached for lines with same envelope structure
   - No immediate need - performance is acceptable

3. **Enhanced error messages**
   - Field extraction could include whether it came from envelope or payload
   - Helpful for debugging plugin configuration

---

## Conclusion

**✅ CONFIRMED:** `envelope_json` is available in `parse_line()` and correctly passed to all helper functions via the `(envelope_json, payload_json)` tuple extraction (lines 212-298).

**✅ WORKING:** All field extraction functions receive envelope context and use the `^` prefix syntax to select between envelope and payload data sources.

**✅ TESTED:** Comprehensive test coverage validates envelope routing (skip/meta/event), field extraction from both levels, and proper handling of unknown types.

**No code changes required** - the envelope context flow is already properly implemented and tested.
