# Pi (badlogic/pi-mono) Session Format Fixtures

## Schema Verification

**Primary Source:** `packages/coding-agent/docs/session-format.md` in [badlogic/pi-mono](https://github.com/badlogic/pi-mono) repository

**Repository:** https://github.com/badlogic/pi-mono

**Canonical Documentation:** https://github.com/badlogic/pi-mono/blob/main/packages/coding-agent/docs/session-format.md

**Key Implementation Files:**
- `packages/coding-agent/src/core/session-manager.ts` - Session entry types and SessionManager class
- `packages/coding-agent/src/core/messages.ts` - Extended message types (BashExecutionMessage, CustomMessage, etc.)
- `packages/ai/src/types.ts` - Base message types (UserMessage, AssistantMessage, ToolResultMessage)
- `packages/agent/src/types.ts` - AgentMessage union type

**Latest Commit:** main branch as of 2026-07-06

**Verification Note:** The current README previously cited `earendil-works/pi-mono` which does not exist. The correct repository is `badlogic/pi-mono`. The project was later moved to the earendil-works organization as `earendil-works/pi` (without the -mono suffix), but the session format documentation cited in the parent bead (bf-4kv6) refers to the badlogic/pi-mono repository.

## Format Overview

Pi stores sessions as JSONL (one JSON object per line) with a tree-based structure using `id`/`parentId` relationships.

### File Path Structure

```
~/.pi/agent/sessions/--<path>--/<timestamp>_<uuid>.jsonl
```

Where `<path>` is the working directory with `/` replaced by `-`.

Example: `/home/user/myproject` → `--home-user-myproject--`

### Session Version

Current version is **3**. Versions 1-2 are auto-migrated on load.

## Session Format (Version 3)

### First Line - Session Header

```json
{
  "type": "session",
  "version": 3,
  "id": "uuid-v4",
  "timestamp": "2024-12-03T14:00:00.000Z",
  "cwd": "/path/to/project"
}
```

For sessions with a parent (created via `/fork`, `/clone`, or `newSession({ parentSession })`):

```json
{
  "type": "session",
  "version": 3,
  "id": "uuid-v4",
  "timestamp": "2024-12-03T14:00:00.000Z",
  "cwd": "/path/to/project",
  "parentSession": "/path/to/original/session.jsonl"
}
```

### Entry Base Structure

All entries (except session header) share this base structure:

```json
{
  "type": "entry-type",
  "id": "8-char-hex",
  "parentId": "parent-id-or-null",
  "timestamp": "ISO-8601-string"
}
```

- `id`: 8-character hex string (collision-checked)
- `parentId`: ID of parent entry, or `null` for first entry
- `timestamp`: ISO-8601 format string (e.g., "2024-12-03T14:00:01.000Z")

### Entry Types

| Type | Description | Handling |
|------|-------------|----------|
| `message` | User/assistant/tool messages | **Event** |
| `session` | Session header (first line) | **Skip** |
| `model_change` | Model selection change | **Skip** |
| `thinking_level_change` | Thinking level change | **Skip** |
| `compaction` | Token compaction summary | **Skip** |
| `branch_summary` | Branch exploration summary | **Skip** |
| `custom` | Extension state data | **Skip** |
| `custom_message` | Extension message (in context) | **Event** (as user message) |
| `label` | Entry label/marker | **Skip** |
| `session_info` | Session metadata | **Skip** |

### Message Structure

#### User Message

```json
{
  "type": "message",
  "id": "a1b2c3d4",
  "parentId": "prev1234",
  "timestamp": "2024-12-03T14:00:01.000Z",
  "message": {
    "role": "user",
    "content": "string or array of content blocks",
    "timestamp": 1234567890000
  }
}
```

#### Assistant Message

```json
{
  "type": "message",
  "id": "b2c3d4e5",
  "parentId": "a1b2c3d4",
  "timestamp": "2024-12-03T14:00:02.000Z",
  "message": {
    "role": "assistant",
    "content": [
      {"type": "text", "text": "..."},
      {"type": "thinking", "thinking": "..."},
      {"type": "toolCall", "id": "...", "name": "...", "arguments": {...}}
    ],
    "api": "anthropic",
    "provider": "anthropic",
    "model": "claude-sonnet-4-5",
    "usage": {
      "input": 1000,
      "output": 500,
      "cacheRead": 0,
      "cacheWrite": 0,
      "totalTokens": 1500,
      "cost": {
        "input": 0.001,
        "output": 0.002,
        "cacheRead": 0,
        "cacheWrite": 0,
        "total": 0.003
      }
    },
    "stopReason": "stop",
    "timestamp": 1234567890000
  }
}
```

`stopReason` values (verified against upstream `pi-ai` types): `"stop" | "length" | "toolUse" | "error" | "aborted"`. Pi normalizes provider-specific values — e.g. OpenAI's raw `tool_calls` is persisted as `"toolUse"`, so `tool_calls` never appears in session JSONL.

#### Tool Result Message

```json
{
  "type": "message",
  "id": "c3d4e5f6",
  "parentId": "b2c3d4e5",
  "timestamp": "2024-12-03T14:00:03.000Z",
  "message": {
    "role": "toolResult",
    "toolCallId": "...",
    "toolName": "...",
    "content": [{"type": "text", "text": "..."}],
    "details": {},
    "isError": false,
    "timestamp": 1234567890000
  }
}
```

#### Custom Message (Extension-injected)

```json
{
  "type": "custom_message",
  "id": "i9j0k1l2",
  "parentId": "h8i9j0k1",
  "timestamp": "2024-12-03T14:25:00.000Z",
  "customType": "my-extension",
  "content": "Injected context...",
  "display": true,
  "details": {}
}
```

### Content Block Types

- **text**: `{"type": "text", "text": "..."}`
- **image**: `{"type": "image", "data": "base64...", "mimeType": "image/jpeg"}`
- **thinking**: `{"type": "thinking", "thinking": "..."}`
- **toolCall**: `{"type": "toolCall", "id": "...", "name": "...", "arguments": {...}}`

### Tool Call/Result Pairing

Tool calls are embedded in assistant message content arrays:

1. Assistant message contains `toolCall` block in content array
2. System creates separate `toolResult` message entry with matching `toolCallId`

### Extended Message Types (pi-coding-agent)

#### Bash Execution Message

```json
{
  "role": "bashExecution",
  "command": "ls -la",
  "output": "total 0",
  "exitCode": 0,
  "cancelled": false,
  "truncated": false,
  "fullOutputPath": "/path/to/output.txt",
  "excludeFromContext": false,
  "timestamp": 1234567890000
}
```

#### Branch Summary Message

```json
{
  "role": "branchSummary",
  "summary": "Branch explored approach A",
  "fromId": "f6g7h8i9",
  "timestamp": 1234567890000
}
```

#### Compaction Summary Message

```json
{
  "role": "compactionSummary",
  "summary": "User discussed X, Y, Z...",
  "tokensBefore": 50000,
  "timestamp": 1234567890000
}
```

## Timestamp Formats

- **Entry timestamp**: ISO-8601 string (e.g., "2024-12-03T14:00:01.000Z")
- **Message timestamp**: Unix milliseconds (number, e.g., 1733228801000)

## Other Entry Types

### Model Change

```json
{
  "type": "model_change",
  "id": "d4e5f6g7",
  "parentId": "c3d4e5f6",
  "timestamp": "2024-12-03T14:05:00.000Z",
  "provider": "openai",
  "modelId": "gpt-4o"
}
```

### Thinking Level Change

```json
{
  "type": "thinking_level_change",
  "id": "e5f6g7h8",
  "parentId": "d4e5f6g7",
  "timestamp": "2024-12-03T14:06:00.000Z",
  "thinkingLevel": "high"
}
```

### Compaction

```json
{
  "type": "compaction",
  "id": "f6g7h8i9",
  "parentId": "e5f6g7h8",
  "timestamp": "2024-12-03T14:10:00.000Z",
  "summary": "User discussed X, Y, Z...",
  "firstKeptEntryId": "c3d4e5f6",
  "tokensBefore": 50000,
  "details": {},
  "fromHook": false
}
```

### Branch Summary

```json
{
  "type": "branch_summary",
  "id": "g7h8i9j0",
  "parentId": "a1b2c3d4",
  "timestamp": "2024-12-03T14:15:00.000Z",
  "fromId": "f6g7h8i9",
  "summary": "Branch explored approach A...",
  "details": {},
  "fromHook": false
}
```

### Custom Entry (Extension State)

```json
{
  "type": "custom",
  "id": "h8i9j0k1",
  "parentId": "g7h8i9j0",
  "timestamp": "2024-12-03T14:20:00.000Z",
  "customType": "my-extension",
  "data": {"count": 42}
}
```

### Label Entry

```json
{
  "type": "label",
  "id": "j0k1l2m3",
  "parentId": "i9j0k1l2",
  "timestamp": "2024-12-03T14:30:00.000Z",
  "targetId": "a1b2c3d4",
  "label": "checkpoint-1"
}
```

### Session Info

```json
{
  "type": "session_info",
  "id": "k1l2m3n4",
  "parentId": "j0k1l2m3",
  "timestamp": "2024-12-03T14:35:00.000Z",
  "name": "Refactor auth module"
}
```

## Tree Structure

Entries form a tree via `id`/`parentId`:

```
[user msg] ─── [assistant] ─── [user msg] ─── [assistant] ─┬─ [user msg] ← current leaf
                                                           │
                                                           └─ [branch_summary] ─── [user msg] ← alternate branch
```

- First entry has `parentId: null`
- Each subsequent entry points to its parent via `parentId`
- Branching creates new children from an earlier entry
- The "leaf" is the current position in the tree

## Fixtures

### `multi-turn-with-tool.jsonl`
Complete multi-turn conversation demonstrating:
- User → Assistant (with tool call) → Tool Result → Assistant → User → Assistant (tool call) → Tool Result → Assistant
- Tool call/result pairing via `toolCallId` matching (tool_abc123 → tool_abc123, tool_def456 → tool_def456)
- Message ordering via `id`/`parentId` chain: 4f8a2b1c → 7d3e9a05 → 1c8f42bd → 92ab607e → 5ef0c318 → 38d94a2f → c07b5e69 → 6b1f80ad
- Entry `id` values are 8-character hex strings (first entry has `parentId: null`)
- Content blocks in assistant messages (text + toolCall arrays)
- Complete message structure with api/provider/model/usage/stopReason for assistant messages (`stopReason: "toolUse"` on tool-call turns, `"stop"` on final replies)
- Proper timestamp formats: entry-level ISO-8601 (e.g. `2024-12-03T14:00:01.000Z`) with a corresponding message-level Unix-millisecond timestamp (e.g. `1733234401000`) that decodes to the same instant

### `edge-case-empty.jsonl`
Session header only (no message entries):
- Tests handling of empty sessions where only the session type line exists
- No messages or events to process
- Validates parser doesn't crash on minimal valid input

### `edge-case-truncated.jsonl`
Session header + incomplete conversation:
- Tests handling of truncated/incomplete sessions (simulates mid-write crash or interrupted write)
- Last line is incomplete JSON, ending mid-structure with **no trailing newline**: `{"type":"message","id":"b3617a08","parentId":"5d90fe12","timestamp":"2024-12-03T16:45:07.000Z","message"`
- Validates graceful error handling and recovery
- Earlier valid lines should still be parsed correctly

## Implementation Notes

### Envelope Structure
- Each line has envelope fields: `type`, `id`, `parentId`, `timestamp` (ISO-8601)
- Message data is nested in `message` field
- Requires envelope unwrapping in plugin config

### Content Handling
- Content can be a string OR array of content blocks
- AgentScribe will stringify arrays automatically
- Tool calls are extracted from arrays when present

### Project Detection
- Session header contains `cwd` field with absolute path
- We skip the session header (type routing)
- Fallback to `parent_dir` method from filename path

### Model Detection
- Extracted from `message.provider` and `message.model` fields
- Only present in assistant messages

### Role Mapping
- `toolResult` → `tool_result`
- `bashExecution` → `execution`
- `custom` → `custom`
- `branchSummary`/`compactionSummary` → `system`

### Timestamp Handling
- Entry-level `timestamp` is ISO-8601 string (from envelope)
- Message-level `timestamp` is Unix milliseconds (number)
- For scraping events, prefer entry timestamp when available
