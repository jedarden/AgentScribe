# Bead bf-5nyf: Envelope Routing Tests Verification

## Task
Implement envelope routing tests for meta and unknown types.

## Verification Results

The tests were already implemented in the codebase:

### test_envelope_routing_meta (src/parser/jsonl.rs:2139-2158)
- ✅ Tests that meta lines accumulate envelope state and produce 0 events
- ✅ Uses type="compaction" which routes to meta
- ✅ Asserts 0 events are produced with clear message

### test_envelope_routing_unknown_type (src/parser/jsonl.rs:2161-2180)
- ✅ Tests that unknown type defaults to skip behavior (0 events)
- ✅ Uses type="unknown_event" which is not in routing map
- ✅ Asserts 0 events are produced with clear message

### Test Results
```bash
$ cargo test --lib 'test_envelope_routing'
running 4 tests
test parser::jsonl::tests::test_envelope_routing_meta ... ok
test parser::jsonl::tests::test_envelope_routing_event ... ok
test parser::jsonl::tests::test_envelope_routing_skip ... ok
test parser::jsonl::tests::test_envelope_routing_unknown_type ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured
```

Full test suite: **649 tests passed**, 0 failed, 0 ignored (35.51s)

### Acceptance Criteria Met
1. ✅ test_envelope_routing_meta: meta lines accumulate envelope state and produce 0 events
2. ✅ test_envelope_routing_unknown_type: unknown type defaults to skip behavior (0 events)
3. ✅ Both tests compile and pass
4. ✅ Edge case handling verified

## Implementation Details
The tests use `create_envelope_test_plugin()` which configures:
- type="message" → "event" routing
- type="compaction" → "meta" routing
- type="session" → "skip" routing
- type="model_change" → "skip" routing

Unknown types not in the map default to "skip" behavior via the `get_routing()` method.

## Conclusion
The requested tests were already implemented and passing. No new code was needed.
