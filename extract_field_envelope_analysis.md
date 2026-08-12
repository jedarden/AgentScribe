# Analysis: `extract_field_envelope` Implementation

## Summary
The caret-prefix (`^`) resolution for envelope-based field extraction is **already implemented** and working correctly in the current codebase. The system handles envelope unwrapping through the `extract_with_envelope` function and its variants.

## Current Implementation

### Core Function: `extract_with_envelope` (lines 142-164 in `src/parser/mod.rs`)

```rust
pub fn extract_with_envelope(
    path: &str,
    payload: &Value,
    envelope: Option<&Value>,
) -> Option<Value> {
    if path.starts_with('^') {
        // Extract from envelope
        if let Some(envelope_path) = path.strip_prefix('^') {
            if let Some(env) = envelope {
                extract_field(env, envelope_path)
            } else {
                None  // No envelope available
            }
        } else {
            None
        }
    } else {
        // Extract from payload
        extract_field(payload, path)
    }
}
```

**Behavior:**
- Paths starting with `^` → extract from envelope (after stripping the `^`)
- Paths without `^` → extract from payload
- Missing envelope with `^` prefix → returns `None`

### Wrapper Functions

1. **`extract_string_with_envelope`** (lines 170-183)
   - Wraps `extract_with_envelope` and converts result to string
   - Handles String, Number, Bool, Null coercion

2. **`parse_timestamp_with_envelope`** (lines 190-224)
   - Wraps `extract_string_with_envelope` for timestamp parsing
   - Supports ISO 8601, Unix epoch, and UTC-naive formats

## Usage in Parsers

### JSONL Parser (`src/parser/jsonl.rs`)

The JSONL parser correctly uses envelope-aware extraction:

```rust
// Timestamp extraction (line 287)
parse_timestamp_with_envelope(ts_field, payload_json, envelope_json)

// Role extraction (line 300)
extract_string_with_envelope(role_field, payload_json, envelope_json)

// Content extraction (line 338)
extract_string_with_envelope(content_field, payload_json, envelope_json)

// Tool name extraction (line 363)
extract_string_with_envelope(tool_field, payload_json, envelope_json)

// Token extraction (lines 371, 375)
extract_string_with_envelope(f, payload_json, envelope_json)
```

### Envelope Unwrapping

The JSONL parser implements envelope unwrapping at lines 44-100 (`unwrap_envelope` function):
- Extracts the `type_field` from the raw JSON
- Routes based on type: `"skip"`, `"meta"`, or `"event"`
- For `"event"` types, extracts the `payload_field` content
- Returns both payload and envelope JSON for downstream processing

## Current Flow

```
Raw JSON Line (e.g., Codex format)
    ↓
unwrap_envelope() - extracts payload and envelope
    ↓
parse_jsonl_line() - receives payload_json + envelope_json
    ↓
Field extraction via extract_string_with_envelope()
    ├─ "^timestamp" → extracted from envelope_json
    ├─ "^session_id" → extracted from envelope_json
    ├─ "role" → extracted from payload_json
    └─ "content" → extracted from payload_json
    ↓
Canonical Event with correct field values
```

## Test Coverage

Comprehensive test coverage exists (lines 298-417):
- `test_extract_with_envelope_from_envelope` - caret prefix extracts from envelope
- `test_extract_with_envelope_from_payload` - no caret extracts from payload
- `test_extract_with_envelope_dot_notation_from_envelope` - nested envelope fields
- `test_extract_with_envelope_dot_notation_from_payload` - nested payload fields
- `test_extract_with_envelope_no_envelope_fallback_to_payload` - missing envelope handling
- `test_extract_with_envelope_caret_prefix_no_envelope_returns_empty` - error case
- `test_extract_with_envelope_missing_field_from_envelope` - missing field handling
- `test_extract_with_envelope_missing_field_from_payload` - missing field handling
- `test_extract_with_envelope_array_from_envelope` - array indexing
- `test_extract_with_envelope_array_from_payload` - array indexing
- `test_extract_with_envelope_empty_path` - empty path handling
- `test_extract_with_envelope_only_caret_prefix` - lone caret handling

## Potential Issues Found

### 1. `extract_field_recursive` in `src/scraper/mod.rs` (lines 710-716)

This function does NOT handle caret-prefix resolution:

```rust
fn extract_field_recursive(&self, value: &Value, path: &str) -> Option<Value> {
    let mut current = value;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    Some(current.clone())
}
```

**Usage locations:**
- Line 664: Model detection from metadata
- Line 691: Model detection from companion metadata files

**Impact:** If a plugin specifies `^model` in model detection config, this will fail to extract from the envelope correctly.

### 2. `extract_from_tool_call` in `src/scraper/file_path_extractor.rs` (line 169)

Uses basic `extract_field` without envelope awareness:

```rust
if let Some(field) = extract_field(tool_call, field_path) {
```

**Impact:** If a plugin specifies a tool_call_field with `^` prefix (e.g., `^input.file_path`), this won't work correctly.

### 3. Other parsers may not use envelope-aware extraction

Need to verify:
- `src/parser/json_array.rs` - uses basic `extract_field`
- `src/parser/sqlite.rs` - uses basic `extract_field`
- `src/parser/json_tree.rs` - may have similar issues
- `src/parser/markdown.rs` - may not be relevant (no JSON)

## Conclusion

**The caret-prefix resolution is implemented and working correctly in the main parsing flow.** The `extract_with_envelope` function and its variants properly handle envelope-based field extraction for the JSONL parser.

**However**, there are edge cases and utility functions that don't use envelope-aware extraction:
1. Model detection in scraper (`extract_field_recursive`)
2. File path extraction from tool calls (`extract_from_tool_call`)
3. Potentially other format parsers

**Recommendation:** Update all field extraction points to use envelope-aware variants (`extract_with_envelope`, `extract_string_with_envelope`, `parse_timestamp_with_envelope`) consistently throughout the codebase.
