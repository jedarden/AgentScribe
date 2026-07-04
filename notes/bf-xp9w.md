# bf-xp9w: Aider Analytics Companion Evaluation

## Task
Evaluate whether aider's `~/.aider.analytics.jsonl` can be consumed via the existing companion_index mechanism to backfill model/token metadata on aider sessions.

## Analytics Format

### File Location
- `~/.aider.analytics.jsonl` (when `--analytics-log` is used)

### Schema (from sample data)
Each JSONL line contains:
```json
{
  "event": "message_send|cli session|launched|exit|gui session|command_*|...",
  "properties": {
    "main_model": "gpt-5.4",
    "weak_model": "gpt-5.4-nano",
    "editor_model": "gpt-5.4",
    "edit_format": "diff",
    "prompt_tokens": 49633,
    "completion_tokens": 163,
    "total_tokens": 49796,
    "cost": 0.1265,
    "total_cost": 1.5067
  },
  "user_id": "c42c4e6b-f054-44d7-ae1f-6726cc41da88",
  "time": 1777055796
}
```

### Available Metadata Fields
- **Model**: `main_model`, `weak_model`, `editor_model`
- **Tokens**: `prompt_tokens`, `completion_tokens`, `total_tokens`
- **Costs**: `cost`, `total_cost`
- **Timestamp**: `time` (Unix timestamp)
- **User**: `user_id` (anonymous UUID)

## Correlation Analysis

### Companion Mechanism Requirements
The companion_index mechanism (src/scraper/companion.rs) requires a JSONL file where each line has a `thread_id` or `session_id` field that maps to the session IDs generated during parsing.

### Chat Session Detection
The aider plugin (plugins/aider.toml) uses:
- **Session detection**: `delimiter` method with pattern `^# aider chat started at `
- **Session IDs**: Generated from delimiter matches in `.aider.chat.history.md` files
- **No timestamps in chat history**: The markdown files don't contain reliable timestamp information

### Why Correlation Fails

1. **No shared session key**: Analytics JSONL has no `thread_id` or `session_id` field
   - Only `user_id` exists, which is a per-user anonymous UUID, not per-session
   - The companion mechanism cannot key analytics entries to chat sessions

2. **Timestamp-based matching is unreliable**:
   - Chat history markdown files lack timestamps
   - Even if timestamps existed, time windows are ambiguous
   - Multiple sessions could overlap in the same time period

3. **Session reconstruction is complex and fragile**:
   - Sessions would need to be reconstructed from sequences of `launched` → `cli session` → `message_send*` → `exit` events
   - No deterministic way to assign a unique ID to the reconstructed session
   - Edge cases (crashes, incomplete sessions, GUI sessions) complicate reconstruction

## Decision

**DO NOT wire aider analytics as a companion_index.**

### Rationale
The companion_index mechanism is designed for direct ID-based lookups (thread_id → metadata). Aider analytics lacks the necessary session identification field, and alternative correlation methods (timestamp matching, session reconstruction) are unreliable and complex.

### Metadata Available from Other Sources
- Model information: Can be extracted from chat content when users switch models (`/model` commands, assistant responses mentioning model switches)
- Token counts: Not reliably available without analytics correlation
- Project context: Available via `project_detection: parent_dir` in the plugin

## Future Alternatives

If model/token metadata is critical for aider sessions, consider:
1. **Scraper-side enrichment**: Parse model mentions from chat content
2. **Custom indexer**: Build a custom analytics indexer that generates synthetic session IDs from event sequences and timestamps (complex, fragile)
3. **Await upstream changes**: Request aider to add session IDs to analytics events

## References
- Sample analytics: https://github.com/aider-ai/aider/blob/main/aider/website/assets/sample-analytics.jsonl
- Analytics documentation: https://aider.chat/docs/more/analytics.html
- Companion mechanism: src/scraper/companion.rs
- Aider plugin: plugins/aider.toml
