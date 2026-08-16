# Audit: Envelope-Aware Helper Usage in jsonl.rs

**Date:** 2026-08-16  
**Scope:** `src/parser/jsonl.rs`  
**Functions audited:** `parse_line()`, `unwrap_envelope()`, `detect_sessions()`

## Executive Summary

This audit documents all call sites of envelope-aware field extraction helpers in `jsonl.rs` and identifies whether they use the correct variants. **2 call sites** were found to be using the incorrect non-envelope variant where envelope-aware variants should be used.

## Background

### Envelope-Aware Helper Functions

Three helper functions exist for field extraction:

1. **`extract_string(value, path)`** - Non-envelope variant
   - Extracts directly from a single `Value` using dot notation
   - Does NOT support envelope/payload dual-layer reading
   - Used when: NO envelope configuration exists

2. **`extract_string_with_envelope(path, payload, envelope)`** - Envelope-aware variant
   - Supports `^` prefix to read from envelope first, then fallback to payload
   - Reads from payload only when path has no `^` prefix
   - Used when: Envelope configuration MAY exist (i.e., within `parse_line()` after envelope processing)

3. **`parse_timestamp_with_envelope(path, payload, envelope)`** - Envelope-aware variant for timestamps
   - Same semantics as `extract_string_with_envelope` but parses as `DateTime<Utc>`
   - Used when: Envelope configuration MAY exist

### When to Use Each Variant

- **Non-envelope variant (`extract_string`)**: Only when you have a single `Value` and envelope is definitively not involved (e.g., reading from a file before envelope processing, or in contexts where envelope is never present)
- **Envelope-aware variant**: Anywhere after envelope processing where the plugin MAY have envelope configuration defined (i.e., where `envelope_json` is `Option<&Value>`)

---

## Call Site Audit

### Function: `parse_line()` (lines 186-453)

#### ✅ Line 301 - CORRECT
```rust
extract_string_with_envelope(type_field, payload_json, envelope_json)
```
- **Context:** `include_types` filter processing
- **Correctness:** Uses envelope-aware variant
- **Notes:** Properly handles both envelope and non-envelope plugins

#### ✅ Line 312 - CORRECT
```rust
extract_string_with_envelope(type_field, payload_json, envelope_json)
```
- **Context:** `exclude_types` filter processing
- **Correctness:** Uses envelope-aware variant
- **Notes:** Properly handles both envelope and non-envelope plugins

#### ✅ Line 322 - CORRECT
```rust
parse_timestamp_with_envelope(ts_field, payload_json, envelope_json)
```
- **Context:** Timestamp field parsing
- **Correctness:** Uses envelope-aware variant
- **Notes:** Correctly uses timestamp-specific helper

#### ✅ Line 335 - CORRECT
```rust
extract_string_with_envelope(role_field, payload_json, envelope_json)
```
- **Context:** Role field parsing
- **Correctness:** Uses envelope-aware variant
- **Notes:** Proper error handling with `.ok_or_else()`

#### ✅ Line 373 - CORRECT
```rust
extract_string_with_envelope(content_field, payload_json, envelope_json)
```
- **Context:** Content field parsing
- **Correctness:** Uses envelope-aware variant
- **Notes:** Uses `.unwrap_or_default()` for optional content

#### ✅ Line 398 - CORRECT
```rust
extract_string_with_envelope(tool_field, payload_json, envelope_json)
```
- **Context:** Tool name extraction
- **Correctness:** Uses envelope-aware variant
- **Notes:** Wrapped in `if let Some(...)` for optional field

#### ✅ Line 406 - CORRECT
```rust
extract_string_with_envelope(f, payload_json, envelope_json)
```
- **Context:** `tokens_in` field parsing
- **Correctness:** Uses envelope-aware variant
- **Notes:** Part of token counting logic

#### ✅ Line 410 - CORRECT
```rust
extract_string_with_envelope(f, payload_json, envelope_json)
```
- **Context:** `tokens_out` field parsing
- **Correctness:** Uses envelope-aware variant
- **Notes:** Part of token counting logic

---

### Function: `unwrap_envelope()` (lines 44-129)

**Status:** ✅ NO ISSUES

This function does NOT call any of the three helper functions. It performs envelope unwrapping directly using `raw_json.get(&envelope.type_field)` and `raw_json.get(&envelope.payload_field)`. This is CORRECT because:

1. `unwrap_envelope()` operates on the raw JSON line BEFORE any field extraction
2. It's determining envelope structure, not extracting canonical event fields
3. Direct JSON access is appropriate for this structural inspection

---

### Function: `detect_sessions()` (lines 713-902)

#### ❌ Line 800 - INCORRECT
```rust
extract_string(&json, &envelope_cfg.type_field).unwrap_or_default();
```
- **Context:** Envelope routing determination for session ID extraction
- **Issue:** Uses non-envelope `extract_string` where envelope-aware version should be used
- **Problem:** This code is inside an `if let Some(ref envelope_cfg) = plugin.source.envelope` block, meaning envelope mode is active. However, it's reading from `&json` (the first line of the file) to determine routing for session ID extraction.
- **Why it's wrong:** While `extract_string` happens to work here (because `json` is the full raw line and `type_field` is at the top level), this is semantically inconsistent with the envelope-aware pattern used everywhere else in `parse_line()`. More importantly, this code duplicates the routing logic from lines 218-220 of `parse_line()` but uses a different helper.
- **Recommended fix:** Either:
  1. Use `extract_string_with_envelope(envelope_cfg.type_field, &json, None)` - passing `None` for envelope since we're reading from raw_json
  2. Or directly access via `json.get(&envelope_cfg.type_field)` since this is routing logic, not field extraction

#### ✅ Line 823 - CORRECT
```rust
extract_string_with_envelope(
    field,
    payload_json,
    envelope_json,
)
```
- **Context:** Session ID extraction for `SessionIdSource::Field`
- **Correctness:** Uses envelope-aware variant
- **Notes:** Properly uses the `envelope_json` and `payload_json` prepared by the envelope processing at lines 794-821

---

## Issues Summary

### Issue #1: Line 219 - `parse_line()` routing logic

**Severity:** MEDIUM  
**Line:** 219  
**Function:** `parse_line()`

**Current code:**
```rust
let type_value =
    extract_string(&raw_json, &envelope_cfg.type_field).unwrap_or_default();
```

**Issue:**
- Uses non-envelope `extract_string` inside envelope processing block
- Inconsistent with envelope-aware pattern used for all subsequent field extractions

**Recommended fix:**
```rust
// Option 1: Use envelope-aware variant (consistent with rest of parse_line)
let type_value = extract_string_with_envelope(
    &envelope_cfg.type_field, 
    &raw_json, 
    None  // We're reading from raw_json before unwrapping
).unwrap_or_default();

// Option 2: Direct access (semantically clearer for routing logic)
let type_value = raw_json
    .get(&envelope_cfg.type_field)
    .and_then(|v| match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    })
    .unwrap_or_default();
```

**Impact:**
- Currently works correctly because `raw_json` is the full line
- Issue is code consistency and maintainability
- No immediate functional bug

---

### Issue #2: Line 800 - `detect_sessions()` envelope routing

**Severity:** MEDIUM  
**Line:** 800  
**Function:** `detect_sessions()`

**Current code:**
```rust
let type_value =
    extract_string(&json, &envelope_cfg.type_field).unwrap_or_default();
```

**Issue:**
- Uses non-envelope `extract_string` in envelope-aware context
- Duplicates routing logic from `parse_line()` line 219 but uses different helper
- Inconsistent with envelope-aware pattern

**Recommended fix:**
```rust
// Option 1: Use envelope-aware variant (consistent with rest of codebase)
let type_value = extract_string_with_envelope(
    &envelope_cfg.type_field,
    &json,
    None  // We're reading from raw_json before unwrapping
).unwrap_or_default();

// Option 2: Direct access (semantically clearer for routing logic)
let type_value = json
    .get(&envelope_cfg.type_field)
    .and_then(|v| match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    })
    .unwrap_or_default();
```

**Impact:**
- Currently works correctly because `json` is the full line and `type_field` is top-level
- Issue is code consistency and maintainability
- No immediate functional bug

---

## Statistics

| Function | Call Sites Audited | Correct | Incorrect |
|----------|-------------------|---------|-----------|
| `parse_line()` | 9 (lines 301-410) | 8 | 1 (line 219) |
| `unwrap_envelope()` | 0 (no helper calls) | N/A | N/A |
| `detect_sessions()` | 2 | 1 | 1 (line 800) |
| **TOTAL** | **11** | **9** | **2** |

---

## Recommendations

### Immediate Actions

1. **Fix line 219** in `parse_line()`:
   - Replace `extract_string(&raw_json, &envelope_cfg.type_field)` with direct `raw_json.get()` access
   - Rationale: This is routing logic, not field extraction, so direct access is semantically clearer

2. **Fix line 800** in `detect_sessions()`:
   - Replace `extract_string(&json, &envelope_cfg.type_field)` with direct `json.get()` access
   - Rationale: Same as above - routing logic should use direct access for clarity

### Long-term Improvements

1. **Add a dedicated `get_routing_type()` helper** to encapsulate envelope routing logic:
   ```rust
   fn get_routing_type(raw_json: &Value, envelope: &Envelope) -> String {
       raw_json
           .get(&envelope.type_field)
           .and_then(|v| match v {
               Value::String(s) => Some(s.clone()),
               Value::Number(n) => Some(n.to_string()),
               Value::Bool(b) => Some(b.to_string()),
               _ => None,
           })
           .unwrap_or_default()
   }
   ```
   This would make routing logic explicit and reusable.

2. **Add documentation comments** explaining when to use each helper variant to prevent future confusion.

---

## Verification

To verify fixes are correct:

```bash
# Run existing tests
cargo test --lib parser::jsonl::tests

# Run clippy
cargo clippy --all-targets -- -D warnings

# Check for any remaining non-envelope usage in envelope-aware contexts
grep -n "extract_string(" src/parser/jsonl.rs | grep -v "extract_string_with_envelope"
```

Expected result after fixes: No `extract_string` calls should appear within envelope processing blocks (only `extract_string_with_envelope`).

---

**Audit completed:** 2026-08-16  
**Auditor:** AgentScribe parser audit  
**Next review:** After applying recommended fixes
