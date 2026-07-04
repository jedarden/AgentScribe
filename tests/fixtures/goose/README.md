# Goose Session Format

## Schema Source

Schema verified from: https://github.com/aaif-goose/goose/issues/2529

Goose stores sessions at `~/.local/share/goose/sessions/*.jsonl` with one JSON object per line.

## Format Structure

### Line 1: Session Metadata

```json
{
  "working_dir": "/Users/zane/Development/mcp_goose_configwatcher",
  "description": "File watcher config setup",
  "message_count": 14,
  "total_tokens": 12710,
  "input_tokens": 11685,
  "output_tokens": 1025,
  "accumulated_total_tokens": 75280,
  "accumulated_input_tokens": 72872,
  "accumulated_output_tokens": 2408
}
```

Key fields:
- `working_dir`: Project directory path (used for project detection)
- `description`: Session description
- `message_count`: Number of messages in session
- `total_tokens`, `input_tokens`, `output_tokens`: Token counts for this session
- `accumulated_*`: Cumulative token counts across sessions

### Subsequent Lines: Messages

```json
{
  "role": "user",
  "created": 1747178328,
  "content": [...]
}
```

- `role`: "user" or "assistant"
- `created`: Unix timestamp (seconds since epoch)
- `content`: Array of content blocks

### Content Block Types

The `content` array contains blocks of different types:

#### 1. Text Block
```json
{
  "type": "text",
  "text": "I have started a basic mcp template..."
}
```

#### 2. Tool Request Block
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

#### 3. Tool Response Block
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

## Event Expansion

AgentScribe expands content blocks as follows:
- `text` → Message content (text string)
- `toolRequest` → tool_call event (extracts tool name from `toolCall.value.name`)
- `toolResponse` → tool_result event (matches tool_call by `id`)

## Tool Correlation

Tool requests and responses are correlated by the `id` field:
- `toolRequest.id` == `toolResponse.id` pairs represent a complete tool interaction
- IDs have format `toolu_<random>_<base62>`

## Differences from Claude Code JSONL Format

Key differences that require special handling in the parser:

1. **Session metadata location**: Goose stores metadata in **line 1** of the JSONL file (`{working_dir, description, message_count, ...}`), while Claude Code uses a separate `session-meta/<uuid>.json` file.

2. **Field naming**: Goose uses `working_dir` vs Claude Code's `cwd`, and `created` (Unix timestamp) vs `timestamp` (ISO 8601).

3. **Content block structure**: Goose uses a `content[]` array with explicit `type` fields (`text`, `toolRequest`, `toolResponse`), while Claude Code embeds `tool_use` content blocks directly in assistant messages and uses separate event types.

4. **Tool request/response format**: Goose's `toolRequest` uses `toolCall.value.name` for the tool name and `toolCall.value.arguments` for parameters, while Claude Code uses `name` and `input` at the block level.

5. **No separate metadata file**: All session information is contained within the single JSONL file, whereas Claude Code has companion files for facets, summaries, and session metadata.

These differences are handled by the goose-specific event expansion logic in `src/parser/jsonl.rs` (the `content[]` array is unpacked into separate canonical events).
