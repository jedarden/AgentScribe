# Goose Plugin Schema — Executive Summary

## Overview

This document provides a concise summary of the goose plugin schema for AgentScribe. For complete details, see:
- `docs/research/goose-plugin-schema-complete.md` — Full schema documentation
- `docs/research/goose-plugin-schema-and-project-detection.md` — Project detection details
- `docs/research-goose-plugin-schema.md` — Initial research findings

**Schema Source:** Verified from https://github.com/aaif-goose/goose/issues/2529

---

## Quick Reference

### Data Location
- **Path:** `~/.local/share/goose/sessions/*.jsonl`
- **Format:** JSONL with first-line session metadata

### File Structure

**Line 1: Session Metadata**
```json
{
  "working_dir": "/home/coding/projects/my-app",
  "description": "Debug memory leak in worker process",
  "message_count": 4,
  "total_tokens": 1250,
  "input_tokens": 980,
  "output_tokens": 270
}
```

**Subsequent Lines: Message Events**
```json
{
  "role": "user",
  "created": 1747178328,
  "content": [{"type":"text","text":"I'm seeing a memory leak..."}]
}
```

---

## Required Fields

### Session Metadata (Line 1)

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `working_dir` | string | **yes** | **Project path** — absolute path to project directory |
| `description` | string | no | Session description |
| `message_count` | integer | yes | Number of messages |
| `total_tokens` | integer | yes | Session token count |
| `input_tokens` | integer | yes | Input tokens |
| `output_tokens` | integer | yes | Output tokens |

### Message Events

| Field | Type | Required | Values | Description |
|-------|------|----------|--------|-------------|
| `role` | string | **yes** | `"user"`, `"assistant"` | Message role |
| `created` | integer | **yes** | Unix timestamp | Seconds since epoch |
| `content` | array | **yes** | Content blocks | Array of content block objects |

---

## Content Block Types

### 1. Text Block
```json
{"type":"text","text":"Hello world"}
```

### 2. Tool Request Block
```json
{
  "type":"toolRequest",
  "id":"toolu_bdrk_01H6Vzjip2dzWDWJMQkiwevZ",
  "toolCall":{
    "status":"success",
    "value":{"name":"developer__shell","arguments":{"command":"ls -la"}}
  }
}
```

### 3. Tool Response Block
```json
{
  "type":"toolResponse",
  "id":"toolu_bdrk_01H6Vzjip2dzWDWJMQkiwevZ",
  "toolResult":{"status":"success","value":[...]}
}
```

---

## Field Constraints

### timestamp (`created`)
- **Format:** Integer (Unix timestamp, seconds since epoch)
- **Example:** `1747178328`
- **Not:** ISO 8601 format

### role
- **Values:** `"user"` or `"assistant"` only
- **Case:** Lowercase
- **Required:** On all message lines

### content
- **Format:** JSON array
- **Required:** Yes (cannot be null/undefined)
- **Types:** "text", "toolRequest", "toolResponse"

### project (`working_dir`)
- **Format:** Absolute file path string
- **Example:** `"/home/coding/projects/my-app"`
- **Location:** Line 1 only (session metadata)
- **Required:** Yes

---

## Plugin Configuration

```toml
[plugin]
name = "goose"
version = "1.0"

[source]
paths = ["~/.local/share/goose/sessions/*.jsonl"]
format = "jsonl"

[source.session_detection]
method = "one-file-per-session"
session_id_from = "filename"

[parser]
timestamp = "created"        # Unix timestamp
role = "role"
content = "content"

[parser.static]
source_agent = "goose"

[parser.project]
method = "field"            # Extract from first-line metadata
field = "working_dir"

[parser.model]
source = "none"             # Model not available

[parser.file_paths]
content_regex = true
```

---

## Default Values

| Field | Default | Source |
|-------|---------|--------|
| `source_agent` | `"goose"` | Static field |
| `model` | `null` | Not available in goose logs |
| `project` | From `working_dir` or `parent_dir` | Detection method |
| `file_paths` | Extracted via regex | `content_regex = true` |

---

## Event Expansion

AgentScribe expands goose content blocks into canonical events:

- **text block** → message content text
- **toolRequest block** → `tool_call` event (extracts tool name from `toolCall.value.name`)
- **toolResponse block** → `tool_result` event (correlated with tool_call by `id`)

---

## Key Differences from Other Agents

| Aspect | Goose | Claude Code | Codex |
|--------|-------|-------------|-------|
| **Session metadata** | Line 1 of JSONL | Separate file | Line 1 of JSONL |
| **Project field** | `working_dir` | `cwd` | `cwd` |
| **Timestamp** | Unix (`created`) | ISO 8601 | ISO 8601 |
| **Content** | `content[]` array | Embedded with `type` | Nested blocks |
| **Tool calls** | `toolRequest`/`toolResponse` | `tool_use` | `function_call` |
| **Model** | Not available | In metadata | In metadata |

---

## Acceptance Criteria — ✅ Complete

- ✅ Schema documentation located and read
- ✅ All required fields listed with types
- ✅ Field constraints documented
- ✅ Examples and defaults noted

---

## References

- **Plugin config:** `plugins/goose.toml`
- **Test fixtures:** `tests/fixtures/goose/sample_session.jsonl`
- **Fixture docs:** `tests/fixtures/goose/README.md`
- **Schema verification:** https://github.com/aaif-goose/goose/issues/2529
- **Implementation:** `src/parser/jsonl.rs`
