# Lookup-Related Tests Reference

This document catalogs the lookup and field access tests found in the AgentScribe codebase to serve as reference patterns for writing new tests.

## Test File Locations

### Primary Test Files

1. **`/home/coding/AgentScribe/src/parser/mod.rs`** - Core envelope-first field lookup tests
   - Lines 294-400: `extract_with_envelope()` function tests
   - Tests caret-prefix notation, dot notation, fallback behavior

2. **`/home/coding/AgentScribe/src/parser/jsonl.rs`** - JSONL parser envelope field extraction tests
   - Lines 1144-2820: Comprehensive envelope and caret-prefix tests
   - Lines 1969-2820: `^-prefixed envelope field extraction tests` section
   - Lines 1292-1412: `Envelope type routing and field extraction tests` section
   - Lines 1683-1967: `unwrap_envelope unit tests` section

3. **`/home/coding/AgentScribe/src/search.rs`** - Session lookup and search tests
   - Session ID lookup tests
   - Error fingerprint lookup tests

## Naming Conventions Used

### Pattern 1: `test_<function>_<scenario>`
- `test_extract_with_envelope_from_envelope()`
- `test_extract_with_envelope_from_payload()`
- `test_extract_with_envelope_dot_notation_from_envelope()`

### Pattern 2: `test_<feature>_<behavior>_<detail>`
- `test_parse_line_caret_prefix_reads_from_wrapper()`
- `test_parse_line_no_caret_prefix_reads_from_payload()`
- `test_parse_line_caret_prefix_tool_name_from_wrapper()`

### Pattern 3: `test_<function>_<condition>_<expected_result>`
- `test_extract_with_envelope_caret_prefix_no_envelope_fallback_to_payload()`
- `test_extract_with_envelope_missing_field_from_envelope_fallback_to_payload()`

## Example Tests to Use as Patterns

### Example 1: Basic Caret-Prefix Lookup (parser/mod.rs)

**File**: `/home/coding/AgentScribe/src/parser/mod.rs`  
**Lines**: ~329-336 (estimated)  
**Test Function**: `test_extract_with_envelope_from_envelope()`

```rust
#[test]
fn test_extract_with_envelope_from_envelope() {
    let payload = json!({"role": "user", "content": "hello"});
    let envelope = json!({"timestamp": "2026-03-16T12:00:00Z", "session_id": "abc123"});
    
    // Extract from envelope using ^ prefix
    let result = extract_with_envelope("^timestamp", &payload, Some(&envelope));
    assert_eq!(result, Some(json!("2026-03-16T12:00:00Z")));
}
```

**What it tests**: Caret-prefix notation reads from envelope wrapper, not payload

---

### Example 2: Caret-Prefix Fallback Behavior (parser/mod.rs)

**File**: `/home/coding/AgentScribe/src/parser/mod.rs`  
**Lines**: ~378-385 (estimated)  
**Test Function**: `test_extract_with_envelope_caret_prefix_no_envelope_fallback_to_payload()`

```rust
#[test]
fn test_extract_with_envelope_caret_prefix_no_envelope_fallback_to_payload() {
    let payload = json!({"role": "user", "content": "hello", "timestamp": "2026-03-16T12:00:00Z"});
    
    // ^ prefix with no envelope should fallback to payload
    let result = extract_with_envelope("^timestamp", &payload, None);
    assert_eq!(result, Some(json!("2026-03-16T12:00:00Z")));
}
```

**What it tests**: Fallback behavior when envelope is None

---

### Example 3: Integration Test with Caret-Prefix (parser/jsonl.rs)

**File**: `/home/coding/AgentScribe/src/parser/jsonl.rs`  
**Lines**: 2018-2040  
**Test Function**: `test_parse_line_caret_prefix_reads_from_wrapper()`

```rust
#[test]
fn test_parse_line_caret_prefix_reads_from_wrapper() {
    // Test that ^timestamp reads from envelope wrapper, not payload
    let plugin = create_caret_envelope_test_plugin();
    let context = ParseContext::new(
        "test-session".to_string(),
        "test".to_string(),
        "/tmp/test.jsonl".to_string(),
    );

    // Envelope line: timestamp at wrapper level, payload has different timestamp
    let line = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user", "content": "Hello", "timestamp": "2026-03-16T10:00:00Z"}}"#;
    let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

    assert_eq!(events.len(), 1);
    let event = &events[0];

    // Should use wrapper timestamp (12:00:00Z), not payload timestamp (10:00:00Z)
    assert_eq!(
        event.ts.to_rfc3339(),
        "2026-03-16T12:00:00+00:00",
        "^timestamp should read from wrapper level"
    );
}
```

**What it tests**: Full integration test of caret-prefix field lookup in JSONL parsing

---

### Example 4: Mixed Caret and Payload Fields (parser/jsonl.rs)

**File**: `/home/coding/AgentScribe/src/parser/jsonl.rs`  
**Lines**: 2175-2224  
**Test Function**: `test_parse_line_mixed_caret_and_payload_fields()`

```rust
#[test]
fn test_parse_line_mixed_caret_and_payload_fields() {
    // Test that we can mix ^prefixed fields (wrapper) and regular fields (payload)
    let mut plugin = create_caret_envelope_test_plugin();
    plugin.parser.timestamp = Some("^timestamp".to_string());  // From wrapper
    plugin.parser.role = Some("role".to_string());              // From payload
    plugin.parser.content = Some("content".to_string());        // From payload
    
    let context = ParseContext::new(
        "test-session".to_string(),
        "test".to_string(),
        "/tmp/test.jsonl".to_string(),
    );

    // Mix: timestamp from wrapper, role/content from payload
    let line = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user", "content": "Hello"}}"#;
    let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

    assert_eq!(events.len(), 1);
    let event = &events[0];

    assert_eq!(
        event.ts.to_rfc3339(),
        "2026-03-16T12:00:00+00:00",
        "timestamp (^ prefix) from wrapper"
    );
    assert_eq!(event.role, Role::User, "role (no ^) from payload");
    assert_eq!(event.content, "Hello", "content (no ^) from payload");
}
```

**What it tests**: Mixing caret-prefixed and non-caret-prefixed field lookups

---

## Other Notable Lookup Tests

### Core Function Tests (parser/mod.rs)

- `test_has_caret_prefix_with_caret()` - Tests caret prefix detection
- `test_has_caret_prefix_without_caret()` - Tests no caret prefix detection
- `test_extract_with_envelope_dot_notation_from_envelope()` - Tests dot notation: `^outer.ts`
- `test_extract_with_envelope_missing_field_from_envelope_fallback_to_payload()` - Tests fallback when field missing in envelope

### JSONL Parser Tests (parser/jsonl.rs)

- `test_parse_line_no_caret_prefix_reads_from_payload()` - Tests regular fields read from payload (lines 2043-2066)
- `test_parse_line_caret_prefix_tool_name_from_wrapper()` - Tests caret prefix for tool_name field (lines 2069-2094)
- `test_parse_line_caret_prefix_tokens_from_wrapper()` - Tests caret prefix for tokens_in/tokens_out fields (lines 2097-2130)
- `test_parse_line_missing_payload_field_with_caret_prefix()` - Tests error handling when payload field missing (lines 2131-2152)
- `test_parse_line_non_object_payload_with_caret_prefix()` - Tests error handling for non-object payloads (lines 2153-2174)
- `test_fixture_envelope_with_caret_prefix_parses_correctly()` - Tests fixture file with caret prefix (lines 2254-2366)

### Type Field Extraction Tests (parser/jsonl.rs)

Lines 2367-2820 contain comprehensive tests for type field extraction:
- `test_unwrap_envelope_basic_type_field_extraction()` - Basic type extraction from envelope
- `test_type_field_extraction_string_value()` - String type values
- `test_type_field_extraction_number_value()` - Number type values  
- `test_type_field_extraction_bool_value()` - Boolean type values
- `test_type_field_extraction_missing_defaults_to_empty_string()` - Missing type field defaults
- `test_type_field_extraction_empty_string_value()` - Empty string type values
- `test_type_field_extraction_null_value()` - Null type values

## Test Organization Patterns

### Section Comments

Tests are organized into clearly labeled sections:

```rust
// -- Envelope tests --
// -- Envelope type routing and field extraction tests --
// -- Skip/meta/unknown routing: fixture-based tests --
// -- unwrap_envelope unit tests --
// -- ^-prefixed envelope field extraction tests --
```

### Helper Functions

Common helper functions for creating test plugins:

```rust
fn create_caret_envelope_test_plugin() -> Plugin { ... }
fn create_envelope_test_plugin() -> Plugin { ... }
```

## Key Features Tested

1. **Caret-prefix notation** (`^field`) for envelope-first lookup
2. **Fallback behavior**: envelope → payload when envelope is missing
3. **Dot notation** for nested fields (`^outer.ts`)
4. **Array indexing** support (`^items[0].name`)
5. **Type coercion** for string extraction
6. **Mixed field sources**: some from envelope, some from payload
7. **Error handling**: missing fields, non-object payloads
8. **Type field extraction**: string, number, boolean, null values
9. **Session ID lookup** in search functionality
10. **Field mapping** configuration testing

## Summary

The AgentScribe codebase contains comprehensive lookup-related tests with clear naming patterns. When writing new lookup tests, follow these conventions:

- Use `test_<feature>_<behavior>_<detail>` naming pattern
- Group related tests in sections with `// -- <description> --` comments
- Test both success and failure scenarios
- Include clear assertions with descriptive messages
- Mix caret-prefixed and non-caret-prefixed field tests
- Test fallback behavior when envelope is missing
- Test error conditions (missing fields, invalid data types)

---

**Generated**: 2026-08-12  
**Source**: Comprehensive search of AgentScribe test files  
**Coverage**: All lookup and field access functionality across parser modules
