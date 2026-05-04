# AgentScribe CLI Reference

## Global Options

These options apply to all commands:

```
agentscribe [global-options] <command> [command-options]
```

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--data-dir <path>` | `-d` | `~/.agentscribe` | Root data directory. Overridden by `AGENTSCRIBE_DATA_DIR` env var. |
| `--config <path>` | `-c` | `~/.agentscribe/config.toml` | Path to global config file. |
| `--json` | `-j` | off | Output in JSON format. Applies to all commands. When enabled, all output is machine-readable JSON — no human-formatted tables or progress bars. |
| `--quiet` | `-q` | off | Suppress non-essential output. Errors still print to stderr. |
| `--verbose` | `-v` | off | Enable debug-level logging to stderr. Repeatable (`-vv` for trace). |
| `--color <when>` | | `auto` | Color output: `auto`, `always`, `never`. Auto-detects terminal. |
| `--help` | `-h` | | Show help for the current command. |
| `--version` | `-V` | | Print version and exit. |

---

## `agentscribe scrape`

Discover agent log files, parse them, normalize to canonical format, and write to the sessions directory. Updates the Tantivy index incrementally.

### Usage

```
agentscribe scrape [options]
```

### Options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--plugin <name>` | `-p` | all plugins | Scrape only the named plugin. Can be repeated (`-p claude-code -p aider`). |
| `--project <path>` | | all projects | Scrape only sessions from the given project directory. Matches against the session's `project` field. |
| `--file <path>` | | | Scrape a single source file. Useful for testing a plugin against one log file. Requires `--plugin`. |
| `--dry-run` | `-n` | off | Show what would be scraped without writing any files or updating the index. |
| `--output-events` | | off | With `--dry-run`, print each parsed event as a JSON object to stdout. Use to verify field mapping. |
| `--force` | `-f` | off | Ignore scrape state (last-seen offsets) and re-scrape everything. Does not delete existing sessions — deduplicates by session ID. |
| `--no-index` | | off | Write session files but skip Tantivy index update. Useful for bulk imports where you'll `index rebuild` afterward. |
| `--since <datetime>` | | | Only scrape sessions with activity after this timestamp. ISO 8601 or relative (`24h`, `7d`, `1w`). |

### Examples

```bash
# Scrape all configured plugins
agentscribe scrape

# Scrape only Claude Code sessions
agentscribe scrape --plugin claude-code

# Dry-run with event output to verify a new plugin
agentscribe scrape --plugin pilot --dry-run --output-events

# Scrape a single file for debugging
agentscribe scrape --plugin aider --file ~/myproject/.aider.chat.history.md

# Re-scrape everything from scratch
agentscribe scrape --force

# Bulk import without indexing (rebuild index after)
agentscribe scrape --force --no-index
agentscribe index rebuild
```

### Output

Human-readable (default):
```
Scraping claude-code...
  Found 12 new sessions (skipped 89 unchanged)
  Wrote 12 session files to ~/.agentscribe/sessions/claude-code/
Scraping aider...
  Found 3 new sessions (skipped 5 unchanged)
  Wrote 3 session files to ~/.agentscribe/sessions/aider/
Index updated: 15 documents added (1,262 total)
```

JSON (`--json`):
```json
{
  "plugins": [
    {
      "name": "claude-code",
      "new_sessions": 12,
      "skipped_sessions": 89,
      "errors": []
    },
    {
      "name": "aider",
      "new_sessions": 3,
      "skipped_sessions": 5,
      "errors": []
    }
  ],
  "index": {
    "documents_added": 15,
    "total_documents": 1262
  }
}
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success — all plugins scraped without errors |
| 1 | Partial failure — some plugins had errors, others succeeded. Check `errors` in JSON output. |
| 2 | Total failure — no sessions scraped. Likely a config or permissions issue. |

---

## `agentscribe search`

Query the Tantivy index for matching sessions. The primary interface for agents seeking past solutions.

### Usage

```
agentscribe search <query> [options]
```

### Arguments

| Argument | Required | Description |
|----------|----------|-------------|
| `<query>` | no* | Search query string. Supports Tantivy query syntax: `+required -excluded "exact phrase"`, field-scoped queries (`content:migration`), boolean operators (`AND`, `OR`, `NOT`), fuzzy terms (`migrat~1`), wildcard (`migrat*`). Required unless using `--error`, `--code`, `--solution-only`, `--like`, or `--session`. |

### Options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--error <pattern>` | | | Search error fingerprints. Matches against normalized error patterns (e.g., `ConnectionError:Connection refused`). |
| `--code <query>` | | | Search extracted code artifacts by content. |
| `--lang <language>` | | | Language filter for `--code` search (e.g., `rust`, `python`, `typescript`). |
| `--solution-only` | | off | Return only sessions that have an extracted solution. |
| `--like <session-id>` | | | Find sessions similar to the given session ID using TF-IDF similarity. |
| `--session <id>` | | | Retrieve a specific session by ID. Ignores query and filters. Returns the full session content. |
| `--max-results <n>` | `-n` | `10` | Maximum number of sessions to return. |
| `--snippet-length <n>` | | `200` | Maximum character length of content snippets per result. Set to `0` to omit snippets. |
| `--token-budget <n>` | | | Token budget for greedy knapsack context packing. Optimally packs results within the token limit. |
| `--agent <name>` | `-a` | all | Filter to sessions from this agent type. Can be repeated. |
| `--project <path>` | | all | Filter to sessions from this project directory. |
| `--since <datetime>` | | | Only match sessions after this timestamp. ISO 8601 or relative (`24h`, `7d`, `1w`). |
| `--before <datetime>` | | | Only match sessions before this timestamp. |
| `--tag <tag>` | `-t` | | Filter by tag. Can be repeated (AND logic). |
| `--outcome <outcome>` | | | Filter by session outcome: `success`, `failure`, `abandoned`, `unknown`. |
| `--type <type>` | | all | Filter by document type: `session`, `code_artifact`. |
| `--model <name>` | | all | Filter by LLM model name. |
| `--sort <field>` | `-s` | `relevance` | Sort order: `relevance` (BM25 score), `newest`, `oldest`, `turns`. |
| `--offset <n>` | | `0` | Skip first N results. For pagination. |
| `--fuzzy` | | off | Enable fuzzy matching on all query terms (Levenshtein distance 1). Useful when agents don't know exact terminology. |

### Examples

```bash
# Basic search
agentscribe search "postgres migration"

# Agent-oriented: JSON output, limited results, short snippets
agentscribe search "database connection timeout" --json -n 3 --snippet-length 150

# Scoped to a specific agent and project
agentscribe search "deploy" --agent claude-code --project /home/coding/myapp

# Recent sessions only
agentscribe search "build failure" --since 7d

# Fuzzy search for misspellings or approximate terms
agentscribe search "kuberntes" --fuzzy

# Exact phrase search
agentscribe search '"connection pool exhausted"'

# Advanced Tantivy query syntax
agentscribe search '+content:migration +agent:claude-code -content:rollback'

# Retrieve a specific session's full content
agentscribe search --session claude-code/83f5a4e7

# Search by error fingerprint
agentscribe search --error "ConnectionError"

# Search code artifacts
agentscribe search --code "fn migrate" --lang rust

# Find sessions with extracted solutions
agentscribe search "auth token" --solution-only

# Find similar sessions
agentscribe search --like claude-code/83f5a4e7

# Context packing for agent prompts (fit within token budget)
agentscribe search "error handling" --token-budget 4000 --json

# Filter by document type and model
agentscribe search "refactor" --type code_artifact --model claude-sonnet-4-20250514
```

### Output

Human-readable (default):
```
3 results for "postgres migration" (searched 1,262 sessions in 4ms)

[1] claude-code/83f5a4e7  (score: 8.42)
    Project:  /home/coding/myapp
    Date:     2026-03-14 10:30
    Turns:    42
    Outcome:  success
    Summary:  Migrated Postgres schema from v3 to v4, added rollback script
    Snippet:  ...ran ALTER TABLE to add the new columns, then backfilled
              existing rows. The migration took 3 minutes on a 2M row table...

[2] aider/session-2026-03-10  (score: 6.18)
    Project:  /home/coding/api-server
    Date:     2026-03-10 14:15
    Turns:    18
    Outcome:  success
    Summary:  Fixed Postgres connection pooling, added retry logic
    Snippet:  ...the connection pool was exhausting under load because
              max_connections was set to 10. Bumped to 50 and added...

[3] codex/abc-thread-123  (score: 4.01)
    Project:  /home/coding/data-pipeline
    Date:     2026-03-08 09:00
    Turns:    7
    Outcome:  failure
    Summary:  Attempted Postgres 14->16 upgrade, hit extension incompatibility
    Snippet:  ...pg_trgm extension not available in pg16 container image.
              Rolled back to pg14 and opened an issue...
```

JSON (`--json`):
```json
{
  "query": "postgres migration",
  "total_matches": 3,
  "search_time_ms": 4,
  "sessions_searched": 1262,
  "results": [
    {
      "session_id": "claude-code/83f5a4e7",
      "source_agent": "claude-code",
      "project": "/home/coding/myapp",
      "timestamp": "2026-03-14T10:30:00Z",
      "turns": 42,
      "outcome": "success",
      "score": 8.42,
      "summary": "Migrated Postgres schema from v3 to v4, added rollback script",
      "snippet": "...ran ALTER TABLE to add the new columns, then backfilled existing rows. The migration took 3 minutes on a 2M row table...",
      "tags": ["postgres", "migration", "schema", "alter-table"]
    }
  ]
}
```

### Full Session Retrieval

When `--session <id>` is used, returns the complete normalized conversation:

```bash
agentscribe search --session claude-code/83f5a4e7 --json
```

```json
{
  "session_id": "claude-code/83f5a4e7",
  "source_agent": "claude-code",
  "project": "/home/coding/myapp",
  "started": "2026-03-14T10:30:00Z",
  "ended": "2026-03-14T11:15:00Z",
  "turns": 42,
  "outcome": "success",
  "summary": "Migrated Postgres schema from v3 to v4, added rollback script",
  "events": [
    {
      "ts": "2026-03-14T10:30:00Z",
      "role": "user",
      "content": "I need to migrate the Postgres schema from v3 to v4..."
    },
    {
      "ts": "2026-03-14T10:30:15Z",
      "role": "assistant",
      "content": "I'll create a migration script that..."
    }
  ]
}
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Results found |
| 0 | No results (empty results array, not an error) |
| 1 | Query parse error or invalid filter |
| 2 | Index not found — run `agentscribe scrape` first |

---

## `agentscribe index`

Manage the Tantivy search index.

### Usage

```
agentscribe index <subcommand> [options]
```

### Subcommands

#### `agentscribe index rebuild`

Drop the existing Tantivy index and rebuild it from all normalized session files. Use after a bulk import, after changing the schema, or to recover from index corruption.

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--plugin <name>` | `-p` | all | Only re-index sessions from this plugin. |
| `--heap-size <mb>` | | `50` | Tantivy writer heap size in MB. Higher = faster indexing, more RAM. |

```bash
# Full rebuild
agentscribe index rebuild

# Rebuild only Claude Code sessions
agentscribe index rebuild --plugin claude-code

# Fast rebuild with more memory
agentscribe index rebuild --heap-size 100
```

Output:
```
Dropping existing index...
Rebuilding from 1,262 session files...
  [========================================] 1262/1262 (47/s)
Index rebuilt in 26.8s
```

#### `agentscribe index stats`

Show index statistics.

```bash
agentscribe index stats
```

Output:
```
Tantivy index at ~/.agentscribe/index/tantivy/
  Documents:    1,262
  Segments:     4
  Size on disk: 38 MB
  Last updated: 2026-03-16 12:30:00
  Fields:       content, summary, session_id, source_agent, project, tags, timestamp, turn_count, outcome
```

JSON (`--json`):
```json
{
  "path": "~/.agentscribe/index/tantivy/",
  "documents": 1262,
  "segments": 4,
  "size_bytes": 39845888,
  "last_updated": "2026-03-16T12:30:00Z",
  "fields": ["content", "summary", "session_id", "source_agent", "project", "tags", "timestamp", "turn_count", "outcome"]
}
```

#### `agentscribe index optimize`

Force-merge Tantivy segments for better query performance. Normally unnecessary — Tantivy auto-merges in the background.

```bash
agentscribe index optimize
```

Output:
```
Merging 4 segments into 1... done (3.2s)
Index size: 38 MB -> 35 MB
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Index error (corruption, missing session files) |
| 2 | No session files found — run `agentscribe scrape` first |

---

## `agentscribe status`

Show a summary of what AgentScribe knows: tracked agents, session counts, last scrape time, daemon state, and disk usage.

### Usage

```
agentscribe status [options]
```

### Options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--plugin <name>` | `-p` | all | Show status for a specific plugin only. |

### Examples

```bash
agentscribe status
```

Output:
```
AgentScribe v0.1.0
Data dir: ~/.agentscribe (142 MB)

Daemon: running (PID 12345, uptime 3d 14h, RSS 12 MB)

Plugins:
  claude-code    89 sessions   last scraped 2m ago     ~/.claude/projects/*/*.jsonl
  aider           8 sessions   last scraped 2m ago     ~/projects/*/.aider.chat.history.md
  codex          14 sessions   last scraped 2m ago     ~/.codex/sessions/**/*.jsonl
  opencode        0 sessions   never scraped           ~/.local/share/opencode/storage/

Index: 111 documents, 4 segments, 38 MB on disk

Scrape state: incremental (tracking offsets for 6 source paths)
```

JSON (`--json`):
```json
{
  "version": "0.1.0",
  "data_dir": "~/.agentscribe",
  "data_dir_bytes": 148897792,
  "daemon": {
    "running": true,
    "pid": 12345,
    "uptime_seconds": 309600,
    "rss_bytes": 12582912
  },
  "plugins": [
    {
      "name": "claude-code",
      "sessions": 89,
      "last_scraped": "2026-03-16T12:28:00Z",
      "source_paths": ["~/.claude/projects/*/*.jsonl"]
    }
  ],
  "index": {
    "documents": 111,
    "segments": 4,
    "size_bytes": 39845888
  }
}
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Data dir not found or not initialized |

---

## `agentscribe daemon`

Manage the background daemon that watches for new log data and scrapes automatically.

### Usage

```
agentscribe daemon <subcommand> [options]
```

### Subcommands

#### `agentscribe daemon start`

Start the daemon in the background. Writes PID to `~/.agentscribe/agentscribe.pid`. Logs to `~/.agentscribe/daemon.log`.

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--mcp` | | off | Enable MCP server on the daemon. Listens on a Unix socket at `~/.agentscribe/mcp.sock`. |
| `--log-level <level>` | | `info` | Daemon log level: `error`, `warn`, `info`, `debug`, `trace`. |
| `--scrape-debounce <duration>` | | `5s` | Wait this long after a file change before scraping. Prevents thrashing during active agent sessions. |

```bash
# Start with defaults
agentscribe daemon start

# Start with MCP server enabled
agentscribe daemon start --mcp

# Start with debug logging
agentscribe daemon start --log-level debug
```

Output:
```
AgentScribe daemon started (PID 12345)
  Watching: 4 plugin source paths
  MCP: disabled
  Log: ~/.agentscribe/daemon.log
```

#### `agentscribe daemon stop`

Stop the running daemon. Sends SIGTERM and waits for clean shutdown.

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--force` | | off | Send SIGKILL if the daemon doesn't stop within 5 seconds. |

```bash
agentscribe daemon stop
```

Output:
```
Stopping AgentScribe daemon (PID 12345)... stopped.
```

#### `agentscribe daemon status`

Show daemon state, resource usage, and activity.

```bash
agentscribe daemon status
```

Output:
```
AgentScribe daemon (PID 12345)
  Status:     running
  Uptime:     3d 14h
  RSS:        12 MB
  Peak RSS:   47 MB
  MCP:        disabled
  Log:        ~/.agentscribe/daemon.log

  Watches:    4 paths
  Last scrape: 2m ago (3 new sessions from claude-code)
  Total scrapes: 847 since start
  Errors:     0
```

JSON (`--json`):
```json
{
  "pid": 12345,
  "status": "running",
  "uptime_seconds": 309600,
  "rss_bytes": 12582912,
  "peak_rss_bytes": 49283072,
  "mcp_enabled": false,
  "log_path": "~/.agentscribe/daemon.log",
  "watches": 4,
  "last_scrape": "2026-03-16T12:28:00Z",
  "last_scrape_new_sessions": 3,
  "total_scrapes": 847,
  "errors": 0
}
```

#### `agentscribe daemon run`

Run the daemon in the foreground. Use this when managing the process via systemd, supervisord, or similar. Does not daemonize, does not write a PID file. Logs to stderr.

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--mcp` | | off | Enable MCP server. |
| `--log-level <level>` | | `info` | Log level. |
| `--scrape-debounce <duration>` | | `5s` | Debounce period. |

```bash
# Foreground (for systemd)
agentscribe daemon run

# With MCP and debug logging
agentscribe daemon run --mcp --log-level debug
```

#### `agentscribe daemon logs`

Tail the daemon log file. Convenience wrapper for reading `~/.agentscribe/daemon.log`.

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--follow` | `-f` | off | Follow new output (like `tail -f`). |
| `--lines <n>` | `-n` | `50` | Number of lines to show. |
| `--level <level>` | | all | Filter to this log level or above. |

```bash
# Last 50 lines
agentscribe daemon logs

# Follow in real time
agentscribe daemon logs -f

# Only errors
agentscribe daemon logs --level error -n 100
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Daemon not running (for `stop`, `status`) or failed to start |
| 2 | PID file exists but process is dead (stale PID) — cleaned up automatically |

---

## `agentscribe plugins`

Manage scraper plugin definitions.

### Usage

```
agentscribe plugins <subcommand> [options]
```

### Subcommands

#### `agentscribe plugins list`

Show all registered plugins, their source paths, and whether their paths match any files.

```bash
agentscribe plugins list
```

Output:
```
Plugins (4 registered):

  claude-code  v1.0  jsonl       ~/.claude/projects/*/*.jsonl           → 101 files matched
  aider        v1.0  markdown    ~/projects/*/.aider.chat.history.md    → 3 files matched
  codex        v1.0  jsonl       ~/.codex/sessions/**/*.jsonl           → 0 files matched
  opencode     v1.0  sqlite      ~/.local/share/opencode/storage/**     → 0 files matched

Plugin dir: ~/.agentscribe/plugins/
```

JSON (`--json`):
```json
{
  "plugin_dir": "~/.agentscribe/plugins/",
  "plugins": [
    {
      "name": "claude-code",
      "version": "1.0",
      "format": "jsonl",
      "source_paths": ["~/.claude/projects/*/*.jsonl"],
      "matched_files": 101,
      "config_path": "~/.agentscribe/plugins/claude-code.toml"
    }
  ]
}
```

#### `agentscribe plugins validate <path>`

Validate a plugin TOML file for correctness. Checks required fields, format compatibility, glob pattern resolution, field mapping syntax, and role map target values.

| Argument | Required | Description |
|----------|----------|-------------|
| `<path>` | yes | Path to the plugin TOML file to validate. |

```bash
agentscribe plugins validate ~/.agentscribe/plugins/pilot.toml
```

Output (success):
```
Validating pilot.toml...
  [ok] Required fields present
  [ok] Format "jsonl" is supported
  [ok] Source paths resolve to 14 files
  [ok] Session detection method "one-file-per-session" is valid
  [ok] Field mappings use valid dot-notation
  [ok] Role map targets are valid canonical roles
  [ok] No conflicting include/exclude type filters

Valid. Ready to scrape.
```

Output (errors):
```
Validating broken.toml...
  [ok] Required fields present
  [ok] Format "jsonl" is supported
  [WARN] Source paths resolve to 0 files — check that the paths exist
  [ERR] Field mapping "role" references "msg.speaker" but no "msg" object found in sample data
  [ERR] Role map target "bot" is not a valid canonical role (expected: user, assistant, system, tool_call, tool_result)

2 errors, 1 warning. Fix errors before scraping.
```

#### `agentscribe plugins show <name>`

Print the full plugin configuration.

```bash
agentscribe plugins show claude-code
```

Output:
```
Plugin: claude-code (v1.0)
Config: ~/.agentscribe/plugins/claude-code.toml

[source]
  paths   = ["~/.claude/projects/*/*.jsonl"]
  exclude = ["*/subagents/*"]
  format  = jsonl

[source.session_detection]
  method          = one-file-per-session
  session_id_from = filename

[parser]
  timestamp = timestamp
  role      = message.role
  content   = message.content
  type      = type

[parser.static]
  source_agent = claude-code

[metadata]
  session_meta   = ~/.claude/usage-data/session-meta/{session_id}.json
  session_facets = ~/.claude/usage-data/facets/{session_id}.json
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success (or validation passed) |
| 1 | Validation failed with errors |
| 2 | Plugin file not found |

---

## `agentscribe config`

View and modify global configuration.

### Usage

```
agentscribe config <subcommand> [options]
```

### Subcommands

#### `agentscribe config show`

Print the current global configuration.

```bash
agentscribe config show
```

Output:
```
Config: ~/.agentscribe/config.toml

[general]
  data_dir = ~/.agentscribe
  log_level = info

[scrape]
  debounce = 5s
  default_heap_size_mb = 50

[index]
  tantivy_heap_size_mb = 50

[daemon]
  mcp = false
  mcp_socket = ~/.agentscribe/mcp.sock

[sqlite]
  cache_size_pages = 2000
```

#### `agentscribe config set <key> <value>`

Set a configuration value.

| Argument | Required | Description |
|----------|----------|-------------|
| `<key>` | yes | Dot-notation config key (e.g., `daemon.mcp`, `index.tantivy_heap_size_mb`). |
| `<value>` | yes | Value to set. Type is inferred (boolean, integer, string). |

```bash
# Enable MCP on daemon
agentscribe config set daemon.mcp true

# Increase Tantivy writer heap
agentscribe config set index.tantivy_heap_size_mb 100

# Change SQLite page cache
agentscribe config set sqlite.cache_size_pages 4000
```

Output:
```
Set daemon.mcp = true in ~/.agentscribe/config.toml
```

#### `agentscribe config get <key>`

Get a single configuration value.

```bash
agentscribe config get daemon.mcp
```

Output:
```
true
```

#### `agentscribe config init`

Create the data directory and default config file. Run once after installing AgentScribe. Copies bundled plugins to the plugins directory.

```bash
agentscribe config init
```

Output:
```
Created ~/.agentscribe/
Created ~/.agentscribe/config.toml (default config)
Created ~/.agentscribe/plugins/ (4 bundled plugins)
  claude-code.toml
  aider.toml
  opencode.toml
  codex.toml
Created ~/.agentscribe/sessions/
Created ~/.agentscribe/index/
Created ~/.agentscribe/state/

AgentScribe initialized. Run `agentscribe scrape` to start.
```

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--force` | | off | Overwrite existing config and plugins. Does not delete session data or the index. |

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Invalid key or value |
| 2 | Config file not found (for `show`, `get`, `set` — run `config init` first) |

---

## `agentscribe summarize`

Generate or regenerate a Markdown summary for a session. (Phase 3 feature.)

### Usage

```
agentscribe summarize <session-id> [options]
```

### Arguments

| Argument | Required | Description |
|----------|----------|-------------|
| `<session-id>` | yes | Session ID in `<agent>/<id>` format. |

### Options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--force` | `-f` | off | Regenerate even if a summary already exists. |
| `--method <method>` | | `extractive` | Summarization method: `extractive` (keyword/heuristic, no external deps) or `llm` (requires configured LLM endpoint). |
| `--stdout` | | off | Print summary to stdout instead of writing to the session's `.md` file. |

### Examples

```bash
# Summarize a session
agentscribe summarize claude-code/83f5a4e7

# Regenerate with LLM-powered summarization
agentscribe summarize claude-code/83f5a4e7 --force --method llm

# Preview without writing
agentscribe summarize claude-code/83f5a4e7 --stdout
```

Output:
```
Summary written to ~/.agentscribe/sessions/claude-code/83f5a4e7.md
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Session not found |
| 2 | Summary already exists (use `--force` to overwrite) |

---

## `agentscribe pulse-report`

Generate comprehensive quarterly analytics reports from the AgentScribe index. Provides executive summaries, monthly breakdowns, agent comparisons, error patterns, model usage, and PR/media highlights for "State of AI Coding" reports.

### Usage

```
agentscribe pulse-report [options]
```

### Options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--quarter <quarter>` | `-q` | `current` | Quarter to report on. Format: `YYYY-Q1` through `YYYY-Q4` or `current` for the current calendar quarter. Case-insensitive. |
| `--output <path>` | `-o` | stdout | Write output to a file instead of printing to stdout. |
| `--format <format>` | `-f` | `markdown` | Output format: `markdown`, `html`, or `json`. |

### Examples

```bash
# Generate report for current quarter
agentscribe pulse-report

# Specific quarter
agentscribe pulse-report --quarter 2026-Q1

# Save as HTML
agentscribe pulse-report --quarter 2026-Q2 --format html --output q2-report.html

# JSON for further processing
agentscribe pulse-report --format json --output report.json
```

### Output

**Markdown** (default):
- Full report with tables, ASCII charts, and methodology section
- Suitable for documentation, git commits, or conversion to PDF via pandoc

**HTML**:
- Self-contained HTML with inline CSS (~3KB) and responsive design
- Dark/light mode support via `prefers-color-scheme`
- No external dependencies
- Suitable for web hosting or email attachments

**JSON**:
- Structured data for programmatic consumption or further analysis

Markdown output includes:
- Executive Summary with total sessions, success rate, avg turns, tokens, cost
- Monthly Breakdown table with ASCII charts for session volume and success rate
- Agent Comparison table with success rate comparison chart
- Problem Type Distribution chart
- Model Usage table
- Top Error Patterns list
- Key Insights with auto-generated observations
- PR & Media Highlights with statistics for announcements
- Methodology section explaining data sources and calculations

HTML output includes:
- Summary cards with key metrics
- Interactive bar charts
- Responsive tables
- Color-coded insights by category
- Footer with methodology

JSON output structure:
```json
{
  "quarter": "Q1 2026",
  "period_start": "2026-01-01T00:00:00Z",
  "period_end": "2026-03-31T23:59:59Z",
  "total_sessions": 1247,
  "overall_success_rate": 73.2,
  "overall_avg_turns": 9.4,
  "estimated_total_tokens": 5900000.0,
  "estimated_total_cost": 142.30,
  "monthly_breakdown": [...],
  "agent_metrics": [...],
  "model_usage": [...],
  "top_error_patterns": [...],
  "problem_type_distribution": [...],
  "weekly_trend": [...],
  "key_insights": [...],
  "pr_highlights": [...],
  "computed_at": "2026-04-01T09:00:00Z"
}
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Invalid quarter format |
| 2 | Data dir not initialized |

---

## `agentscribe recurring`

Detect recurring problems by grouping error fingerprints across sessions. Shows problems that happen repeatedly, which projects they affect, and links to sessions that fixed them.

### Usage

```
agentscribe recurring [options]
```

### Options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--since <datetime>` | | `30d` | Only consider sessions after this timestamp. ISO 8601 or relative (`30d`, `12w`). |
| `--threshold <n>` | | `3` | Minimum occurrence count to report. Problems with fewer occurrences are excluded. |
| `--json` | | off | Output in JSON format. |

### Examples

```bash
# Find recurring problems in the last 30 days
agentscribe recurring

# Only problems that occurred 5+ times
agentscribe recurring --threshold 5

# Last 90 days
agentscribe recurring --since 90d

# JSON for scripting
agentscribe recurring --since 30d --json
```

### Output

Human-readable (default):
```
Recurring Problems (since 2026-02-20, threshold: 3)

[1] ConnectionError:Connection refused to {host}:{port}
    Occurrences: 5 sessions, 12 events
    Projects:    myapp, api-server
    Agents:      claude-code, aider
    Fixed by:    claude-code
    Last seen:   2026-03-15
    Last fix:    claude-code/abc123 (2026-03-15)

[2] TypeError:Cannot read properties of undefined (reading 'map')
    Occurrences: 3 sessions, 7 events
    Projects:    frontend
    Agents:      claude-code
    Fixed by:    (none)
    Last seen:   2026-03-12
```

JSON (`--json`):
```json
{
  "since": "2026-02-20T00:00:00Z",
  "threshold": 3,
  "problems": [
    {
      "fingerprint": "ConnectionError:Connection refused to {host}:{port}",
      "session_count": 5,
      "event_count": 12,
      "projects": ["myapp", "api-server"],
      "agents": ["claude-code", "aider"],
      "fix_agents": ["claude-code"],
      "last_seen": "2026-03-15T10:30:00Z",
      "last_fix_session": "claude-code/abc123",
      "last_fix_at": "2026-03-15T10:45:00Z"
    }
  ]
}
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 0 | No recurring problems found (empty array) |
| 1 | Data dir not initialized |
| 2 | No sessions found in the time range |

---

## `agentscribe rules`

Distill patterns from past agent sessions into project-specific rules files. Analyzes past sessions to extract corrections, conventions, context hints, and warnings.

### Usage

```
agentscribe rules <project-path> [options]
```

### Arguments

| Argument | Required | Description |
|----------|----------|-------------|
| `<project-path>` | yes | Path to the project directory to extract rules for. |

### Options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--format <format>` | | `claude` | Output format: `claude` (CLAUDE.md), `cursor` (.cursorrules), or `aider` (.aider.conf.yml). |
| `--json` | | off | Output rules as JSON without writing to a file. |

### Examples

```bash
# Generate CLAUDE.md rules for a project
agentscribe rules ~/my-project --format claude

# Generate .cursorrules
agentscribe rules ~/my-project --format cursor

# Preview rules without writing
agentscribe rules ~/my-project --json
```

### Output

Human-readable (default):
```
Wrote 12 rules to ~/my-project/CLAUDE.md (47 sessions analyzed)
```

JSON (`--json`):
```json
{
  "project_path": "/home/user/my-project",
  "sessions_analyzed": 47,
  "rules": [
    {"type": "correction", "rule": "Use `uuid v7` for all new ID columns, not v4"},
    {"type": "convention", "rule": "API endpoints use kebab-case URLs, snake_case response fields"},
    {"type": "context", "rule": "The `legacy` module wraps the old PHP API — don't refactor it"},
    {"type": "warning", "rule": "Don't modify migration files after they've been applied"}
  ]
}
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success (rules written or output) |
| 1 | No sessions found for the project |
| 2 | Data dir not initialized |

---

## `agentscribe analytics`

Cross-agent performance comparison and analytics. Reports success rates, specialization patterns, cost estimates, and weekly trends.

### Usage

```
agentscribe analytics [options]
```

### Options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--agent <name>` | `-a` | all | Filter to a specific agent. |
| `--project <path>` | `-p` | all | Filter to sessions from this project directory. |
| `--since <datetime>` | | all time | Only include sessions after this date. ISO 8601 or relative (`30d`, `12w`). |
| `--json` | | off | Output in JSON format. |

### Examples

```bash
# Overall analytics
agentscribe analytics

# Specific agent
agentscribe analytics --agent claude-code

# Last 30 days
agentscribe analytics --since 30d

# Specific project
agentscribe analytics --project ~/my-project

# JSON for dashboards
agentscribe analytics --since 30d --json
```

### Output

Human-readable (default):
```
AgentScribe Analytics (2026-02-20 to 2026-03-20)

Total sessions: 47 | Success rate: 80.9% | Avg turns: 12.3

Agent Breakdown:
  claude-code    31 sessions  83.9% success  10.5 avg turns (success)  ~$2.50 est. cost
  aider           8 sessions  75.0% success  14.2 avg turns (success)  ~$0.80 est. cost
  codex           8 sessions  87.5% success   8.1 avg turns (success)  ~$1.20 est. cost

Specialization:
  claude-code:  debug (15), feature (10), refactor (4), configuration (2)
  aider:        feature (5), debug (2), documentation (1)
  codex:        debug (5), feature (3)

Problem Types:
  debug           22 sessions  81.8% success
  feature         18 sessions  88.9% success
  refactor         4 sessions  75.0% success
  configuration    2 sessions  50.0% success
  documentation    1 session  100.0% success
```

JSON (`--json`):
```json
{
  "period_start": "2026-02-20T00:00:00Z",
  "period_end": "2026-03-20T00:00:00Z",
  "total_sessions": 47,
  "overall_success_rate": 80.9,
  "overall_avg_turns": 12.3,
  "agents": [
    {
      "agent": "claude-code",
      "total_sessions": 31,
      "success_count": 26,
      "failure_count": 3,
      "abandoned_count": 2,
      "success_rate": 83.9,
      "avg_turns_success": 10.5,
      "specialization": {"debug": 15, "feature": 10, "refactor": 4, "configuration": 2},
      "estimated_cost": 2.50
    }
  ],
  "problem_types": [...],
  "trends": [...]
}
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Data dir not initialized |
| 2 | No sessions found |

---

## `agentscribe digest`

Generate an activity digest summary over a configurable time period. Produces a Markdown report suitable for a 2-minute skim.

### Usage

```
agentscribe digest [options]
```

### Options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--since <period>` | | `7d` | Time period to cover. ISO 8601 or relative (`7d`, `30d`, `12w`). |
| `--output <path>` | `-o` | stdout | Write output to a file instead of printing to stdout. |
| `--json` | | off | Output in JSON format instead of Markdown. |

### Examples

```bash
# Weekly digest to stdout
agentscribe digest --since 7d

# Save monthly digest to file
agentscribe digest --since 30d --output ~/reports/march-digest.md

# JSON for custom processing
agentscribe digest --since 7d --json
```

### Output

The Markdown digest includes:
- Session counts by agent and project
- Recurring problems detected
- Agent comparison table
- Most-touched files
- New error patterns discovered
- Token usage and estimated costs

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Data dir not initialized |
| 2 | No sessions in the time range |

---

## `agentscribe capacity`

Show per-account Claude Code utilization matching the `/status` output. Displays 5h and 7d rolling windows, per-model windows, burn rates, and forecasts. Supports multi-account setups (e.g., personal vs work credentials).

### Usage

```
agentscribe capacity [options]
```

### Options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--account-dir <path>` | | `~/.claude` + auto-discovered `~/.claude-*` | Claude config directories to scan (each = one account). Repeatable. |
| `--cache-max-age <seconds>` | | `600` | Maximum age of cached `usage.json` before falling back to JSONL estimation. |
| `--json` | `-j` | off | Output in JSON format. |

### Examples

```bash
# Show capacity for all auto-discovered accounts
agentscribe capacity

# Specific account directories
agentscribe capacity --account-dir ~/.claude --account-dir ~/.claude-work

# Use cached data even if older (up to 1 hour)
agentscribe capacity --cache-max-age 3600

# JSON for programmatic access
agentscribe capacity --json
```

### Output

Human-readable (default):
```
Claude Code Capacity

Account: claude-default (max / default_claude_max_20x)
  Source: api_cache
  5h window:   24.5%  [█████░░░░░░░░░░░░░░]  resets in 2h 15m
  7d window:   94.2%  [████████████████████░]  resets in 4h 30m
    sonnet        82.0%  [█████████████████░░░]
    opus          91.5%  [███████████████████░]
    cowork        45.0%  [████████░░░░░░░░░░░]
  Burn rate:  8,450 tokens/min
  Forecast:   5h full in 1h 45m
  Turns:      127 (5h)  2,840 (7d)

Account: .claude-work (pro / default)
  Source: jsonl_estimate
  5h window:   67.8%  [████████████████░░░░░]  resets in 1h 30m
  7d window:   23.4%  [█████░░░░░░░░░░░░░░░]  resets in 5d 12h
  Burn rate:  2,100 tokens/min
  Turns:      42 (5h)  580 (7d)
```

**Visual meter:** ASCII progress bar shows utilization percentage. Each `█` represents ~5% (20 bars = 100%).

**Source indicators:**
- `api_cache`: Exact numbers from cached Claude Code API response (`~/.cache/claude-usage/usage.json`)
- `jsonl_estimate`: Approximate from parsing JSONL logs (uses cost-equivalent token weighting)

**Per-model windows:** Shown for 7d window when available from cached API (sonnet, opus, cowork, omelette). Not available in JSONL fallback mode.

**Burn rate:** Cost-equivalent tokens per minute averaged over the last hour.

**Forecast:** Minutes until the window hits 100% at current burn rate. `null` if burn rate is zero or already at 100%.

JSON (`--json`):
```json
[
  {
    "account_id": "claude-default",
    "adapter": "claude",
    "plan_type": "max",
    "rate_limit_tier": "default_claude_max_20x",
    "utilization_5h": 24.5,
    "utilization_7d": 94.2,
    "resets_at_5h": "2026-05-03T16:15:00Z",
    "resets_at_7d": "2026-05-03T18:30:00Z",
    "model_windows_7d": [
      {"model": "sonnet", "utilization": 82.0, "resets_at": "2026-05-03T18:30:00Z"},
      {"model": "opus", "utilization": 91.5, "resets_at": "2026-05-03T18:30:00Z"}
    ],
    "tokens_5h": 127000,
    "tokens_7d": 2840000,
    "turns_5h": 127,
    "turns_7d": 2840,
    "burn_rate_per_min": 8450.0,
    "forecast_full_5h_min": 105.0,
    "forecast_full_7d_min": null,
    "source": "api_cache",
    "computed_at": "2026-05-03T14:20:00Z"
  }
]
```

### Data Sources

The capacity meter reads from two sources (in priority order):

1. **Cached API response** (`~/.cache/claude-usage/usage.json`) — exact numbers matching Claude Code's `/status` output. This file is written by Claude Code when you run `/status` or check limits.

2. **JSONL-based estimation** — fallback when cache is stale or missing. Parses `~/.claude/projects/*/*.jsonl` files and counts tokens using cost-equivalent weighting:
   - Input tokens: 1.0× weight
   - Output tokens: ~5× weight (matching API pricing ratio)
   - Cache read tokens: ~0.1× weight
   - Cache write tokens: ~0.25× weight

The JSONL fallback is inherently approximate because the exact weighting formula is proprietary. The cached API response should be preferred whenever available.

### Account Discovery

By default, scans:
- `~/.claude` (named "claude-default" in output)
- All `~/.claude-*` directories that contain `.credentials.json`

For example, if you have `~/.claude` and `~/.claude-work`, both will be scanned and reported separately.

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | No Claude accounts found |

---

## `agentscribe context`

Pre-task priming query for agent workers. Returns a formatted context block with past solutions, project conventions, and file notes — ready for direct injection into agent prompts.

### Usage

```
agentscribe context <query> [options]
```

### Arguments

| Argument | Required | Description |
|----------|----------|-------------|
| `<query>` | yes | Task description to search for context. Used to find relevant past solutions. |

### Options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--token-budget <n>` | | `3000` | Token budget for context packing. Sections are prioritized: past solutions > conventions > file notes. |
| `--project <path>` | | cwd | Project path for rules extraction. Defaults to current directory. |
| `--json` | | off | Output sections as JSON object. |

### Examples

```bash
# Get context for implementing JWT authentication
agentscribe context "implement JWT authentication" --token-budget 4000

# Get context for a specific project
agentscribe context "fix the database connection pool" --project ~/myproject

# JSON output for programmatic assembly
agentscribe context "handle file upload errors" --json
```

### Output

Human-readable (default):
```
### Past Solutions
- Migrated Postgres schema from v3 to v4, added rollback script
  Ran ALTER TABLE to add the new columns, then backfilled existing rows.
- Fixed Postgres connection pooling, added retry logic
  The connection pool was exhausting under load because max_connections...

### Project Conventions
- Use uuid v7, not v4, for new database columns
- API endpoints use kebab-case, response fields use snake_case
- Don't modify applied migration files

### File Notes
**src/auth/middleware.rs**
  - Error: ConnectionError:Connection refused to {host}:{port}
```

JSON (`--json`):
```json
{
  "past_solutions": "- Migrated Postgres schema...",
  "conventions": "- Use uuid v7, not v4...",
  "file_notes": "**src/auth/middleware.rs**\n  - Error: ConnectionError..."
}
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Data dir not initialized |
| 2 | No sessions found |

---

## `agentscribe render`

Render a session as self-contained HTML or Markdown for sharing, linking from commits, or human review outside the terminal.

### Usage

```
agentscribe render <session-id> [options]
```

### Arguments

| Argument | Required | Description |
|----------|----------|-------------|
| `<session-id>` | yes | Session ID in `<agent>/<id>` format. |

### Options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--output <path>` | `-o` | stdout | Write output to a file instead of printing to stdout. |
| `--format <format>` | | `html` | Output format: `html` or `markdown`. |

### Examples

```bash
# Render as HTML to stdout
agentscribe render claude-code/83f5a4e7

# Save as Markdown
agentscribe render claude-code/83f5a4e7 --format markdown --output session.md

# Create a gist (via GitHub CLI)
agentscribe render claude-code/83f5a4e7 --output /tmp/session.html && gh gist create /tmp/session.html

# Link from commit message
git commit -m "Fix auth bug

Session: https://gist.github.com/abc123"
```

### Output

HTML output includes:
- Self-contained file with inline CSS (~3KB)
- Syntax highlighting via embedded highlight.js subset (~50KB)
- Dark/light mode support via `prefers-color-scheme`
- Header with project, agent, outcome, duration, files touched
- Conversation with role badges and timestamps
- Footer with session ID and generation timestamp

Markdown output includes:
- YAML frontmatter with session metadata
- Conversation as alternating role blocks
- Code blocks fenced with language tags
- Separator lines between turns

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Session not found |
| 2 | Data dir not initialized |

---

## Environment Variables

| Variable | Description |
|----------|-------------|
| `AGENTSCRIBE_DATA_DIR` | Override the data directory (default: `~/.agentscribe`). Takes precedence over `--data-dir` flag. |
| `AGENTSCRIBE_LOG` | Set log level for CLI commands: `error`, `warn`, `info`, `debug`, `trace`. Equivalent to `-v` / `-vv`. |
| `AGENTSCRIBE_NO_COLOR` | Disable color output. Equivalent to `--color never`. |
| `AGENTSCRIBE_CONFIG` | Override config file path. |

---

## Shell Completions

Generate shell completions for your shell (via `clap`'s built-in support):

```bash
# Bash
agentscribe completions bash > ~/.local/share/bash-completion/completions/agentscribe

# Zsh
agentscribe completions zsh > ~/.zfunc/_agentscribe

# Fish
agentscribe completions fish > ~/.config/fish/completions/agentscribe.fish
```
