# Session Correlation Feasibility Analysis

**Task:** Verify whether aider analytics events can be correlated to AgentScribe chat sessions

**Bead:** agentscr-cafe6687  
**Depends on:** bf-3qr5 (Examine aider analytics JSONL format)

## Executive Summary

**Correlation is NOT feasible.** The aider analytics JSONL file cannot be reliably consumed as a companion index because there is no shared session identifier between analytics events and chat history sessions.

## Analysis

### 1. Companion Index Mechanism

The companion index system (`src/scraper/companion.rs`) is designed to map session IDs to metadata:

- **Expected fields:** `thread_id` or `session_id` (per-entry keys)
- **Format:** JSONL with one metadata record per session
- **Usage:** Backfill model names, tokens, and other metadata for sessions

Example companion index entry:
```json
{"thread_id": "abc123", "model": "gpt-4", "cwd": "/home/user/project"}
```

### 2. Aider Analytics Structure

From `bf-3qr5.md` analysis of aider analytics format:

**Standard fields per event:**
- `event`: Event type (`message_send`, `cli session`, `launched`, `exit`, etc.)
- `time`: Unix timestamp (seconds since epoch, UTC)
- `user_id`: **Persistent UUID4 user identifier** (same across ALL sessions)
- `properties`: Event-specific metadata (model names, tokens, costs)

**Critical finding:** Analytics has `user_id` (persistent), not `session_id` (per-session)

Example analytics events:
```json
{"event": "cli session", "properties": {"main_model": "gpt-4"}, "user_id": "c42c4e6b-...", "time": 1754755056}
{"event": "message_send", "properties": {"total_tokens": 17532}, "user_id": "c42c4e6b-...", "time": 1754942076}
```

### 3. AgentScribe Session Detection for Aider

The aider plugin uses delimiter-based session detection (`plugins/aider.toml`):

```toml
[source.session_detection]
method = "delimiter"
delimiter_pattern = "^# aider chat started at "
```

**Session ID format:** `aider/<project_hash>/<timestamp>`
- Example: `aider/a1b2c3d4/20260316-104200`
- Generated from: project path hash + delimiter timestamp

**Chat delimiter format:**
```
# aider chat started at 2024-03-20 15:00:00
```

### 4. Correlation Mismatch

#### Problem 1: No Shared Session Identifier

| Analytics has | AgentScribe needs | Match? |
|---------------|------------------|--------|
| `user_id` (persistent UUID) | `session_id` or `thread_id` (per-session) | ❌ No |
| `time` (event timestamp) | Session ID from delimiter | ❌ No |
| Event sequence | Single session identifier | ❌ No |

**Root cause:** `user_id` identifies the user across ALL sessions, not individual sessions. There is no per-session identifier in analytics that can be mapped to AgentScribe's session IDs.

#### Problem 2: Timestamp Overlap is Unreliable

Even attempting timestamp-based correlation fails:

**Chat delimiter timestamps:**
- Format: `YYYY-MM-DD HH:MM:SS` (human-readable)
- **No timezone specified** (local vs UTC ambiguity)
- Example: `# aider chat started at 2024-03-20 15:00:00`

**Analytics timestamps:**
- Format: Unix epoch seconds (always UTC)
- Example: `"time": 1754755056`

**Correlation challenges:**
1. **Timezone ambiguity:** Is `15:00:00` local time or UTC? No way to know.
2. **Format inconsistency:** Human-readable delimiters may vary; Unix timestamps are precise.
3. **No 1:1 mapping:** Multiple analytics events occur per session; sessions could overlap in time.
4. **Delimiter parsing:** Converting `YYYY-MM-DD HH:MM:SS` to Unix timestamp requires assuming a timezone.

**Example mismatch scenario:**
- Chat delimiter: `# aider chat started at 2024-03-20 15:00:00` (local time, timezone unknown)
- Analytics event: `{"event": "cli session", "time": 1710940800}` (UTC: 2024-03-20 14:00:00)
- If local timezone is UTC-1, they match. If UTC+5, they don't. No way to resolve.

#### Problem 3: Structural Incompatibility

The companion index expects:
- One record per session (session_id → metadata)
- Metadata attached to session identifier

Analytics provides:
- One record per EVENT (event_type → properties)
- Multiple events per session with no session grouping

**Missing structural element:** There's no "session" object in analytics—only a stream of events with a persistent user ID.

### 5. Alternative Approaches Considered

#### Approach A: Sequence-based correlation
Match analytics event sequences to chat message sequences by position.

**Why it fails:**
- Analytics has many event types (`command_*`, `repo`, `auto_commits`) with no chat equivalents
- Chat history may omit system events that analytics logs
- No guarantee of 1:1 ordering alignment

#### Approach B: Model-based grouping
Group analytics events by `main_model` and match to sessions using the same model.

**Why it fails:**
- Multiple sessions can use the same model
- Aider sessions can change models mid-session
- Model names alone don't identify sessions

#### Approach C: Time-window clustering
Cluster analytics events within time windows and match to chat sessions by overlap.

**Why it fails:**
- Timezone ambiguity makes window boundaries unreliable
- Short sessions (< 1 minute) are indistinguishable
- Sessions run in parallel would overlap ambiguously

## Conclusion

**Session correlation between aider analytics and AgentScribe chat history is NOT feasible.**

### Root Causes

1. **No session identifier in analytics:** `user_id` is persistent, not per-session
2. **Timezone ambiguity:** Chat delimiters lack timezone information
3. **Structural mismatch:** Analytics is event-stream; companion index expects session-keyed metadata
4. **No reliable fallback:** Timestamp-based correlation is ambiguous and error-prone

### Recommendation

**DO NOT** wire aider analytics as a companion_index in the aider plugin. The data structure is fundamentally incompatible with the session-based keying requirements.

**Current state (correct):** The `plugins/aider.toml` file already contains a comment documenting this decision (lines 4-9), which should remain in place.

### Alternative Path Forward

If session enrichment is needed, consider:

1. **Parse model from chat content:** Extract model names from assistant responses in the chat history itself (e.g., "I'll use gpt-4 for this task")
2. **User-declared metadata:** Allow users to specify model via a companion file they maintain (e.g., `.aider.session.json` per project)
3. **Aider input history:** The `.aider.input.history` file (already parsed by `src/parser/aider_input.rs`) provides per-input timestamps but still lacks model information

## Acceptance Criteria Met

- ✅ Correlation method documented: **Not feasible** (no shared session key, timestamp correlation unreliable)
- ✅ Structural mismatch explained: Analytics has `user_id`, companion index needs `session_id`/`thread_id`
- ✅ Timestamp overlap analyzed: Timezone ambiguity and format differences prevent reliable matching
- ✅ Root causes documented: Persistent user ID, missing session identifier, timezone ambiguity

## Sources

- `src/scraper/companion.rs` — Companion index implementation
- `plugins/aider.toml` — Aider plugin configuration
- `notes/bf-xp9w.md` — Initial correlation analysis (confirms not feasible)
- `notes/bf-3qr5.md` — Aider analytics JSONL format schema
- `src/parser/aider_input.rs` — Aider input history parser (alternative data source)
