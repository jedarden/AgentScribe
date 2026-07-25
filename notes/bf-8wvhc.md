# Session Tracking and parent_session_id Research

**Bead ID:** bf-8wvhc  
**Date:** 2026-07-25  
**Task:** Research session tracking and parent_session_id requirements

## Overview

This document summarizes the current implementation of session tracking and the `parent_session_id` field in AgentScribe. The `parent_session_id` functionality is **already implemented** in the codebase, with detection, storage, and indexing capabilities.

## 1. Current Session Data Structure

### SessionManifest (`src/event.rs`)

```rust
pub struct SessionManifest {
    pub session_id: String,
    pub source_agent: String,
    pub project: Option<String>,
    pub started: DateTime<Utc>,
    pub ended: Option<DateTime<Utc>>,
    pub turns: u32,
    pub summary: Option<String>,
    pub outcome: Option<String>,
    pub tags: Vec<String>,
    pub files_touched: Vec<String>,
    pub model: Option<String>,
    /// Parent session ID for subagent sessions (format: <agent>/<id>)
    pub parent_session_id: Option<String>,
}
```

**Key Points:**
- `parent_session_id` is defined as `Option<String>` to track parent-child relationships
- The field is intended for subagent sessions where the format is `<agent>/<id>`
- Default value is `None` (set in `SessionManifest::new()`)

### Event Structure (`src/event.rs`)

```rust
pub struct Event {
    pub ts: DateTime<Utc>,
    pub session_id: String,        // Only the current session ID
    pub source_agent: String,
    pub source_version: Option<String>,
    pub project: Option<String>,
    pub role: Role,
    pub content: String,
    // ... other fields
    // NOTE: Events do NOT contain parent_session_id
}
```

**Key Point:** Individual events do not contain `parent_session_id` - it's only at the session manifest level.

## 2. Session Detection Mechanism

### SessionInfo Structure (`src/parser/mod.rs`)

```rust
pub struct SessionInfo {
    pub session_id: String,
    pub start_offset: u64,
    pub end_offset: u64,
    pub metadata: Option<Value>,
    /// Parent session ID (populated for subagent sessions where this session
    /// is a child of another session)
    pub parent_session_id: Option<String>,
}
```

### Subagent Detection Logic (`src/parser/jsonl.rs`)

The JSONL parser automatically detects subagent sessions by analyzing the directory structure:

```
~/.claude/projects/<path>/<parent-session-uuid>/subagents/agent-<id>.jsonl
```

**Detection Algorithm:**
1. Find the `subagents` component in the file path
2. Extract the parent session UUID (component immediately before `subagents`)
3. Verify that `projects` appears somewhere before the parent session
4. Extract the parent session ID and populate `SessionInfo.parent_session_id`

**Code Location:** `src/parser/jsonl.rs` - lines 230-280

```rust
let parent_session_id = source_path
    .components()
    .collect::<Vec<_>>()
    .iter()
    .position(|c| c.as_os_str() == "subagents")
    .and_then(|subagents_idx| {
        // Parent session UUID is the component before "subagents"
        if subagents_idx >= 2 {
            let components: Vec<_> = source_path.components().collect();
            let parent_idx = subagents_idx - 1;
            
            // Check if there's a "projects" component somewhere before the parent session
            let has_projects_before_parent = components[..parent_idx]
                .iter()
                .any(|c| c.as_os_str() == "projects");
            
            if has_projects_before_parent {
                components
                    .get(parent_idx)
                    .and_then(|c| c.as_os_os_str().to_str())
                    .map(|s| s.to_string())
            } else {
                None
            }
        } else {
            None
        }
    });
```

## 3. Index Schema and Storage

### Tantivy Index Schema (`src/index.rs`)

The `parent_session_id` field is fully integrated into the search index schema:

```rust
pub struct IndexFields {
    // ... other fields
    pub session_id: Field,
    pub parent_session_id: Field,  // ✓ Defined
    // ... more fields
}

// Schema definition
fn build_schema() -> Schema {
    let mut builder = Schema::builder();
    
    // ... other fields
    let parent_session_id = builder.add_text_field("parent_session_id", STRING | STORED | FAST);
    
    builder.build()
}
```

**Field Properties:**
- **Type:** TEXT
- **Flags:** `STRING | STORED | FAST`
  - `STRING`: Exact match searches
  - `STORED`: Retrieved in search results
  - `FAST`: Used for aggregations/faceting

### Index Document Building (`src/index.rs`)

When building index documents from session manifests:

```rust
if let Some(ref parent_session_id) = manifest.parent_session_id {
    doc.add_text(fields.parent_session_id, parent_session_id);
}
```

**Search Integration:** The field can be used for:
- Filtering sessions by parent
- Aggregating child sessions under a parent
- Faceted browsing by parent session

## 4. Status Output Implementation

### PluginStatus Structure (`src/cli.rs`)

```rust
struct PluginStatus {
    name: String,
    sessions: usize,              // Total sessions
    events: u64,                  // Total events
    last_scraped: Option<DateTime<Utc>>,
    source_paths: Vec<String>,
    source_files: usize,
    bytes: u64,
    truncation_limit: Option<u32>,
    // Subagent session tracking
    subagent_sessions: usize,     // ✓ Separate count
    subagent_events: u64,        // ✓ Separate event count
}
```

### Subagent Detection in Status (`src/cli.rs`)

The status command detects subagent sessions by examining the `source_agent` field:

```rust
for session_id in &sessions {
    if let Ok(events) = scraper.read_session(session_id) {
        plugin_events += events.len() as u64;
        
        // Detect subagent sessions by checking source_agent in events
        if let Some(first_event) = events.first() {
            if first_event.source_agent == "claude-code-subagent" {
                subagent_session_count += 1;
                subagent_event_count += events.len() as u64;
            }
        }
    }
}
```

**Detection Method:** Checks if `source_agent == "claude-code-subagent"`

### Status Output Format

**Human-readable:**
```
  claude-code      10 sessions   1,234 events   2m ago  (3 source files, 12MB)
    └─ 2 subagent sessions (156 events)
```

**JSON output:**
```json
{
  "name": "claude-code",
  "sessions": 10,
  "events": 1234,
  "source_files": 3,
  "bytes": 12582912,
  "source_paths": ["/path/to/logs"],
  "subagent_sessions": 2,
  "subagent_events": 156,
  "last_scraped": "2026-07-25T10:30:00Z"
}
```

## 5. Rendering Implementation

### HTML Rendering (`src/render.rs`)

**Current Implementation:** The HTML renderer does NOT display `parent_session_id`

```rust
pub fn render_html(events: &[Event], meta: &SessionManifest) -> Result<String> {
    // ... renders:
    // - Project
    // - Agent
    // - Outcome (if present)
    // - Duration
    // - Models (if present)
    // - Files touched (if present)
    // NOTE: parent_session_id is NOT rendered
}
```

### Markdown Rendering (`src/render.rs`)

**Current Implementation:** The Markdown renderer does NOT display `parent_session_id`

```rust
pub fn render_markdown(events: &[Event], meta: &SessionManifest) -> Result<String> {
    // YAML frontmatter includes:
    // - session_id
    // - source_agent
    // - project
    // - started
    // - ended
    // - turns
    // - outcome
    // - files_touched
    // - model
    // NOTE: parent_session_id is NOT included
}
```

**Opportunity:** Both rendering functions could be enhanced to display parent-child relationships.

## 6. Key Findings

### ✓ Already Implemented

1. **Data Structure**: `parent_session_id` exists in `SessionManifest` and `SessionInfo`
2. **Detection**: Automatic subagent detection from directory structure
3. **Indexing**: Fully integrated into Tantivy schema with proper field configuration
4. **Status Output**: Separate tracking and display of subagent sessions
5. **Search**: Can be used for filtering, faceting, and aggregation

### ⚠️ Not Yet Implemented

1. **Rendering**: `parent_session_id` not displayed in HTML/Markdown exports
2. **CLI Display**: No explicit parent-child relationship display in search results
3. **Validation**: No validation that `parent_session_id` references an existing session

### 🔍 Detection Methods

**Currently Used:**
1. **Path-based detection** (JSONL parser): Extracts parent UUID from directory structure
2. **source_agent check** (status command): Detects subagents by `source_agent == "claude-code-subagent"`

**Potential Enhancement:**
- These two methods should be consistent - path-based detection should align with source_agent value

### 📊 Data Flow

```
1. File Scrape (JSONL)
   ├─ Detect subagent path structure
   └─ Set SessionInfo.parent_session_id

2. Session Manifest Creation
   └─ parent_session_id copied from SessionInfo

3. Indexing
   └─ Document stores parent_session_id in Tantivy

4. Status/Query
   └─ Separate counting of subagent vs main sessions

5. Rendering
   └─ (NOT IMPLEMENTED) parent_session_id display
```

## 7. Recommendations

### For Display Enhancement

1. **Add parent_session_id to render outputs:**
   - HTML: Add to header section if present
   - Markdown: Add to YAML frontmatter if present

2. **Enhance search results:**
   - Show parent-child relationships in session listings
   - Add option to group sessions by parent

### For Validation

1. **Add referential integrity check:**
   - Validate that `parent_session_id` references an existing session
   - Add warning if parent session not found

2. **Consistency check:**
   - Ensure path-based detection matches source_agent-based detection
   - Add test coverage for edge cases

## 8. Related Files

- **Core Structures:**
  - `src/event.rs` - SessionManifest, Event
  - `src/parser/mod.rs` - SessionInfo, ParseContext
  
- **Detection & Parsing:**
  - `src/parser/jsonl.rs` - Subagent path detection
  - `src/parser/mod.rs` - Session detection trait
  
- **Indexing:**
  - `src/index.rs` - Schema, document building, field definitions
  
- **Display:**
  - `src/cli.rs` - Status command, PluginStatus
  - `src/render.rs` - HTML/Markdown rendering
  
- **Tests:**
  - `src/parser/jsonl/jsonl_subagent_test.rs` - Subagent detection tests

---

**Conclusion:** The `parent_session_id` field is fully implemented in the data model, detection, and indexing layers. The primary gaps are in display/rendering and validation, which could be addressed in future enhancements.