# Goose Plugin Schema — Complete Documentation

## Overview

This document provides comprehensive documentation of the goose plugin schema for AgentScribe, including all required fields, types, constraints, and examples based on actual goose log files and plugin configuration.

**Source Schema Verified From:** https://github.com/aaif-goose/goose/issues/2529

---

## 1. Plugin Configuration Schema

### 1.1 Complete Plugin TOML

```toml
[plugin]
name = "goose"
version = "1.0"

[source]
paths = ["~/.local/share/goose/sessions/*.jsonl"]
exclude = []
format = "jsonl"

[source.session_detection]
method = "one-file-per-session"
session_id_from = "filename"

[parser]
timestamp = "created"        # Unix timestamp (seconds since epoch)
role = "role"               # "user" or "assistant"
content = "content"         # Array of content blocks

[parser.static]
source_agent = "goose"

[parser.project]
method = "field"            # Extract from first-line metadata
field = "working_dir"       # Field name in session metadata (line 1)

[parser.model]
source = "none"             # Model information not available in goose logs

[parser.file_paths]
content_regex = true        # Extract file paths from content via regex
```

### 1.2 Plugin Metadata Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Plugin identifier: "goose" |
| `version` | string | yes | Plugin definition version: "1.0" |

---

## 2. Source Configuration

### 2.1 Source Fields

| Field | Type | Required | Values | Description |
|-------|------|----------|--------|-------------|
| `paths` | array | yes | glob patterns | `["~/.local/share/goose/sessions/*.jsonl"]` |
| `exclude` | array | no | glob patterns | Patterns to exclude from matches |
| `format` | string | yes | `"jsonl"` | File format identifier |

### 2.2 Session Detection Fields

| Field | Type | Required | Values | Description |
|-------|------|----------|--------|-------------|
| `method` | string | yes | `"one-file-per-session"` | Each JSONL file is one session |
| `session_id_from` | string | yes | `"filename"` | Session ID extracted from filename |

---

## 3. Parser Field Mapping

### 3.1 Core Parser Fields

| Canonical Field | Source Field | Type | Location | Constraints |
|----------------|--------------|------|----------|-------------|
| `timestamp` | `created` | integer | Message lines | Unix timestamp (seconds since epoch) |
| `role` | `role` | string | Message lines | Must be "user" or "assistant" |
| `content` | `content` | array | Message lines | Array of content blocks (see §4) |

### 3.2 Static Fields

| Field | Value | Type | Description |
|-------|-------|------|-------------|
| `source_agent` | `"goose"` | string | Identifies the agent type in canonical events |

### 3.3 Project Detection Fields

| Field | Value | Type | Description |
|-------|-------|------|-------------|
| `method` | `"field"` | string | Extract project path from JSON field |
| `field` | `"working_dir"` | string | Field name in first-line session metadata |

**Acceptable values for `working_dir`:**
- Format: Absolute file path string
- Example: `"/home/coding/projects/my-app"`
- Location: Line 1 of JSONL file (session metadata only)

**Alternative method** (if field-based detection not implemented):
```toml
[parser.project]
method = "parent_dir"  # Uses parent directory of session file
```

### 3.4 Model Detection Fields

| Field | Value | Type | Description |
|-------|-------|------|-------------|
| `source` | `"none"` | string | Model information not available in goose logs |

**Note:** Goose logs do not contain model name information. All sessions will have `model: null` in the canonical schema.

### 3.5 File Path Extraction Fields

| Field | Value | Type | Description |
|-------|-------|------|-------------|
| `content_regex` | `true` | boolean | Extract file paths from content using regex patterns |

---

## 4. Data Format Structure

### 4.1 File Structure

Goose session files use a **two-part JSONL structure**:

#### **Line 1: Session Metadata**

```json
{
  "working_dir": "/home/coding/projects/my-app",
  "description": "Debug memory leak in worker process",
  "message_count": 4,
  "total_tokens": 1250,
  "input_tokens": 980,
  "output_tokens": 270,
  "accumulated_total_tokens": 75280,
  "accumulated_input_tokens": 72872,
  "accumulated_output_tokens": 2408
}
```

**Session Metadata Fields:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `working_dir` | string | yes | **Primary project path** - absolute path to project directory |
| `description` | string | no | Human-readable session description |
| `message_count` | integer | yes | Number of messages in this session |
| `total_tokens` | integer | yes | Token count for this session |
| `input_tokens` | integer | yes | Input tokens for this session |
| `output_tokens` | integer | yes | Output tokens for this session |
| `accumulated_total_tokens` | integer | yes | Cumulative total tokens across all sessions |
| `accumulated_input_tokens` | integer | yes | Cumulative input tokens across all sessions |
| `accumulated_output_tokens` | integer | yes | Cumulative output tokens across all sessions |

#### **Subsequent Lines: Message Events**

```json
{
  "role": "user",
  "created": 1747178328,
  "content": [...]
}
```

**Message Event Fields:**

| Field | Type | Required | Values | Description |
|-------|------|----------|--------|-------------|
| `role` | string | yes | `"user"` or `"assistant"` | Message role |
| `created` | integer | yes | Unix timestamp | Seconds since epoch |
| `content` | array | yes | Content blocks | Array of content block objects (see §4.2) |

### 4.2 Content Block Types

The `content` array contains blocks of different types that AgentScribe expands into canonical events:

#### Type 1: Text Block

```json
{
  "type": "text",
  "text": "I have started a basic mcp template..."
}
```

**Fields:**
- `type`: "text"
- `text`: String content of the message

→ Becomes message content text in canonical event

#### Type 2: Tool Request Block

```json
{
  "type": "toolRequest",
  "id": "toolu_bdrk_01H6Vzjip2dzWDWJMQkiwevZ",
  "toolCall": {
    "status": "success",
    "value": {
      "name": "developer__shell",
      "arguments": {
        "command": "ls -la"
      }
    }
  }
}
```

**Fields:**
- `type`: "toolRequest"
- `id`: Tool correlation ID (matches with toolResponse)
- `toolCall.status`: "success" or other status
- `toolCall.value.name`: Tool name (e.g., "developer__shell")
- `toolCall.value.arguments`: Tool parameters object

→ Expands to `tool_call` canonical event

#### Type 3: Tool Response Block

```json
{
  "type": "toolResponse",
  "id": "toolu_bdrk_01H6Vzjip2dzWDWJMQkiwevZ",
  "toolResult": {
    "status": "success",
    "value": [...]
  }
}
```

**Fields:**
- `type`: "toolResponse"
- `id`: Tool correlation ID (matches with toolRequest)
- `toolResult.status`: "success" or other status
- `toolResult.value`: Tool output (can be nested content blocks)

→ Expands to `tool_result` canonical event

---

## 5. Field Constraints and Validation

### 5.1 Timestamp Field

**Field name:** `created`

**Acceptable values:**
- Format: Integer (Unix timestamp)
- Unit: Seconds since epoch
- Example: `1747178328`

**Constraints:**
- Must be parseable as integer
- Represents seconds (not milliseconds)
- No ISO 8601 format support

### 5.2 Role Field

**Field name:** `role`

**Acceptable values:**
- `"user"` - User message
- `"assistant"` - Assistant message

**Constraints:**
- Required on all message lines
- Must be lowercase string
- No other roles supported (no "system", "tool", etc. in base role field)

### 5.3 Content Field

**Field name:** `content`

**Acceptable values:**
- Format: JSON array of content block objects
- Each block must have `type` field
- Supported types: "text", "toolRequest", "toolResponse"

**Constraints:**
- Required on all message lines
- Must be valid JSON array
- Cannot be null or undefined
- Array can contain multiple blocks (e.g., text + toolRequest)

### 5.4 Project Field (working_dir)

**Field name:** `working_dir`

**Acceptable values:**
- Format: Absolute file path string
- Example: `"/home/coding/projects/my-app"`

**Constraints:**
- Located in line 1 only (session metadata)
- Must be absolute path (starts with `/`)
- Should be valid directory path
- Not present on message lines

---

## 6. Examples and Default Values

### 6.1 Complete Example Session File

```jsonl
{"working_dir":"/home/coding/projects/my-app","description":"Debug memory leak in worker process","message_count":4,"total_tokens":1250,"input_tokens":980,"output_tokens":270}
{"role":"user","created":1747178328,"content":[{"type":"text","text":"I'm seeing a memory leak in my worker process. Can you help me investigate?"}]}
{"role":"assistant","created":1747178334,"content":[{"type":"text","text":"I'll help you investigate the memory leak. Let's start by checking the current memory usage and looking for potential issues."},{"type":"toolRequest","id":"toolu_bdrk_01H6Vzjip2dzWDWJMQkiwevZ","toolCall":{"status":"success","value":{"name":"developer__shell","arguments":{"command":"ps aux | grep worker"}}}}]}
{"role":"user","created":1747178334,"content":[{"type":"toolResponse","id":"toolu_bdrk_01H6Vzjip2dzWDWJMQkiwevZ","toolResult":{"status":"success","value":[{"type":"text","text":"USER       PID %CPU %MEM    VSZ   RSS TTY      STAT START   TIME COMMAND\ncoding    1234  2.5  8.5 123456 87654 pts/0    R+   10:30   0:15 node worker.js\n"}]}}]}
```

### 6.2 Default Values

| Field | Default Value | Source |
|-------|---------------|--------|
| `source_agent` | `"goose"` | Static field in parser config |
| `model` | `null` | Not available in goose logs |
| `project` | Extracted from `working_dir` or parent directory | Project detection method |
| `file_paths` | Extracted via regex from content | `content_regex = true` |

---

## 7. Event Expansion Rules

AgentScribe expands goose content blocks into canonical events as follows:

### 7.1 Text Block → Message Content

```json
{"type":"text","text":"Hello world"}
```

→ Canonical event:
```json
{
  "role": "user",
  "content": "Hello world"
}
```

### 7.2 Tool Request → tool_call Event

```json
{
  "type": "toolRequest",
  "id": "toolu_bdrk_01H6Vzjip2dzWDWJMQkiwevZ",
  "toolCall": {
    "status": "success",
    "value": {
      "name": "developer__shell",
      "arguments": {
        "command": "ls -la"
      }
    }
  }
}
```

→ Canonical event:
```json
{
  "role": "tool_call",
  "tool": "developer__shell",
  "content": "{\"command\":\"ls -la\"}",
  "tool_id": "toolu_bdrk_01H6Vzjip2dzWDWJMQkiwevZ"
}
```

### 7.3 Tool Response → tool_result Event

```json
{
  "type": "toolResponse",
  "id": "toolu_bdrk_01H6Vzjip2dzWDWJMQkiwevZ",
  "toolResult": {
    "status": "success",
    "value": [...]
  }
}
```

→ Canonical event:
```json
{
  "role": "tool_result",
  "tool": "developer__shell",
  "content": "...",
  "tool_id": "toolu_bdrk_01H6Vzjip2dzWDWJMQkiwevZ"
}
```

### 7.4 Tool Correlation

Tool requests and responses are correlated by the `id` field:
- `toolRequest.id` == `toolResponse.id` → Same tool interaction
- ID format: `toolu_<random>_<base62>`
- AgentScribe matches these to create complete tool call + result pairs

---

## 8. Implementation Notes

### 8.1 Current Implementation Status

**What Works:**
- ✅ JSONL parsing for standard message lines
- ✅ Field mapping for timestamp, role, content
- ✅ Static field assignment (source_agent)
- ✅ Project detection with `ParentDir` method (fallback)

**Implementation Gaps:**
- ⚠️ **Field-based project detection for JSONL**: `ProjectDetection::Field` is not fully implemented for JSONL format (parser returns `None` for field-based detection)
- ⚠️ **First-line metadata extraction**: JSONL parser needs to extract `working_dir` from line 1 and store in ParseContext
- ⚠️ **content[] array expansion**: Parser needs to expand content blocks into separate canonical events

### 8.2 Current Workaround

If field-based project detection is not fully implemented, use `ParentDir` method:

```toml
[parser.project]
method = "parent_dir"  # Uses parent directory of session file
```

This is less accurate than using `working_dir` from metadata but works immediately.

### 8.3 Differences from Other Agents

| Aspect | Goose | Claude Code | Codex |
|--------|-------|-------------|-------|
| **Session metadata location** | Line 1 of JSONL | Separate `session-meta/<uuid>.json` file | Line 1 of JSONL |
| **Project field** | `working_dir` | `cwd` | `cwd` |
| **Timestamp format** | Unix timestamp (`created`) | ISO 8601 (`timestamp`) | ISO 8601 (`timestamp`) |
| **Content structure** | `content[]` array with blocks | Embedded in message with `type` field | Nested `content[]` blocks |
| **Tool calls** | `toolRequest`/`toolResponse` blocks | `tool_use` content blocks | `function_call`/`function_call_output` |
| **Model tracking** | Not available | In metadata file | In metadata |
| **Envelope unwrapping** | Not needed | Not needed | Required |

---

## 9. Acceptance Criteria — Status

- ✅ **Schema documentation located and read** — All plugin fields documented
- ✅ **All required fields listed with types** — Complete field mapping table
- ✅ **Field constraints documented** — Validation rules for each field
- ✅ **Examples or defaults noted** — Complete session file example provided

---

## 10. References

- **Plugin source:** `plugins/goose.toml`
- **Test fixtures:** `tests/fixtures/goose/sample_session.jsonl`
- **Fixture documentation:** `tests/fixtures/goose/README.md`
- **Implementation:** `src/parser/jsonl.rs` (JSONL parser)
- **Schema verification:** https://github.com/aaif-goose/goose/issues/2529
- **Related research:**
  - `docs/research-goose-plugin-schema.md`
  - `docs/research/goose-plugin-schema-and-project-detection.md`

---

## Summary

The goose plugin schema defines a JSONL-based conversation log format with:

1. **First-line session metadata** containing `working_dir` for project detection
2. **Message events** with `role`, `created` (Unix timestamp), and `content[]` array
3. **Content block types**: text, toolRequest, toolResponse
4. **Project detection** via `working_dir` field (line 1) or parent directory fallback
5. **No model information** available (all sessions have `model: null`)
6. **Tool correlation** via matching `id` fields between toolRequest and toolResponse blocks

The plugin configuration maps these native fields to AgentScribe's canonical event schema, enabling unified search and analytics across all supported agent types.
