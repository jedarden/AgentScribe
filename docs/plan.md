# AgentScribe — Implementation Plan

## Overview

AgentScribe is a CLI binary that scrapes conversation logs from multiple coding agent types, normalizes them into a canonical format, and stores them as flat files with searchable indexes. It serves two purposes:

1. **Archive** — capture and preserve the full prompt/response history from all agents running in an environment
2. **Search** — provide a query interface that agents can invoke to find past solutions, error patterns, and reference implementations

---

## Problem

Every coding agent stores its conversation history differently — JSONL, Markdown, JSON trees, SQLite blobs. When multiple agents operate in the same environment, there is no unified way to search across their collective knowledge. A solution one agent found last week is invisible to another agent today.

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
- **Note:** No session boundaries — single continuous file per project

### OpenCode
- **Location:** `~/.local/share/opencode/storage/`
- **Format:** Individual JSON files in a hierarchy
- **Schema:** `session/{projectId}/{sessionId}.json`, `message/{sessionId}/{messageId}.json`, `part/{messageId}/{partId}.json`
- **Fields:** role, cost, tokens, timestamps; parts contain text, tool calls, tool results

### Codex (OpenAI)
- **Location:** `~/.codex/sessions/YYYY/MM/DD/rollout-{session_id}.jsonl`
- **Format:** JSONL, one event per line
- **Schema:** `{type, role, content, session_id, timestamp}` plus tool call details
- **Note:** Internal format, subject to change

### Cursor
- **Location:** `~/.config/Cursor/User/workspaceStorage/{hash}/state.vscdb` + `globalStorage/state.vscdb`
- **Format:** SQLite (`state.vscdb`), key-value ItemTable
- **Schema:** JSON blobs under keys like `workbench.panel.aichat.view.aichat.chatdata`, `composerData`
- **Note:** Requires SQLite extraction; files can grow very large (25GB+)

### Windsurf / Codeium
- **Location:** `~/.config/Windsurf/User/globalStorage/state.vscdb` + `workspaceStorage/{hash}/state.vscdb`
- **Format:** SQLite (`state.vscdb`), same ItemTable pattern as Cursor
- **Schema:** JSON blobs under `memento/interactive-session`, `interactive.sessions`
- **Note:** Hard limit of 20 conversations (21st overwrites oldest) — scrape early or lose data

---

## Architecture

```
agentscribe <command>
├── scrape      # Discover and read agent log files from known locations
├── index       # Build/rebuild searchable indexes over normalized data
├── search      # Query the index — designed to be called by other agents
├── status      # Show what agents/sessions are tracked, last scrape time
├── daemon      # Long-running background process (start|stop|status|run)
├── plugins     # Manage scraper plugin definitions (list|validate)
└── config      # Manage source paths, agent types, data directory location
```

### Scraper Plugin System

Each agent type is defined by a **scraper plugin** — a declarative config + parser that tells AgentScribe where to find logs and how to normalize them. Adding a new agent type means adding a new plugin definition, not modifying core code.

```toml
# ~/.agentscribe/plugins/claude-code.toml

[plugin]
name = "claude-code"
version = "1.0"

[source]
# Where to find log files — supports globs and env var expansion
paths = ["~/.claude/projects/*/*.jsonl"]
exclude = ["*/subagents/*"]  # Handle sub-agents separately or skip
format = "jsonl"             # jsonl | markdown | json-tree | sqlite

[source.session_detection]
# How to determine session boundaries
method = "one-file-per-session"  # one-file-per-session | delimiter | timestamp-gap
session_id_from = "filename"     # filename | field:sessionId | auto

[parser]
# Field mapping from source format to canonical schema
timestamp = "timestamp"          # JSONPath or field name
role = "message.role"
content = "message.content"
type = "type"
# Static metadata applied to all events from this source
[parser.static]
source_agent = "claude-code"

[metadata]
# Optional: paths to supplementary metadata
session_meta = "~/.claude/usage-data/session-meta/{session_id}.json"
session_facets = "~/.claude/usage-data/facets/{session_id}.json"
```

```toml
# ~/.agentscribe/plugins/aider.toml

[plugin]
name = "aider"
version = "1.0"

[source]
# Aider stores history in project directories — need a search root
paths = ["~/projects/*/.aider.chat.history.md", "~/repos/*/.aider.chat.history.md"]
format = "markdown"

[source.session_detection]
method = "timestamp-gap"         # No session markers — split on time gaps
gap_threshold = "30m"            # New session if >30min gap between entries

[parser]
# Markdown-specific parsing rules
user_prefix = "#### "
tool_prefix = "> "
assistant_prefix = ""           # Bare text = assistant
[parser.static]
source_agent = "aider"
```

Bundled plugins ship for Claude Code, Aider, OpenCode, and Codex. Users can add custom plugins for any agent by dropping a TOML file in `~/.agentscribe/plugins/`.

CLI:
- `agentscribe plugins list` — show registered plugins and their source paths
- `agentscribe plugins validate <file>` — check a plugin definition for errors
- `agentscribe scrape --plugin <name>` — scrape only a specific agent type

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
│   ├── <agent>/<session-id>.jsonl # Normalized conversation logs
│   └── <agent>/<session-id>.md    # Markdown summary (human/agent readable)
├── index/
│   └── tantivy/                   # Tantivy search index (rebuilt from sessions if needed)
└── state/
    └── scrape-state.json          # Last-seen offsets/timestamps per source
```

---

## Canonical Event Schema

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
  "tags": ["git", "migration", "postgres"]
}
```

### Session Manifest Entry

```json
{
  "session_id": "claude-code/83f5a4e7",
  "source_agent": "claude-code",
  "project": "/home/coding/myproject",
  "started": "2026-03-16T10:00:00Z",
  "ended": "2026-03-16T10:45:00Z",
  "turns": 42,
  "summary": "Migrated Postgres schema from v3 to v4, added rollback script",
  "outcome": "success",
  "tags": ["postgres", "migration", "schema"]
}
```

---

## Phases

### Phase 1 — Plugin System, Scraping & Normalization
- Implement the scraper plugin framework:
  - TOML-based plugin definitions (source paths, format, field mapping)
  - Format-specific parsers: JSONL, Markdown, JSON tree (pluggable by format field)
  - Session detection strategies: one-file-per-session, timestamp-gap, delimiter
- Bundle default plugins for Claude Code, Aider, OpenCode, Codex
- Normalize all formats to canonical event schema via field mapping
- Write normalized sessions as JSONL flat files
- Track scrape state (last-seen offsets/timestamps) to support incremental scrapes
- CLI: `agentscribe scrape [--plugin <name>] [--project <path>]`
- CLI: `agentscribe plugins list|validate`

### Phase 2 — Tantivy Indexing & Search
- Build Tantivy index from normalized sessions (BM25 scoring, faceted fields)
- Incremental indexing: add new sessions on scrape, no full rebuild required
- Tag extraction: pull tags from content (tool names, file types, error patterns) into faceted fields
- CLI: `agentscribe index rebuild` (full rebuild from flat files)
- CLI: `agentscribe search <query> [--agent <type>] [--project <path>] [--since <date>]`
- Fuzzy search via Tantivy's Levenshtein term queries
- Output: ranked results with session ID, summary, and matching snippets
- Designed for agent consumption: structured output mode (`--json`) for programmatic use

### Phase 3 — Summaries & Enrichment
- Auto-generate Markdown summaries per session (extractive, no LLM required for v1)
- Optional LLM-powered summarization for richer summaries
- Outcome detection: did the session succeed, fail, or get abandoned?
- `agentscribe summarize <session-id>`

### Phase 4 — Daemon Mode & MCP
- Implement `agentscribe daemon start|stop|status|run`
- File watcher (inotify/fswatch) for automatic scraping on log changes
- Incremental index updates (no full rebuild on every new session)
- Optional MCP server mode: expose `search` and `status` as MCP tools
- Systemd unit file for user-level service management

### Phase 5 — SQLite Format Support & Extended Agents
- Add `sqlite` format parser to the plugin framework (for Cursor, Windsurf, etc.)
- Bundle plugins for Cursor and Windsurf (SQLite extraction + JSON blob parsing)
- Community plugin registry or examples directory
- Git auto-commit integration: optionally commit new sessions to a git repo on scrape

---

## Search Interface (Agent-Facing)

The primary consumer of search is other agents running in the environment. The interface must be:

- **CLI-callable** — agents invoke `agentscribe search "database migration" --json` as a tool/subprocess
- **Structured output** — JSON results with session IDs, summaries, snippets, and relevance scores
- **Filterable** — by agent type, project, date range, outcome, tags
- **Fast** — index-based lookup, no full-text scan on every query
- **Context-sized** — results include enough context to be useful but not so much they blow up an agent's context window; configurable `--max-results` and `--snippet-length`

Example agent workflow:
```
# Agent hits an error with Postgres migrations
# Before trying to solve it, check if a prior session already solved it
agentscribe search "postgres migration error" --json --max-results 3

# Returns matching sessions with summaries and relevant snippets
# Agent reads the results and applies the known solution
```

---

## Design Principles

- **CLI-first, MCP-also** — the CLI is the primary interface; MCP is a secondary access layer for agents that support it
- **Flat files first** — all data is plain text (JSONL + Markdown); works without any database
- **Git-native** — append-only JSONL and Markdown are diff-friendly; the data dir can be a git repo
- **Incremental** — scraping tracks offsets so re-runs are fast; only new data is processed
- **Agent-readable** — search output is structured JSON; summaries are Markdown
- **No external dependencies** — core scraping, indexing, and search work offline with no APIs
- **Non-invasive** — read-only access to agent logs; never modifies source files
- **Pluggable** — new agent types are added via TOML plugin definitions, not code changes

---

## Decisions

### CLI over MCP, support both

The CLI is the primary interface. Every feature must work via `agentscribe <command>`. Agents call it as a subprocess — this is the most universal integration path since every agent can shell out.

MCP is a secondary access layer, not a replacement. AgentScribe can optionally run as an MCP server exposing `search` and `status` as tools, so agents with native MCP support (Claude Code, etc.) can query it without subprocess overhead. But MCP is never required — it's a convenience wrapper over the same logic the CLI uses.

```
CLI (primary)     →  core library  →  flat files
MCP server (opt)  →  core library  →  flat files
```

### Daemon Mode

AgentScribe needs a long-running background mode for continuous scraping. This is a **daemon** — a process that:

1. **Watches** agent log directories for changes (via inotify/fswatch, not polling)
2. **Scrapes incrementally** when new log data appears
3. **Rebuilds indexes** after ingesting new sessions
4. **Serves MCP** if enabled (the daemon is the natural place to host the MCP server)

The daemon is managed via the CLI:

```bash
# Start the daemon (backgrounds itself, writes PID to ~/.agentscribe/agentscribe.pid)
agentscribe daemon start

# Check status
agentscribe daemon status

# Stop
agentscribe daemon stop

# Run in foreground (for systemd/supervisord management)
agentscribe daemon run
```

**Daemon vs CLI scraping:**
- `agentscribe scrape` — one-shot, on-demand. Good for manual runs, cron, CI.
- `agentscribe daemon start` — continuous. Watches for changes, scrapes automatically, keeps indexes hot.

Both use the same scraping logic. The daemon just wraps it in a watch loop.

**Systemd integration** (optional):

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

```bash
systemctl --user enable --now agentscribe
```

**What the daemon does NOT do:**
- It does not require root
- It does not open network ports (unless MCP is enabled, and even then only on localhost/unix socket)
- It does not modify agent log files — read-only always

## Decision: Language — Rust

Rust, primarily because of **Tantivy**.

AgentScribe's core value is indexing and searching conversation logs. The embedded full-text search engine is the most performance-critical component. Tantivy is the best embeddable full-text search library available in any language — it matches or exceeds Lucene's feature set with 3-5x faster indexing than Go's Bleve, sub-millisecond query latency, and compact columnar index storage.

### Key Rust crates for the stack

| Component | Crate | Purpose |
|-----------|-------|---------|
| Full-text search | `tantivy` | Inverted index, BM25 scoring, faceted search, fuzzy/phrase queries |
| JSON parsing | `serde_json` / `simd-json` | Streaming JSONL parsing, zero-copy deserialization |
| File watching | `notify` | Cross-platform inotify/kqueue/FSEvents wrapper |
| Markdown parsing | `pulldown-cmark` | Streaming CommonMark parser for Aider logs |
| SQLite | `rusqlite` (bundled) | Read Cursor/Windsurf state.vscdb files |
| TOML config | `toml` / `serde` | Plugin definition parsing |
| CLI framework | `clap` | Command/subcommand parsing, shell completions |
| Async runtime | `tokio` | Daemon mode, file watcher event loop |
| Glob matching | `globset` | Source path expansion in plugin definitions |

### Why not Go

Go's `bleve` search library is 3-5x slower for indexing and 1.5-3x slower for queries. Go's GC adds unpredictable latency — a problem when agents invoke `agentscribe search` and expect fast, consistent responses. Go's JSON parsing allocates on every string field; Rust's `serde` borrows from the input buffer. For streaming 50MB JSONL files, Rust uses 1-5MB working memory vs Go's 10-30MB.

Go's only advantage is easier cross-compilation, which Rust mitigates via `cross` (Docker-based cross-compilation) and `rusqlite`'s bundled SQLite feature.

---

## Decision: Search Engine Architecture — Tantivy-based (Meilisearch-inspired)

Meilisearch's core engine (`milli`) demonstrates that sub-50ms full-text search is achievable through a specific combination of data structures. AgentScribe adopts the same approach at a smaller scale, using Tantivy as the foundation.

### What to adopt from Meilisearch

1. **FST + Levenshtein automata** for typo-tolerant vocabulary lookup — Tantivy includes this via its fuzzy term query support
2. **Roaring bitmaps** for document ID set operations (intersection, union) — Tantivy uses these internally
3. **Pre-computed positional data** to avoid runtime text scanning — Tantivy stores positions per term
4. **Memory-mapped index files** for zero-copy reads — Tantivy mmaps its segments

### What NOT to adopt

- Meilisearch's 20+ LMDB databases per index — overkill for conversation logs
- Word-pair proximity pre-computation — relevant for natural language search over millions of documents, not for searching thousands of agent sessions
- The full bucket-sort ranking cascade — BM25 with field boosting is sufficient for AgentScribe's use case
- Meilisearch's `charabia` multi-language tokenizer — agent conversations are predominantly English/code; Tantivy's default tokenizer pipeline suffices

### Tantivy schema for AgentScribe

```rust
let mut schema_builder = Schema::builder();

// Indexed + stored fields
schema_builder.add_text_field("content", TEXT | STORED);      // Full conversation text
schema_builder.add_text_field("summary", TEXT | STORED);       // Session summary
schema_builder.add_text_field("session_id", STRING | STORED);  // Exact match
schema_builder.add_text_field("source_agent", STRING | STORED | FAST); // Faceted filter
schema_builder.add_text_field("project", STRING | STORED | FAST);      // Faceted filter
schema_builder.add_text_field("tags", STRING | STORED | FAST);         // Multi-valued facet

// Date + numeric for range queries
schema_builder.add_date_field("timestamp", INDEXED | STORED | FAST);
schema_builder.add_u64_field("turn_count", INDEXED | STORED | FAST);

// Stored-only (not searchable, just returned in results)
schema_builder.add_text_field("outcome", STORED);
```

### Index lifecycle

- **One Tantivy index** for all sessions (not per-agent). Cross-agent search is the primary use case.
- **Incremental indexing**: new sessions are added via `IndexWriter::add_document()` on scrape. No full rebuild needed.
- **Commit on scrape completion**: `writer.commit()` after each scrape batch. Index is searchable immediately.
- **Segment merging**: Tantivy handles background segment merging automatically via its `MergePolicy`.
- **Index location**: `~/.agentscribe/index/tantivy/` — this directory replaces the JSONL-based index files from the earlier plan.

### Flat files remain the source of truth

Tantivy is the search index, not the data store. The normalized JSONL session files under `~/.agentscribe/sessions/` remain the authoritative data. If the Tantivy index corrupts, it can be rebuilt from the flat files:

```bash
agentscribe index rebuild  # Drops and rebuilds the Tantivy index from session files
```

---

## Decision: Session Boundary Detection per Agent

### Claude Code — One file per session

Each `<session-uuid>.jsonl` file is exactly one session. The filename UUID matches the `sessionId` field on every line.

- **Start signal**: First line is `type: "progress"` with `hookEvent: "SessionStart"`
- **End signal**: None explicit — last line in the file is the end
- **Resumed sessions**: Same file gets a second `SessionStart` event appended — treat entire file as one session
- **Sub-agents**: `<session-uuid>/subagents/agent-<id>.jsonl` — same format with `isSidechain: true`. Exclude by default, optional separate plugin.
- **Headless sessions** (`claude --print`): May lack `SessionStart` hook. Line 0 may be `type: "queue-operation"`. Still one file = one session.

```toml
[source.session_detection]
method = "one-file-per-session"
session_id_from = "filename"
```

### Aider — Delimiter in continuous file

Each `aider` launch writes `# aider chat started at YYYY-MM-DD HH:MM:SS` to `.aider.chat.history.md`. Everything between one delimiter and the next (or EOF) is one session.

- **Start signal**: `# aider chat started at <datetime>` (written by `io.py`)
- **End signal**: Next delimiter line, or EOF
- **Timestamp enrichment**: `.aider.input.history` has per-input timestamps (`# YYYY-MM-DD HH:MM:SS.ffffff`) that can correlate with chat history entries for finer granularity
- **Edge case**: Scripted/non-interactive aider (`--yes`, piped input) may not write the session marker. Fallback to `timestamp-gap` using `.aider.input.history` timestamps if no delimiters found.

```toml
[source.session_detection]
method = "delimiter"
delimiter_pattern = "^# aider chat started at (.+)$"
```

### OpenCode — One row per session (SQLite)

Current versions use SQLite (`.opencode/` in project dir). Each session is a row in the `sessions` table with a descending ULID.

- **Start signal**: `time_created` field
- **End signal**: `time_updated` field
- **Child sessions**: Auto-compact creates child sessions linked via `parentID` — each is a discrete session
- **Legacy format**: Older versions used JSON files at `~/.local/share/opencode/storage/session/{projectId}/{sessionId}.json` — one file per session

```toml
[source.session_detection]
method = "one-file-per-session"
session_id_from = "field:id"
```

### Codex — One file per session

Each `rollout-<id>.jsonl` (or `.jsonl.zst`) file is one session. Line 1 is a `RolloutLine::Meta` header with `thread_id`.

- **Start signal**: `RolloutLine::Meta` on line 1 with `thread_id`, `cwd`, `model`
- **End signal**: EOF
- **Resumed sessions**: New events appended to same file, no new Meta header — entire file is one session
- **Compressed files**: May be `.jsonl.zst` (zstandard) — requires decompression
- **Ephemeral sessions**: `EventPersistenceMode::None` writes no file at all — nothing to scrape
- **Companion index**: `~/.codex/session_index.jsonl` maps thread IDs to metadata

```toml
[source.session_detection]
method = "one-file-per-session"
session_id_from = "field:thread_id"
```

### Cursor — One key per session (SQLite)

Conversations stored in `state.vscdb` SQLite database. Each session is a `composerData:<composerId>` key in the `cursorDiskKV` table.

- **Start signal**: `createdAt` millisecond timestamp in the JSON blob
- **End signal**: `lastUpdatedAt` + `status` field (`"completed"` or `"aborted"`)
- **Messages**: Separate `bubbleId:<composerId>:<bubbleId>` keys, ordered by `rowid ASC`
- **Two databases**: Global (`globalStorage/state.vscdb`) has content, workspace (`workspaceStorage/<hash>/state.vscdb`) has UI state
- **Schema evolution**: Pre-v2.0 used `ItemTable` key `workbench.panel.aichat.view.aichat.chatdata` with inline `tabs[]`/`bubbles[]` arrays
- **Size warning**: Global DB can grow to 25GB+ — must open read-only (`?mode=ro`)

```toml
[source.session_detection]
method = "one-file-per-session"
session_id_from = "field:composerId"
```

### Windsurf — One key per session (SQLite)

Same architecture as Cursor (`state.vscdb`, `cursorDiskKV` table, `composerData:<composerId>` keys).

- **Start/end signals**: Same as Cursor (`createdAt`, `lastUpdatedAt`)
- **Critical limitation**: Hard limit of 20 conversations — the 21st overwrites the oldest. **Must scrape frequently or data is permanently lost.**
- **Format instability**: Newer versions may use protobuf with no public schema, or cloud-first storage with no local persistence. Plugin may break across Windsurf updates.

```toml
[source.session_detection]
method = "one-file-per-session"
session_id_from = "field:composerId"
```
