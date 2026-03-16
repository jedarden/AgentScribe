# AgentScribe — Implementation Plan

## Overview

AgentScribe captures logging and telemetry from all running agents in an environment and stores it into flat files with searchable indexing. The goal is to make agent history accessible to other agents seeking past solutions, error patterns, and reference implementations.

---

## Goals

- **Capture** — ingest structured logs, prompts, completions, and telemetry from multiple concurrent agents
- **Store** — persist data as flat files (JSONL + Markdown) organized for human readability and git compatibility
- **Index** — build lightweight searchable indexes without requiring a database
- **Serve** — expose a query interface agents can call to retrieve relevant past context
- **Backup** — all data is git-committable; no binary blobs, no external dependencies

---

## Architecture

```
AgentScribe/
├── ingest/          # Receivers: HTTP endpoint, stdin pipe, file watcher
├── store/           # Flat-file writer: JSONL logs + Markdown summaries
├── index/           # Index builder: keyword, tag, and embedding-based
├── query/           # Query interface: CLI + HTTP API for agent lookups
├── docs/            # Plans, architecture notes, ADRs
└── data/            # Runtime data directory (git-ignored by default)
    ├── sessions/    # Per-session JSONL logs
    ├── index/       # Index files (inverted keyword, tag map)
    └── summaries/   # Markdown summaries per session
```

---

## Phases

### Phase 1 — Core Ingestion & Storage
- Define the canonical AgentScribe event schema (session ID, agent ID, timestamp, event type, payload)
- Implement HTTP ingest endpoint (POST /ingest)
- Implement flat-file writer: append-only JSONL per session, one file per agent per day
- Implement basic keyword index (inverted index as JSONL)
- CLI: `agentscribe ingest`, `agentscribe index rebuild`

### Phase 2 — Query Interface
- CLI query: `agentscribe search <terms>`
- HTTP query endpoint: GET /query?q=...
- Result format: ranked list of matching sessions with snippet context
- Tag-based filtering: filter by agent ID, date range, event type

### Phase 3 — Summarization & Embedding Index
- Auto-generate Markdown summaries per session on close
- Optional embedding index (local, no external API required) for semantic search
- `agentscribe summarize <session-id>`

### Phase 4 — Agent API & Integration
- Standardized agent-facing API spec (OpenAPI)
- Client libraries / SDK stubs for common agent frameworks
- Git auto-commit hook: commit new sessions on rotation

---

## Data Formats

### Event (JSONL line)
```json
{
  "ts": "2026-03-16T12:00:00Z",
  "session_id": "abc123",
  "agent_id": "claude-code-1",
  "event_type": "prompt|completion|tool_call|tool_result|error|metadata",
  "payload": {}
}
```

### Keyword Index Entry
```json
{
  "term": "database migration",
  "sessions": ["abc123", "def456"],
  "last_updated": "2026-03-16T12:00:00Z"
}
```

---

## Design Principles

- **Flat files first** — every piece of data is a plain text file; the system works without any database
- **Git-native** — append-only JSONL and Markdown are diff-friendly and back up cleanly
- **Agent-readable** — summaries are in Markdown so agents can read them directly as context
- **No external dependencies for core** — indexing and search work offline with no cloud APIs required
- **Extensible** — embedding/semantic search is an optional layer, not a requirement
