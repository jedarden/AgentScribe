# Bead bf-7qnh: Helper Functions Already Implemented

## Task
Implement `create_test_plugin()` and `create_envelope_plugin()` helper functions.

## Status: ✅ COMPLETE (Already Implemented)

Both helper functions are already fully implemented in `/home/coding/AgentScribe/tests/test_helpers.rs`:

### `create_test_plugin()` (lines 139-169)
- Creates a basic `Plugin` instance for testing
- Configured with name "test", version "1.0"
- JSONL format, one-file-per-session detection from filename
- Basic parser with timestamp, role, and content fields
- No envelope routing, no array handling
- Static field: source_agent = "test"

### `create_envelope_plugin()` (lines 198-242)
- Creates a `Plugin` with envelope routing enabled
- Configured with name "test-envelope", version "1.0"
- JSONL format, one-file-per-session detection from filename
- Envelope routing with type mapping:
  - "message" → "event"
  - "session" → "skip"
  - "compaction" → "meta"
  - "model_change" → "skip"
- Role mapping: "toolResult" → "tool_result"
- Parser configured for envelope fields (timestamp, role, content)
- Static field: source_agent = "test-envelope"

## Verification

All tests pass:
```bash
$ cargo test --test test_helpers -- test_create_test_plugin test_create_envelope_plugin
running 2 tests
test tests::test_create_envelope_plugin ... ok
test tests::test_create_test_plugin ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 5 filtered out
```

Build compiles without errors.

## Acceptance Criteria Met
- ✅ create_test_plugin() correctly constructs a test Plugin instance
- ✅ create_envelope_plugin() correctly constructs a Plugin with envelope routing enabled
- ✅ Both functions compile without errors
- ✅ Functions return valid Plugin instances suitable for testing

No additional work required.
