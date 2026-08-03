# Task bf-7qnh: create_test_plugin() and create_envelope_plugin() Implementation

## Summary

Verified that both helper functions are correctly implemented in `tests/test_helpers.rs` and all associated tests pass.

## Functions Verified

### create_test_plugin() (lines 139-169)
- Returns a minimal Plugin instance for testing
- PluginMeta: name="test", version="1.0"
- Source configuration:
  - Format: Jsonl
  - Path: /tmp/test.jsonl
  - Session detection: OneFilePerSession from Filename
  - **No envelope routing** (envelope: None)
- Parser configuration:
  - timestamp, role, content fields mapped
  - Static field: source_agent="test"

### create_envelope_plugin() (lines 198-242)
- Returns a Plugin instance with envelope routing configured
- PluginMeta: name="test-envelope", version="1.0"
- Source configuration:
  - Format: Jsonl
  - Path: /tmp/test-envelope.jsonl
  - **Envelope routing enabled**:
    - payload_field: "message"
    - type_field: "type"
    - type_routing:
      - message → event
      - session → skip
      - compaction → meta
      - model_change → skip
- Parser configuration:
  - timestamp, role, content fields mapped
  - role_map: toolResult → tool_result
  - Static field: source_agent="test-envelope"

## Test Results

All 7 tests in test_helpers module pass:
- test_setup_temp_directory_creates_required_structure ✓
- test_setup_temp_directory_is_unique ✓
- test_create_claude_code_plugin_structure ✓
- test_create_claude_code_plugin_includes_subagents ✓
- test_create_simple_parser ✓
- **test_create_test_plugin ✓** (verified Plugin structure and absence of envelope)
- **test_create_envelope_plugin ✓** (verified envelope routing, type_routing map, and role_map)

## Compilation

- `cargo check --tests`: No errors
- Both functions compile without warnings
- Ready for use in envelope routing tests

## Acceptance Criteria Met

- ✓ create_test_plugin() correctly constructs a test Plugin instance
- ✓ create_envelope_plugin() correctly constructs a Plugin with envelope routing enabled
- ✓ Both functions compile without errors
- ✓ Functions return valid Plugin instances suitable for testing
