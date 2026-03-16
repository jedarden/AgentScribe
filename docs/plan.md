# AgentScribe — Implementation Plan

## Overview

AgentScribe is a Rust CLI binary that scrapes conversation logs from multiple coding agent types, normalizes them into a canonical format, and stores them as flat files with a Tantivy search index. It serves three purposes:

1. **Archive** — capture and preserve the full prompt/response history from all agents running in an environment
2. **Search** — provide a query interface that agents can invoke to find past solutions, error patterns, and reference implementations
3. **Learn** — distill accumulated agent knowledge into actionable intelligence: error→solution mappings, anti-patterns, project rules, and effectiveness analytics

---

## Problem

Every coding agent stores its conversation history differently — JSONL, Markdown, JSON trees, SQLite blobs. When multiple agents operate in the same environment, there is no unified way to search across their collective knowledge. A solution one agent found last week is invisible to another agent today. Mistakes are repeated. Patterns go unnoticed. Institutional knowledge is locked inside individual agent log formats.

---

## Agent Log Formats (Known Sources)

### Claude Code
- **Location:** `~/.claude/projects/<path-encoded>/<session-uuid>.jsonl`
- **Format:** JSONL, one event per line
- **Schema:** `{type, uuid, sessionId, timestamp, cwd, version, gitBranch, message: {role, content}}`
- **Types:** `user`, `assistant`, `progress`, `file-history-snapshot`
- **Sub-agents:** `<session-uuid>/subagents/agent-<id>.jsonl` (same format, `isSidechain: true`)
- **Metadata:** `~/.claude/usage-data/session-meta/<session-uuid>.json` (duration, tokens, tool counts)
- **Summaries:** `~/.claude/usage-data/facets/<session-uuid>.json` (LLM-generated goal/outcome/summary)

### Aider
- **Location:** `.aider.chat.history.md` in project root
- **Format:** Markdown, append-only
- **Schema:** `#### ` prefix = user message, `> ` prefix = tool output, bare text = assistant response
- **Input history:** `.aider.input.history` (prompt_toolkit format: `# timestamp` + `+` prefixed lines)
- **Session marker:** `# aider chat started at YYYY-MM-DD HH:MM:SS` delimits sessions

### OpenCode
- **Location:** `~/.local/share/opencode/storage/` (legacy JSON) or `.opencode/` (current SQLite)
- **Format:** Individual JSON files in a hierarchy (legacy) or SQLite via Drizzle ORM (current)
- **Schema:** `session/{projectId}/{sessionId}.json`, `message/{sessionId}/{messageId}.json`, `part/{messageId}/{partId}.json`
- **Fields:** role, cost, tokens, timestamps; parts contain text, tool calls, tool results

### Codex (OpenAI)
- **Location:** `~/.codex/sessions/YYYY/MM/DD/rollout-{session_id}.jsonl` (may be `.jsonl.zst`)
- **Format:** JSONL, one event per line
- **Schema:** Line 1 is `RolloutLine::Meta` with `{thread_id, cwd, model}`, subsequent lines are `RolloutLine::Item` events
- **Note:** Internal format, subject to change; ephemeral mode writes no file

### Cursor
- **Location:** `~/.config/Cursor/User/globalStorage/state.vscdb` + `workspaceStorage/{hash}/state.vscdb`
- **Format:** SQLite (`state.vscdb`), `cursorDiskKV` table
- **Schema:** `composerData:<composerId>` keys for session metadata, `bubbleId:<composerId>:<bubbleId>` keys for messages
- **Note:** Requires SQLite extraction; global DB can grow to 25GB+; schema varies across versions

### Windsurf / Codeium
- **Location:** `~/.config/Windsurf/User/globalStorage/state.vscdb` + `workspaceStorage/{hash}/state.vscdb`
- **Format:** SQLite (`state.vscdb`), same `cursorDiskKV` table pattern as Cursor
- **Note:** Hard limit of 20 conversations (21st overwrites oldest) — scrape early or lose data

---

## Architecture

### CLI Commands

```
agentscribe <command>
├── config      # Manage global config and data directory (init|show|set|get)
├── plugins     # Manage scraper plugin definitions (list|validate|show)
├── scrape      # Discover and read agent log files from known locations
├── index       # Manage the Tantivy search index (rebuild|stats|optimize)
├── search      # Query the index — primary interface for agents
├── blame       # Bidirectional git commit ↔ session linking
├── file        # File knowledge map — show all sessions that touched a file
├── recurring   # Surface problems that keep being solved repeatedly
├── rules       # Auto-generate project rules from session patterns
├── analytics   # Agent effectiveness metrics and comparisons
├── summarize   # Generate Markdown summaries for sessions
├── digest      # Automated activity summary over a time period
├── status      # Show tracked agents, session counts, daemon state
├── daemon      # Long-running background process (start|stop|status|run|logs)
├── gc          # Delete old sessions, compact index, reclaim disk space
├── shell-hook  # Generate shell integration for search-on-error (bash|zsh|fish)
└── completions # Generate shell completions (bash|zsh|fish)
```

See [cli-reference.md](cli-reference.md) for detailed help on every command, flag, output format, and exit code.

### Scraper Plugin System

Each agent type is defined by a **scraper plugin** — a declarative TOML config that tells AgentScribe where to find logs and how to normalize them. Adding a new agent type means adding a plugin definition, not modifying code. See [../plugins/BUILDING_PLUGINS.md](../plugins/BUILDING_PLUGINS.md) for the full plugin authoring guide.

```toml
# ~/.agentscribe/plugins/claude-code.toml
[plugin]
name = "claude-code"
version = "1.0"

[source]
paths = ["~/.claude/projects/*/*.jsonl"]
exclude = ["*/subagents/*"]
format = "jsonl"

[source.session_detection]
method = "one-file-per-session"
session_id_from = "filename"

[parser]
timestamp = "timestamp"
role = "message.role"
content = "message.content"
type = "type"

[parser.static]
source_agent = "claude-code"

[parser.project]
method = "field:cwd"                 # Extract project path from session's cwd field
# Alternatives: "parent_dir" (parent of the log file), "git_root" (git rev-parse --show-toplevel)

[parser.file_paths]
tool_call_field = "input.file_path"  # Structured extraction from tool_call events
content_regex = true                 # Also extract paths from content via regex

[metadata]
session_meta = "~/.claude/usage-data/session-meta/{session_id}.json"
session_facets = "~/.claude/usage-data/facets/{session_id}.json"
```

```toml
# ~/.agentscribe/plugins/aider.toml
[plugin]
name = "aider"
version = "1.0"

[source]
paths = ["~/projects/*/.aider.chat.history.md", "~/repos/*/.aider.chat.history.md"]
format = "markdown"

[source.session_detection]
method = "delimiter"
delimiter_pattern = "^# aider chat started at (.+)$"

[parser]
user_prefix = "#### "
tool_prefix = "> "
assistant_prefix = ""

[parser.static]
source_agent = "aider"

[parser.project]
method = "parent_dir"                # Aider creates .aider.chat.history.md in the project root

[parser.file_paths]
content_regex = true                 # Aider has no structured tool_call fields; extract paths from content
```

Bundled plugins ship for Claude Code, Aider, OpenCode, and Codex. Cursor and Windsurf plugins are added in Phase 5 (SQLite format support). Users can add custom plugins by dropping a TOML file in `~/.agentscribe/plugins/`.

### Data Directory Layout

```
~/.agentscribe/                    # Or configurable via AGENTSCRIBE_DATA_DIR
├── config.toml                    # Global config
├── plugins/                       # Scraper plugin definitions (one TOML per agent type)
│   ├── claude-code.toml
│   ├── aider.toml
│   ├── opencode.toml
│   └── codex.toml
├── sessions/
│   ├── <agent>/<session-id>.jsonl # Normalized conversation logs (source of truth)
│   └── <agent>/<session-id>.md    # Markdown summary (human/agent readable)
├── index/
│   └── tantivy/                   # Tantivy search index (rebuildable from sessions)
└── state/
    └── scrape-state.json          # Per-source-file tracking (see below)
```

**Scrape state schema** — tracks position per source file for incremental scraping:

```json
{
  "sources": {
    "/home/user/.claude/projects/-home-coding/83f5a4e7.jsonl": {
      "plugin": "claude-code",
      "last_byte_offset": 485632,
      "last_modified": "2026-03-16T12:00:00Z",
      "last_scraped": "2026-03-16T12:05:00Z",
      "session_ids": ["claude-code/83f5a4e7"]
    },
    "/home/user/projects/myapp/.aider.chat.history.md": {
      "plugin": "aider",
      "last_byte_offset": 128000,
      "last_modified": "2026-03-16T11:00:00Z",
      "last_scraped": "2026-03-16T11:05:00Z",
      "sessions_found": 8,
      "last_delimiter_offset": 125000
    }
  }
}
```

**Per-format incremental strategy:**
- **JSONL:** Seek to `last_byte_offset`, read new lines. Resume is exact.
- **Markdown (Aider):** Seek to `last_delimiter_offset`, re-parse from the last delimiter boundary to pick up any appended content in the current session plus new sessions.
- **SQLite:** Compare file `mtime` to `last_scraped`. If unchanged, skip. If changed, query for sessions with `time_updated > last_scraped`.
- **Truncation detection:** If `last_byte_offset > current_file_size`, the file was truncated or rewritten (e.g., Windsurf's 20-conversation overwrite). Trigger a full rescan of that file.

**Session update on re-scrape:** When a source file changes after a session was already scraped (e.g., a resumed Claude Code session appends new events), AgentScribe uses a **replace strategy:**
1. Re-parse the entire source file for that session (source files are the authority)
2. Overwrite the normalized `<session-id>.jsonl` file (it's derived data)
3. Delete the old Tantivy document by `session_id`, then add the updated document
4. Re-run enrichment (outcome detection, solution extraction, error fingerprinting) on the updated session

For Aider (delimiter-based): only the last session in the file can change (previous sessions are immutable once a new delimiter appears). The scrape state's `last_delimiter_offset` identifies which session is still open.

Tantivy does not support in-place document updates — delete + add is the standard pattern. This is fast because it operates on one document at a time.

### Global Configuration (`config.toml`)

```toml
[general]
data_dir = "~/.agentscribe"           # Override with AGENTSCRIBE_DATA_DIR
log_level = "info"                    # error | warn | info | debug | trace

[scrape]
debounce_seconds = 5                  # Wait after file change before scraping
max_session_age_days = 0              # 0 = no limit; >0 = ignore sessions older than N days

[index]
tantivy_heap_size_mb = 50             # IndexWriter memory budget

[search]
default_max_results = 10
default_snippet_length = 200

[daemon]
mcp_enabled = false
mcp_socket = "~/.agentscribe/mcp.sock"
pid_file = "~/.agentscribe/agentscribe.pid"
log_file = "~/.agentscribe/daemon.log"

[sqlite]
cache_size_pages = 2000               # PRAGMA cache_size (~8MB)

[shell_hook]
stderr_capture = false                # Don't capture stderr (see Feature Details)
background = true                     # Run search in background subprocess

[outcome.weights]                     # Signal weights for outcome detection
success_confirmation = 3              # User says "thanks", "LGTM", etc.
success_clean_exit = 2                # Last tool_result exit code 0
failure_rejection = 3                 # User says "no", "wrong", "revert"
failure_error_exit = 2                # Last tool_result has error
abandoned_no_response = 2             # No user message after last assistant turn
threshold = 3                         # Minimum score to classify (below = unknown)

[cost.models]                         # Per-1M-token costs for analytics
"claude-sonnet-4-20250514" = { input = 3.0, output = 15.0 }
"claude-opus-4-20250514" = { input = 15.0, output = 75.0 }
"gpt-4o" = { input = 2.5, output = 10.0 }
"deepseek-chat" = { input = 0.14, output = 0.28 }
```

---

## Data Model

### Canonical Event Schema

Every conversation turn from every agent is normalized to this format:

```json
{
  "ts": "2026-03-16T12:00:00Z",
  "session_id": "claude-code/83f5a4e7",
  "source_agent": "claude-code",
  "source_version": "1.0.16",
  "project": "/home/coding/myproject",
  "role": "user|assistant|tool_call|tool_result|system",
  "content": "the text content",
  "tool": null,
  "tokens": {"input": 1200, "output": 450},
  "tags": ["git", "migration", "postgres"],
  "file_paths": ["/home/coding/myproject/src/auth.rs"],
  "error_fingerprints": ["ConnectionRefusedError:{host}:{port}"]
}
```

| Field | Type | Description |
|-------|------|-------------|
| `ts` | string | ISO 8601 timestamp |
| `session_id` | string | `<agent>/<id>` — auto-prefixed by AgentScribe |
| `source_agent` | string | Plugin name |
| `source_version` | string | Agent version, if available |
| `project` | string | Absolute path to the project directory |
| `role` | string | `user`, `assistant`, `system`, `tool_call`, `tool_result` |
| `content` | string | The message/event text |
| `tool` | string? | Tool name for `tool_call`/`tool_result` roles |
| `tokens` | object? | `{input, output}` token counts |
| `tags` | string[] | Auto-extracted tags (tool names, file types, technologies) |
| `file_paths` | string[] | File paths referenced in this event (extracted from tool calls) |
| `error_fingerprints` | string[] | Normalized error patterns found in this event |
| `model` | string? | LLM model name, if available in source data. `null` when unknown. |

### Session Manifest Entry

Each session produces a manifest entry used for search results and analytics:

```json
{
  "session_id": "claude-code/83f5a4e7",
  "source_agent": "claude-code",
  "project": "/home/coding/myproject",
  "started": "2026-03-16T10:00:00Z",
  "ended": "2026-03-16T10:45:00Z",
  "turns": 42,
  "summary": "Migrated Postgres schema from v3 to v4, added rollback script",
  "solution_summary": "ALTER TABLE users ADD COLUMN...; cargo sqlx migrate run",
  "outcome": "success|failure|abandoned|unknown",
  "tags": ["postgres", "migration", "schema"],
  "files_touched": ["db/migrations/004.sql", "src/models/user.rs"],
  "git_commits": ["a1b2c3d", "e4f5g6h"],
  "error_fingerprints": ["ConnectionRefusedError:{host}:{port}"],
  "model": "claude-sonnet-4-20250514"
}
```

### Tantivy Index Schema

```rust
let mut schema_builder = Schema::builder();

// Full-text searchable + stored
schema_builder.add_text_field("content", TEXT | STORED);
schema_builder.add_text_field("summary", TEXT | STORED);
schema_builder.add_text_field("solution_summary", TEXT | STORED);
schema_builder.add_text_field("code_content", TEXT | STORED);

// Exact match + faceted filtering
schema_builder.add_text_field("session_id", STRING | STORED);
schema_builder.add_text_field("source_agent", STRING | STORED | FAST);
schema_builder.add_text_field("project", STRING | STORED | FAST);
schema_builder.add_text_field("tags", STRING | STORED | FAST);
schema_builder.add_text_field("outcome", STRING | STORED | FAST);
schema_builder.add_text_field("error_fingerprint", STRING | STORED | FAST);
schema_builder.add_text_field("file_paths", STRING | STORED | FAST);
schema_builder.add_text_field("git_commits", STRING | STORED | FAST);
schema_builder.add_text_field("doc_type", STRING | STORED | FAST); // "session" | "code_artifact"

// Code artifact fields
schema_builder.add_text_field("code_language", STRING | STORED | FAST);
schema_builder.add_text_field("code_file_path", STRING | STORED | FAST);
schema_builder.add_bool_field("code_is_final", STORED | FAST);

// Date + numeric for range queries
schema_builder.add_date_field("timestamp", INDEXED | STORED | FAST);
schema_builder.add_u64_field("turn_count", INDEXED | STORED | FAST);
```

**Index principles:**
- One Tantivy index for all sessions and code artifacts. Cross-agent search is the primary use case.
- **One Tantivy document per session.** The `content` field is the conversation concatenated with role prefixes (`user: ...`, `assistant: ...`, `tool_call: ...`). This gives BM25 correct document-level term frequency and keeps session-level fields on a single document. For turn-level retrieval, fetch the flat JSONL file via `--session <id>`.
- **Content truncation policy:** `user` and `assistant` content is indexed in full (most searchable). `tool_call` content is indexed in full (contains file paths, arguments). `tool_result` content is truncated to first 1000 chars per event (captures errors and key output; full content is in the flat file). Total document content is capped at 500KB — if exceeded, the middle is trimmed (keep first 250KB + last 250KB) to preserve both the problem statement and the resolution.
- **Separate Tantivy documents per code artifact** with `doc_type: "code_artifact"`. Code artifacts share `session_id`, `source_agent`, `project`, and `timestamp` fields with their parent session for correlation.
- **Default search returns sessions only.** The `doc_type` facet filter is always applied: `agentscribe search "auth"` → `doc_type: "session"`. `agentscribe search --code "auth"` → `doc_type: "code_artifact"`. No mixed results, no cross-type score confusion.
- Incremental indexing: `IndexWriter::add_document()` on scrape. No full rebuild needed.
- Flat files remain the source of truth. `agentscribe index rebuild` recreates the index from session files if corrupted.
- Tantivy handles segment merging automatically via its `MergePolicy`.

---

## Phases

### Phase 1 — Plugin System, Scraping & Normalization
- Implement the scraper plugin framework:
  - TOML-based plugin definitions (source paths, format, field mapping)
  - Format-specific parsers: JSONL, Markdown, JSON tree (pluggable by `format` field)
  - Session detection strategies: `one-file-per-session`, `timestamp-gap`, `delimiter`
- Bundle default plugins for Claude Code, Aider, OpenCode, Codex
- Normalize all formats to canonical event schema via field mapping. **Event expansion:** Some agents embed tool calls inside assistant messages (e.g., Claude Code's `assistant` events contain `tool_use` content blocks). The format parser — not the TOML config — handles splitting these compound events into atomic canonical events (`assistant` text + `tool_call` + `tool_result`). The TOML maps simple fields; structural transformations are parser code. This keeps the plugin TOML declarative while handling agent-specific structural differences in the parser implementation.
- Write normalized sessions as JSONL flat files
- Track scrape state (last-seen offsets/timestamps) for incremental scrapes
- Extract file paths from events via two methods:
  - **Structured:** For agents with typed tool_call events (Claude Code, Codex, OpenCode), extract from the tool's input fields (e.g., `input.file_path`). The plugin TOML specifies the field path under `[parser.file_paths] tool_call_field = "input.file_path"`.
  - **Regex fallback:** For content strings (Bash commands, Aider text), match strings that contain `/` and a known file extension, or start with `./`, `~/`, `/`. Filter false positives (URLs, ANSI escape sequences). Relative paths resolved against the session's `project` field.
- CLI: `agentscribe config init`, `agentscribe scrape`, `agentscribe plugins list|validate|show`

### Phase 2 — Tantivy Indexing & Search
- Build Tantivy index from normalized sessions (BM25 scoring, faceted fields)
- Incremental indexing on scrape (no full rebuild required)
- Tag extraction via three-tier pipeline:
  - **Explicit:** tool names from `tool_call` events (`Edit`, `Bash`, `Read`), languages from code fence markers (` ```rust ` → `rust`)
  - **Structural:** file extensions from `file_paths` (`.rs` → `rust`, `.py` → `python`), command names from Bash content (`docker`, `git`, `npm`, `cargo`, `kubectl`), error types from `error_fingerprints`
  - **Keyword list:** match content against a bundled technology dictionary (~200 terms: framework names, databases, cloud services, protocols). Exact word-boundary matches only to avoid false positives.
- Fuzzy search via Tantivy's Levenshtein term queries
- "More like this" search via Tantivy's built-in `MoreLikeThisQuery` (`--like <session-id>`)
- Structured output mode (`--json`) for agent consumption
- Context budget packing: `--token-budget <n>` optimally fills available context using greedy knapsack (replaces fixed `--max-results` + `--snippet-length`)
- CLI: `agentscribe search`, `agentscribe index rebuild|stats|optimize`, `agentscribe status`

### Phase 3 — Enrichment & Intelligence
- **Summaries:** Auto-generate from a deterministic template — first user prompt (truncated to 200 chars) + outcome + files touched + solution summary (if available). The `summary` field (one-line, used in search results) is the first sentence of the first user prompt + outcome label. No ML/NLP needed — the first user prompt is always the best single-sentence description of what the session was about. Optional LLM-powered summarization can replace this in a later version.
- **Outcome detection:** Classify sessions using a signal-scoring system (see Feature Details: Outcome Detection)
- **Solution extraction:** For successful sessions, identify the resolution window and extract the fix into `solution_summary` (see Feature Details: Solution Extraction)
- **Error fingerprinting:** Regex pipeline extracts error patterns from `tool_result` and `assistant` events. Normalize by stripping variable parts (line numbers, paths, PIDs, timestamps). Store as `error_fingerprint` facet field for cross-session matching
- **Anti-pattern detection:** For failed/abandoned sessions, tag the approaches that preceded user rejection ("no", "wrong", "revert") as anti-patterns. Store what was tried, why it failed, and what worked instead
- **Code artifact extraction:** Extract fenced code blocks from conversations. Index each with language, file path, session ID, and whether it was the final applied version. Searchable via `--code` flag
- **Git commit correlation:** For sessions in git repos, run `git log` with the session's time window to find associated commits. Build reverse index: `commit_hash → session_id`
- CLI: `agentscribe summarize`, `agentscribe blame`, `agentscribe file`

### Phase 4 — Daemon Mode & MCP
- `agentscribe daemon start|stop|status|run|logs`
- File watcher (inotify/fswatch) for automatic scraping on log changes
- Scrape debounce (default 5s) to avoid thrashing during active sessions
- Incremental index updates (no full rebuild on every new session)
- Optional MCP server mode (Unix socket at `~/.agentscribe/mcp.sock`) exposing four tools:
  - `agentscribe_search` — parameters: `query` (string), `max_results` (int, default 10), `token_budget` (int, optional), `agent` (string, optional), `project` (string, optional), `since` (string, optional), `outcome` (string, optional), `error` (string, optional), `code` (bool, optional), `lang` (string, optional), `solution_only` (bool, optional), `like` (string, optional). Returns: same JSON as `agentscribe search --json`.
  - `agentscribe_status` — no parameters. Returns: plugin list, session counts, daemon state, index stats.
  - `agentscribe_blame` — parameters: `file` (string), `line` (int). Returns: matching session(s) with summaries.
  - `agentscribe_file` — parameters: `path` (string). Returns: chronological session list for the file.
  - All tools call the same core library as the CLI — MCP is a thin wrapper, not a separate implementation.
- Systemd user-level service integration

### Phase 5 — SQLite Format Support & Extended Agents
- Add `sqlite` format parser to the plugin framework (for Cursor, Windsurf, etc.)
- Bundle plugins for Cursor and Windsurf (SQLite extraction + JSON blob parsing)
- Community plugin examples directory
- Git auto-commit integration: optionally commit new sessions to a git repo on scrape

### Phase 6 — Analytics, Knowledge Synthesis & Shell Integration
- **Recurring problem detection:** Group error fingerprints by frequency; flag problems solved 3+ times within a configurable window (default 30 days). `agentscribe recurring`
- **Agent effectiveness analytics:** Aggregate session metadata by agent type — success rates, turns/tokens per resolution, specialization, trends, cost efficiency. `agentscribe analytics`
- **Auto-generated project rules:** Distill session patterns into CLAUDE.md/.cursorrules files. Extract user corrections, tool preferences, architecture conventions, known pitfalls. `agentscribe rules`
- **File knowledge map enhancements:** "Known gotchas" section using anti-patterns and solution extractions filtered to the file
- **Weekly digest:** Automated activity summary with session counts, recurring problems, agent comparison, most-touched files, token usage trends. `agentscribe digest`
- **Search-on-error shell hook:** `agentscribe shell-hook bash|zsh|fish` generates shell integration that auto-queries the error fingerprint index when any command fails. Background subprocess, never blocks the shell.
- **Session lifecycle management:** `agentscribe gc [--older-than <duration>] [--dry-run]` deletes normalized session files older than the specified age, removes their Tantivy documents, and runs `index optimize` to compact segments. Respects `max_session_age_days` from config.toml as a default. `--dry-run` shows what would be deleted without acting.

---

## Search Interface (Agent-Facing)

The primary consumer of search is other agents running in the environment. The interface is designed for programmatic use.

### Core Principles

- **CLI-callable** — agents invoke `agentscribe search` as a subprocess
- **Structured output** — `--json` returns machine-readable results
- **Fast** — Tantivy index-based lookup, sub-50ms typical latency
- **Context-aware** — `--token-budget` lets agents specify how much context they can spare

### Search Modes

| Mode | Flag | Description |
|------|------|-------------|
| Full-text | (default) | BM25-ranked search across session content and summaries |
| Error lookup | `--error <pattern>` | Match against normalized error fingerprints |
| Anti-pattern | `--anti-patterns <query>` | Find approaches that failed for a given problem |
| Code search | `--code <query> [--lang <lang>]` | Search extracted code artifacts by content and language |
| Solution-only | `--solution-only` | Return only the extracted solution, not the full session |
| Similar sessions | `--like <session-id>` | Find sessions with similar content (Tantivy MoreLikeThis) |
| File history | via `agentscribe file <path>` | All sessions that touched a given file |

### Filtering

All search modes support these filters:

| Filter | Flag | Description |
|--------|------|-------------|
| Agent type | `--agent <name>` | Filter by source agent (repeatable) |
| Project | `--project <path>` | Filter by project directory |
| Date range | `--since <date>` / `--before <date>` | ISO 8601 or relative (`24h`, `7d`, `1w`) |
| Tags | `--tag <tag>` | Filter by tag (repeatable, AND logic) |
| Outcome | `--outcome <value>` | `success`, `failure`, `abandoned`, `unknown` |

### Output Sizing

| Parameter | Description |
|-----------|-------------|
| `--max-results <n>` | Maximum number of results (default 10) |
| `--snippet-length <n>` | Max chars per snippet (default 200) |
| `--token-budget <n>` | Replaces both above: optimally pack results into N tokens using greedy knapsack. Maximizes information density within the agent's available context. |

### Example Agent Workflow

```bash
# Agent hits an error — check if it's been solved before
agentscribe search --error "ENOSPC: no space left on device" --json -n 1

# Check if an approach is known-bad before trying it
agentscribe search --anti-patterns "mock database" --project /home/coding/myapp --json

# Find working code for a specific pattern
agentscribe search --code "connection pool config" --lang rust --json -n 3

# Get just the fix, not the debugging journey
agentscribe search "postgres migration v3 to v4" --solution-only --json

# Context-constrained: fill 4000 tokens optimally
agentscribe search "redis caching" --token-budget 4000 --json

# What conversation produced this line of code?
agentscribe blame src/auth.rs:42

# What does every agent know about this file?
agentscribe file src/auth/middleware.rs

# Find sessions similar to this one
agentscribe search --like claude-code/83f5a4e7 --json -n 5

# Weekly activity summary
agentscribe digest --since 7d
```

---

## Design Principles

- **CLI-first, MCP-also** — the CLI is the primary interface; MCP is a secondary access layer for agents that support it
- **Flat files first** — all data is plain text (JSONL + Markdown); the Tantivy index is derived and rebuildable
- **Git-native** — append-only JSONL and Markdown are diff-friendly; the data dir can be a git repo
- **Incremental** — scraping tracks offsets so re-runs are fast; only new data is processed
- **Agent-readable** — search output is structured JSON; summaries are Markdown
- **No external dependencies** — core scraping, indexing, and search work offline with no APIs
- **Non-invasive** — read-only access to agent logs; never modifies source files
- **Pluggable** — new agent types are added via TOML plugin definitions, not code changes
- **Low footprint** — daemon idles under 20MB RSS; scraping stays under 50MB regardless of source file size
- **Learning system** — every session makes future sessions better via error fingerprinting, anti-patterns, and auto-generated rules

---

## Memory Budget

AgentScribe runs alongside the agents it monitors. The memory budget is designed to be invisible.

### Target: <20MB idle, <50MB active, <100MB peak

| Component | Expected RSS | Notes |
|-----------|-------------|-------|
| **Daemon idle** (watcher + tokio) | 5-10MB | `notify` inotify watcher + minimal tokio runtime |
| **JSONL streaming parse** | 1-5MB | `serde_json::StreamDeserializer` processes one line at a time regardless of file size |
| **Tantivy index (search)** | 10-30MB | Memory-mapped segments; RSS is only actively accessed pages |
| **Tantivy indexing (write)** | 20-50MB | Controlled via `IndexWriter::new(heap_size)` |
| **SQLite reads** (Cursor/Windsurf) | 5-15MB | `rusqlite` read-only; `PRAGMA cache_size = -8000` (8MB) |
| **Markdown parsing** (Aider) | 1-2MB | `pulldown-cmark` streaming pull parser |

| Mode | Max RSS |
|------|---------|
| CLI one-shot search | 15-35MB |
| CLI one-shot scrape | 25-55MB |
| Daemon idle | 8-15MB |
| Daemon active scrape | 30-60MB |
| Daemon peak (scrape + search) | 50-90MB |

### Rules to Stay Within Budget

1. **Stream, never slurp.** One JSONL line at a time via `BufReader`. Never read an entire file into memory.
2. **Cap Tantivy's writer heap.** 20-50MB via `IndexWriter::new(arena_size)`.
3. **Cap SQLite page cache.** `PRAGMA cache_size = -8000` (8MB).
4. **Drop the index reader when idle.** Reopen on query; let the OS reclaim mapped pages.
5. **Process one source file at a time.** No parallel scraping — I/O is the bottleneck.
6. **Use Rust's default allocator.** It returns memory to the OS. Avoid `jemalloc` unless profiling shows fragmentation.

### Monitoring

```
agentscribe daemon status

# AgentScribe daemon (PID 12345)
#   Uptime:     3d 14h
#   RSS:        12MB (idle)
#   Peak RSS:   47MB
#   Sessions:   1,247 indexed
#   Index size: 38MB on disk
#   Last scrape: 2m ago (3 new sessions)
```

---

## Decisions

### Language: Rust

Rust, primarily because of **Tantivy** — the best embeddable full-text search library in any language. 3-5x faster indexing than Go's Bleve, sub-millisecond query latency, compact columnar index storage.

| Component | Crate | Purpose |
|-----------|-------|---------|
| Full-text search | `tantivy` | Inverted index, BM25, faceted search, fuzzy/phrase queries |
| JSON parsing | `serde_json` / `simd-json` | Streaming JSONL, zero-copy deserialization |
| File watching | `notify` | Cross-platform inotify/kqueue/FSEvents |
| Markdown parsing | `pulldown-cmark` | Streaming CommonMark parser for Aider logs |
| SQLite | `rusqlite` (bundled) | Read Cursor/Windsurf state.vscdb files |
| TOML config | `toml` / `serde` | Plugin definition parsing |
| CLI framework | `clap` | Command/subcommand parsing, shell completions |
| Async runtime | `tokio` | Daemon mode, file watcher event loop |
| Glob matching | `globset` | Source path expansion in plugin definitions |

### Search Engine: Tantivy (Meilisearch-inspired)

Meilisearch's core engine (`milli`) demonstrates that sub-50ms search is achievable through FSTs + Levenshtein automata, roaring bitmaps for set operations, pre-computed positional data, and memory-mapped index files. Tantivy provides all of this.

**Adopted from Meilisearch:** FST + Levenshtein automata for typo-tolerant lookup, roaring bitmaps for document ID set operations, pre-computed positional data, memory-mapped segments.

**Not adopted:** 20+ LMDB databases per index, word-pair proximity pre-computation, full bucket-sort ranking cascade, `charabia` multi-language tokenizer — all overkill for conversation logs at AgentScribe's scale.

### CLI over MCP, Support Both

The CLI is the primary interface. Every feature works via `agentscribe <command>`. Agents call it as a subprocess — the most universal integration path.

MCP is an optional secondary layer. The daemon can host an MCP server exposing `search`, `status`, `blame`, and `file` as tools via Unix socket. MCP is never required.

```
CLI (primary)     →  core library  →  flat files + Tantivy
MCP server (opt)  →  core library  →  flat files + Tantivy
```

### Daemon Mode

A long-running background process for continuous scraping:

1. **Watches** agent log directories for changes (inotify/fswatch, not polling)
2. **Scrapes incrementally** when new log data appears (5s debounce)
3. **Updates indexes** after ingesting new sessions
4. **Serves MCP** if enabled

```bash
agentscribe daemon start          # Background, writes PID file
agentscribe daemon run            # Foreground (for systemd)
agentscribe daemon stop           # SIGTERM + clean shutdown
agentscribe daemon status         # State, RSS, activity
agentscribe daemon logs [-f]      # Tail the daemon log
```

**Systemd integration:**
```ini
# ~/.config/systemd/user/agentscribe.service
[Unit]
Description=AgentScribe daemon
After=network.target

[Service]
ExecStart=%h/.local/bin/agentscribe daemon run
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
```

The daemon does not require root, does not open network ports (unless MCP is enabled on localhost/Unix socket), and never modifies agent log files.

---

## Session Boundary Detection

### Claude Code — One file per session

Each `<session-uuid>.jsonl` file is one session. The filename UUID matches the `sessionId` field.

- **Start:** `hookEvent: "SessionStart"` on line 1. Resumed sessions append a second `SessionStart` — treat the entire file as one session.
- **End:** Last line in file (no explicit end marker).
- **Sub-agents:** `<session-uuid>/subagents/agent-<id>.jsonl`. Excluded by default.
- **Headless:** (`claude --print`) may lack hooks. Still one file = one session.

```toml
[source.session_detection]
method = "one-file-per-session"
session_id_from = "filename"
```

### Aider — Delimiter in continuous file

Each `aider` launch writes `# aider chat started at YYYY-MM-DD HH:MM:SS`. Everything between one delimiter and the next (or EOF) is one session.

- **Start:** `# aider chat started at <datetime>` (written by `io.py`).
- **End:** Next delimiter line, or EOF.
- **Session ID:** `aider/<project_hash>/<timestamp>` — project_hash is first 8 chars of SHA-256 of the parent directory's absolute path; timestamp is the delimiter datetime formatted as `YYYYMMDD-HHMMSS`. Example: `aider/a1b2c3d4/20260316-104200`. Deterministic (re-scraping produces the same ID), human-readable, collision-resistant.
- **Enrichment:** `.aider.input.history` has per-input timestamps for finer granularity.
- **Edge case:** Scripted aider (`--yes`, piped input) may not write the marker. Fallback to `timestamp-gap` using `.aider.input.history`. Session ID uses file mtime for the timestamp component.

```toml
[source.session_detection]
method = "delimiter"
delimiter_pattern = "^# aider chat started at (.+)$"
```

### OpenCode — One row per session (SQLite)

Each session is a row in the `sessions` table with a descending ULID.

- **Start/End:** `time_created` / `time_updated` fields.
- **Child sessions:** Auto-compact creates children via `parentID` — each is discrete.
- **Legacy:** Older versions used JSON files at `~/.local/share/opencode/storage/`.

```toml
[source.session_detection]
method = "one-file-per-session"
session_id_from = "field:id"
```

### Codex — One file per session

Each `rollout-<id>.jsonl` (or `.jsonl.zst`) is one session. Line 1 is a `RolloutLine::Meta` header.

- **Start:** `RolloutLine::Meta` with `thread_id`, `cwd`, `model`.
- **End:** EOF. Resumed sessions append to same file.
- **Compressed:** May be `.jsonl.zst` — requires decompression.
- **Ephemeral:** `EventPersistenceMode::None` writes no file.
- **Companion:** `~/.codex/session_index.jsonl` maps thread IDs to metadata.

```toml
[source.session_detection]
method = "one-file-per-session"
session_id_from = "field:thread_id"
```

### Cursor — One key per session (SQLite)

Each session is a `composerData:<composerId>` key in `cursorDiskKV`.

- **Start/End:** `createdAt` / `lastUpdatedAt` millisecond timestamps. `status`: `"completed"` or `"aborted"`.
- **Messages:** `bubbleId:<composerId>:<bubbleId>` keys, ordered by `rowid ASC`.
- **Two databases:** Global has content, workspace has UI state.
- **Size warning:** Global DB can grow to 25GB+. Open read-only (`?mode=ro`).

```toml
[source.session_detection]
method = "one-file-per-session"
session_id_from = "field:composerId"
```

### Windsurf — One key per session (SQLite)

Same architecture as Cursor. **Critical limitation:** Hard limit of 20 conversations — the 21st overwrites the oldest. Must scrape frequently or data is permanently lost. Format may use protobuf with no public schema in newer versions.

```toml
[source.session_detection]
method = "one-file-per-session"
session_id_from = "field:composerId"
```

---

## Feature Details

### Outcome Detection

A signal-scoring system classifies each session. Each signal contributes a weighted score toward one outcome. The highest-scoring classification wins. Conflicting signals cancel out and produce `unknown`.

**Signals for `success` (positive score):**
- User's last message matches confirmation patterns: `thanks`, `that works`, `perfect`, `LGTM`, `looks good`, `great`, `nice` (+3)
- Last `tool_result` has exit code 0 or contains no error patterns (+2)
- Session ends with a short user turn (<30 chars) after an assistant turn — likely a confirmation (+1)
- A git commit was made in the session's time window (+1)
- Tests pass in the final Bash tool call (exit code 0 + content matches `test|spec|check`) (+2)

**Signals for `failure` (positive score):**
- User's last message matches rejection patterns: `no`, `wrong`, `doesn't work`, `broken`, `revert`, `undo` (+3)
- Last `tool_result` contains error patterns (stack trace, non-zero exit code) (+2)
- Last assistant response contains apology patterns: `I apologize`, `sorry about`, `my mistake` (+1)
- User says `stop`, `nevermind`, `forget it` (+3)

**Signals for `abandoned`:**
- Session's last event is an `assistant` message with no subsequent `user` message (+2)
- Time gap between last event and file modification time is >1 hour (+1)
- Session has <3 turns (started but never engaged) (+1)

**Default:** If no outcome scores above a configurable threshold (default: 3), the session is classified as `unknown`.

**Why signal-scoring:** New signals can be added without changing the algorithm. Ambiguous sessions get `unknown` rather than a wrong classification. Weights are tunable in `config.toml` under `[outcome.weights]`.

---

### Error Fingerprinting + Solution Index

Automatically extract error messages and stack traces from conversations, normalize them, and build an `error_fingerprint → [solution_sessions]` index.

**Structural matchers** detect that a line is an error:

| Language/Type | Pattern |
|---------------|---------|
| Python | `^\s*\w+Error: .+` or `^Traceback \(most recent call last\):` |
| Rust | `^error\[E\d+\]:` or `^thread '.+' panicked at` |
| Node/JS | `^\w+Error: .+` at start of line |
| Shell | `^.+: command not found$` or `^.+: No such file or directory$` |
| Go | `^panic: .+` or `^fatal error:` |
| Generic | `^(FATAL\|ERROR\|CRITICAL\|FAIL)[\s:]+` |
| HTTP | `HTTP/\d\.\d\s+[45]\d{2}` |
| Exit codes | `exit code (\d+)` or `exited with status (\d+)` where code != 0 |

**Normalizers** strip variable parts in this order:

| Replacement | Pattern | Example |
|-------------|---------|---------|
| `{path}:{line}:{col}` | `/path/to/file.rs:42:5` | File paths with line numbers |
| `{path}` | Absolute or relative file paths | `/home/user/src/main.rs` → `{path}` |
| `{host}` | IPs, hostnames, FQDNs | `192.168.1.100` → `{host}` |
| `{port}` | `:\d{2,5}` following a host | `:5432` → `:{port}` |
| `{pid}` | `PID \d+` or `pid=\d+` | `PID 12345` → `PID {pid}` |
| `{ts}` | ISO 8601, common datetime formats | `2026-03-16T12:00:00Z` → `{ts}` |
| `{uuid}` | UUID v4/v7 patterns | `550e8400-e29b-...` → `{uuid}` |
| `{addr}` | Hex memory addresses `0x[0-9a-f]+` | `0x7fff5fbff8c0` → `{addr}` |

**Fingerprint format:** `<error_type>:<normalized_message>`. Example:
- Input: `ConnectionRefusedError: Connection refused to postgres-primary.svc:5432`
- Fingerprint: `ConnectionRefusedError:Connection refused to {host}:{port}`

- **Index:** Fingerprints stored as Tantivy `STRING | FAST` facet. Queryable via `agentscribe search --error`
- **Ranking:** Sessions that both encountered AND resolved the error rank highest, sorted by recency
- **Extensibility:** Users can add custom matchers and normalizers in `config.toml` under `[error_patterns.custom]`

```bash
agentscribe search --error "ENOSPC: no space left on device" --json -n 1
```

### Anti-Pattern Library

Catalog approaches that failed so agents don't repeat known mistakes.

- **Detection:** Sessions with `outcome: "failure"` or `"abandoned"` are analyzed. The "rejection window" is the 3 tool_calls immediately preceding a user message matching rejection patterns (`no`, `wrong`, `doesn't work`, `revert`, `undo`, `stop`). These tool_calls are the anti-pattern.
- **Storage:** Each anti-pattern records: (1) what was tried (the tool_call names + truncated arguments), (2) why it failed (the user's rejection message), (3) the error fingerprint active at that point.
- **Linking to solutions:** A "working alternative" is found by querying for sessions with the SAME error fingerprint + same project + `outcome: "success"` + later timestamp. The successful session's `solution_summary` becomes the anti-pattern's "what worked instead." If no matching successful session exists, the field is `null` — the anti-pattern simply says "this doesn't work" without a known alternative.
- **Query:** `agentscribe search --anti-patterns "mock database" --json`

### Code Artifact Extraction

Index code blocks as first-class searchable artifacts, separate from session content.

- **Extraction:** Fenced code blocks from assistant responses and tool_call/tool_result events
- **Metadata:** Language (from fence marker), file path (from surrounding tool_call context), session ID, final-version flag
- **`code_is_final` detection:** A code block is marked final when it is the last code block for a given file path within the session AND the session's outcome is `success`. This is a positional + outcome heuristic that works across all agent formats without agent-specific logic. No need to inspect individual tool_result success — the session outcome is the authority.
- **Ranking:** Final/applied code blocks rank higher than intermediate drafts
- **Query:** `agentscribe search --code "connection pool" --lang rust --json`
- **Index:** Stored as documents with `doc_type: "code_artifact"` in the same Tantivy index

### Solution Extraction

For successful sessions, extract the fix into `solution_summary` by identifying the **resolution window** — the consecutive sequence of tool_calls between the last error and the session end (or user confirmation).

**Finding the resolution window:**
1. Walk backward from the session's last event
2. The window **ends** at: user confirmation signal (`thanks`, `LGTM`, `that works`), or session end
3. The window **starts** at: the last `tool_result` containing an error before the window end, OR the last user prompt if no error is found
4. All `tool_call` events within this window are collected as the solution

**What goes into `solution_summary`:**
- All `Edit`/`Write` tool calls in the window (the code changes), concatenated in order
- All `Bash` tool calls in the window that exited 0 (the commands that worked)
- The assistant explanation immediately preceding the first tool call in the window (the rationale)
- If no tool calls exist in the window, fall back to the code block in the final assistant turn

This captures multi-file edits + verification commands as a single solution rather than extracting just one tool call.

Queryable via `agentscribe search "postgres migration" --solution-only --json`.

### Git Blame Bridge

Bidirectional linking between conversations and git commits.

- **Forward:** During scrape enrichment, `git log --after=<start> --before=<end>` finds commits made during the session. Stored as `git_commits` field.
- **Reverse:** `commit_hash → session_id` index via Tantivy facet.
- **Overlap handling:** When multiple sessions have overlapping time windows, a commit is attributed to ALL matching sessions. Matches are scored by: (1) file overlap — if the session's `files_touched` intersects the commit's changed files, score +2; (2) time containment — commit timestamp within session window, score +1. `agentscribe blame` displays all matching sessions sorted by score, so the user can disambiguate.
- **CLI:** `agentscribe blame src/auth.rs:42` runs `git blame`, finds the commit, then looks up the session(s).

### Context Budget Packing

`--token-budget <n>` replaces `--max-results` + `--snippet-length` with a single constraint.

- **Estimation:** `ceil(chars / 4)` tokens per result
- **Algorithm:** Greedy knapsack — rank by relevance, pack until budget full
- **Adaptive:** Chooses between fewer results with longer snippets vs more results with shorter snippets, maximizing total relevance coverage
- **Integration:** Solution-only mode produces shorter results, so more fit in the budget

### Recurring Problem Detection

Surface problems solved 3+ times within a window (default 30 days).

- **Source:** Error fingerprint frequency analysis (GROUP BY fingerprint, HAVING count >= 3)
- **Output:** Frequency, affected projects, which agents solved it, last fix applied
- **CLI:** `agentscribe recurring [--since <date>] [--threshold <n>]`

### File Knowledge Map

For any file, show every conversation any agent has ever had about it.

- **Index:** `file_path → [session_ids]` reverse index via Tantivy multi-valued STRING field
- **Sources:** File paths extracted from tool_call events (Read, Edit, Write, Bash file arguments)
- **Output:** Chronological list of sessions with summaries, outcomes, and known gotchas (from anti-patterns filtered to this file)
- **CLI:** `agentscribe file <path>` or `agentscribe file "src/auth/**"` for directory-level

### Auto-Generated Project Rules

Distill session patterns into rules files for specific agents.

**What gets extracted:**
- Explicit user corrections ("don't use X, use Y") → rule
- Repeated tool preferences (if `pnpm` always used, not `npm`) → convention
- Architecture patterns (test directory, ORM, framework) → context
- Known pitfalls from the anti-pattern library → warnings

**CLI:** `agentscribe rules <project-path> [--format claude|cursor|aider]`

**Output formats:**
- `--format claude` → `CLAUDE.md` — Markdown with `# Codebase`, `## Conventions`, `## Known Issues` sections. Rules as bullet points. Compatible with Claude Code's CLAUDE.md spec.
- `--format cursor` → `.cursorrules` — plain text, one instruction per line. Cursor reads this as system-level context.
- `--format aider` → `.aider.conf.yml` — YAML with `read:` (key files to always load) and `message:` (convention reminders appended to system prompt).

No LLM required for v1 — frequency-based heuristics over tool invocations and user corrections. LLM refinement can enhance later.

### Agent Effectiveness Analytics

Cross-agent performance comparison — data only possible with AgentScribe's unified view.

**Metrics:**
- Success rate per agent
- Average turns and tokens per successful outcome
- Specialization by problem type
- Trends over time (model updates, prompt changes)
- Cost efficiency (outcome quality per dollar of token spend, using `[cost.models]` from config.toml). **Best-effort:** model name is extracted from source metadata where available (Claude Code `session-meta.json`, Codex `RolloutLine::Meta`, OpenCode session fields). Agents that don't log the model (e.g., Aider) produce `model: null` — their sessions are excluded from cost calculations but included in all other metrics.

**Problem type classification** (rule-based, stored as a tag):

| Type | Signals |
|------|---------|
| `debug` | Session contains error fingerprints, or content matches `fix\|bug\|error\|crash\|broken\|not working` |
| `feature` | `Write` tool calls create new files, or content matches `add\|implement\|create\|build\|new feature` |
| `refactor` | Content matches `refactor\|rename\|move\|extract\|clean up`, and no new files created |
| `investigation` | Read-to-Edit tool call ratio >3:1, or content matches `explain\|how does\|what is\|understand` |
| `configuration` | `files_touched` includes `.toml`, `.yaml`, `.json`, `.env`, `Dockerfile`, `Makefile` |
| `documentation` | `files_touched` includes `.md`, `.rst`, or content matches `document\|readme\|changelog` |

First match in priority order above is the primary type. Multiple types can apply as secondary tags.

**CLI:** `agentscribe analytics [--agent <name>] [--project <path>] [--since <date>]`

### Search-on-Error Shell Hook

A shell integration (`PROMPT_COMMAND` for bash, `precmd` for zsh) that detects when any command exits non-zero and silently queries AgentScribe in the background.

```
💡 AgentScribe: this error was solved in session claude-code/83f5 — run `agentscribe search --session claude-code/83f5`
```

- **No stderr capture.** Intercepting stderr is fragile (breaks pipes, interferes with progress bars, requires `exec` redirection). Instead, the hook passes the failed command text (via `fc -ln -1`) and exit code to `agentscribe search --error "<command> exit code <code>" --json -n 1`. The error fingerprint index is searched via full-text, so `cargo test` failing will match past sessions where `cargo test` also failed — no exact stderr match needed.
- **Performance:** Background subprocess (`&`); never blocks the shell. A temp file holds the result. On the next prompt, if the file exists and is non-empty, print the one-line hint and delete the file. If the search takes longer than one prompt cycle, the result is silently discarded.
- **Setup:** `eval "$(agentscribe shell-hook bash)"` in `.bashrc` or `eval "$(agentscribe shell-hook zsh)"` in `.zshrc`
- **CLI:** `agentscribe shell-hook bash|zsh|fish` generates the shell integration snippet

### "More Like This" Search

Given a session ID, find the most similar sessions across all agents and projects using Tantivy's built-in `MoreLikeThis` query.

- **How it works:** Tantivy extracts the most significant terms from the source document (by TF-IDF weight) and uses them to query the index for documents with similar term distributions
- **Query:** `agentscribe search --like <session-id> [--json] [-n <max>]`
- **Value:** Discovery of related work you didn't know existed. "I just fixed a connection pooling issue — what other connection pooling sessions exist?" Surfaces cross-project knowledge that keyword search misses because different sessions use different terminology
- **Implementation:** Near-zero — Tantivy's `MoreLikeThisQuery` is built-in. Wrap it in a CLI flag.

### Weekly Digest

Automated summary of all agent activity over a configurable period.

- **Content:** Sessions completed, problems solved, recurring issues detected, agent comparison, most-touched files, new error patterns discovered, token usage trends
- **Format:** Markdown, suitable for a developer to skim in 2 minutes
- **CLI:** `agentscribe digest [--since 7d] [--output <path>]`
- **Automation:** Can be triggered by cron and written to a file, piped to email, or posted to Slack

```bash
agentscribe digest --since 7d

# AgentScribe Weekly Digest (Mar 9 – Mar 16)
#
# Sessions: 47 completed across 3 projects
# Agents:   claude-code (31), aider (9), codex (7)
# Outcome:  38 success, 5 failure, 4 abandoned
# Tokens:   1.2M input, 890K output (~$14.20 estimated)
#
# Recurring problems:
#   - ENOSPC in Docker builds (5 occurrences) — needs permanent fix
#   - Postgres cold-start timeout (3 occurrences)
#
# Most-touched files:
#   - src/auth/middleware.rs (7 sessions)
#   - db/migrations/ (5 sessions)
#
# Agent highlight:
#   - Aider: 100% success rate on refactoring tasks (5/5)
#   - Codex: 43% abandonment rate — highest of all agents
```

---

## Related Documents

- [cli-reference.md](cli-reference.md) — Detailed help for every CLI command, flag, output format, and exit code
- [../plugins/BUILDING_PLUGINS.md](../plugins/BUILDING_PLUGINS.md) — Comprehensive guide for building scraper plugins
- [new-features-01.md](new-features-01.md) — Extended feature descriptions with implementation details and example outputs
