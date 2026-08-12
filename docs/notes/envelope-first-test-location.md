# Envelope-First Lookup Test Location and Pattern

## Location

**File:** `src/parser/mod.rs`  
**Module:** `#[cfg(test)] mod tests` (starting at line 276)  
**Section:** Add new tests after the existing `extract_with_envelope` tests (lines 326-445)

## Test Pattern to Follow

### Function Naming Convention
```rust
#[test]
fn test_extract_with_envelope_<specific_scenario>() {
    // test implementation
}
```

Naming pattern: `test_extract_with_envelope_<behavior_being_tested>`

Examples of existing test names:
- `test_extract_with_envelope_from_envelope`
- `test_extract_with_envelope_from_payload`
- `test_extract_with_envelope_caret_prefix_no_envelope_fallback_to_payload`
- `test_extract_with_envelope_missing_field_from_envelope_fallback_to_payload`
- `test_extract_with_envelope_array_from_envelope`

### Test Structure (AAA Pattern)

**1. Arrange** - Create test data using `json!` macro:
```rust
let payload = json!({"role": "user", "content": "hello"});
let envelope = json!({"timestamp": "2026-03-16T12:00:00Z", "session_id": "abc123"});
```

**2. Act** - Call the function being tested:
```rust
let result = extract_with_envelope("^timestamp", &payload, Some(&envelope));
```

**3. Assert** - Verify the result:
```rust
assert_eq!(result, Some(json!("2026-03-16T12:00:00Z")));
```

### Complete Example Test

```rust
#[test]
fn test_extract_with_envelope_from_envelope() {
    // Arrange
    let payload = json!({"role": "user", "content": "hello"});
    let envelope = json!({"timestamp": "2026-03-16T12:00:00Z", "session_id": "abc123"});

    // Act
    let result = extract_with_envelope("^timestamp", &payload, Some(&envelope));

    // Assert
    assert_eq!(result, Some(json!("2026-03-16T12:00:00Z")));
}
```

## Key Test Scenarios to Cover

Based on existing test patterns, new envelope-first lookup tests should cover:

1. **Basic envelope lookup** - Field exists in envelope, use `^` prefix
2. **Basic payload lookup** - Field exists in payload, no `^` prefix
3. **Dot notation** - Nested fields in both envelope and payload
4. **Array indexing** - Array element access with `[n]` notation
5. **Fallback behavior** - Missing field in envelope falls back to payload
6. **Caret prefix fallback** - `^` prefix with no envelope falls back to payload
7. **Missing both** - Field missing from both returns `None`
8. **Edge cases** - Empty paths, only caret prefix, null values

## Assertion Style

Use `assert_eq!` for equality checks:
```rust
assert_eq!(result, Some(json!("expected_value")));
assert_eq!(result, None);
```

For more complex scenarios, use pattern matching:
```rust
match result {
    Some(json) => {
        assert_eq!(json["field"], "expected");
    }
    None => panic!("Expected Some value but got None"),
}
```

## Required Imports

The test module already includes these imports (line 278-279):
```rust
use super::*;
use serde_json::json;
```

These provide access to:
- All parent module functions (`extract_with_envelope`, `extract_string`, etc.)
- `json!` macro for creating test data

## Integration with Existing Test Suite

All tests in this module run with:
```bash
cargo test
```

The tests are unit tests for the parser module's field extraction logic, specifically testing the envelope-first lookup behavior where fields prefixed with `^` are looked up in the envelope layer first, then fall back to the payload layer if not found.

## Reference Examples

See existing tests in `src/parser/mod.rs` lines 329-445 for complete working examples of:
- Simple field extraction from envelope vs payload
- Dot notation for nested fields
- Array element extraction
- Fallback behavior when fields are missing
- Edge cases and error handling

## Notes

- Each test should be **self-contained** and **independent** - no test depends on another test's state
- Tests use **descriptive names** that clearly indicate what behavior is being tested
- The **AAA pattern** (Arrange-Act-Assert) makes tests easy to read and understand
- Tests focus on **one specific behavior** per test function
