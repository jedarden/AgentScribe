# Goose Plugin Schema and Project Detection Research

## Overview

This document provides a comprehensive understanding of the goose plugin configuration schema, how project detection works, and how first-line metadata patterns are handled in AgentScribe's JSONL parser.

## 1. Goose Plugin Schema

### 1.1 Data Location

- **Path**: `~/.local/share/goose/sessions/*.jsonl`
- **Format**: JSONL (one JSON object per line)

### 1.2 File Structure

Goose session files have a specific two-part structure:

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

**Key fields:**
- `working_dir`: **Primary project path field** - absolute path to the project directory
- `description`: Human-readable session description
- `message_count`: Number of messages in the session
- `total_tokens`, `input_tokens`, `output_tokens`: Token counts for this session
- `accumulated_*`: Cumulative token counts across sessions

#### **Subsequent Lines: Messages**

```json
{
  "role": "user",
  "created": 1747178328,
  "content": [
    {
      "type": "text",
      "text": "I'm seeing a memory leak in my worker process. Can you help me investigate?"
    }
  ]
}
```

**Message fields:**
- `role`: "user" or "assistant"
- `created`: Unix timestamp (seconds since epoch)
- `content`: Array of content blocks (text, toolRequest, toolResponse)

### 1.3 Content Block Types

The `content` array contains blocks of different types that AgentScribe expands into canonical events:

#### Text Block
```json
{
  "type": "text",
  "text": "I have started a basic mcp template..."
}
```
→ Becomes message content text

#### Tool Request Block
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
→ Expands to `tool_call` event (extracts tool name from `toolCall.value.name`)

#### Tool Response Block
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
→ Expands to `tool_result` event (correlated with tool_call by `id`)

### 1.4 Required Fields for Plugin Configuration

For goose, the essential field mappings are:

| Canonical Field | Source Field | Notes |
|----------------|--------------|-------|
| `timestamp` | `created` | Unix timestamp (seconds) |
| `role` | `role` | "user" or "assistant" |
| `content` | `content` | Array of content blocks |
| `project` | `working_dir` | From first-line metadata (line 1) |

## 2. Project Detection Mechanisms

### 2.1 ProjectDetection Enum

AgentScribe supports three project detection strategies defined in `src/plugin.rs`:

```rust
pub enum ProjectDetection {
    Field { field: String },  // Extract from a JSON field
    ParentDir,                // Use parent directory of log file
    GitRoot,                  // Use git repository root
}
```

### 2.2 How ProjectDetection::Field Works

#### Configuration (in `plugins/goose.toml`)

```toml
[parser.project]
method = "field"
field = "working_dir"
```

#### Implementation (from `src/scraper/mod.rs`)

The scraper's `detect_project()` method handles field-based detection:

```rust
// Line 663-667 in src/scraper/mod.rs
ProjectDetection::Field { field: _ } => {
    // For field-based detection, we need to extract from the first event
    // This is handled in the parser, return None here
    Ok(None)
}
```

This means field-based project detection is **parser-specific**, not scraper-level.

#### Parser Implementation

Different parsers handle field-based project detection differently:

**For JSON Array format** (`src/parser/json_array.rs` lines 175-181):

```rust
// Set project: prefer field extraction from event, fall back to context
event.project =
    if let Some(ProjectDetection::Field { field }) = plugin.parser.project.as_ref() {
        extract_string(item, field).or_else(|| context.project.clone())
    } else {
        context.project.clone()
    };
```

This extracts the project from **each event item**.

**For JSONL format** (`src/parser/jsonl.rs` line 387):

```rust
// Set project from context
event.project = context.project.clone();
```

JSONL uses the **ParseContext** to carry project information across all events.

### 2.3 How First-Line Metadata Sets Project

The key insight is that the first line of goose JSONL files contains the session metadata with the `working_dir` field. Here's how it flows:

1. **Parser reads line 1**: Session metadata with `working_dir`
2. **Extract project field**: Parser should extract `working_dir` from line 1
3. **Store in ParseContext**: Project value is stored in `ParseContext.project`
4. **Apply to all events**: Every subsequent event gets `event.project = context.project.clone()`

#### Current Implementation Gap

Looking at the existing code, there's a **missing implementation** for extracting project from first-line metadata in JSONL format:

- The scraper calls `detect_project()` which returns `Ok(None)` for Field detection
- The ParseContext is created with `project: None` (line 414 in `src/parser/mod.rs`)
- Events get `event.project = context.project.clone()` which is `None`

**The missing piece**: The JSONL parser needs to:
1. Read the first line
2. Extract the project field (e.g., `working_dir`)
3. Create ParseContext with that project value
4. Use that context for parsing all subsequent lines

### 2.4 Alternative: ParentDir Detection

If field-based detection is not implemented, goose can use `ParentDir`:

```toml
[parser.project]
method = "parent_dir"  # Uses parent directory of session file
```

This is simpler but less accurate than using the actual `working_dir` from the session metadata.

## 3. Envelope System for First-Line Metadata

### 3.1 What is the Envelope System?

AgentScribe has an **envelope unwrapping system** for handling wrapped JSONL lines where event data is nested inside a wrapper structure (like Codex's `{type, timestamp, payload}` format).

### 3.2 Envelope Configuration

```toml
[source.envelope]
payload_field = "payload"    # Field containing actual event data
type_field = "type"          # Field for routing decisions
type_routing = {             # Maps type values to actions
    message = "event",       # → produce canonical events
    session = "skip",        # → ignore this line
    compaction = "meta"      # → accumulate metadata (future)
}
```

### 3.3 Routing Actions

- **`"event"`**: Extract payload, produce canonical events
- **`"skip"`**: Ignore this line entirely
- **`"meta"`**: Accumulate metadata (not yet fully implemented - returns `Vec::new()`)

### 3.4 Does Goose Need Envelope Unwrapping?

**Current goose plugin**: No envelope configured

**Schema analysis**: Looking at the goose format:
- Line 1: `{working_dir, description, ...}` - no `type` field, no wrapper
- Message lines: `{role, created, content[]}` - no wrapper

**Conclusion**: Goose does **NOT** need envelope unwrapping. It's a flat JSONL structure.

The first line is just a different JSON schema than subsequent lines, not a wrapped structure.

### 3.5 How First-Line Metadata Should Work for Goose

Since goose doesn't use envelope unwrapping, the project extraction should work as follows:

1. **JSONL parser reads first line**: Parse as JSON
2. **Extract `working_dir` field**: Use the `ProjectDetection::Field { field: "working_dir" }` config
3. **Create ParseContext with project**: 
   ```rust
   let project = extract_string(&first_line_json, "working_dir");
   let context = ParseContext::new(session_id, source_agent, source_file)
       .with_project(project);
   ```
4. **Parse rest of file with context**: All events get the project from context

## 4. Required Plugin Configuration Fields

### 4.1 Essential Fields for Goose Plugin

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
role = "role"               # "user" or "assistant"
content = "content"         # Array of content blocks

[parser.static]
source_agent = "goose"

[parser.project]
method = "field"            # Extract from first-line metadata
field = "working_dir"       # Field name in line 1

[parser.model]
source = "none"             # Model not available in goose logs

[parser.file_paths]
content_regex = true        # Extract paths from content
```

### 4.2 Field Value Acceptance

#### timestamp field
- **Accepts**: Unix timestamp (seconds since epoch)
- **Format**: Integer or string representing seconds
- **Example**: `1747178328`

#### role field
- **Accepts**: "user" or "assistant"
- **Format**: String
- **Required**: Yes - every message line must have a role

#### content field
- **Accepts**: Array of content blocks
- **Format**: JSON array with objects containing `type` field
- **Block types**: "text", "toolRequest", "toolResponse"

#### project field (working_dir)
- **Accepts**: Absolute file path
- **Format**: String path
- **Example**: "/home/coding/projects/my-app"
- **Location**: Line 1 only (session metadata)

## 5. Comparison with Other Plugins

### 5.1 vs Claude Code

| Aspect | Goose | Claude Code |
|--------|-------|-------------|
| **Session metadata** | Line 1 of JSONL | Separate `session-meta/<uuid>.json` file |
| **Project field** | `working_dir` | `cwd` |
| **Timestamp format** | Unix timestamp (`created`) | ISO 8601 (`timestamp`) |
| **Content structure** | `content[]` array with blocks | Embedded in message with `type` field |
| **Tool calls** | `toolRequest`/`toolResponse` blocks | `tool_use` content blocks |
| **Model tracking** | Not available | In metadata file |

### 5.2 vs Codex

| Aspect | Goose | Codex |
|--------|-------|-------|
| **Structure** | Flat JSONL | Envelope-wrapped JSONL |
| **First line** | Session metadata | `RolloutLine::Meta` |
| **Type routing** | Not needed | Required (skip/meta/event) |
| **Project field** | `working_dir` (line 1) | `cwd` (in meta) |

## 6. Current Implementation Status

### 6.1 What Works

- ✅ JSONL parsing for standard message lines
- ✅ Field mapping for timestamp, role, content
- ✅ Static field assignment (source_agent)
- ✅ Project detection with ParentDir method

### 6.2 What Needs Implementation

- ⚠️ **First-line metadata extraction**: JSONL parser needs to read line 1, extract project field, and store in ParseContext
- ⚠️ **Field-based project detection for JSONL**: Currently only implemented for JsonArray format, not Jsonl
- ⚠️ **content[] array expansion**: Parser needs to expand content blocks into separate events (text → content, toolRequest → tool_call, toolResponse → tool_result)

### 6.3 Current Workaround

The current goose plugin configuration uses:

```toml
[parser.project]
method = "field"
field = "working_dir"
```

But this relies on parser-level extraction that may not be fully implemented. A working alternative is:

```toml
[parser.project]
method = "parent_dir"
```

This uses the parent directory of the session file as the project path, which is less accurate but functional.

## 7. Summary and Recommendations

### 7.1 Schema Requirements

The goose plugin requires:
1. **Line 1 handling**: Extract session metadata (`working_dir`, `description`, etc.)
2. **Field-based project detection**: Implement `ProjectDetection::Field` for JSONL format
3. **Content array expansion**: Parse `content[]` blocks into canonical events
4. **Tool correlation**: Match `toolRequest.id` with `toolResponse.id`

### 7.2 Project Detection Field Values

For goose, the project detection field is:
- **Field name**: `working_dir`
- **Location**: Line 1 (first line of JSONL file)
- **Format**: Absolute file path string
- **Example value**: `/home/coding/projects/my-app`

### 7.3 Ready to Create Config

With this understanding, you can create a functional goose plugin configuration using:

1. **Project detection**: `method = "field"` with `field = "working_dir"` (requires parser implementation) OR `method = "parent_dir"` (works immediately)
2. **Timestamp field**: `timestamp = "created"` (Unix timestamp)
3. **Role field**: `role = "role"` (user/assistant)
4. **Content field**: `content = "content"` (array of blocks)

### 7.4 Next Steps

To fully support goose with accurate project detection:

1. Implement first-line metadata extraction in JSONL parser
2. Implement content[] array expansion
3. Add tool request/response correlation
4. Test with real goose session files

For now, the plugin can function with `method = "parent_dir"` as a workaround until field-based detection is fully implemented for JSONL format.
