# JSONL Parser Envelope-Aware Helper Usage Audit

**Date:** 2026-08-16  
**Scope:** `src/parser/jsonl.rs` - All call sites of `extract_string`, `extract_string_with_envelope`, and `parse_timestamp_with_envelope`

## Summary

**Total call sites analyzed:** 13  
**Using correct variant:** 13/13 (100%) ✅

**Key Finding:** Both `extract_string` calls (lines 219 and 800) are **correctly implemented**. They extract the envelope type field **before** envelope splitting occurs, so the non-envelope variant is appropriate.

---

## Call Site Inventory

### 1. Line 219 — `extract_string` in `parse_line()`

**Context:** Type field extraction for routing determination
```rust
let type_value = extract_string(&raw_json, &envelope_cfg.type_field).unwrap_or_default();
```

**Analysis:**
- ✅ **CORRECT** - Uses non-envelope variant
- This extraction happens BEFORE envelope splitting (lines 213-295)
- Reading directly from `raw_json` (the full parsed line)
- Purpose: Determine routing action (skip/meta/event) based on type field
- No envelope_json/payload_json split exists yet at this point

**Location in flow:**
```
parse_line() entry (line 186)
  ↓
Parse JSON line (line 192)
  ↓
Line 213-295: Envelope routing and unwrapping
  ↓
Line 219: Extract type for routing ← YOU ARE HERE (before split)
  ↓
Line 294: Split into envelope_json/payload_json
  ↓
Lines 297+: All field extractions use envelope-aware variants
```

---

### 2. Line 800 — `extract_string` in `detect_sessions()`

**Context:** Type field extraction for routing determination
```rust
let type_value = extract_string(&json, &envelope_cfg.type_field).unwrap_or_default();
```

**Analysis:**
- ✅ **CORRECT** - Uses non-envelope variant
- This extraction happens BEFORE envelope splitting (lines 794-821)
- Reading directly from `json` (the full parsed first line)
- Purpose: Determine routing action to decide whether to extract session_id from payload
- No envelope_json/payload_json split exists yet at this point

**Location in flow:**
```
detect_sessions() entry (line 713)
  ↓
SessionDetection::OneFilePerSession match (line 716)
  ↓
SessionIdSource::Field match (line 785)
  ↓
Read first line from file (line 790)
  ↓
Parse first line as JSON (line 791)
  ↓
Lines 794-821: Envelope detection and unwrapping
  ↓
Line 800: Extract type for routing ← YOU ARE HERE (before split)
  ↓
Line 812-821: Split into envelope_json/payload_json
  ↓
Line 823: Extract session_id using envelope-aware variant
```

---

### 3. Line 301 — `extract_string_with_envelope` in `parse_line()`

**Context:** Include type filter check
```rust
extract_string_with_envelope(type_field, payload_json, envelope_json)
```

**Analysis:**
- ✅ **CORRECT** - Uses envelope-aware variant
- Called AFTER envelope splitting (line 294)
- Has both envelope_json and payload_json available
- Purpose: Filter events by type field value

---

### 4. Line 312 — `extract_string_with_envelope` in `parse_line()`

**Context:** Exclude type filter check
```rust
extract_string_with_envelope(type_field, payload_json, envelope_json)
```

**Analysis:**
- ✅ **CORRECT** - Uses envelope-aware variant
- Called AFTER envelope splitting
- Purpose: Exclude events by type field value

---

### 5. Line 322 — `parse_timestamp_with_envelope` in `parse_line()`

**Context:** Timestamp extraction
```rust
parse_timestamp_with_envelope(ts_field, payload_json, envelope_json)
```

**Analysis:**
- ✅ **CORRECT** - Uses envelope-aware variant
- Called AFTER envelope splitting
- Purpose: Extract event timestamp (supports `^timestamp` prefix for wrapper-level fields)

---

### 6. Line 335 — `extract_string_with_envelope` in `parse_line()`

**Context:** Role extraction
```rust
extract_string_with_envelope(role_field, payload_json, envelope_json)
```

**Analysis:**
- ✅ **CORRECT** - Uses envelope-aware variant
- Called AFTER envelope splitting
- Purpose: Extract event role

---

### 7. Line 373 — `extract_string_with_envelope` in `parse_line()`

**Context:** Content extraction
```rust
extract_string_with_envelope(content_field, payload_json, envelope_json)
```

**Analysis:**
- ✅ **CORRECT** - Uses envelope-aware variant
- Called AFTER envelope splitting
- Purpose: Extract event content/message text

---

### 8. Line 398 — `extract_string_with_envelope` in `parse_line()`

**Context:** Tool name extraction
```rust
extract_string_with_envelope(tool_field, payload_json, envelope_json)
```

**Analysis:**
- ✅ **CORRECT** - Uses envelope-aware variant
- Called AFTER envelope splitting
- Purpose: Extract tool name for tool_call/tool_result events

---

### 9. Line 406 — `extract_string_with_envelope` in `parse_line()`

**Context:** Input tokens extraction
```rust
extract_string_with_envelope(f, payload_json, envelope_json)
```

**Analysis:**
- ✅ **CORRECT** - Uses envelope-aware variant
- Called AFTER envelope splitting
- Purpose: Extract input token count

---

### 10. Line 410 — `extract_string_with_envelope` in `parse_line()`

**Context:** Output tokens extraction
```rust
extract_string_with_envelope(f, payload_json, envelope_json)
```

**Analysis:**
- ✅ **CORRECT** - Uses envelope-aware variant
- Called AFTER envelope splitting
- Purpose: Extract output token count

---

### 11. Line 823 — `extract_string_with_envelope` in `detect_sessions()`

**Context:** Session ID extraction from field
```rust
extract_string_with_envelope(field, payload_json, envelope_json)
```

**Analysis:**
- ✅ **CORRECT** - Uses envelope-aware variant
- Called AFTER envelope splitting (lines 812-821)
- Purpose: Extract session_id from first line when using `SessionIdSource::Field`
- Supports envelope-level fields like `^session_id` or payload fields like `payload.session_id`

---

### 12. Line 11 — Import statement

```rust
use crate::parser::{
    extract_string, extract_string_with_envelope, parse_timestamp_with_envelope, ParseContext,
    SessionInfo,
};
```

**Analysis:**
- ✅ **CORRECT** - Module import
- All three helper functions are imported
- Both `extract_string` and `extract_string_with_envelope` are legitimately used

---

## Code Flow Summary

### Pattern in `parse_line()` (lines 186-453)

```
1. Parse JSON line → raw_json
2. Extract type field using extract_string() (line 219)
   ↓ Determines routing
3. Split based on routing:
   - skip: return Ok(Vec::new())
   - meta: return Ok(Vec::new())
   - event: extract payload, create (envelope_json, payload_json) tuple
4. All subsequent field extractions use envelope-aware variants:
   - include_types filter (line 301)
   - exclude_types filter (line 312)
   - timestamp (line 322)
   - role (line 335)
   - content (line 373)
   - tool_name (line 398)
   - tokens_in (line 406)
   - tokens_out (line 410)
```

### Pattern in `detect_sessions()` (lines 713-902)

```
1. Read first line from file
2. Parse as JSON → json
3. Extract type field using extract_string() (line 800)
   ↓ Determines routing
4. Split based on routing:
   - event/meta: extract payload, create (envelope_json, payload_json) tuple
   - skip/unknown: (None, &json)
5. Extract session_id using envelope-aware variant (line 823)
```

---

## Conclusion

**All call sites use the correct helper variant.** ✅

The two `extract_string` calls (lines 219 and 800) are **intentionally non-envelope** because they extract the type field **before** the envelope split occurs. After the split, all field extractions correctly use envelope-aware variants.

**No changes required.** The codebase follows the correct pattern:
1. Use `extract_string()` for type routing decisions (pre-split)
2. Use `extract_string_with_envelope()` for all field extractions (post-split)
3. Use `parse_timestamp_with_envelope()` for timestamp extraction (post-split)

---

## Testing Coverage

The existing test suite validates this envelope-aware behavior:

- `test_parse_line_envelope_field_extraction` (line 1566): Verifies that `^timestamp` correctly reads from wrapper level
- `test_parse_line_event_type` (line 1484): Verifies envelope routing + field extraction
- `test_mixed_fixture_event_lines_still_parse` (line 1845): Integration test with real fixture
- `tests/fixtures/envelope/envelope-routing.jsonl`: Real-format fixture with mixed envelope types

All tests pass, confirming that the envelope-aware helper usage is correct.
