# Bead bf-16vn: Basic Envelope Routing Tests

## Task
Implement basic envelope routing tests for event and skip types.

## Status: Already Completed

The required tests have already been implemented in `/home/coding/AgentScribe/src/parser/jsonl.rs`:

### test_envelope_routing_event (line 2079)
Tests that event lines correctly unwrap payload and produce events:
- Creates envelope test plugin with `type="message"` routing to `event`
- Verifies payload unwrapping extracts role/content from nested payload object
- Verifies timestamp extraction from envelope wrapper using `^timestamp` prefix
- Produces exactly 1 event with correct fields

### test_envelope_routing_skip (line 2117)
Tests that skip lines drop (produce 0 events):
- Creates envelope test plugin with `type="session"` routing to `skip`
- Verifies lines routed to skip are dropped
- Produces 0 events (line dropped)

## Acceptance Criteria
All criteria met:
- ✅ test_envelope_routing_event: event lines correctly unwrap payload and produce events
- ✅ test_envelope_routing_skip: skip lines drop (produce 0 events)
- ✅ Both tests compile and pass
- ✅ Correct routing behavior verified for both event and skip line types

## Test Results
```
running 4 tests
test parser::jsonl::tests::test_envelope_routing_meta ... ok
test parser::jsonl::tests::test_envelope_routing_event ... ok
test parser::jsonl::tests::test_envelope_routing_skip ... ok
test parser::jsonl::tests::test_envelope_routing_unknown_type ... ok

test result: ok. 4 passed; 0 failed; 0 ignored
```

Full test suite: 649 passed, 0 failed.

## Related Work
The envelope routing infrastructure was implemented in prior commits:
- `unwrap_envelope()` function handles routing logic
- `get_routing()` determines routing action for each type value
- Field extraction with `^` prefix reads from wrapper vs payload
