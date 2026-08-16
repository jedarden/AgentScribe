# Session Correlation Feasibility Analysis

**Task:** Verify session correlation feasibility between aider analytics events and AgentScribe chat sessions  
**Bead:** agentscr-cafe6687  
**Related:** bf-xp9w (aider analytics companion evaluation)

## Executive Summary

**Correlation is NOT feasible.** Aider analytics events cannot be reliably correlated to AgentScribe chat sessions due to fundamental structural incompatibilities:

1. **No shared session identifier** - Analytics lacks per-session IDs
2. **Persistent user ID only** - `user_id` is global across all sessions, not session-specific  
3. **Unreliable timestamp matching** - Timezone ambiguity and format differences prevent precise correlation

---

## Companion Index Mechanism

### How It Works

AgentScribe's companion index system (`src/scraper/companion.rs`) is designed to correlate companion metadata files with main session logs. It:

- Loads JSONL files containing per-session metadata
- Keys entries by `thread_id` or `session_id` fields
- Returns metadata for a given session ID via `CompanionIndex::get(session_id)`

**Expected structure:**
```json
{"thread_id": "abc123", "model": "gpt-4", "cwd": "/home/user/project"}
{"thread_id": "def456", "model": "gpt-3.5-turbo", "cwd": "/home/user/other"}
```

### Requirements

The companion index mechanism requires:
- **Per-session keys:** Each entry must have a `thread_id` or `session_id` field
- **1:1 mapping:** One metadata record per session
- **Direct lookup:** Session ID → metadata via hash map lookup

---

## Aider Analytics Structure

### Format

From `aider/analytics.py`, each analytics event has:

```json
{
  "event": "message_send",
  "properties": {
    "main_model": "gpt-4",
    "weak_model": "gpt-3.5-turbo",
    "total_tokens": 1234,
    "total_cost": 0.05
  },
  "user_id": "uuid-string-for-user",
  "time": 1710492000
}
```

**Key fields:**
- `user_id`: Persistent UUID4 identifying the user across ALL sessions
- `time`: Unix timestamp (seconds since epoch, UTC)
- `properties`: Event metadata (model, tokens, cost)

### What's Missing

**No session identifier exists.** The analytics file contains:
- `user_id`: Global user identifier (same value in every event)
- `time`: Event timestamp (not a session ID)

There is NO field that uniquely identifies a session or can be used as a foreign key to chat history.

---

## AgentScribe Aider Session Detection

### Delimiter-Based Detection

The Aider plugin uses delimiter-based session detection (`plugins/aider.toml`):

```toml
[source.session_detection]
method = "delimiter"
delimiter_pattern = "^# aider chat started at "
```

### Session ID Generation

From `src/parser/markdown.rs::detect_sessions()`:

1. Split file by delimiter lines: `# aider chat started at 2024-03-15 10:00:00`
2. Generate session IDs: `{filename}-{session_num}`
   - Example: `.aider.chat.history.md-0`, `.aider.chat.history.md-1`
3. Extract timestamp from delimiter (human-readable format)

### Session Metadata

**Delimiter format:**
```
# aider chat started at 2024-03-15 10:00:00
```

**Problems:**
- No timezone specified (local time? UTC?)
- Human-readable format (not easily parsed)
- No session ID or thread ID
- Timestamp represents session start, not individual events

---

## Correlation Attempts

### 1. Direct Session Key Lookup

**Attempt:** Use `user_id` as session identifier

**Problem:** `user_id` is persistent across ALL sessions. Every analytics event from the same user has the same `user_id`. There is no per-session granularity.

**Result:** ❌ NOT FEASIBLE - Cannot distinguish between sessions

---

### 2. Timestamp Overlap Matching

**Attempt:** Correlate delimiter timestamps with analytics event timestamps

**Problems:**

1. **Timezone ambiguity**
   - Delimiter: `2024-03-15 10:00:00` (no timezone)
   - Analytics: `1710492000` (UTC Unix timestamp)
   - Cannot reliably match without knowing the delimiter's timezone

2. **No 1:1 mapping**
   - One delimiter = session start
   - Multiple analytics events per session (each message is an event)
   - Cannot determine which delimiter corresponds to which event sequence

3. **Timestamp format inconsistency**
   - Delimiter timestamps are human-readable and potentially inconsistent
   - Analytics timestamps are precise Unix epochs
   - Parsing delimiter timestamps is unreliable

4. **Session boundary ambiguity**
   - Sessions detected by delimiter position in file
   - Analytics events have independent timestamps
   - No clear way to map event sequences to delimiter ranges

**Result:** ❌ NOT FEASIBLE - Too much ambiguity for reliable correlation

---

### 3. Hybrid Approach (User ID + Time Window)

**Attempt:** Combine `user_id` with approximate time windows

**Problems:**

1. **User ID adds no value** - Same user ID for all sessions from same user
2. **Time windows are ambiguous** - Same timezone problems as timestamp overlap
3. **Multiple sessions could overlap** - Sessions can be concurrent or overlapping in time

**Result:** ❌ NOT FEASIBLE - User ID doesn't reduce ambiguity

---

## Structural Incompatibility Summary

| Aspect | Aider Chat History | Aider Analytics | Compatible? |
|--------|-------------------|-----------------|--------------|
| **Session identifier** | Delimiter position | None | ❌ NO |
| **User identifier** | None | `user_id` (global) | ❌ NO (different scopes) |
| **Timestamp format** | Human-readable (`YYYY-MM-DD HH:MM:SS`) | Unix epoch (UTC) | ⚠️ AMBIGUOUS |
| **Timezone** | Not specified | UTC | ❌ NO |
| **Granularity** | Per session | Per event | ❌ NO |
| **Count** | 1 record per session | N events per session | ❌ NO |

---

## Conclusion

### Why Correlation Is Not Feasible

1. **No shared session identifier:** Analytics lacks a field that can be used as a foreign key to chat sessions
2. **User ID is global, not per-session:** `user_id` identifies the user across all sessions, not individual sessions
3. **Timestamp matching is unreliable:** Timezone ambiguity, format differences, and lack of 1:1 mapping prevent precise correlation
4. **Structural mismatch:** Chat history is session-based; analytics is event-based with no session grouping

### Recommendation

**Do NOT wire aider analytics as a companion index.** The data structure is fundamentally incompatible with the companion mechanism's session-based keying requirements.

The documentation in `plugins/aider.toml` correctly identifies this incompatibility:

```toml
# Companion index decision (bf-xp9w):
# Aider analytics JSONL (~/.aider.analytics.jsonl) is NOT wired as a companion_index
# because it lacks session-level identifiers needed for correlation.
```

### Alternative Approaches

If analytics metadata (model, tokens, cost) is needed for aider sessions:

1. **Add session IDs to analytics:** Modify aider to emit a session identifier in analytics events
2. **Separate metadata file:** Create a companion file that explicitly maps session IDs to metadata
3. **Post-scrape enrichment:** Use project, timestamp heuristics, or user input to backfill metadata

---

## Related Documentation

- **Full analysis:** `notes/bf-xp9w.md` - aider analytics companion evaluation
- **Plugin config:** `plugins/aider.toml` - Aider plugin with companion index decision
- **Companion implementation:** `src/scraper/companion.rs` - Companion index mechanism
- **Session detection:** `src/parser/markdown.rs` - Delimiter-based session detection

---

**Status:** ✅ Analysis complete - Correlation confirmed as NOT feasible  
**Next steps:** None - This bead can be closed with documented infeasibility
