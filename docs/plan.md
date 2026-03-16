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
│   ├── keywords.jsonl             # Inverted keyword index
│   ├── tags.jsonl                 # Tag index (agent, project, date, outcome)
│   └── sessions.jsonl             # Session manifest (id, agent, project, date, summary)
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

### Phase 2 — Indexing & Search
- Build inverted keyword index from normalized sessions
- Build session manifest with metadata (agent, project, dates, turn count)
- Tag extraction: pull tags from content (tool names, file types, error patterns)
- CLI: `agentscribe index rebuild`
- CLI: `agentscribe search <query> [--agent <type>] [--project <path>] [--since <date>]`
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

## Open Questions

- **Language choice:** Go for single-binary distribution? Rust? Python for faster iteration?
- **Embedding search:** worth including local embedding-based semantic search in core, or keep it as a plugin?
- **Session boundary detection:** Aider has no session markers — how to split a continuous history file into logical sessions?
