# AgentScribe Reflection Guide

This guide explains how to build reflection tools on top of AgentScribe. Reflection tools analyze agent sessions to extract behavioral patterns, detect recurring issues, and feed insights back into agent configuration.

## Overview

AgentScribe provides a **reflection data pipeline** for external tools:

1. **Capture**: Agent sessions are scraped and stored as JSONL event streams
2. **Enrichment**: Behavioral signals are computed and stored as sidecar files
3. **Export**: `agentscribe reflect sessions --json` exports structured session data
4. **Analysis**: External tools analyze patterns, detect issues, and generate insights
5. **Write-back**: Tools annotate sessions with tags and notes via `agentscribe annotate`

```
┌─────────────┐      ┌──────────────┐      ┌─────────────┐
│  Agent      │      │  AgentScribe │      │  Reflection │
│  Sessions   │ ──►  │  Capture     │ ──►  │  Tool       │
└─────────────┘      └──────────────┘      └─────────────┘
                           │                        │
                           ▼                        ▼
                    ┌──────────────┐      ┌─────────────┐
                    │  Behavioral  │      │  Patterns   │
                    │  Signals     │      │  Detected   │
                    └──────────────┘      └─────────────┘
                                                  │
                                                  ▼
                                          ┌──────────────┐
                                          │  Annotations │
                                          │  (Write-back)│
                                          └──────────────┘
```

## Data Flow

### 1. Session Capture

Agent sessions are automatically captured by the scraper plugins:

```bash
# Run the daemon to continuously capture new sessions
agentscribe daemon start --mcp

# Or manually scrape
agentscribe scrape --plugin claude-code
```

Each session is stored as:
- **Events**: `~/.agentscribe/sessions/<agent>/<session-id>.jsonl`
- **Manifest**: Embedded in index (metadata, outcome, tags)
- **Behavioral Signals**: `~/.agentscribe/sessions/<agent>/<session-id>.behavioral.json`
- **Annotations**: `~/.agentscribe/sessions/<agent>/<session-id>.annotations.json`

### 2. Behavioral Signals

Behavioral signals are automatically computed during scraping and stored as sidecar files:

```json
{
  "tool_call_count": 8,
  "tool_call_counts_by_name": {"Read": 5, "Bash": 2, "Edit": 1},
  "re_read_files": ["/project/CLAUDE.md"],
  "re_read_count": 1,
  "bash_failure_count": 0,
  "multi_edit_files": [],
  "duration_secs": 900,
  "assistant_turn_ratio": 0.4,
  "read_config_files": ["/project/CLAUDE.md"],
  "modified_config_files": [],
  "cwd_switch_count": 0
}
```

These signals power the reflection export without re-parsing events.

### 3. Reflection Export

Export sessions with behavioral metadata for analysis:

```bash
# Export last 30 days of sessions
agentscribe reflect sessions --since 30d --json > sessions.json

# Export only sessions that modified config files
agentscribe reflect sessions --modified-config-only --json > config-changes.json

# Export failed sessions from a specific project
agentscribe reflect sessions --project /home/user/project --outcome failure --json
```

The JSON output is **stable and versioned** - see [Schema Reference](#schema-reference) below.

### 4. Pattern Detection

Your reflection tool analyzes the exported data to detect patterns:

```python
import json

# Load sessions
with open('sessions.json') as f:
    sessions = json.load(f)

# Find sessions with high re-read counts
re_read_heavy = [s for s in sessions if s['re_read_count'] > 3]

# Find sessions that modified CLAUDE.md
config_modifiers = [s for s in sessions if any('CLAUDE.md' in f for f in s['modified_config_files'])]

# Find bash-heavy sessions
bash_heavy = [s for s in sessions if s['tool_call_counts'].get('Bash', 0) > 10]
```

### 5. Annotation Write-back

Tag sessions with detected patterns:

```bash
# Tag a session as leading to a config change
agentscribe annotate claude-code/abc123 --tag led-to-config-change \
  --note "Agent added new rule after encountering this error"

# Tag a session with a repeated mistake pattern
agentscribe annotate aider/def456 --tag repeated-mistake \
  --note "Agent made same error 3 times before resolving" \
  --created-by reflection-tool
```

Annotations survive re-scraping - they're stored in separate sidecar files.

## Polling Strategy

### Recommended Cadence

Poll reflection exports at these intervals based on your analysis needs:

| Analysis Type | Poll Cadence | Command |
|--------------|--------------|---------|
| Real-time monitoring | 1-5 minutes | `agentscribe reflect sessions --since 5m --json` |
| Daily pattern detection | Hourly | `agentscribe reflect sessions --since 24h --json` |
| Weekly reports | Daily | `agentscribe reflect sessions --since 7d --json` |
| Historical analysis | One-time | `agentscribe reflect sessions --json` |

### Efficient Polling

Use the `--since` flag to avoid re-exporting all sessions:

```bash
# Poll for new sessions since last check
LAST_CHECK="2026-03-15T10:00:00Z"
agentscribe reflect sessions --since "$LAST_CHECK" --json

# Or use relative time
agentscribe reflect sessions --since 5m --json
```

For daemon mode, enable the MCP server for real-time querying:

```bash
agentscribe daemon start --mcp
```

Then use MCP tools for in-session queries without polling.

## Config File Correlation

A common reflection pattern is correlating sessions with config file changes (CLAUDE.md, AGENTS.md, etc.).

### Finding Sessions Preceding Config Changes

```python
import json
import subprocess
from datetime import datetime, timedelta

def get_config_change_sessions(project_path, lookback_minutes=30):
    """Find sessions that preceded config file changes."""
    
    # Get sessions that modified config files
    result = subprocess.run([
        'agentscribe', 'reflect', 'sessions',
        '--project', project_path,
        '--modified-config-only',
        '--json'
    ], capture_output=True, text=True)
    
    config_modifiers = json.loads(result.stdout)
    
    # For each config modifier, find prior sessions
    prior_sessions = []
    for session in config_modifiers:
        # Get sessions from the same project in the 30 minutes before
        cutoff = datetime.fromisoformat(session['started']) - timedelta(minutes=lookback_minutes)
        since_str = cutoff.isoformat()
        
        prior_result = subprocess.run([
            'agentscribe', 'reflect', 'sessions',
            '--project', project_path,
            '--since', since_str,
            '--json'
        ], capture_output=True, text=True)
        
        priors = json.loads(prior_result.stdout)
        prior_sessions.extend(priors)
    
    return prior_sessions
```

### Detecting Rule-Addition Patterns

```python
def find_rule_addition_patterns(sessions):
    """Find sessions that typically lead to CLAUDE.md changes."""
    
    patterns = {}
    
    for session in sessions:
        # Check if any annotations indicate config change
        # (requires annotation write-back from another tool)
        # Or check config_changes_after field
        
        if session.get('config_changes_after'):
            for change in session['config_changes_after']:
                if 'CLAUDE.md' in change['file']:
                    key = (
                        session['outcome'],
                        tuple(session['tags']),
                        session.get('model', 'unknown')
                    )
                    patterns[key] = patterns.get(key, 0) + 1
    
    # Sort by frequency
    return sorted(patterns.items(), key=lambda x: x[1], reverse=True)
```

## MCP Tools for In-Session Querying

The AgentScribe daemon exposes MCP tools for real-time session queries without CLI invocation.

### Available MCP Tools

| Tool | Purpose |
|------|---------|
| `agentscribe_search` | Full-text and faceted search across sessions |
| `agentscribe_status` | Plugin list, session counts, daemon state |
| `agentscribe_blame` | File path → sessions that touched it |
| `agentscribe_file` | Chronological session list for a file |

### Using MCP Tools

When the daemon is running with `--mcp`, connect via JSON-RPC 2.0:

```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "agentscribe_search",
    "arguments": {
      "query": "authentication error",
      "project": "/home/user/myproject",
      "since": "7d"
    }
  },
  "id": 1
}
```

See [MCP documentation](https://modelcontextprotocol.io/) for client setup.

## Schema Reference

The reflection export schema is stable and versioned. See [docs/cli-reference.md](cli-reference.md#agentscribe-reflect) for the complete schema reference.

### Key Schemas

#### ReflectSession (CLI output)

```typescript
{
  session_id: string,              // "<agent>/<session-id>"
  agent: string,                   // plugin name
  project: string | null,          // project path
  started: string,                 // ISO 8601
  ended: string | null,            // ISO 8601
  duration_secs: number,          // u64
  outcome: string,                 // "success" | "failure" | "abandoned" | "unknown"
  summary: string | null,
  tags: string[],                  // may be empty
  model: string | null,
  tool_call_counts: Record<string, number>,  // tool name → count
  re_read_count: number,          // u32
  bash_failure_count: number,      // u32
  read_config_files: string[],     // config files read
  modified_config_files: string[], // config files modified
  error_fingerprints: string[],    // error signatures
  anti_patterns: Array<{           // detected anti-patterns
    pattern: string
  }>,
  files_touched: string[],
  config_changes_after: Array<{    // config changes after session
    file: string,
    minutes_after: number
  }> | null
}
```

#### BehavioralSignals (sidecar file)

```typescript
{
  tool_call_count: number,         // u32
  tool_call_counts_by_name: Record<string, number>,
  re_read_files: string[],         // files read >1x
  re_read_count: number,           // u32
  bash_failure_count: number,      // u32
  multi_edit_files: string[],      // files edited >1x
  duration_secs: number,           // u64
  assistant_turn_ratio: number,    // f32, 0.0-1.0
  read_config_files: string[],      // config files read
  modified_config_files: string[], // config files modified
  cwd_switch_count: number         // u32
}
```

#### Annotation (sidecar file)

```typescript
{
  session_id: string,
  annotations: Array<{
    tag: string,
    note: string | null,
    created_at: string,             // ISO 8601
    created_by: string             // "human" | "reflection-tool" | "agentscribe"
  }>
}
```

### Stability Guarantees

**Fields are STABLE**: New fields may be added at any time, but existing fields will never be renamed, removed, or retyped without a major version bump.

## Example: Simple Reflection Script

Here's a complete example of a reflection tool that finds sessions preceding CLAUDE.md changes:

```bash
#!/usr/bin/env bash
# reflect-config-changes.sh
# Find sessions that typically precede CLAUDE.md modifications

set -euo pipefail

PROJECT="${1:-.}"
LOOKBACK_MINUTES="${2:-30}"

echo "Finding sessions in $PROJECT that preceded CLAUDE.md changes (last ${LOOKBACK_MINUTES}m)..."
echo

# Get sessions that modified CLAUDE.md (or other config files)
CONFIG_MODIFIERS=$(agentscribe reflect sessions \
  --project "$PROJECT" \
  --modified-config-only \
  --json)

if [ -z "$CONFIG_MODIFIERS" ] || [ "$CONFIG_MODIFIERS" = "[]" ]; then
    echo "No config-modifying sessions found."
    exit 0
fi

# Extract the most recent config-modifying session
RECENT_CONFIG=$(echo "$CONFIG_MODIFIERS" | jq -r '.[0]')
SESSION_ID=$(echo "$RECENT_CONFIG" | jq -r '.session_id')
STARTED=$(echo "$RECENT_CONFIG" | jq -r '.started')

echo "Most recent config-modifying session: $SESSION_ID"
echo "Started at: $STARTED"
echo

# Find sessions in the lookback window before that
CUTOFF=$(date -d "$STARTED - $LOOKBACK_MINUTES minutes" -Iseconds 2>/dev/null || \
        date -j -v-"${LOOKBACK_MINUTES}M" -f "%Y-%m-%dT%H:%M:%SZ" "$STARTED" +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null)

echo "Looking for sessions since $CUTOFF..."
echo

PRIOR_SESSIONS=$(agentscribe reflect sessions \
  --project "$PROJECT" \
  --since "$CUTOFF" \
  --json)

echo "$PRIOR_SESSIONS" | jq -r '
  .[]
  | "\(.session_id)\t\(.outcome)\t\(.duration_secs)s\t\(.re_read_count) re-reads\t\(.bash_failure_count) failures"
' | column -t -s $'\t'

echo
echo "Summary:"
echo "$PRIOR_SESSIONS" | jq -r '
  length as $total
  | map(.outcome) | group_by(.) | map({outcome: .[0], count: length})
  | "Total sessions: \($total)\nOutcomes: \(map("\(.outcome): \(.count)") | join(", "))"
'
```

Usage:

```bash
./reflect-config-changes.sh /home/user/myproject 30
```

Output:

```
Finding sessions in /home/user/myproject that preceded CLAUDE.md changes (last 30m)...

Most recent config-modifying session: claude-code/abc123
Started at: 2026-03-15T10:45:00Z

Looking for sessions since 2026-03-15T10:15:00Z...

claude-code/abc123    success  900s    1 re-reads    0 failures
aider/def456          failure  300s    0 re-reads    2 failures

Summary:
Total sessions: 2
Outcomes: success: 1, failure: 1
```

## Advanced Patterns

### Detecting Repeated Mistakes

```python
def detect_repeated_mistakes(sessions):
    """Find sessions with similar error patterns that failed."""
    
    from collections import defaultdict
    
    error_clusters = defaultdict(list)
    
    for session in sessions:
        if session['outcome'] != 'failure':
            continue
            
        for fp in session['error_fingerprints']:
            error_clusters[fp].append(session)
    
    # Find errors that occurred multiple times
    repeated = {fp: sessions for fp, sessions in error_clusters.items() if len(sessions) > 1}
    
    return repeated
```

### Finding High-Frequency Tools

```python
def find_tool_frequency_patterns(sessions):
    """Find which tools are most frequently used by outcome."""
    
    from collections import defaultdict
    
    outcome_tools = defaultdict(lambda: defaultdict(int))
    
    for session in sessions:
        outcome = session['outcome']
        for tool, count in session['tool_call_counts'].items():
            outcome_tools[outcome][tool] += count
    
    return outcome_tools
```

### Detecting Config Drift

```python
def detect_config_drift(sessions):
    """Find projects where config files are frequently modified."""
    
    from collections import defaultdict
    
    project_config_changes = defaultdict(int)
    
    for session in sessions:
        if session['modified_config_files']:
            project = session.get('project', 'unknown')
            project_config_changes[project] += len(session['modified_config_files'])
    
    # Sort by frequency
    return sorted(project_config_changes.items(), key=lambda x: x[1], reverse=True)
```

## Best Practices

### 1. Use Efficient Filters

Narrow your export to avoid processing unnecessary data:

```bash
# Good: Specific project and outcome
agentscribe reflect sessions --project "$PROJECT" --outcome failure --json

# Avoid: Exporting all sessions when you only need a subset
agentscribe reflect sessions --json  # Expensive for large datasets
```

### 2. Cache Results

Reflection analysis can be expensive. Cache results:

```bash
# Cache the export
CACHE_FILE="/tmp/reflect-cache-$(date +%s).json"
agentscribe reflect sessions --since 7d --json > "$CACHE_FILE"

# Run multiple analyses on the cached data
python analyze_patterns.py "$CACHE_FILE"
python detect_errors.py "$CACHE_FILE"
python report_outcomes.py "$CACHE_FILE"
```

### 3. Use Annotations for Persistence

Tag detected patterns so they survive re-analysis:

```bash
# Tag sessions with detected patterns
agentscribe annotate "$SESSION_ID" --tag repeated-error \
  --note "Same error occurred 3 times in this session" \
  --created-by reflection-tool
```

Later, filter by annotations:

```bash
agentscribe reflect sessions --tags repeated-error --json
```

### 4. Monitor System Health

Track behavioral signals for system health monitoring:

```bash
# Find sessions with unusual behavior patterns
agentscribe reflect sessions --json | jq '
  map(select(
    .bash_failure_count > 5 or
    .re_read_count > 10 or
    .duration_secs > 3600
  ))
'
```

### 5. Respect Privacy

Be mindful of sensitive data in sessions:

- Sanitize file paths before external logging
- Redact sensitive content from summaries
- Respect `.gitignore` and project visibility

## Troubleshooting

### No Sessions Returned

If `agentscribe reflect sessions` returns empty results:

```bash
# Check that sessions exist
agentscribe status

# Check scrape status
agentscribe status --plugin claude-code

# Try without filters
agentscribe reflect sessions --json | jq 'length'
```

### Sidecar Files Missing

Behavioral signals are stored as sidecar files. If they're missing:

```bash
# Re-scrape with enrichment
agentscribe scrape --plugin claude-code

# Or rebuild from events (slower)
agentscribe index rebuild
```

### MCP Tools Not Responding

If MCP tools fail:

```bash
# Check daemon status
agentscribe daemon status

# Check if MCP is enabled
agentscribe daemon status | grep MCP

# Restart daemon with MCP
agentscribe daemon stop
agentscribe daemon start --mcp
```

## Further Reading

- [CLI Reference](cli-reference.md#agentscribe-reflect) - Complete reflection API schema
- [Configuration](configuration.md) - Daemon setup and MCP configuration
- [Workflows](workflows.md) - Integration patterns
