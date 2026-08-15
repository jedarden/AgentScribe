# Function Signature Examination: build_manifest_from_events

**Location:** `src/index.rs` line 627

**Function Signature:**
```rust
pub fn build_manifest_from_events(
    events: &[Event],
    session_id: &str,
    source_agent: &str,
    project: Option<&str>,
    model: Option<&str>,
    parent_session_id: Option<&str>,
) -> SessionManifest
```

## Parameters (in order):

1. **`events: &[Event]`** - Slice of normalized event objects to process
2. **`session_id: &str`** - Unique session identifier (e.g., "claude-code/83f5a4e7")
3. **`source_agent: &str`** - Agent type name (e.g., "claude-code", "aider")
4. **`project: Option<&str>`** - Optional project directory path
5. **`model: Option<&str>`** - Optional LLM model name (e.g., "claude-sonnet-4-20250514")
6. **`parent_session_id: Option<&str>`** - Optional parent session ID for tracking session hierarchy

## Return Type:
- **`SessionManifest`** - Manifest structure with session metadata (started, ended, turns, files_touched, etc.)

## Purpose:
This function builds a minimal `SessionManifest` directly from scraped events for indexing, creating metadata without enrichment data (summary, outcome, etc.) which is added later in the pipeline.

## Confirmed:
✅ Function located at line 627
✅ Takes exactly 6 parameters
✅ Includes `parent_session_id: Option<&str>` as the 6th parameter
✅ Parameter types and order documented above
