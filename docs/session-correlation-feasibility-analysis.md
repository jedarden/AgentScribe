# Session Correlation Feasibility Analysis

**Bead:** agentscr-cafe6687  
**Date:** 2026-08-15  
**Task:** Verify session correlation feasibility between aider analytics and AgentScribe chat sessions

## Executive Summary

**Result: NOT FEASIBLE** — aider analytics events cannot be reliably correlated to AgentScribe chat sessions.

The correlation is not feasible due to three fundamental issues:

1. **No shared session identifier** — analytics data lacks per-session IDs
2. **Ambiguous timestamp matching** — timezone ambiguity and format inconsistencies
3. **Structural incompatibility** — analytics doesn't meet companion index requirements

## Analysis

### 1. Companion Index Mechanism

The companion_index mechanism (`src/scraper/companion.rs`) requires:

- **Key fields:** `thread_id` or `session_id` for indexing entries
- **Structure:** JSONL file with per-session metadata records
- **Example entry:**
  ```json
  {
    "thread_id": "abc123",
    "model": "gpt-4",
    "cwd": "/home/user/project"
  }
  ```

### 2. AgentScribe Aider Session IDs

**Generation method:** Delimiter-based detection

**Session ID format:** `aider/<project_hash>/<timestamp>`

- `project_hash`: First 8 chars of SHA-256 of parent directory's absolute path
- `timestamp`: Delimiter datetime formatted as `YYYYMMDD-HHMMSS`
- **Example:** `aider/a1b2c3d4/20260316-104200`

**Delimiter pattern:**
```
# aider chat started at 2024-03-15 10:00:00
```

**Properties:**
- Deterministic (re-scraping produces same ID)
- Human-readable
- Collision-resistant

### 3. Aider Analytics Format

**Source:** `~/.aider.analytics.jsonl`

**Event structure:**
```json
{
  "event": "message_send",
  "properties": {
    "main_model": "gpt-4",
    "weak_model": "gpt-3.5-turbo",
    "editor_model": "gpt-4",
    "total_tokens": 1234,
    "total_cost": 0.05
  },
  "user_id": "uuid-string-for-user",
  "time": 1710492000
}
```

**Key fields:**
- `event`: Event name ("message_send", "edit", etc.)
- `properties`: Model names, tokens, costs
- `user_id`: **Persistent UUID4** — same across ALL sessions for a user
- `time`: Unix timestamp (seconds since epoch, UTC)

### 4. Correlation Barriers

#### Problem 1: No Session Identifier

**Analytics provides:**
- `user_id`: Persistent user identifier (not session-specific)
- `time`: Event timestamp (not session ID)

**Companion index requires:**
- `thread_id` or `session_id` field
- Per-session metadata records

**Result:** ❌ Analytics data does not meet the structural requirements for companion index correlation.

#### Problem 2: Timestamp Ambiguity

Even attempting timestamp-based correlation fails due to:

1. **Timezone ambiguity:**
   - Chat delimiter: `2024-03-15 10:00:00` (no timezone specified)
   - Analytics timestamp: `1710492000` (UTC Unix epoch)
   - Cannot reliably match without knowing delimiter timezone

2. **Timestamp mismatch:**
   - Delimiter timestamp: When session started
   - Analytics events: Distributed throughout session
   - No clear anchor point for correlation

3. **No 1:1 mapping:**
   - Multiple analytics events occur within a single session
   - Multiple sessions could potentially overlap or have similar timestamps
   - Ambiguous which analytics events belong to which session

4. **Format inconsistency:**
   - Delimiter format is human-readable and potentially inconsistent
   - Reliable parsing and conversion to Unix timestamps is problematic

### 5. Timestamp Overlap Analysis

**Hypothetical approach:** Match analytics events to sessions by time proximity

**Issues:**
- No unique matching window — sessions can overlap
- Timezone ambiguity makes matching unreliable
- Multiple events per session creates ambiguity
- No guaranteed temporal ordering between files

**Conclusion:** ❌ Timestamp overlap is not a viable fallback method.

## Conclusion

**Correlation is NOT feasible.** The aider analytics JSONL file cannot be reliably consumed as a companion index because:

1. **No session identifier:** Analytics events lack a `session_id` or `thread_id` field that can be correlated to chat history sessions
2. **Persistent user ID only:** The `user_id` field identifies the user across all sessions, not individual sessions
3. **Ambiguous timestamp matching:** Time-based correlation is unreliable due to timezone ambiguity, format differences, and lack of 1:1 mapping

**Recommendation:** Do NOT wire aider analytics as a companion index. The data structure is fundamentally incompatible with the companion mechanism's session-based keying requirements.

## Sources

- Companion index implementation: `src/scraper/companion.rs`
- Aider plugin configuration: `plugins/aider.toml`
- Session detection: `src/parser/markdown.rs`
- Aider analytics evaluation: `notes/bf-xp9w.md`
- AgentScribe architecture: `docs/plan.md`
- Aider analytics documentation: https://aider.chat/docs/more/analytics.html
- Aider source: https://github.com/Aider-AI/aider/blob/main/aider/analytics.py
