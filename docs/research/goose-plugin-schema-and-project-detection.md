# Goose Plugin Schema and Project Detection Patterns

## Overview

This document researches the goose plugin configuration schema and explains how project detection works in AgentScribe's plugin system.

## Goose Plugin Configuration Schema

### Current goose.toml (v1.0)

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
timestamp = "timestamp"
role = "role"
content = "content"

[parser.static]
source_agent = "goose"

[parser.project]
method = "field"
field = "working_dir"

[parser.model]
source = "none"

[parser.file_paths]
content_regex = true
```

### Log File Structure

Goose stores conversations as JSONL files with:

1. **Line 1**: Session metadata
   ```json
   {
     "working_dir": "/path/to/project",
     "description": "Session description",
     "timestamp": "2026-03-16T12:00:00Z",
     ...
   }
   ```

2. **Subsequent lines**: Message events
   ```json
   {
     "role": "user|assistant|system",
     "content": ["...array of content blocks..."],
     "timestamp": "2026-03-16T12:00:00Z"
   }
   ```

### Required Fields and Acceptable Values

#### Plugin Metadata
- **name** (string): Plugin identifier, e.g., `"goose"`
- **version** (string): Semver version, e.g., `"1.0"`

#### Source Configuration
- **paths** (array of strings): Glob patterns for log files
  - Must use `~` for home directory expansion
  - Supports `**` for recursive matching
  - Example: `["~/.local/share/goose/sessions/*.jsonl"]`

- **format** (string): Log file format
  - Acceptable values: `"jsonl"`, `"markdown"`, `"json-tree"`, `"sqlite"`, `"json-array"`
  - For goose: `"jsonl"`

- **exclude** (array of strings): Optional glob patterns to exclude
  - Example: `["*/subagents/*"]`

#### Session Detection
- **method** (string): How to identify sessions within log files
  - Acceptable values for JSONL:
    - `"one-file-per-session"`: Each file is one session
    - `"timestamp-gap"`: Detect sessions by time gaps between events
    - `"delimiter"`: Use pattern matching (for Markdown)

- **session_id_from** (string): Where to extract the session ID
  - `"filename"`: Use the filename (without extension)
  - `"field:<field_name>"`: Extract from a field in the first line

#### Parser Configuration
- **timestamp** (string): Field path for event timestamp
  - Uses dot notation for nested fields: `"message.timestamp"`
  - Can use `^` prefix to read from envelope wrapper: `"^timestamp"`

- **role** (string): Field path for message role
  - Required for all formats
  - Maps to canonical roles: `user`, `assistant`, `system`, `tool_call`, `tool_result`

- **content** (string): Field path for message content
  - Required for all formats
  - For goose, this may be an array: `"content"`

- **role_map** (optional map): Map source roles to canonical roles
  - Example: `toolResult = "tool_result"`

- **static** (map): Static fields to add to all events
  - `source_agent`: Always set (e.g., `"goose"`)

#### Project Detection

```toml
[parser.project]
method = "field"
field = "working_dir"
```

**Method types:**
1. **`method = "field"`**: Extract from a field in the event data
   - **`field`** (string): Field name to extract project path from
   - For goose: `"working_dir"` (from first-line metadata)

2. **`method = "parent_dir"`**: Use parent directory of the log file
   - No additional configuration needed
   - Example: Aider uses this (`.aider.chat.history.md` is in project root)

3. **`method = "git_root"`**: Run `git rev-parse --show-toplevel`
   - Finds the git repository root
   - Used when logs are within a git repo but not at the root

**How `ProjectDetection::Field` works:**

1. In the scraper (`src/scraper/mod.rs`), the `detect_project` function checks the project detection method:

```rust
ProjectDetection::Field { field: _ } => {
    // For field-based detection, we need to extract from the first event
    // This is handled in the parser, return None here
    Ok(None)
}
```

2. The actual field extraction happens during parsing:
   - The parser reads the first line of the JSONL file
   - Extracts the specified field (e.g., `working_dir`)
   - Sets it as the `project` field in the `ParseContext`
   - All subsequent events use this project value

3. Field extraction uses recursive JSON traversal:
   - Supports nested paths: `"metadata.working_dir"`
   - Can read from envelope wrapper with `^` prefix: `"^working_dir"`
   - Returns `None` if field is missing

#### Model Detection

```toml
[parser.model]
source = "none"
```

**Source types:**
1. **`source = "none"`**: No model information available
   - Used for agents that don't log model names
   - Results in `model: null` in canonical events

2. **`source = "static"`**: Hardcoded model name
   - **`value`** (string): Model name to use
   - Example: `value = "gpt-4"`

3. **`source = "metadata"`**: Extract from session metadata file
   - **`field`** (string): Field path in metadata JSON
   - Used with companion files like Claude Code's `session-meta.json`

4. **`source = "event"`**: Extract from event data
   - **`field`** (string): Field path in event JSON
   - Can use `^` prefix for envelope fields

## How jsonl.rs Handles First-Line Metadata

### Envelope Unwrapping (for agents with envelope structure)

When a plugin defines `[source.envelope]`, the JSONL parser can unwrap nested structures:

```toml
[source.envelope]
payload_field = "payload"      # Field containing the actual event
type_field = "type"            # Field containing the event type
type_routing = { session_meta = "meta", message = "event", noise = "skip" }
```

**Type routing actions:**
- **`"event"`**: Extract payload and produce canonical events
- **`"meta"`**: Accumulate metadata, produce no events
- **`"skip"`**: Drop the line entirely

**Field extraction with `^` prefix:**
- **`^field`**: Read from envelope wrapper (outer layer)
- **`field`**: Read from payload (inner layer)

Example:
```toml
[parser]
timestamp = "^timestamp"    # From wrapper
role = "role"                # From payload
content = "content"          # From payload
```

For goose, the first line metadata could be handled via:
1. **Envelope mode** (if goose adopts envelope structure):
   - Route first line type to `"meta"`
   - Accumulate `working_dir` into session metadata
   
2. **Simple field extraction** (current approach):
   - Read first line during `detect_sessions`
   - Extract `working_dir` field
   - Store in session context for all events

### First-Line Metadata Accumulation

Currently, the jsonl parser handles metadata lines through:

1. **Session detection** (`detect_sessions`):
   - Opens file, reads first line
   - Extracts session ID from filename or field
   - Stores session metadata (start/end offsets, parent session)

2. **ParseContext creation**:
   - Created before parsing any events
   - Contains session_id, source_agent, source_file
   - Can be extended to include pre-extracted project/model

3. **Event parsing** (`parse_line`):
   - Each line is parsed into events
   - Events inherit project from ParseContext
   - Field extraction can reference envelope or payload

## Content Array Expansion (for `content[]` blocks)

Goose messages have `content` as an array of blocks. This requires parser-side expansion:

```rust
// In jsonl.rs parse_line()
if let Some(content_array) = extract_content_array(&payload_json, "content") {
    for block in content_array {
        events.push(Event {
            content: block.text,
            role: mapped_role,
            // ... other fields
        });
    }
}
```

This is similar to Claude Code's `tool_use` expansion where compound events are split into atomic canonical events.

## Best Practices for Goose Plugin

1. **Use field-based project detection**: Since goose logs `working_dir`, use:
   ```toml
   [parser.project]
   method = "field"
   field = "working_dir"
   ```

2. **Handle content arrays**: The parser needs to expand `content[]` blocks into individual events

3. **Model detection**: Set to `none` unless goose adds model logging

4. **Session detection**: Use `one-file-per-session` with `filename` extraction

5. **Consider envelope migration**: If goose adopts envelope structure, use:
   ```toml
   [source.envelope]
   payload_field = "message"
   type_field = "type"
   type_routing = { session = "meta", message = "event" }
   ```

## Testing Strategy

To verify the goose plugin works correctly:

1. **Fixture creation**: Capture real goose session files
   - Session with metadata line
   - Multiple message events
   - Content array examples

2. **Conformance tests**: Run plugin conformance suite
   - Validate plugin schema
   - Test fixture parsing
   - Verify project extraction

3. **Integration tests**: Test end-to-end scraping
   - Scrape real goose logs
   - Verify canonical events
   - Check project field is correct

## References

- `/home/coding/AgentScribe/plugins/goose.toml` - Current goose plugin
- `/home/coding/AgentScribe/src/parser/jsonl.rs` - JSONL parser implementation
- `/home/coding/AgentScribe/src/plugin.rs` - Plugin schema definitions
- `/home/coding/AgentScribe/src/scraper/mod.rs` - Scraping orchestration
- `/home/coding/AgentScribe/plugins/BUILDING_PLUGINS.md` - Plugin authoring guide
