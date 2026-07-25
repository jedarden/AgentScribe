# Test Analysis: test_multiple_subagents_same_parent

## Location
`tests/parent_session_tests.rs:293-410`

## Test Purpose
Validates that multiple subagent sessions sharing the same parent session are correctly identified, scraped, and processed with proper parent-child relationships.

## Expected Behavior

### Path Structure
The test creates files in the following structure:
```
source/claude-code/
├── parent-shared-123.jsonl              # Parent session
└── parent-shared-123/
    └── subagents/
        ├── agent-000.jsonl             # Subagent 1
        ├── agent-001.jsonl             # Subagent 2
        └── agent-002.jsonl             # Subagent 3
```

### Expected Behavior
1. **Scraping phase**: All 4 sessions (1 parent + 3 subagents) should be discovered and scraped
2. **Session identification**: Subagent sessions should be distinguished by checking if `source_agent == "claude-code-subagent"`
3. **Parent linking**: All subagent sessions should have the same parent_session_id pointing to `parent-shared-123`
4. **Event parsing**: Each subagent session should contain exactly 2 events with correct `source_agent` values

## All Assertions

| Line(s) | Assertion | Description |
|---------|-----------|-------------|
| 342-346 | `result.sessions_scraped == 1 + subagent_count` | Should scrape parent and all subagent sessions (4 total) |
| 388-392 | `subagent_sessions.len() == subagent_count` | Should have all subagent sessions (3) |
| 397-398 | `scraper.read_session(session_path)` succeeds | Each subagent session should be readable |
| 400 | `events.len() == 2` | Each subagent should have 2 events |
| 402-408 | `event.source_agent == "claude-code-subagent"` | All events in subagent sessions should have source_agent = "claude-code-subagent" |

## Key Implementation Details

1. **Subagent detection method**: The test identifies subagent sessions by:
   - Filtering out the parent session (by comparing session ID)
   - Checking if the first event has `source_agent == "claude-code-subagent"`

2. **Session counting**: Subagent sessions are counted after filtering, not during scraping

3. **Parent session ID format**: The parent session is formatted as `claude-code/parent-shared-123`

4. **Debug output**: The test includes extensive `eprintln!` statements for debugging session discovery
