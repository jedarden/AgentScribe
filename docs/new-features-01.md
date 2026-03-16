# AgentScribe — Future Features (Post-MVP)

These ten features transform AgentScribe from a passive archive into an active intelligence layer. Each is designed to be high-impact while staying implementable on top of the existing scrape→normalize→index pipeline.

---

## 1. Error Fingerprinting + Solution Index

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

## 2. Anti-Pattern Library

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

## 3. Code Artifact Extraction

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

## 4. Solution Extraction

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

## 5. Git Blame Bridge

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

## 6. Context Budget Packing

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

## 7. Recurring Problem Detection

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

## 8. File Knowledge Map

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

## 9. Auto-Generated Project Rules

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

## 10. Agent Effectiveness Analytics

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
