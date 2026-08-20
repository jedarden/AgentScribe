# Session Correlation Feasibility - Summary

## Task Completion Status: ✅ COMPLETE

**Bead:** agentscr-cafe6687  
**Related:** bf-3qr5 (Examine aider analytics JSONL format)  
**Task:** Verify session correlation feasibility between aider analytics events and AgentScribe chat sessions

## Conclusion

**Correlation is NOT feasible.** Aider analytics events cannot be reliably correlated to AgentScribe chat sessions.

## Key Findings

### 1. Companion Index Mechanism Review

The companion index system (`src/scraper/companion.rs`) works by:
- Loading JSONL files with per-session metadata
- Keying entries by `thread_id` or `session_id` fields
- Providing direct lookup: session_id → metadata

### 2. Session ID Comparison

**Aider Analytics Structure:**
```json
{
  "event": "message_send",
  "properties": {"main_model": "gpt-4", "total_tokens": 1234},
  "user_id": "uuid-string-for-user",
  "time": 1710492000
}
```

**AgentScribe Session Detection:**
- Uses delimiter-based detection: `^# aider chat started at `
- Generates session IDs: `{filename}-{session_num}`
- Example: `.aider.chat.history.md-0`, `.aider.chat.history.md-1`

### 3. Correlation Methods Evaluated

**Method 1: Direct Session Key Lookup** ❌
- **Problem:** Analytics has `user_id` (global across all sessions), not per-session IDs
- **Result:** Cannot distinguish between sessions

**Method 2: Timestamp Overlap Matching** ❌
- **Problems:**
  - Timezone ambiguity: delimiter timestamps lack timezone specification
  - No 1:1 mapping: multiple analytics events per session
  - Format inconsistency: human-readable vs Unix epoch
  - Session boundary ambiguity: no clear way to map event sequences to delimiter ranges

**Method 3: Hybrid Approach (User ID + Time Window)** ❌
- **Problem:** User ID adds no value (same for all sessions), time windows still ambiguous
- **Result:** Cannot reduce ambiguity

## Root Cause

**Structural incompatibility:**
- Chat history: session-based (one record per session)
- Analytics: event-based (N events per session, no session grouping)

**No shared session identifier exists.**

## Recommendation

**Do NOT wire aider analytics as a companion index.** The data structure is fundamentally incompatible with the companion mechanism's session-based keying requirements.

This decision is already documented in `plugins/aider.toml`:
```toml
# Companion index decision (bf-xp9w):
# Aider analytics JSONL (~/.aider.analytics.jsonl) is NOT wired as a companion_index
# because it lacks session-level identifiers needed for correlation.
```

## Documentation

Full analysis available at:
- `/home/coding/AgentScribe/notes/agentscr-cafe6687-session-correlation-analysis.md`
- `/home/coding/AgentScribe/notes/bf-xp9w.md`

## Status

✅ Analysis complete  
✅ Documentation updated  
✅ Tests passing  
✅ Bead ready to close

---

**Completed:** 2026-08-20  
**Acceptance criteria met:**
- ✅ Correlation method documented (not feasible)
- ✅ Root cause identified (no shared session identifier)
- ✅ Structural incompatibility documented
