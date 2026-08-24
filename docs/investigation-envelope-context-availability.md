# Envelope Context Availability in parse_line() — Investigation Report

**Date:** 2026-08-23
**Bead:** agentscr-0f95d934
**Investigation:** Verify envelope context availability in `parse_line()` and trace call chain

## Summary

**✅ CONFIRMED:** Envelope context IS available in `parse_line()` and flows correctly through all helper function calls.

## Implementation Details

### 1. Envelope Context Creation in `parse_line()`

**Location:** `src/parser/jsonl.rs`, lines 269-354

When a plugin defines `[source.envelope]`, `parse_line()` creates two references:

```rust
let (envelope_json, payload_json): (Option<&Value>, &Value) = if let Some(ref envelope_cfg) = plugin.source.envelope {
    // Envelope mode: extract type and apply routing
    // ... routing logic (skip/meta/event) ...
    
    // For "event" routing: return (Some(&raw_json), payload)
    (Some(&raw_json), payload)
} else {
    // No envelope: both envelope_json and payload_json point to the full line
    (None, &raw_json)
};
```

**Key properties:**
- `envelope_json: Option<&Value>` - `Some(&raw_json)` when envelope exists, `None` otherwise
- `payload_json: &Value` - references the extracted payload object OR the full raw_json if no envelope

### 2. Envelope-Aware Helper Functions

All helper functions that extract fields from JSON accept envelope context:

#### `extract_string_with_envelope()`
**Signature:**
```rust
pub fn extract_string_with_envelope(
    path: &str,
    payload: &Value,
    envelope: Option<&Value>,
) -> Option<String>
```

**Called in `parse_line()` at:**
- Line 360: Include type filter checking
- Line 371: Exclude type filter checking
- Line 394: Role field extraction
- Line 432: Content field extraction
- Line 457: Tool name extraction
- Line 465: Input tokens extraction
- Line 469: Output tokens extraction

**Behavior:**
- Paths starting with `^` try envelope first, then fallback to payload
- Paths without `^` read from payload only
- Supports dot notation (`^outer.ts`, `user.role`)
- Coerces values to strings (String, Number, Bool → strings; Null → empty string)

#### `parse_timestamp_with_envelope()`
**Signature:**
```rust
pub fn parse_timestamp_with_envelope(
    path: &str,
    payload: &Value,
    envelope: Option<&Value>,
) -> Result<DateTime<Utc>>
```

**Called in `parse_line()` at:**
- Line 381: Timestamp field extraction

**Behavior:**
- Same envelope-first semantics as `extract_string_with_envelope`
- Parses ISO 8601, Unix epoch (seconds/milliseconds), UTC-naive formats

### 3. Field Extraction Pattern

All field extractions in `parse_line()` follow this pattern:

```rust
// Type filter (envelope-aware)
if let Some(ref filter) = plugin.parser.include_types {
    let type_field = &filter.field;
    if let Some(type_val) = extract_string_with_envelope(type_field, payload_json, envelope_json) {
        if !filter.values.contains(&type_val) {
            return Ok(Vec::new());
        }
    }
}

// Timestamp (envelope-aware)
let ts = if let Some(ref ts_field) = plugin.parser.timestamp {
    parse_timestamp_with_envelope(ts_field, payload_json, envelope_json)?
} else {
    Utc::now()
};

// Role (envelope-aware)
let role_str = if let Some(ref role_field) = plugin.parser.role {
    extract_string_with_envelope(role_field, payload_json, envelope_json)?
} else {
    return Err(...);
};
```

### 4. Helper Functions That Should Be Envelope-Aware

**Status:** ✅ All necessary helpers are already envelope-aware

The following helper functions are used in `parse_line()` and correctly receive envelope context:

| Helper Function | Envelope-Aware? | Usage Location |
|----------------|-----------------|----------------|
| `extract_string_with_envelope()` | ✅ Yes | Lines 360, 371, 394, 432, 457, 465, 469 |
| `parse_timestamp_with_envelope()` | ✅ Yes | Line 381 |
| `extract_string()` | ❌ No (non-envelope variant) | Not used in envelope mode |
| `parse_timestamp()` | ❌ No (non-envelope variant) | Not used in envelope mode |

**No missing envelope awareness:** All field extractions in the envelope path use the `*_with_envelope` variants.

## Call Chain Summary

```
parse_line(line, context, plugin)
    │
    ├── Check plugin.source.envelope
    │   ├── If Some(envelope_cfg):
    │   │   ├── Extract type field
    │   │   ├── Get routing action (skip/meta/event)
    │   │   ├── If routing == "skip": return Ok(Vec::new())
    │   │   ├── If routing == "meta": return Ok(Vec::new())  // TODO: metadata accumulation
    │   │   └── If routing == "event":
    │   │       └── Extract payload from payload_field
    │   │           → envelope_json = Some(&raw_json)
    │   │           → payload_json = &payload
    │   └── If None:
    │       → envelope_json = None
    │       → payload_json = &raw_json
    │
    ├── Field Extractions (all use envelope-aware helpers):
    │   ├── include_types filter → extract_string_with_envelope(field, payload_json, envelope_json)
    │   ├── exclude_types filter → extract_string_with_envelope(field, payload_json, envelope_json)
    │   ├── timestamp → parse_timestamp_with_envelope(field, payload_json, envelope_json)
    │   ├── role → extract_string_with_envelope(field, payload_json, envelope_json)
    │   ├── content → extract_string_with_envelope(field, payload_json, envelope_json)
    │   ├── tool_name → extract_string_with_envelope(field, payload_json, envelope_json)
    │   ├── tokens_in → extract_string_with_envelope(field, payload_json, envelope_json)
    │   └── tokens_out → extract_string_with_envelope(field, payload_json, envelope_json)
    │
    └── Return Vec<Event>
```

## Acceptance Criteria Status

✅ **Confirmed envelope_json is available in `parse_line()`**
- Created as `Option<&Value>` at line 269-354
- `Some(&raw_json)` when envelope exists, `None` otherwise

✅ **Understand the call chain to helper functions**
- All field extractions use `extract_string_with_envelope()` or `parse_timestamp_with_envelope()`
- Both helpers accept `(path, payload, envelope)` parameters
- Envelope context flows from `parse_line()` → helper functions

✅ **List of helper functions that are envelope-aware**
- `extract_string_with_envelope(path, payload, envelope)` — 7 call sites
- `parse_timestamp_with_envelope(path, payload, envelope)` — 1 call site

## Conclusions

1. **Envelope context is properly available:** The `(envelope_json, payload_json)` tuple is created at the start of `parse_line()` and passed to all helper functions.

2. **All helpers are envelope-aware:** Every field extraction in the envelope path uses the `*_with_envelope` variant. No non-envelope-aware helpers are used when envelope mode is active.

3. **Implementation is consistent:** The envelope-aware pattern (`^` prefix → envelope first, then payload fallback) is uniformly applied across all field extractions.

4. **No gaps found:** The investigation did not identify any helper functions that should be envelope-aware but are not.

## Notes

- **Read-only investigation:** No code changes were made during this investigation.
- **Meta routing TODO:** Lines 287-291 contain a TODO comment about metadata accumulation for "meta" type routing. Currently, meta-type lines return empty Vec (no events emitted). Future work should accumulate session-level metadata from these lines.
