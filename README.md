# AgentScribe

Archive, search, and learn from coding agent conversations.

AgentScribe normalizes logs from multiple AI coding agents into a unified format, indexes them with full-text search, and extracts actionable intelligence — error patterns, solutions, anti-patterns, and auto-generated project rules.

## Supported Agents

| Agent | Format | Plugin |
|-------|--------|--------|
| Claude Code | JSONL | `claude-code` (bundled) |
| Aider | Markdown | `aider` (bundled) |
| OpenCode | JSON tree | `opencode` (bundled) |
| Codex | JSONL | `codex` (bundled) |
| Cursor | SQLite | `cursor` (bundled) |
| Windsurf | SQLite | `windsurf` (bundled) |

Add new agents by writing a single TOML file — no code changes needed. See the [Plugin Authoring Guide](plugins/BUILDING_PLUGINS.md).

## Features

- **Multi-agent archival** — Scrape and normalize conversations from any supported agent into a canonical JSONL format
- **Incremental scraping** — Tracks file offsets; only processes new data on each run
- **Full-text search** — BM25-ranked search with fuzzy matching, faceted filters, "more like this", and code search
- **Semantic vector search** — **[NON-FUNCTIONAL STUB]** Embedding-based similarity search requires turbovec BLAS dependency fixes before it can function. The `embed` subcommand and `--semantic`/`--hybrid` search flags exist but will fail with clear error messages explaining the issue. See src/vector.rs for restoration steps.
- **Intelligence pipeline** — Outcome detection, solution extraction, error fingerprinting, anti-pattern detection
- **Cross-agent analytics** — Compare success rates, specialization, cost efficiency across agents
- **Recurring problems** — Detect errors that happen repeatedly, with links to sessions that fixed them
- **Auto-generated rules** — Extract project conventions from past sessions into CLAUDE.md / .cursorrules / .aider.conf.yml
- **Activity digests** — Weekly summaries of agent activity, recurring problems, and trends
- **Background daemon** — Watches for new log data and scrapes automatically
- **MCP server** — Exposes search and blame as Model Context Protocol tools over a Unix socket, so agents can query their own history during a session
- **Plugin system** — Add support for new agents with a declarative TOML config

## Installation

### From source

Requires Rust 1.75+ and a C compiler (for `libsqlite3-sys`).

```bash
git clone https://github.com/jedarden/AgentScribe.git
cd AgentScribe
cargo install --path .
```

Alternatively, use the bundled install script:

```bash
git clone https://github.com/jedarden/AgentScribe.git
cd AgentScribe
./install.sh
```

### Shell completions

```bash
# Bash
agentscribe completions bash > ~/.local/share/bash-completion/completions/agentscribe

# Zsh
agentscribe completions zsh > ~/.zfunc/_agentscribe

# Fish
agentscribe completions fish > ~/.config/fish/completions/agentscribe.fish
```

## Quick Start

```bash
# 1. Initialize data directory and install bundled plugins
agentscribe config init

# 2. Scrape all agent logs
agentscribe scrape

# 3. Search across sessions
agentscribe search "database connection timeout"

# 4. Check for recurring problems
agentscribe recurring --since 30d

# 5. Generate analytics
agentscribe analytics --json

# 6. Auto-generate project rules
agentscribe rules ~/my-project --format claude
```

## Architecture

```
Agent Logs (JSONL, Markdown, JSON-tree, SQLite)
         │
         ▼
   ┌─────────────┐
   │   Scrapers   │  Plugin-driven: each agent has a TOML config
   │  (per-agent) │  that declares paths, format, field mappings
   └──────┬───────┘
          │
          ▼
   ┌─────────────┐
   │   Parsers    │  Format-specific: JSONL, Markdown, JSON-tree, SQLite
   │ (normalize)  │  Output: canonical event schema
   └──────┬───────┘
          │
          ▼
   ┌─────────────┐
   │ Enrichment   │  Outcome detection, error fingerprinting,
   │  Pipeline    │  solution extraction, anti-patterns,
   │              │  code artifacts, git correlation
   └──────┬───────┘
          │
          ▼
   ┌─────────────┐
   │    Index     │  Tantivy full-text index (BM25)
   │              │  Faceted filters, fuzzy matching
   └──────┬───────┘
          │
          ▼
   ┌─────────────┐
   │    Search    │  Full-text, error lookup, code search,
   │              │  "more like this", session retrieval
   └─────────────┘
```

### MCP Server

When the daemon is running, it can also expose a Model Context Protocol server over a Unix socket. This lets agents (Claude Code, Cursor, etc.) call AgentScribe tools directly during a session — searching past conversations or tracing which sessions touched a given file.

Enable in `~/.agentscribe/config.toml`:

```toml
[daemon]
mcp_enabled = true
mcp_socket_path = "~/.agentscribe/mcp.sock"
```

Four MCP tools are exposed:

| Tool | Description |
|------|-------------|
| `agentscribe_search` | Full-text and faceted search across all archived sessions |
| `agentscribe_status` | Plugin list, session counts, daemon state, index stats |
| `agentscribe_blame` | Given a file path (and optional line), find sessions that touched it |
| `agentscribe_file` | Chronological session list for a file path |

Connect via any MCP-compatible client:

```json
{
  "mcpServers": {
    "agentscribe": {
      "command": "agentscribe",
      "args": ["mcp", "--socket", "~/.agentscribe/mcp.sock"]
    }
  }
}
```

### Data Directory Layout

```
~/.agentscribe/
├── config.toml          # Global configuration
├── agentscribe.pid      # Daemon PID file
├── daemon.log           # Daemon log output
├── daemon_state.json    # Daemon runtime state
├── plugins/             # Plugin TOML files
│   ├── claude-code.toml
│   ├── aider.toml
│   ├── codex.toml
│   └── opencode.toml
├── sessions/            # Normalized session JSONL files
│   ├── claude-code/
│   ├── aider/
│   └── codex/
├── index/               # Tantivy search index
│   └── tantivy/
└── state/               # Scrape state (offset tracking)
```

## Documentation

- [CLI Reference](docs/cli-reference.md) — All commands, flags, and output formats
- [Configuration Reference](docs/configuration.md) — All `config.toml` options
- [Plugin Authoring Guide](plugins/BUILDING_PLUGINS.md) — How to add support for new agents
- [Example Workflows](docs/workflows.md) — Real-world usage scenarios
- [Implementation Plan](docs/plan.md) — Architecture and design decisions

## Configuration

AgentScribe reads configuration from `~/.agentscribe/config.toml` (or `$XDG_CONFIG_DIR/agentscribe/config.toml`). See the [Configuration Reference](docs/configuration.md) for all options.

```toml
[general]
log_level = "info"

[scrape]
debounce_seconds = 5
max_session_age_days = 0

[index]
tantivy_heap_size_mb = 50

[search]
default_max_results = 10
default_snippet_length = 200
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `AGENTSCRIBE_DATA_DIR` | Override data directory (default: `~/.agentscribe`) |
| `AGENTSCRIBE_CONFIG` | Override config file path |
| `AGENTSCRIBE_LOG` | Set log level: `error`, `warn`, `info`, `debug`, `trace` |
| `AGENTSCRIBE_NO_COLOR` | Disable color output |

## License

MIT
