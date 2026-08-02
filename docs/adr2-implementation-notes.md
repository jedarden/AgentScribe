# ADR-2 Implementation Notes

**Bead:** bf-1pkfp  
**Date:** 2026-08-01  
**Status:** Complete

## Overview

This document provides implementation details and rationale for ADR-2: "Stop storing full session content a second time; chunk-level vectors off by default."

## Problem Statement

Prior to ADR-2, AgentScribe's Tantivy index stored the `content` field (marked `TEXT | STORED`), which duplicated the full conversation text already present in `sessions/<plugin>/<id>.jsonl`. This caused the index to grow to **76GB** against a **~385MB** normalized corpus.

### Root Cause Analysis

1. **Stored Content Duplication:** The `content` field held the full conversation (role-prefixed, capped at 500KB/session) for every session document. This text was already durably stored in the normalized JSONL files, resulting in ~200x storage overhead.

2. **Chunk-Level Vector Default:** `VectorConfig::index_chunks` defaulted to `true`, building and storing chunk-level embeddings (overlapping 512-token windows) alongside session-level embeddings. At 500K sessions, this added ~1.15GB vs ~192MB for session-level alone — a **6x disk cost increase**.

3. **Design Drift:** The code had drifted from the documented intent in `docs/plan.md`, which already recommended `index_chunks = false` as the default.

## Solution

### 1. Content Field: Indexed, Not Stored

**Schema Change:** `src/index.rs::build_schema()`
- Changed from: `TEXT | STORED`
- Changed to: `TEXT` (indexed only)

**Rationale:**
- The normalized JSONL sessions are the source of truth (see `docs/plan.md` Data Directory Layout)
- Storing the same text twice provides no redundancy benefit, only storage overhead
- Search only needs the text for snippets and more-like-this, both low-frequency operations

### 2. Shared Reconstruction Fallback

**New Function:** `src/scraper/mod.rs::load_session_content()`

Re-reads and re-normalizes a session's JSONL file to reproduce the content string that used to be read from the stored field. This provides a shared fallback for all consumers that need the raw text:

- `src/search.rs`: Snippet extraction, more-like-this term extraction
- `src/analytics.rs`: Cost estimation, problem-type classification
- Transitively: `digest.rs`, `pulse_report.rs`, `file_knowledge.rs`

**Performance Impact:**
- **Search operations:** One JSONL read per top-K result (bounded, acceptable)
- **Full-corpus scans:** One JSONL read per session (consistent with `gc --dry-run` pattern)
- **Graceful degradation:** Returns `None` if session file is missing, allowing empty string fallbacks

### 3. Chunk-Level Vectors: Opt-in by Default

**Config Change:** `src/config.rs::VectorConfig`
- Changed `index_chunks` default from `true` to `false`
- Session-level embeddings remain enabled by default (`index_sessions: true`)

**Rationale:**
- Session-level embeddings answer the primary use case: "find a past session that solved a similar problem"
- Chunk-level retrieval ("find the exact moment within a session") is a secondary capability
- 6x disk cost reduction for most users who don't need chunk-level precision
- Users can opt-in via `config.toml`: `index_chunks = true`

## Implementation Details

### Code Comments

All code changes reference ADR-2 with detailed comments explaining:

1. **Root cause:** Why the change was necessary (storage duplication, index growth)
2. **The fix:** What was changed and how it solves the problem
3. **Performance impact:** What costs were introduced (JSONL re-reads) and why they're acceptable
4. **Future maintenance:** Patterns to follow when adding new consumers

### Two-Tier Fallback Pattern

Consumers that need session text follow this pattern:

```rust
// Try stored field first (for code-artifact docs where code_content is stored)
let content_text = doc
    .get_first(fields.content)
    .and_then(|v| v.as_str())
    .map(|s| s.to_string())
    .or_else(|| {
        doc.get_first(fields.code_content)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    })
    .or_else(|| load_session_content(data_dir, session_id));
```

This ensures:
- Code artifacts use the stored `code_content` field (fast)
- Session documents re-read JSONL (necessary, acceptable cost)
- Graceful degradation if session file is missing

## Testing

No changes to existing tests were required. The fallback is transparent to callers:

- Search operations return the same snippets (from JSONL instead of doc store)
- Analytics reports produce identical results (cost estimation, problem classification)
- More-like-this queries work identically (same term extraction)

## Migration Notes

### For Existing Deployments

**Reclaiming disk space:** A schema-only change does not repack existing Tantivy segments. To reclaim the disk space from an already-populated index:

```bash
agentscribe index rebuild
```

This is a pure-local, zero-API-cost operation. The index is rebuilt from the durable JSONL sessions.

### Configuration Changes

If you had `index_chunks = true` explicitly set in `config.toml`, that setting is preserved. If you relied on the default, it's now `false` and chunk-level embeddings won't be built. To restore:

```toml
[vector]
index_chunks = true
```

## References

- **Main documentation:** `docs/plan.md` — ADR-2 section (lines 1759-1868)
- **Schema changes:** `src/index.rs::build_schema()`
- **Fallback implementation:** `src/scraper/mod.rs::load_session_content()`
- **Config changes:** `src/config.rs::VectorConfig`
- **Usage examples:** `src/search.rs`, `src/analytics.rs`

## Follow-up Work

Track future beads that reference ADR-2 for:

1. **Monitoring:** Add index size tracking to `agentscribe status`
2. **Optimization:** Consider caching `content_length` separately if profiling shows JSONL re-reads are a bottleneck
3. **Documentation:** Keep these notes current if the pattern evolves

---

**Document maintained by:** bead bf-1pkfp  
**Last updated:** 2026-08-01
