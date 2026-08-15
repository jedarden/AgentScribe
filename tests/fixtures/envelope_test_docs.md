# envelope_test.jsonl Fixture Documentation

## Overview

This fixture demonstrates envelope-based JSONL parsing with type-based routing. It contains 8 lines representing a mixed conversation session with system events, user/assistant messages, and tool calls.

## Total Lines

**8 lines** (non-empty JSON objects)

## File Structure

Each line is a JSON object with an **envelope structure**:

```json
{
  "type": "<event_type>",
  "timestamp": "<ISO_8601_timestamp>",
  "payload": {
    // Event-specific data
  }
}
```

## Line-by-Line Breakdown

### Line 1: Session Start (meta type)
```json
{
  "type": "session_start",
  "timestamp": "2026-07-04T10:00:00Z",
  "payload": {
    "session_id": "env-test-001",
    "model": "claude-sonnet-5",
    "cwd": "/home/user/project",
    "role": "system",
    "content": "Session starting"
  }
}
```
- **Type:** `session_start` → routes to `meta` in envelope mode
- **Purpose:** Session metadata accumulation (session_id, model, cwd)
- **Events produced:** 0 (metadata only, not emitted as event)

### Line 2: Heartbeat (skip type)
```json
{
  "type": "heartbeat",
  "timestamp": "2026-07-04T10:00:05Z",
  "payload": {
    "status": "ok",
    "role": "system",
    "content": "Heartbeat"
  }
}
```
- **Type:** `heartbeat` → routes to `skip` in envelope mode
- **Purpose:** System health check (noise, should be filtered out)
- **Events produced:** 0 (dropped by skip routing)

### Line 3: Ping (skip type)
```json
{
  "type": "ping",
  "timestamp": "2026-07-04T10:00:10Z",
  "payload": {
    "seq": 1,
    "role": "system",
    "content": "Ping"
  }
}
```
- **Type:** `ping` → routes to `skip` in envelope mode
- **Purpose:** Low-level protocol message (noise, should be filtered out)
- **Events produced:** 0 (dropped by skip routing)

### Line 4: User Message (event type)
```json
{
  "type": "message",
  "timestamp": "2026-07-04T10:00:15Z",
  "payload": {
    "role": "user",
    "content": "Refactor the authentication module to use JWT tokens"
  }
}
```
- **Type:** `message` → routes to `event` in envelope mode
- **Purpose:** User prompt / task description
- **Events produced:** 1 (unwrapped and emitted)

### Line 5: Assistant Response (event type)
```json
{
  "type": "message",
  "timestamp": "2026-07-04T10:00:20Z",
  "payload": {
    "role": "assistant",
    "content": "I'll refactor the authentication module. Let me start by reading the current implementation."
  }
}
```
- **Type:** `message` → routes to `event` in envelope mode
- **Purpose:** Assistant acknowledgment and plan
- **Events produced:** 1 (unwrapped and emitted)

### Line 6: Tool Call (event type)
```json
{
  "type": "message",
  "timestamp": "2026-07-04T10:00:25Z",
  "payload": {
    "role": "tool_call",
    "content": "Reading src/auth/mod.rs",
    "tool_name": "Read"
  }
}
```
- **Type:** `message` → routes to `event` in envelope mode
- **Purpose:** Tool invocation (file read operation)
- **Events produced:** 1 (unwrapped and emitted)

### Line 7: Assistant Response with Solution (event type)
```json
{
  "type": "message",
  "timestamp": "2026-07-04T10:00:30Z",
  "payload": {
    "role": "assistant",
    "content": "Here's the refactored authentication module using JWT."
  }
}
```
- **Type:** `message` → routes to `event` in envelope mode
- **Purpose:** Assistant delivers solution
- **Events produced:** 1 (unwrapped and emitted)

### Line 8: Unknown Event (no routing defined)
```json
{
  "type": "unknown_event",
  "timestamp": "2026-07-04T10:00:35Z",
  "payload": {
    "data": "something unexpected",
    "role": "system",
    "content": "Unknown event"
  }
}
```
- **Type:** `unknown_event` → **not in routing map** → defaults to `skip` in envelope mode
- **Purpose:** Tests default skip behavior for unconfigured types
- **Events produced:** 0 (defaults to skip)

## Envelope-Related Fields

### Wrapper-Level Fields (accessed with `^` prefix)
- `type`: Event type discriminator (used for routing)
- `timestamp`: ISO 8601 timestamp at envelope level

### Payload Fields (nested under `payload`)
- `role`: Message role (user, assistant, system, tool_call)
- `content`: Message text content
- `tool_name`: Tool name (for tool_call events)
- `session_id`: Session identifier (in session_start)
- `model`: Model name (in session_start)
- `cwd`: Current working directory (in session_start)
- Custom fields: `status`, `seq`, `data` (event-specific)

## Type Routing Behavior

| Type | Routes To | Events Produced | Purpose |
|------|-----------|-----------------|---------|
| `session_start` | `meta` | 0 | Session metadata accumulation |
| `heartbeat` | `skip` | 0 | Filter out noise |
| `ping` | `skip` | 0 | Filter out noise |
| `message` | `event` | 1 per line | Actual conversation events |
| `unknown_event` | (none defined) → `skip` | 0 | Default skip behavior |

**Expected event count:** 4 events (from lines 4-7, the `message`-type lines)

## Key Patterns Demonstrated

1. **Envelope unwrapping:** The `payload_field` configuration specifies which field contains the actual event data (`payload` in this fixture)
2. **Type-based routing:** The `type_field` specifies which envelope field holds the type discriminator (`type`), and `type_routing` maps types to processing modes
3. **Field extraction with prefixes:**
   - `^timestamp` reads from envelope wrapper level
   - `payload.role` / `payload.content` read from nested payload
4. **Default skip behavior:** Unknown types not in the routing map default to skip (line 8)
5. **Metadata accumulation:** `meta`-routed types update session state without producing events
6. **Event filtering:** `skip`-routed types are dropped entirely (useful for noise reduction)

## Usage in Tests

This fixture is used to test:
- Envelope parsing and unwrapping (`src/parser/jsonl.rs`)
- Type-based routing (event, meta, skip)
- Field extraction with `^` prefix
- Default skip behavior for unknown types
- Non-envelope parsing (all 8 lines produce events when envelope config is disabled)

## Related Files

- Plugin definition: `tests/fixtures/envelope_test.toml`
- Integration tests: `tests/integration_tests.rs`
- Parser tests: `src/parser/jsonl.rs` (multiple envelope-related tests)
