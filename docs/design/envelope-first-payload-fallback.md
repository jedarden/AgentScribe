# Design: Envelope-First with Payload Fallback for Caret-Prefixed Fields

## Status: Ready to Implement

## Overview

**Current State:** Caret-prefix (`^`) resolution is **already implemented** in `src/parser/mod.rs` via the `extract_with_envelope` function. The system correctly routes fields to envelope or payload based on the `^` prefix.

**Proposed Enhancement:** Change behavior from "envelope ONLY" to "envelope-first with payload FALLBACK" for caret-prefixed fields. This provides graceful degradation when envelope data is missing or incomplete.

**What's Changing:** Currently, `^field` returns `None` if the envelope is `None` or the field isn't found in the envelope. With this change, it will fallback to `payload.field` in those cases.

## Current Implementation Status

The caret-prefix (`^`) resolution is **already implemented** in `src/parser/mod.rs` via the `extract_with_envelope` function and its variants (`extract_string_with_envelope`, `parse_timestamp_with_envelope`).

### Current Behavior

```rust
// Current: extract_with_envelope (lines 142-164 in src/parser/mod.rs)
pub fn extract_with_envelope(
    path: &str,
    payload: &Value,
    envelope: Option<&Value>,
) -> Option<Value> {
    if path.starts_with('^') {
        // Extract from envelope ONLY - no fallback
        if let Some(envelope_path) = path.strip_prefix('^') {
            if let Some(env) = envelope {
                extract_field(env, envelope_path)
            } else {
                None  // No envelope available - returns None, NOT falling back
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

**Current resolution order:**
- `^timestamp` → envelope ONLY, returns `None` if envelope is `None` or field not found
- `timestamp` → payload ONLY

**Problem:** If envelope is missing or incomplete, fields with `^` prefix fail to extract even if the data exists in the payload.

## Proposed Design: Envelope-First with Payload Fallback

### Resolution Order

For caret-prefixed paths (`^field`):

1. **Try envelope first:** Strip `^` prefix and attempt extraction from envelope JSON
2. **Fallback to payload:** If envelope is `None` OR field not found in envelope, attempt extraction from payload using the path WITHOUT the `^` prefix
3. **Return None:** Only if both envelope and payload extraction fail

### Behavior Matrix

| Path Prefix | Envelope Present | Field in Envelope | Field in Payload | Result |
|-------------|-------------------|-------------------|------------------|--------|
| `^field` | Yes ✅ | Yes ✅ | Any | Returns envelope value (priority) |
| `^field` | Yes ✅ | No ❌ | Yes ✅ | Returns payload value (fallback) |
| `^field` | Yes ✅ | No ❌ | No ❌ | Returns None (not found) |
| `^field` | No ❌ | N/A | Yes ✅ | Returns payload value (fallback) |
| `^field` | No ❌ | N/A | No ❌ | Returns None (not found) |
| `field` | Any | Any | Yes ✅ | Returns payload value (no ^ prefix) |
| `field` | Any | Any | No ❌ | Returns None (not found) |

### Edge Cases

#### 1. Missing Envelope (envelope is None)
**Scenario:** Parser operating on a format without envelope structure (e.g., plain JSONL)

```toml
# Plugin config
[parser]
timestamp = "^timestamp"  # Try envelope first, fallback to payload
```

**Resolution:**
- Envelope is `None` → skip envelope extraction
- Fallback to payload → extract `payload.timestamp` (path without `^`)
- Returns payload timestamp if present, None otherwise

#### 2. Missing Field in Envelope
**Scenario:** Envelope exists but field is absent or null

```json
// Envelope: {"timestamp": "2026-03-16T12:00:00Z", "type": "event"}
// Payload: {"timestamp": "2026-03-16T12:05:00Z", "role": "user"}
// Config: timestamp = "^timestamp"
```

**Resolution:**
- Try envelope → found `"2026-03-16T12:00:00Z"` (returns envelope value)
- Payload value is ignored (envelope has priority)

```json
// Envelope: {"type": "event"}  // no timestamp field
// Payload: {"timestamp": "2026-03-16T12:05:00Z", "role": "user"}
// Config: timestamp = "^timestamp"
```

**Resolution:**
- Try envelope → field not found
- Fallback to payload → found `"2026-03-16T12:05:00Z"` (returns payload value)

#### 3. Nested Field Paths with Caret Prefix
**Scenario:** Dot-notation paths with envelope fallback

```toml
[parser]
model = "^metadata.model"  # Try envelope.metadata.model first
content = "message.content"  # Payload only (no ^)
```

**Resolution for `^metadata.model`:**
- Try `envelope.metadata.model` → if found, return it
- Fallback to `payload.metadata.model` → if envelope missing or field absent
- Return None if both fail

#### 4. Array Indexing with Caret Prefix
**Scenario:** Extracting from arrays within envelope

```toml
[parser]
content = "^content[0].text"  # Try envelope.content[0].text first
```

**Resolution:**
- Try `envelope.content[0].text` → if found, return it
- Fallback to `payload.content[0].text` → array indexing supported
- Return None if both fail

#### 5. Special Case: Only Caret Prefix
**Scenario:** Path is just `^` with no field name

```toml
[parser]
field = "^"  # Invalid usage
```

**Resolution:** Return None (already handled by current implementation - defensive check)

#### 6. Empty String Path
**Scenario:** Empty path after stripping caret

```toml
[parser]
field = "^"  # Results in empty string after strip_prefix
```

**Resolution:** Return None (already handled by `extract_field` - returns None for empty path)

#### 7. Null vs. Missing
**Scenario:** Field exists in envelope but value is null

```json
// Envelope: {"model": null}
// Payload: {"model": "gpt-4"}
// Config: model = "^model"
```

**Resolution:**
- Try envelope → found `Value::Null`
- Current `extract_string_with_envelope` converts `Null` to empty string `""`
- Returns `""` (empty string), NOT falling back to payload

**Rationale:** `null` is a valid JSON value indicating "explicitly absent". Fallback should only trigger on missing field, not null value.

## Implementation Strategy

### Option A: Modify `extract_with_envelope` (Recommended)

**Location:** `src/parser/mod.rs`, lines 142-164

**Changes:**

```rust
pub fn extract_with_envelope(
    path: &str,
    payload: &Value,
    envelope: Option<&Value>,
) -> Option<Value> {
    if path.starts_with('^') {
        // Strip the caret prefix
        if let Some(envelope_path) = path.strip_prefix('^') {
            // Try envelope first
            if let Some(env) = envelope {
                if let Some(value) = extract_field(env, envelope_path) {
                    return Some(value);  // Found in envelope
                }
            }
            // Fallback to payload (using path without ^)
            extract_field(payload, envelope_path)
        } else {
            // Should not happen due to starts_with check
            None
        }
    } else {
        // No caret prefix - extract from payload only
        extract_field(payload, path)
    }
}
```

**Pros:**
- Single point of change
- Automatically applies to all wrapper functions (`extract_string_with_envelope`, `parse_timestamp_with_envelope`)
- Maintains existing API contract

**Cons:**
- Changes behavior for existing code that expects None when envelope is missing
- Requires updating tests to reflect new fallback behavior

### Option B: Add New Function, Keep Old

**Location:** `src/parser/mod.rs`

**Add new function:**

```rust
pub fn extract_with_envelope_fallback(
    path: &str,
    payload: &Value,
    envelope: Option<&Value>,
) -> Option<Value> {
    // Envelope-first with payload fallback implementation
    // ... (same as Option A)
}

// Keep existing extract_with_envelope unchanged
pub fn extract_with_envelope(
    path: &str,
    payload: &Value,
    envelope: Option<&Value>,
) -> Option<Value> {
    // Current implementation (no fallback)
    // ... (existing code)
}
```

**Pros:**
- Backwards compatible - existing code unaffected
- Clear separation of behaviors

**Cons:**
- Function explosion (3 variants: extract_field, extract_with_envelope, extract_with_envelope_fallback)
- Requires updating call sites to use new function
- Maintenance burden

### Recommended: Option A

Modify `extract_with_envelope` to implement fallback. This is the cleanest approach because:

1. **Semantic correctness:** Caret prefix already implies "prefer envelope" - fallback is a natural extension
2. **Single source of truth:** All wrapper functions inherit the new behavior
3. **Consistent API:** No new functions to maintain
4. **Better user experience:** Plugin authors get graceful degradation without extra work

## Configuration Examples

### Example 1: Codex Plugin (Real Rollout Format)

```toml
# plugins/codex.toml
[source]
format = "jsonl"

[source.envelope]
payload_field = "payload"
type_field = "type"
type_routing = { session_meta = "meta", response_item = "event", turn_context = "meta", event_msg = "skip" }

[parser]
timestamp = "^timestamp"        # Try envelope.timestamp, fallback to payload.timestamp
role = "role"                   # Payload only (no ^)
content = "message.content[0].text"  # Nested payload field
model = "^model"                # Try envelope.model (from turn_context), fallback to payload.model
```

**Behavior:**
- `^timestamp` → prefers envelope timestamp (wrapper), falls back to payload if missing
- `role` → payload only (no caret)
- `^model` → tries envelope metadata first, gracefully degrades to payload

### Example 2: Mixed Envelope/Payload Strategy

```toml
# plugins/mixed-format.toml
[parser]
# Fields that SHOULD come from envelope (metadata)
timestamp = "^timestamp"
session_id = "^session_id"
model = "^metadata.model"
version = "^metadata.version"

# Fields that come from payload (event data)
role = "role"
content = "content"
tool_name = "tool.name"

# Fields with optional envelope enhancement
project = "^cwd"  # Try envelope.cwd (working directory), fallback to payload.cwd
```

**Rationale:** Distinguishes between metadata fields (envelope-sourced) and event fields (payload-sourced), with optional graceful fallback.

## Migration Path

### Phase 1: Modify Core Function
1. Update `extract_with_envelope` in `src/parser/mod.rs` to implement fallback
2. Run existing tests - identify any that assume "no fallback" behavior
3. Update tests to reflect new expected behavior

### Phase 2: Update Call Sites
1. Audit all usages of caret-prefixed fields in bundled plugins
2. Verify plugin configs work correctly with new fallback behavior
3. Update plugin documentation if needed

### Phase 3: Update Edge Cases
1. Fix `extract_field_recursive` in `src/scraper/mod.rs` (lines 710-716) to use envelope-aware extraction
2. Fix `extract_from_tool_call` in `src/scraper/file_path_extractor.rs` (line 169)
3. Audit other format parsers for envelope-aware extraction

### Phase 4: Documentation
1. Update `docs/plan.md` Phase 9 section to document fallback behavior
2. Update `plugins/BUILDING_PLUGINS.md` with caret-prefix semantics
3. Add examples to plugin documentation

## Testing Strategy

### Unit Tests (src/parser/mod.rs)

Add tests for fallback behavior:

```rust
#[test]
fn test_extract_with_envelope_fallback_to_payload() {
    let envelope = json!({"type": "event"});
    let payload = json!({"model": "gpt-4", "role": "user"});
    
    // Try ^model - not in envelope, should fallback to payload
    let result = extract_with_envelope("^model", &payload, Some(&envelope));
    assert_eq!(result, Some(json!("gpt-4")));
}

#[test]
fn test_extract_with_envelope_priority() {
    let envelope = json!({"model": "claude-sonnet-4"});
    let payload = json!({"model": "gpt-4"});
    
    // Envelope has priority
    let result = extract_with_envelope("^model", &payload, Some(&envelope));
    assert_eq!(result, Some(json!("claude-sonnet-4")));
}

#[test]
fn test_extract_with_envelope_none_envelope_fallback() {
    let payload = json!({"timestamp": "2026-03-16T12:00:00Z"});
    
    // No envelope - should fallback to payload
    let result = extract_with_envelope("^timestamp", &payload, None);
    assert_eq!(result, Some(json!("2026-03-16T12:00:00Z")));
}

#[test]
fn test_extract_with_envelope_both_missing() {
    let envelope = json!({"type": "event"});
    let payload = json!({"role": "user"});
    
    // Field missing in both envelope and payload
    let result = extract_with_envelope("^model", &payload, Some(&envelope));
    assert_eq!(result, None);
}

#[test]
fn test_extract_with_envelope_null_value_no_fallback() {
    let envelope = json!({"model": null});
    let payload = json!({"model": "gpt-4"});
    
    // Envelope has null (explicitly absent), should NOT fallback
    let result = extract_with_envelope("^model", &payload, Some(&envelope));
    assert_eq!(result, Some(json!(null)));
}
```

### Integration Tests (tests/plugin_conformance.rs)

Test envelope unwrapping with fallback across different plugins:

```rust
#[test]
fn test_codex_plugin_envelope_fallback() {
    // Test real Codex format with missing envelope fields
    // Verify payload fallback works correctly
}

#[test]
fn test_jsonl_parser_caret_prefix_fallback() {
    // Test JSONL parser with various envelope/payload combinations
}
```

## Compatibility Notes

### Breaking Changes

**Behavior change:** Existing code using caret-prefix that expects `None` when envelope is missing will now receive payload values.

**Impact:** Low - most use cases should benefit from graceful degradation.

**Mitigation:** Document the change in CHANGELOG.md and provide migration guide for plugin authors.

### Backwards Compatibility

**No API changes:** Function signatures remain identical.

**Existing plugins:** Continue to work - envelope extraction unchanged, only adds fallback capability.

**No caret prefix:** Behavior unchanged - still extracts from payload only.

## Open Questions

### Q1: Should we add explicit "no fallback" mode?

**Proposal:** Add `^^` (double caret) prefix to mean "envelope ONLY, no fallback".

**Resolution:** Deferred - not required for Phase 9. Can be added later if plugin authors need strict envelope-only mode.

### Q2: How should we handle null values in envelope?

**Current behavior:** `extract_string_with_envelope` converts `null` to empty string `""`.

**Proposal:** Keep current behavior - `null` is a valid value, not "missing".

**Resolution:** Document this behavior clearly for plugin authors.

### Q3: Should we log fallback events?

**Proposal:** Add debug-level logging when fallback occurs.

**Resolution:** No - fallback is expected behavior, not an error. Plugin authors can test their configs with `agentscribe plugins validate`.

## Summary

**Design:** Envelope-first with payload fallback for caret-prefixed fields.

**Implementation:** Modify `extract_with_envelope` in `src/parser/mod.rs` to try envelope first, fallback to payload if envelope is `None` or field not found.

**Behavior:**
- `^field` → try `envelope.field`, fallback to `payload.field`
- `field` → `payload.field` only (unchanged)

**Benefits:**
- Graceful degradation when envelope data is incomplete
- More robust plugin configurations
- Better user experience for plugin authors

**Migration:** Update core function, tests, and documentation. Backwards compatible with existing plugins.
