# Building AgentScribe Plugins

This guide explains how to create a scraper plugin for AgentScribe. A plugin teaches AgentScribe how to find, read, and normalize conversation logs from a specific coding agent.

## What a Plugin Is

A plugin is a single TOML file that declares:

1. **Where** to find log files on disk
2. **What format** the logs are in
3. **How to detect session boundaries** within the logs
4. **How to map fields** from the agent's native format to AgentScribe's canonical schema

No code is required. If your agent writes logs in a supported format (JSONL, Markdown, JSON tree, SQLite), you only need a TOML file.

---

## Quick Start

Create a file at `~/.agentscribe/plugins/<agent-name>.toml`:

```toml
[plugin]
name = "my-agent"
version = "1.0"

[source]
paths = ["~/.my-agent/history/*.jsonl"]
format = "jsonl"

[source.session_detection]
method = "one-file-per-session"
session_id_from = "filename"

[parser]
timestamp = "timestamp"
role = "role"
content = "content"

[parser.static]
source_agent = "my-agent"
```

Validate it:

```bash
agentscribe plugins validate ~/.agentscribe/plugins/my-agent.toml
```

Test a scrape:

```bash
agentscribe scrape --plugin my-agent --dry-run
```

---

## Plugin Specification

### `[plugin]` — Identity

| Field     | Required | Description                              |
|-----------|----------|------------------------------------------|
| `name`    | yes      | Unique identifier for this agent type. Used in session IDs, CLI filters, and output directory names. Must be lowercase, alphanumeric, and hyphens only. |
| `version` | yes      | Plugin definition version. Increment when you change the field mapping or source paths. AgentScribe uses this to detect when re-scraping is needed. |

### `[source]` — Where to Find Logs

| Field     | Required | Description                              |
|-----------|----------|------------------------------------------|
| `paths`   | yes      | Array of glob patterns pointing to log files. Supports `~` (home dir), `$ENV_VAR` expansion, and standard glob syntax (`*`, `**`, `?`). |
| `exclude` | no       | Array of glob patterns to exclude from matches. Evaluated after `paths`. |
| `format`  | yes      | Log file format. One of: `jsonl`, `markdown`, `json-tree`, `sqlite`. See [Format Reference](#format-reference) below. |

**Path examples:**
```toml
# Single known location
paths = ["~/.my-agent/conversations/*.jsonl"]

# Multiple search roots for project-local files
paths = ["~/projects/*/.my-agent.log", "~/repos/*/.my-agent.log"]

# Recursive search
paths = ["~/.my-agent/**/session-*.json"]

# Environment variable expansion
paths = ["$MY_AGENT_DATA_DIR/logs/*.jsonl"]
```

### `[source.session_detection]` — Session Boundaries

Agents store sessions differently. Some create one file per session. Others append to a single continuous file. This section tells AgentScribe how to split logs into discrete sessions.

| Field              | Required | Description                              |
|--------------------|----------|------------------------------------------|
| `method`           | yes      | Detection strategy. One of: `one-file-per-session`, `timestamp-gap`, `delimiter`. |
| `session_id_from`  | depends  | Where to extract the session ID. Required for `one-file-per-session`. |
| `gap_threshold`    | depends  | Time gap that signals a new session. Required for `timestamp-gap`. Format: `"30m"`, `"2h"`, `"1d"`. |
| `delimiter_pattern`| depends  | Regex pattern that marks session boundaries. Required for `delimiter`. |

**Method: `one-file-per-session`**

Each log file is one session. Most agents work this way.

```toml
[source.session_detection]
method = "one-file-per-session"
session_id_from = "filename"     # Use the filename (minus extension) as session ID
# OR
session_id_from = "field:sessionId"  # Extract from a JSON field in the first line
```

**Method: `timestamp-gap`**

For agents that append to a single file (e.g., Aider). A new session starts when the gap between consecutive entries exceeds the threshold.

```toml
[source.session_detection]
method = "timestamp-gap"
gap_threshold = "30m"
```

Note: requires the `timestamp` field mapping in `[parser]` to work. If the source format has no timestamps, use `delimiter` instead.

**Method: `delimiter`**

Split on a pattern in the log content. Useful for agents that write explicit session markers.

```toml
[source.session_detection]
method = "delimiter"
delimiter_pattern = "^---SESSION START---"  # Regex matched against each line
```

### `[parser]` — Field Mapping

Maps fields from the agent's native format to AgentScribe's canonical event schema. The mapping syntax depends on the `format`.

#### For `jsonl` and `json-tree` formats

Use dot-notation paths to reference nested JSON fields:

| Field       | Required | Description                              |
|-------------|----------|------------------------------------------|
| `timestamp` | yes      | Path to the timestamp field. Must be ISO 8601 or Unix epoch (ms). |
| `role`      | yes      | Path to the role field. Value must be one of: `user`, `assistant`, `system`, `tool_call`, `tool_result`. If the source uses different names, use `[parser.role_map]`. |
| `content`   | yes      | Path to the message content field.       |
| `type`      | no       | Path to an event type field. Used to filter which events to include (see `[parser.include_types]`). |
| `tool_name` | no       | Path to the tool/function name for tool call events. |
| `tool_args` | no       | Path to tool call arguments.             |
| `tokens_in` | no       | Path to input token count.               |
| `tokens_out`| no       | Path to output token count.              |

**Dot-notation examples:**
```toml
# Flat structure: {"role": "user", "content": "hello"}
role = "role"
content = "content"

# Nested: {"message": {"role": "user", "content": [{"text": "hello"}]}}
role = "message.role"
content = "message.content[0].text"

# Array access: {"parts": [{"type": "text", "value": "hello"}]}
content = "parts[0].value"
```

#### For `markdown` format

Markdown parsing uses prefix-based role detection:

| Field              | Required | Description                              |
|--------------------|----------|------------------------------------------|
| `user_prefix`      | yes      | Line prefix that identifies user messages (e.g., `"#### "`). |
| `assistant_prefix` | yes      | Line prefix for assistant messages. Use `""` (empty string) if assistant text has no prefix. |
| `tool_prefix`      | no       | Line prefix for tool output (e.g., `"> "`). |
| `system_prefix`    | no       | Line prefix for system messages.         |
| `timestamp_pattern`| no       | Regex to extract timestamps from the log. Group 1 should capture the timestamp string. |

#### For `sqlite` format

SQLite parsing requires specifying the query to extract conversations. AgentScribe opens SQLite databases in read-only mode and never modifies them.

| Field    | Required | Description                              |
|----------|----------|------------------------------------------|
| `query`  | yes      | SQL query that returns rows with the mapped fields. Use `{file}` as placeholder for the database path. |
| `key_filter` | no   | For key-value stores (like Cursor/Windsurf `state.vscdb`), the key pattern to read. When set, only rows whose key matches this pattern are selected. |
| `content_path` | no | JSONPath expression into the extracted value blob. Used when the database stores conversations as nested JSON blobs. Supports `[*]` for array access and dot-notation for nested fields. |

**Agents using this format:** Cursor, Windsurf

**Key-value stores (Cursor/Windsurf):**

Cursor and Windsurf store conversation data in a `state.vscdb` SQLite database as JSON blobs in a key-value table. Use `key_filter` to target the right keys:

```toml
[plugin]
name = "cursor"
version = "1.0"

[source]
paths = ["~/.cursor/state.vscdb", "~/.windsurf/state.vscdb"]
format = "sqlite"

[source.session_detection]
method = "delimiter"
delimiter_pattern = "^---TAB---"    # Sessions are delimited within the JSON blob

[parser]
query = "SELECT value FROM ItemTable WHERE key = 'composerData'"
key_filter = "composerData"
content_path = "tabs[*].messages[*]"
role = "role"
content = "content"
timestamp = "timestamp"

[parser.static]
source_agent = "cursor"
```

**Direct table queries:**

If the database stores conversations in a regular table with structured columns:

```toml
[plugin]
name = "my-agent"
version = "1.0"

[source]
paths = ["~/.my-agent/conversations.db"]
format = "sqlite"

[source.session_detection]
method = "field:session_id"

[parser]
query = "SELECT session_id, role, content, timestamp FROM messages ORDER BY timestamp"
role = "role"
content = "content"
timestamp = "timestamp"

[parser.static]
source_agent = "my-agent"
```

**Note:** AgentScribe opens SQLite databases in read-only mode (`?mode=ro`). If you get "database is locked" errors, the source agent may have an exclusive lock — scrape when the agent is idle.

### `[parser.role_map]` — Role Translation (Optional)

If the agent uses different names for roles, map them to AgentScribe's canonical roles:

```toml
[parser.role_map]
"human" = "user"
"ai" = "assistant"
"function_call" = "tool_call"
"function_result" = "tool_result"
"developer" = "system"
```

### `[parser.include_types]` — Event Filtering (Optional)

If the source contains event types you want to skip (progress events, file snapshots, etc.), specify which types to include:

```toml
[parser.include_types]
field = "type"
values = ["user", "assistant"]  # Only include these event types
```

Or exclude specific types:

```toml
[parser.exclude_types]
field = "type"
values = ["file-history-snapshot", "progress"]
```

### `[parser.static]` — Static Metadata (Optional)

Key-value pairs applied to every event from this plugin. Use this for fields that are constant for the agent type:

```toml
[parser.static]
source_agent = "my-agent"
source_version = "2.1.0"
```

### `[metadata]` — Supplementary Data Sources (Optional)

Paths to additional files that provide per-session metadata (token counts, summaries, outcomes). Supports `{session_id}` interpolation.

```toml
[metadata]
session_meta = "~/.my-agent/meta/{session_id}.json"
session_summary = "~/.my-agent/summaries/{session_id}.md"
```

| Field             | Required | Description                              |
|-------------------|----------|------------------------------------------|
| `session_meta`    | no       | JSON file with session-level metadata (duration, tokens, etc.). |
| `session_summary` | no       | Markdown or text file with a session summary. |
| `session_facets`  | no       | JSON file with LLM-generated session analysis. |

---

## Format Reference

### `jsonl` — JSON Lines

One JSON object per line. Most common format. Each line is parsed independently and field-mapped.

**Agents using this format:** Claude Code, Codex

### `markdown` — Structured Markdown

Append-only Markdown where roles are distinguished by line prefixes (headings, blockquotes, bare text). The parser splits content by prefix and assigns roles.

**Agents using this format:** Aider

### `json-tree` — JSON File Hierarchy

Multiple JSON files organized in a directory tree, where sessions/messages/parts are separate files linked by ID. The parser walks the tree and reassembles conversations.

**Agents using this format:** OpenCode

When using `json-tree`, add a `[source.tree]` section:

```toml
[source.tree]
session_glob = "session/{projectId}/{sessionId}.json"
message_glob = "message/{sessionId}/{messageId}.json"
part_glob = "part/{messageId}/{partId}.json"
session_id_field = "id"
message_session_field = "sessionId"
part_message_field = "messageId"
ordering_field = "createdAt"
```

### `sqlite` — SQLite Database

Data stored in SQLite databases, often as JSON blobs in key-value tables. The parser runs a SQL query, extracts the result, and parses embedded JSON.

**Agents using this format:** Cursor, Windsurf

---

## Canonical Event Schema (Target)

Every event your plugin produces is normalized into this schema. Your field mappings must resolve to these fields:

```json
{
  "ts": "2026-03-16T12:00:00Z",
  "session_id": "my-agent/abc123",
  "source_agent": "my-agent",
  "source_version": "2.1.0",
  "project": "/home/user/myproject",
  "role": "user",
  "content": "the message text",
  "tool": null,
  "tokens": {"input": 1200, "output": 450},
  "tags": ["git", "migration"]
}
```

| Field            | Type   | Description                              |
|------------------|--------|------------------------------------------|
| `ts`             | string | ISO 8601 timestamp of the event          |
| `session_id`     | string | `<agent-name>/<id>` — auto-prefixed by AgentScribe |
| `source_agent`   | string | Plugin name (from `[parser.static]` or `[plugin].name`) |
| `source_version` | string | Agent version, if available              |
| `project`        | string | Absolute path to the project directory   |
| `role`           | string | One of: `user`, `assistant`, `system`, `tool_call`, `tool_result` |
| `content`        | string | The message/event text content           |
| `tool`           | string | Tool name (for `tool_call`/`tool_result` roles), null otherwise |
| `tokens`         | object | `{input, output}` token counts, null if unavailable |
| `tags`           | array  | Extracted tags (auto-generated during indexing) |

---

## Walkthrough: Adding a New Agent

Suppose a new coding agent called "Pilot" stores its logs as JSONL at `~/.pilot/sessions/<date>/<uuid>.jsonl` with this line format:

```json
{"time": 1710590400, "id": "msg_001", "session": "abc-123", "speaker": "human", "text": "Fix the login bug", "input_tokens": 50, "output_tokens": 0}
{"time": 1710590420, "id": "msg_002", "session": "abc-123", "speaker": "ai", "text": "I'll look at auth.py...", "input_tokens": 0, "output_tokens": 200}
```

**Step 1:** Create the plugin file:

```toml
# ~/.agentscribe/plugins/pilot.toml

[plugin]
name = "pilot"
version = "1.0"

[source]
paths = ["~/.pilot/sessions/**/*.jsonl"]
format = "jsonl"

[source.session_detection]
method = "one-file-per-session"
session_id_from = "filename"

[parser]
timestamp = "time"               # Unix epoch — AgentScribe auto-detects epoch vs ISO 8601
role = "speaker"
content = "text"
tokens_in = "input_tokens"
tokens_out = "output_tokens"

[parser.role_map]
"human" = "user"
"ai" = "assistant"

[parser.exclude_types]
field = "speaker"
values = ["system_internal"]     # Skip internal system events if present

[parser.static]
source_agent = "pilot"
```

**Step 2:** Validate:

```bash
agentscribe plugins validate ~/.agentscribe/plugins/pilot.toml
```

Validation checks:
- All required fields present
- `paths` globs resolve to at least one file
- `format` is a supported type
- Field mappings reference valid dot-notation paths
- `role_map` target values are valid canonical roles
- No conflicting `include_types` / `exclude_types`

**Step 3:** Dry-run scrape:

```bash
agentscribe scrape --plugin pilot --dry-run
```

This shows what sessions would be scraped without writing any files. Review the output to confirm:
- Correct number of sessions detected
- Session boundaries are right
- Fields map correctly (timestamps parse, roles resolve)

**Step 4:** Full scrape:

```bash
agentscribe scrape --plugin pilot
```

**Step 5:** Verify:

```bash
agentscribe status
# Should show "pilot" with session count and last scrape time

agentscribe search "login bug" --plugin pilot
# Should find the session from the example above
```

---

## Common Patterns

### Agent that appends to a single file per project

```toml
[source]
paths = ["~/projects/*/.agent-history.log"]
format = "jsonl"

[source.session_detection]
method = "timestamp-gap"
gap_threshold = "1h"
```

### Agent with structured directories but no timestamps

```toml
[source.session_detection]
method = "one-file-per-session"
session_id_from = "filename"
# AgentScribe falls back to file modification time when no timestamp field is available
```

### Agent that uses delimiters between sessions

```toml
[source.session_detection]
method = "delimiter"
delimiter_pattern = "^={3,}\\s*New Session"
```

### Agent with sub-agent / sub-conversation logs

```toml
# Main conversations
[source]
paths = ["~/.agent/sessions/*.jsonl"]
exclude = ["*/sub-agents/*"]

# Optionally create a second plugin for sub-agents
# ~/.agentscribe/plugins/my-agent-subagents.toml
```

### Filtering to only conversation events

Many agents log internal events (file snapshots, progress updates) alongside conversation turns. Use type filtering to keep only what matters:

```toml
[parser.include_types]
field = "event_type"
values = ["message", "tool_call", "tool_result"]
```

---

## Testing Your Plugin

```bash
# Validate the TOML structure and field references
agentscribe plugins validate <plugin-file>

# Dry-run: show what would be scraped without writing
agentscribe scrape --plugin <name> --dry-run

# Scrape a single file to verify parsing
agentscribe scrape --plugin <name> --file <path-to-one-log-file>

# Show parsed events as JSON (for inspecting field mapping)
agentscribe scrape --plugin <name> --dry-run --output-events

# List all registered plugins and their status
agentscribe plugins list
```

---

## Troubleshooting

| Problem | Likely Cause | Fix |
|---------|-------------|-----|
| No files found | `paths` globs don't match | Run `agentscribe plugins validate` — it reports glob match counts. Check `~` expansion and directory existence. |
| Wrong session boundaries | Incorrect `session_detection` method | Try `timestamp-gap` with a shorter threshold, or switch to `delimiter` if the log has markers. |
| Roles not mapping | Agent uses non-standard role names | Add a `[parser.role_map]` section. Run `--dry-run --output-events` to see raw role values. |
| Timestamps not parsing | Non-standard format | AgentScribe auto-detects ISO 8601 and Unix epoch (seconds and milliseconds). For other formats, file an issue. |
| Missing content | Wrong field path | Check dot-notation in `[parser]`. Use `--dry-run --output-events` to see what's being extracted. |
| SQLite locked | Agent is actively writing | AgentScribe opens SQLite in read-only mode (`?mode=ro`). If still locked, the agent may have an exclusive lock — scrape when the agent is idle. |

---

## Community Contribution Workflow

If you write a plugin for an agent that AgentScribe doesn't cover yet, you can share it with the community by submitting it to the `examples/` directory. This is separate from the bundled plugins (which require deeper integration testing); contributed examples are lower-friction and easier to accept.

### Where to put it

Community plugins live in `examples/` at the repository root (not in `plugins/`, which is reserved for bundled plugins). See `examples/README.md` for the current list of contributed plugins.

### Contribution checklist

Before opening a pull request, confirm:

- [ ] Plugin validates cleanly: `agentscribe plugins validate examples/<agent-name>.toml`
- [ ] Dry-run shows correct session and event counts against real log files
- [ ] `name` is lowercase, alphanumeric + hyphens (e.g., `my-agent`)
- [ ] `version` is `"1.0"` for a new plugin
- [ ] `paths` covers all common platform locations (Linux, macOS, Windows if applicable)
- [ ] Non-obvious field choices or version dependencies are explained in comments
- [ ] No hardcoded absolute paths specific to your machine
- [ ] A row has been added to the table in `examples/README.md`

### Versioning your plugin

Increment `version` in the `[plugin]` section whenever you change field mappings or source paths. AgentScribe uses this value to detect when previously-scraped files need to be re-processed.

```toml
[plugin]
name    = "my-agent"
version = "1.1"   # bumped: added token_in / token_out mappings
```

### Submitting

1. Fork the repository.
2. Add your plugin as `examples/<agent-name>.toml`.
3. Add a row to `examples/README.md`.
4. Open a pull request with a brief description of the agent, how you tested the plugin, and which platform(s) you verified path expansion on.

Maintainers will review field mapping quality, path coverage across platforms, and session detection accuracy before merging.
