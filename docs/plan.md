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
├── plugins     # Manage scraper plugin definitions (list|validate|show)
├── config      # Manage source paths, agent types, data directory location
├── summarize   # Generate Markdown summaries for sessions (Phase 3)
└── completions # Generate shell completions (bash|zsh|fish)
```

See [docs/cli-reference.md](cli-reference.md) for detailed help on every command, flag, output format, and exit code.

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

## Future Features (Post-MVP)

These ten features transform AgentScribe from a passive archive into an active intelligence layer. Each is designed to be high-impact while staying implementable on top of the existing scrape→normalize→index pipeline.

### 1. Error Fingerprinting + Solution Index

**The killer feature.** Automatically extract error messages and stack traces from conversations, normalize them (strip line numbers, variable paths, timestamps, PIDs), and build an `error_fingerprint → [solution_sessions]` index. When any agent hits an error, the answer may already be waiting.

**How it works:**
- During scraping, a regex pipeline identifies error patterns in `tool_result` and `assistant` events: stack traces, exception lines, compiler errors, shell exit codes, HTTP status codes
- Errors are normalized to fingerprints: `ConnectionRefusedError: postgres:5432` becomes `ConnectionRefusedError: {host}:{port}`
- Fingerprints are stored as a Tantivy facet field, queryable via `agentscribe search --error "ConnectionRefused"` or exact fingerprint match
- Each fingerprint accumulates a list of sessions that encountered AND resolved it, ranked by recency and outcome

**Why it matters:** Today, when Claude Code hits a Postgres connection error, it has no idea that Aider solved the exact same error two days ago. With error fingerprinting, the solution surfaces instantly — across agents, across projects, across time.

```bash
# Agent hits an error, queries the fingerprint index
agentscribe search --error "ENOSPC: no space left on device" --json --max-results 1

# Returns: "Session aider/xyz solved this by clearing Docker build cache"
```

**Implementation cost:** Medium. Regex-based error extraction during scrape enrichment. Normalization is template-based pattern matching. Fingerprints index as a Tantivy `STRING | FAST` field. The hardest part is building a good error pattern library — but it can start small (Python tracebacks, Rust compiler errors, shell errors) and grow.

---

### 2. Anti-Pattern Library

The inverse of the solution index. Automatically catalog approaches that **failed** so agents don't repeat known mistakes.

**How it works:**
- Sessions with `outcome: "failure"` or `outcome: "abandoned"` are analyzed for what was attempted
- Tool calls that preceded a failure (the last N tool_calls before the user said "no", "wrong", "stop", "revert", "undo") are tagged as anti-patterns
- Anti-patterns are stored with: the approach that was tried, why it failed (extracted from the user's correction), and what worked instead (if a subsequent session solved it differently)
- Queryable via `agentscribe search --anti-patterns "approach to X"`

**Why it matters:** Agents waste enormous amounts of tokens on approaches that have already been proven bad. A developer's most expensive frustration is watching an agent try something for the third time that never works. AgentScribe remembers failures so agents don't have to repeat them.

```bash
# Agent is about to try mocking the database in tests
agentscribe search --anti-patterns "mock database" --project /home/coding/myapp --json

# Returns: "Mocking the DB was attempted 3 times across 2 agents, failed each time.
#           Reason: mock schema drifted from production. Working approach: use testcontainers."
```

**Implementation cost:** Medium. Outcome detection is already in Phase 3. Anti-pattern extraction is heuristic: find the tool calls preceding user rejection ("no", "wrong", "revert", "that broke"), tag them with the error context. No LLM required — pattern matching on user sentiment keywords + outcome field.

---

### 3. Code Artifact Extraction

Index every code block from every conversation as a **first-class searchable artifact**, not just as part of a session blob. When an agent searches for "Redis connection pool config," it gets actual code that worked — not a session summary it has to wade through.

**How it works:**
- During scraping, extract fenced code blocks from assistant responses and tool_call/tool_result events
- Each code artifact is indexed with: language, file path (if inferable from surrounding context), session ID, whether it was a draft or the final version, and whether it was applied successfully (tool_result success)
- Final/applied code blocks are ranked higher than intermediate drafts
- Queryable via `agentscribe search --code "connection pool" --lang rust`

**Schema addition:**
```rust
schema_builder.add_text_field("code_content", TEXT | STORED);
schema_builder.add_text_field("code_language", STRING | STORED | FAST);
schema_builder.add_text_field("code_file_path", STRING | STORED | FAST);
schema_builder.add_bool_field("code_is_final", STORED | FAST);
```

**Why it matters:** The most useful thing in a past session is usually a code block. Today you search, find a session, read through 42 turns to find the one code block that matters. Code artifact extraction lets you skip straight to the answer.

```bash
agentscribe search --code "systemd unit file" --lang ini --json -n 3

# Returns: 3 actual unit file snippets from past sessions, ranked by
#          recency and whether they were the final applied version
```

**Implementation cost:** Low-medium. Code block extraction from Markdown and JSONL is straightforward regex/parser work. Language detection from fenced block markers (` ```rust `). File path inference from preceding tool_call context. Indexed as additional documents in the same Tantivy index with a `doc_type: "code_artifact"` facet.

---

### 4. Solution Extraction

When returning search results, don't return the full conversation. Extract just the **solution** — the key code change, the command that worked, the configuration fix. Return a concise "playbook" an agent can act on immediately.

**How it works:**
- For each session with `outcome: "success"`, identify the resolution point: the final sequence of tool_calls before the user confirmed success
- Extract the "solution" as: the last Edit/Write tool calls (the actual code changes), the last shell commands that succeeded, and the assistant's explanation immediately preceding them
- Store as a `solution_summary` field, separate from the full session content
- Search results include the solution by default, full session content on request

**Heuristics for finding the solution:**
1. The last `Edit` or `Write` tool call in a successful session
2. The assistant response immediately before user says "thanks", "that works", "perfect", "LGTM"
3. The last `Bash` tool call that returned exit code 0 after a series of failures
4. The code block in the final assistant turn of a successful session

**Why it matters:** An agent with 128K context doesn't want to read a 42-turn debugging conversation. It wants: "Change line 15 of auth.rs from X to Y." Solution extraction turns verbose history into actionable answers.

```bash
agentscribe search "postgres migration v3 to v4" --solution-only --json

# Returns: "Edit db/migrations/004.sql: ALTER TABLE users ADD COLUMN...
#           Then: cargo sqlx migrate run"
# Not: 42 turns of debugging, investigation, and false starts
```

**Implementation cost:** Medium. The heuristics are content-pattern based — scan backward from session end for tool_call events, look for success signals in user responses. No LLM required for v1 (LLM-powered extraction is a Phase 3 enhancement that can produce even better summaries later).

---

### 5. Git Blame Bridge

Bidirectional linking between agent conversations and git commits. For any commit, see the conversation that produced it. For any conversation, see the commits it generated.

**How it works:**
- During scrape enrichment, for each session with a `project` path that is a git repo:
  - Run `git log --after=<session_start> --before=<session_end> --format="%H %s" -- <project_path>`
  - Store matching commit hashes as a `commits` field on the session
- Build a reverse index: `commit_hash → session_id`
- New CLI command: `agentscribe blame <file>:<line>` runs `git blame` to find the commit, then looks up which session produced it

**Why it matters:** `git blame` tells you WHAT changed and WHO committed it. AgentScribe blame tells you **WHY** — the full conversation, the reasoning, the alternatives that were considered and rejected. This is the context that git blame has always been missing.

```bash
# "Why was this line written this way?"
agentscribe blame src/auth.rs:42

# Returns: Session claude-code/83f5a4e7
#   "Changed from bcrypt to argon2id because bcrypt has a 72-byte
#    password length limit that was truncating long passphrases"

# From a session, see what code it produced
agentscribe search --session claude-code/83f5a4e7 --show-commits

# Returns: 3 commits (a]b2c3d, e4f5g6h, i7j8k9l)
```

**Schema addition:**
```rust
schema_builder.add_text_field("git_commits", STRING | STORED | FAST);  // Multi-valued
```

**Implementation cost:** Low. `git log` with timestamp range is a single subprocess call per session during scrape enrichment. The reverse index is a Tantivy facet field. `agentscribe blame` is `git blame` + index lookup. The only complexity is handling repos with high commit frequency (narrow the window using file paths from tool_call events).

---

### 6. Context Budget Packing

When an agent queries AgentScribe, it has a finite context window. `--token-budget 4000` tells AgentScribe "I have 4000 tokens to spare — fill them optimally." AgentScribe maximizes information density within the constraint.

**How it works:**
- Estimate tokens per result (4 chars per token, roughly)
- Greedy knapsack: rank results by relevance score, then greedily pack results into the budget
- Adaptive snippet sizing: if budget allows 3 results with 500-char snippets OR 6 results with 150-char snippets, choose the option with higher total relevance coverage
- Solution-only mode integrates naturally: solutions are much shorter than full sessions, so more fit in the budget

**Why it matters:** Every agent has a different context window, and every agent call has a different amount of remaining context. A fixed `--max-results 3 --snippet-length 200` is a guess. `--token-budget 4000` is precise — AgentScribe does the math to maximize value per token.

```bash
# Agent has ~4000 tokens of context to spare
agentscribe search "redis caching" --token-budget 4000 --json

# AgentScribe returns: 4 results with optimally-sized snippets
# that together fit within 4000 tokens, prioritized by relevance

# Tight budget — get just the essentials
agentscribe search "redis caching" --token-budget 500 --json

# Returns: 1 result, solution-only, tightly trimmed
```

**Implementation cost:** Low. Token estimation is `ceil(chars / 4)`. Greedy knapsack is ~20 lines of code. The insight is that this parameter replaces both `--max-results` and `--snippet-length` with a single, more meaningful constraint.

---

### 7. Recurring Problem Detection

Detect when the same problem keeps being solved repeatedly. Surface systemic issues that need a permanent fix, not another agent session.

**How it works:**
- Builds on error fingerprinting (#1): when the same fingerprint appears in 3+ sessions within a configurable window (default 30 days), flag it as recurring
- Tracks frequency, affected projects, and which agents solved it
- New command: `agentscribe recurring` lists problems sorted by frequency
- Integrates with search: results for recurring problems include a `recurring: true` flag and the frequency count

**Why it matters:** If an agent solves "Docker build cache full" every week, the real fix is a cron job or CI config change, not another agent session. Recurring problem detection surfaces the problems that are worth solving permanently — the difference between treating symptoms and curing the disease.

```bash
agentscribe recurring --since 30d

# Recurring problems (last 30 days):
#
# [5 occurrences]  ENOSPC in Docker builds
#   Projects: myapp, api-server, worker
#   Agents:   claude-code (3), aider (2)
#   Last fix: "docker system prune" (temporary)
#   Suggestion: Permanent fix needed — this keeps coming back
#
# [3 occurrences]  Postgres connection timeout on cold start
#   Projects: api-server
#   Agents:   claude-code (2), codex (1)
#   Last fix: "Added retry with backoff" (each time)
```

**Implementation cost:** Low. This is a GROUP BY on error fingerprints with a HAVING count >= 3. The data is already there from feature #1. The CLI formatting and threshold config are trivial. The value-to-effort ratio is extreme.

---

### 8. File Knowledge Map

For any file in your codebase, show its complete "institutional memory" — every conversation any agent has ever had about it. Not just `git blame` (who changed what), but the reasoning, the rejected alternatives, the gotchas discovered.

**How it works:**
- During scraping, extract file paths from tool_call events (Read, Edit, Write, Bash commands referencing files) and tool_result events
- Build a reverse index: `file_path → [session_ids]` stored as a Tantivy facet
- New command: `agentscribe file <path>` shows all sessions that touched the file, with summaries and solution extracts
- Supports glob patterns: `agentscribe file "src/auth/**"` for directory-level knowledge

**Why it matters:** When you're about to modify `src/auth/middleware.rs`, you want to know: what has every agent learned about this file? What are the known pitfalls? What design decisions were made and why? This is the context that makes agents smarter over time — they learn from every past interaction with a file, not just the current state of the code.

```bash
agentscribe file src/auth/middleware.rs

# Knowledge map for src/auth/middleware.rs (7 sessions):
#
# 2026-03-14  claude-code/83f5  Switched bcrypt→argon2id (72-byte limit)
# 2026-03-10  aider/sess-42     Added rate limiting to /login endpoint
# 2026-03-08  claude-code/a1b2  Fixed session token leak in error path
# 2026-03-01  codex/thr-789     Initial auth middleware implementation
# 2026-02-25  claude-code/c3d4  Attempted JWT migration — ABANDONED (reverted)
# 2026-02-20  aider/sess-31     Added CORS headers for mobile client
# 2026-02-15  claude-code/e5f6  Created file (initial scaffolding)
#
# Known gotchas:
#   - JWT migration was attempted and reverted (see session c3d4)
#   - bcrypt has a 72-byte password limit — use argon2id instead
```

**Implementation cost:** Low-medium. File path extraction from tool events is straightforward string parsing. The reverse index is a Tantivy multi-valued STRING field. The "known gotchas" section uses anti-patterns (#2) and solution extraction (#4) filtered to this file.

---

### 9. Auto-Generated Project Rules

Analyze all sessions for a project and automatically distill recurring patterns, corrections, and preferences into a rules file (CLAUDE.md, .cursorrules, etc.). Closes the feedback loop — AgentScribe learns from conversations and feeds knowledge back into agents.

**How it works:**
- Aggregate all sessions for a project path
- Extract patterns: user corrections ("no, use pnpm not npm"), repeated tool configurations, file paths that are always included, recurring context that agents need to be told
- Detect conventions: testing frameworks used, import styles, directory structures
- Generate a rules file with discovered patterns, formatted for the target agent
- New command: `agentscribe rules <project-path> [--format claude|cursor|aider]`

**What gets extracted:**
- **Explicit corrections**: "don't use X, use Y" → rule
- **Repeated patterns**: if 5+ sessions all start by reading the same file, that file matters → add to context
- **Tool preferences**: if `pnpm` is always used instead of `npm`, that's a convention → rule
- **Architecture patterns**: if tests always go in `__tests__/`, if the ORM is always Prisma, if the API follows a specific pattern → rules
- **Known pitfalls**: from the anti-pattern library → warnings

**Why it matters:** Today, developers manually write CLAUDE.md files. They forget things, they miss patterns, the files go stale. AgentScribe has the data to generate and update these files automatically. Every session makes future sessions better — without the developer doing anything.

```bash
agentscribe rules /home/coding/myapp --format claude

# Generated CLAUDE.md rules for /home/coding/myapp
# (based on 47 sessions from 3 agents):
#
# - Use pnpm, not npm (corrected in 4 sessions)
# - Tests go in __tests__/ using vitest, not jest
# - Database is Postgres 16 via Prisma ORM
# - Auth uses argon2id, NOT bcrypt (72-byte limit)
# - Run `pnpm check` before committing
# - The middleware in src/auth/ has known complexity — see AgentScribe
#   file map before modifying
```

**Implementation cost:** Medium. Pattern extraction is frequency analysis over normalized events: count user corrections, count tool invocations, count file paths. Convention detection is TF-IDF over tool arguments. No LLM needed for v1 — frequency-based heuristics produce useful rules. LLM refinement can come later.

---

### 10. Agent Effectiveness Analytics

Compare agent performance across problem types, projects, and time periods. Data that is **only possible** with AgentScribe's cross-agent view — no individual agent knows how it compares to others.

**How it works:**
- Aggregate session metadata by agent type: outcomes, turn counts, token usage, duration
- Categorize sessions by problem type (bug fix, feature, refactor, investigation — inferred from tags and content patterns)
- Compute per-agent metrics: success rate, average turns to resolution, tokens per successful outcome, rate of abandoned sessions
- New command: `agentscribe analytics [--agent <name>] [--project <path>] [--since <date>]`

**Metrics:**
- **Success rate**: % of sessions with `outcome: "success"` per agent
- **Efficiency**: average turns and tokens per successful outcome
- **Specialization**: which problem types each agent handles best
- **Trend**: is an agent getting better or worse over time (model updates, prompt changes)
- **Cost efficiency**: outcome quality per dollar of token spend

**Why it matters:** You might think Claude Code is better for debugging and Aider is better for refactoring, but you don't actually know. AgentScribe does. It has every session from every agent, with outcomes. Now you can make data-driven decisions about which agent to use for what — or just let the data confirm your intuition.

```bash
agentscribe analytics --since 30d

# Agent Effectiveness (last 30 days):
#
#                  Sessions  Success%  Avg Turns  Avg Tokens   Top Category
# claude-code           89     84.3%      24.1      45,200    debugging (91% success)
# aider                  8     75.0%      12.5      18,300    refactoring (100% success)
# codex                 14     71.4%      18.7      32,100    feature (80% success)
#
# Insights:
#   - Claude Code has the highest success rate overall
#   - Aider is most efficient (fewest turns/tokens per success)
#   - Aider has never failed a refactoring task (8/8 success)
#   - Codex abandonment rate is 21% — highest of all agents
```

**Implementation cost:** Low. All the raw data is already in the session manifest (outcome, turns, tokens, agent, timestamps). This is pure aggregation and display — SQL-style GROUP BY on indexed fields. Tantivy's faceted search handles the grouping natively. The CLI formatting is the main work.

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
- **Low footprint** — daemon idles under 20MB RSS; scraping stays under 50MB regardless of source file size

---

## Design: Memory Budget

AgentScribe runs alongside the agents it monitors. RAM is shared with Claude Code, IDEs, build tools, and the agents themselves. The memory budget is designed to be invisible.

### Target: <20MB idle, <50MB active, <100MB peak

| Component | Expected RSS | Notes |
|-----------|-------------|-------|
| **Daemon idle** (watcher + tokio) | 5-10MB | `notify` inotify watcher + minimal tokio runtime. No work happening. |
| **JSONL streaming parse** | 1-5MB | `serde_json::StreamDeserializer` processes one line at a time. Working set is one JSON object regardless of file size. A 50MB JSONL file uses the same RAM as a 500KB one. |
| **Tantivy index (search)** | 10-30MB | Memory-mapped segments. Virtual memory is large (Tantivy maps entire segments), but RSS is only pages actively accessed. For AgentScribe's index size (thousands of sessions, not millions of documents), resident pages stay small. After a search completes, the OS pages out unused mappings. |
| **Tantivy indexing (write)** | 20-50MB | Controlled via `IndexWriter::new(heap_size)`. Set to 20-50MB. Tantivy buffers documents in memory until this budget is hit, then flushes a segment to disk. |
| **SQLite reads** (Cursor/Windsurf) | 5-15MB | `rusqlite` with read-only mode. SQLite's own page cache is configurable via `PRAGMA cache_size`. Set conservatively (2000 pages = ~8MB). For Cursor's 25GB+ databases, only the queried pages are loaded — not the whole file. |
| **Markdown parsing** (Aider) | 1-2MB | `pulldown-cmark` is a streaming pull parser. No intermediate AST allocation. |
| **Plugin configs** | <1MB | TOML files are tiny. |

### Total budget

| Mode | Max RSS | When |
|------|---------|------|
| CLI one-shot search | 15-35MB | Loads Tantivy index, runs query, exits. Tantivy mmaps segments but only touches pages needed for the query. |
| CLI one-shot scrape | 25-55MB | Streams source files + writes to Tantivy. Dominant cost is `IndexWriter` heap budget. |
| Daemon idle | 8-15MB | Watcher loop, no active work. Tantivy index not loaded until a query or scrape triggers it. |
| Daemon active scrape | 30-60MB | Same as CLI scrape, held briefly during file processing, then drops back to idle. |
| Daemon peak (scrape + concurrent search) | 50-90MB | Both writer and reader active simultaneously. Rare. |

### How to stay within budget

1. **Stream, never slurp.** Never read an entire JSONL/JSON file into memory. Use `BufReader` + line-by-line deserialization. This is the single most important rule.
2. **Cap Tantivy's writer heap.** `IndexWriter::new(arena_size)` — set to 20-50MB. When the buffer fills, Tantivy flushes to disk automatically.
3. **Cap SQLite page cache.** `PRAGMA cache_size = -8000` (8MB). Prevents Cursor's 25GB database from ballooning resident memory.
4. **Drop the index between operations in daemon mode.** Don't hold the Tantivy `IndexReader` open when idle. Reopen on query, let the OS reclaim mapped pages when done.
5. **Process one source file at a time.** Don't parallelize scraping across multiple files — the I/O is already the bottleneck, and parallel parsing multiplies working set.
6. **Use `jemalloc` or the system allocator with care.** Rust's default allocator returns memory to the OS. Avoid `jemalloc` unless profiling shows fragmentation issues, since jemalloc retains freed pages.

### Monitoring

The daemon should expose its own memory stats via `agentscribe daemon status`:

```
AgentScribe daemon (PID 12345)
  Uptime:     3d 14h
  RSS:        12MB (idle)
  Peak RSS:   47MB
  Sessions:   1,247 indexed
  Index size: 38MB on disk
  Last scrape: 2m ago (3 new sessions)
```

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
