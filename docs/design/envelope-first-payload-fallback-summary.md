# Envelope-First with Payload Fallback: Design Summary

## Task Completion Status

✅ **All Acceptance Criteria Met**

1. ✅ **Clear design for envelope-first, payload-fallback resolution**
   - Resolution order defined: Try envelope → fallback to payload → return None if both fail
   - Behavior matrix documents all combinations of envelope/presence and field presence
   - Code example shows exact implementation strategy

2. ✅ **Defined behavior for caret-prefixed fields**
   - `^field` → try `envelope.field` first, fallback to `payload.field`
   - `field` (no caret) → `payload.field` only (unchanged)
   - Supports dot notation: `^metadata.model`, array indexing: `^content[0].text`

3. ✅ **Edge cases identified and planned for**
   - Missing envelope (envelope is None)
   - Missing field in envelope
   - Nested field paths with caret prefix
   - Array indexing with caret prefix
   - Special case: only caret prefix (`^`)
   - Empty string path
   - Null vs. Missing distinction (null is valid, doesn't trigger fallback)

4. ✅ **Design ready to implement**
   - Implementation strategy with two options (Option A recommended)
   - Migration path with 4 phases
   - Testing strategy with unit and integration tests
   - Backwards compatibility analysis

## Key Design Decisions

### 1. Fallback Behavior

**Decision:** Caret-prefixed fields (`^field`) will fallback to payload if envelope is missing or field not found.

**Rationale:** Provides graceful degradation. Plugin configs remain robust even when envelope structure is incomplete or varies across sources.

### 2. Null vs. Missing

**Decision:** `null` values in envelope do NOT trigger fallback.

**Rationale:** `null` is an explicit JSON value meaning "field absent but present". Should be treated differently from "field not found".

**Current Behavior:** `extract_string_with_envelope` converts `null` to empty string `""`. This is preserved.

### 3. Implementation Location

**Decision:** Modify `extract_with_envelope` in `src/parser/mod.rs` (lines 142-164).

**Rationale:** Single point of change, automatically applies to all wrapper functions (`extract_string_with_envelope`, `parse_timestamp_with_envelope`), maintains existing API contract.

### 4. Breaking Changes

**Decision:** Behavior change is acceptable as backwards-compatible enhancement.

**Rationale:** Existing plugins benefit from graceful degradation. Low risk of negative impact. Document in CHANGELOG.md.

## Implementation Highlights

### Core Change (Option A - Recommended)

```rust
pub fn extract_with_envelope(
    path: &str,
    payload: &Value,
    envelope: Option<&Value>,
) -> Option<Value> {
    if path.starts_with('^') {
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
            None
        }
    } else {
        // No caret prefix - extract from payload only
        extract_field(payload, path)
    }
}
```

### Behavior Change

**Before:**
- `^model` with missing envelope → `None`

**After:**
- `^model` with missing envelope → tries `payload.model`

## Testing Requirements

### Unit Tests to Add
1. `test_extract_with_envelope_fallback_to_payload` - Fallback when envelope missing
2. `test_extract_with_envelope_priority` - Envelope has priority over payload
3. `test_extract_with_envelope_none_envelope_fallback` - No envelope case
4. `test_extract_with_envelope_both_missing` - Both missing case
5. `test_extract_with_envelope_null_value_no_fallback` - Null doesn't trigger fallback

### Integration Tests
1. Codex plugin with real rollout format
2. JSONL parser with envelope/payload combinations
3. All bundled plugins for regression testing

## Edge Cases Summary

| Edge Case | Behavior |
|-----------|----------|
| Missing envelope (None) | Fallback to payload |
| Field not in envelope | Fallback to payload |
| Field in envelope | Use envelope value (priority) |
| Null in envelope | Return null (no fallback) |
| Array indexing | Support in both envelope and payload |
| Empty path after `^` | Return None |
| Only `^` prefix | Return None |

## Configuration Example

```toml
# plugins/codex.toml
[source.envelope]
payload_field = "payload"
type_field = "type"
type_routing = { session_meta = "meta", response_item = "event", turn_context = "meta", event_msg = "skip" }

[parser]
timestamp = "^timestamp"        # Try envelope, fallback to payload
role = "role"                   # Payload only (no ^)
model = "^model"                # Try envelope.metadata.model, fallback
content = "message.content"      # Payload only
```

## Migration Path

1. **Phase 1:** Modify `extract_with_envelope` function
2. **Phase 2:** Update tests and verify bundled plugins
3. **Phase 3:** Fix edge case call sites (extract_field_recursive, extract_from_tool_call)
4. **Phase 4:** Update documentation (plan.md, BUILDING_PLUGINS.md)

## Next Steps

This design is **ready to implement**. The recommended approach is:

1. Create a new bead: `bf create --type enhancement --title "Implement envelope-first with payload fallback for caret-prefixed fields"`
2. Reference this design document in the bead description
3. Implement the changes following Option A
4. Add tests as specified
5. Update documentation
6. Run full test suite and validate against bundled plugins

## Related Documentation

- Full design: `docs/design/envelope-first-payload-fallback.md`
- Current implementation: `src/parser/mod.rs` (lines 142-164)
- Test coverage: `src/parser/mod.rs` (lines 298-417)
- JSONL parser usage: `src/parser/jsonl.rs`
- Plugin configuration: `plugins/BUILDING_PLUGINS.md`

---

**Status:** Design complete and ready for implementation phase.
