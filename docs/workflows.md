# Example Workflows

Real-world scenarios showing how to use AgentScribe effectively.

---

## 1. First-Time Setup with Claude Code

You're a Claude Code user who wants to start archiving and searching past conversations.

```bash
# Initialize AgentScribe
agentscribe config init

# Verify the Claude Code plugin is installed
agentscribe plugins list

# Check which log files were found
agentscribe plugins show claude-code

# Scrape all Claude Code sessions
agentscribe scrape --plugin claude-code

# Check the status
agentscribe status

# Try a search
agentscribe search "refactor" --since 30d
```

**What happens:** AgentScribe discovers all `~/.claude/projects/*/*.jsonl` files, parses them into normalized sessions, runs the enrichment pipeline (outcome detection, error fingerprinting, solution extraction), and builds a Tantivy full-text index.

---

## 2. Cross-Agent Search Across Multiple Tools

You use Claude Code for complex tasks and Aider for quick edits. You want to search across both.

```bash
# Scrape both agents
agentscribe scrape --plugin claude-code
agentscribe scrape --plugin aider

# Search for a specific error across both
agentscribe search "postgres connection refused" --since 14d

# Compare how each agent handled database tasks
agentscribe search "database migration" --agent claude-code
agentscribe search "database migration" --agent aider

# Find sessions where you got a working solution
agentscribe search "auth token refresh" --outcome success --json
```

**Tip:** Use `--agent` to compare how different agents approach the same problem. The JSON output includes the `source_agent` field for filtering in downstream tools.

---

## 3. Finding and Fixing Recurring Errors

You keep hitting the same errors across different projects. Use recurring problem detection to identify patterns and find past fixes.

```bash
# Detect recurring problems over the last 30 days
agentscribe recurring --since 30d

# Increase threshold to focus on the most persistent issues
agentscribe recurring --since 90d --threshold 5

# Once you find a problem, search for how it was fixed
agentscribe search --error "ConnectionError:Connection refused"

# Look at a specific fix session
agentscribe search --session claude-code/abc123

# Search for alternative approaches to the same problem
agentscribe search "connection pool" --solution-only --since 60d
```

**What happens:** `recurring` groups sessions by error fingerprint, counts occurrences, and links to the last session that resolved the problem. This lets you quickly find proven fixes for errors that keep coming back.

---

## 4. Auto-Generating Project Rules from Session History

You want your coding agents to learn from past mistakes and follow established conventions.

```bash
# Generate CLAUDE.md rules from past sessions
agentscribe rules ~/my-project --format claude

# This writes rules to ~/my-project/CLAUDE.md
# Examples of what it extracts:
#   - "Always use uuid v7, not v4, for new database columns"
#   - "API endpoints use kebab-case, response fields use snake_case"
#   - "Don't modify applied migration files"
```

For Cursor projects:

```bash
agentscribe rules ~/my-project --format cursor
# Writes to ~/my-project/.cursorrules
```

For Aider projects:

```bash
agentscribe rules ~/my-project --format aider
# Writes to ~/my-project/.aider.conf.yml
```

**What happens:** The `rules` command analyzes all past sessions for the project and extracts:
- **Corrections** — Things that were wrong and had to be fixed
- **Conventions** — Patterns that appear consistently across sessions
- **Context** — Domain knowledge discovered during sessions
- **Warnings** — Things that caused problems when changed

---

## 5. Weekly Review with Analytics and Digests

You want a regular overview of agent activity, costs, and effectiveness.

```bash
# Generate a weekly digest
agentscribe digest --since 7d --output ~/reports/weekly-digest.md

# Get detailed analytics
agentscribe analytics --since 7d --json > ~/reports/analytics-weekly.json

# Compare agents
agentscribe analytics --since 30d
```

**What the digest includes:**
- Session counts by agent and project
- Recurring problems detected this week
- Agent comparison (success rates, turns)
- Most-touched files
- New error patterns
- Token usage and cost estimates

**Set up a weekly cron job:**

```bash
# Add to crontab: weekly digest every Monday at 9am
0 9 * * 1 agentscribe digest --since 7d --output ~/reports/weekly-$(date +\%Y-\%m-\%d).md
```

---

## 6. Background Daemon for Automatic Scraping

You don't want to manually run `agentscribe scrape` every time you finish an agent session.

```bash
# Start the daemon
agentscribe daemon start

# Check it's running
agentscribe daemon status

# View logs
agentscribe daemon logs -f

# The daemon watches all plugin source paths
# When new data appears, it waits the debounce period (default 5s)
# then scrapes and indexes automatically
```

**For systemd (recommended on servers):**

```ini
# /etc/systemd/system/agentscribe.service
[Unit]
Description=AgentScribe daemon
After=network.target

[Service]
Type=simple
User=coding
ExecStart=/home/coding/.cargo/bin/agentscribe daemon run
Restart=on-failure
RestartSec=10

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable agentscribe
sudo systemctl start agentscribe
journalctl -u agentscribe -f
```

---

## 7. Context Packing for Agent Prompts

You want to feed relevant past solutions into a new agent session as context.

```bash
# Search with token budget for optimal context packing
agentscribe search "kubernetes deployment rollback" \
  --token-budget 4000 \
  --solution-only \
  --outcome success \
  --since 30d \
  --json

# The greedy knapsack algorithm packs the most relevant results
# within your token budget, prioritizing solutions
```

**In a CLAUDE.md or prompt:**

```markdown
## Relevant Past Sessions
The following past solutions may be helpful for this task:
{paste the JSON output}
```

---

## 8. Adding a New Agent Plugin

A new coding agent called "Devon" stores logs at `~/.devon/sessions/*.jsonl`.

```bash
# Create the plugin file
cat > ~/.agentscribe/plugins/devon.toml << 'EOF'
[plugin]
name = "devon"
version = "1.0"

[source]
paths = ["~/.devon/sessions/*.jsonl"]
format = "jsonl"

[source.session_detection]
method = "one-file-per-session"
session_id_from = "filename"

[parser]
timestamp = "timestamp"
role = "role"
content = "content"

[parser.static]
source_agent = "devon"
EOF

# Validate the plugin
agentscribe plugins validate ~/.agentscribe/plugins/devon.toml

# Dry-run to test
agentscribe scrape --plugin devon --dry-run

# Scrape for real
agentscribe scrape --plugin devon

# Verify
agentscribe status --plugin devon
```

See the [Plugin Authoring Guide](../plugins/BUILDING_PLUGINS.md) for the full specification.
