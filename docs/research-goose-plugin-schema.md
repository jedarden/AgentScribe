# Goose Plugin Schema and Project Detection Research

## Summary

Research completed on the goose plugin configuration schema, first-line JSONL metadata handling, and project detection patterns in AgentScribe.

## Key Findings

### 1. Goose Plugin Configuration Schema

**Current Plugin:** `plugins/goose.toml` (version 1.0)

```toml
[parser.project]
# Extract project path from first-line metadata's working_dir field
method = "field"
field = "working_dir"
```

**Data Format:**
- **Location:** `~/.local/share/goose/sessions/*.jsonl`
- **Format:** JSONL with first-line session metadata
- **Line 1:** Session metadata object including `working_dir` field
- **Subsequent lines:** Message events with `role` and `content[]` blocks

### 2. ProjectDetection::Field Implementation Status

**Current Implementation:** ⚠️ **NOT IMPLEMENTED for JSONL format**

The `ProjectDetection::Field { field: String }` variant is defined in `src/plugin.rs`:
```rust
pub enum ProjectDetection {
    Field { field: String },  // Extract from field in session metadata
    ParentDir,               // Use parent directory
    GitRoot,                 // Use git rev-parse
}
```

**How it works:**
- In `src/scraper/mod.rs::detect_project()` (lines 730-771):
  - `ParentDir`: Returns parent directory of log file
  - `GitRoot`: Runs `git rev-parse --show-toplevel`
  - `Field { field: _ }`: Returns `None` with comment: *"For field-based detection, we need to extract from the first event. This is handled in the parser, return None here"*

**Problem:** The comment says it's "handled in the parser" but jsonl.rs does NOT implement this.

**Evidence from code:**
1. `src/parser/jsonl.rs` has NO code to extract project from first-line metadata
2. Line 229 has TODO: `"Future session metadata accumulation (project, model, version)"`
3. Line 765-768 in scraper returns None for Field detection
4. Only `json_array.rs` implements ProjectDetection::Field (line 177-181)

### 3. How json_array.rs Implements Field Detection (Working Example)

From `src/parser/json_array.rs` lines 176-181:
```rust
event.project =
    if let Some(ProjectDetection::Field { field }) = plugin.parser.project.as_ref() {
        extract_string(item, field).or_else(|| context.project.clone())
    } else {
        context.project.clone()
    };
```

This works because json_array reads each item as an event and can extract fields from it. For jsonl with first-line metadata, the project field is only in line 1, not in every event.

### 4. First-Line Metadata in jsonl.rs

**Current behavior:**
- First line IS read for session ID extraction (lines 786-850)
- Envelope unwrapping works for first line (lines 44-129 in `unwrap_envelope()`)
- First-line metadata can populate `SessionInfo.metadata` (optional Value field)
- BUT metadata is NOT used for project/model extraction in parsing

**Code evidence:**
```rust
// src/parser/jsonl.rs:786-791
SessionIdSource::Field(field) => {
    match open_file_maybe_zst(source_path) {
        Ok(mut reader) => {
            let mut first_line = String::new();
            if reader.read_line(&mut first_line).is_ok() {
                if let Ok(json) = serde_json::from_str::<Value>(&first_line) {
                    // ... session ID extraction works here
```

**Missing:**
- No extraction of project field from first-line metadata
- No extraction of model field from first-line metadata
- TODO comment at line 229 confirms this is future work

### 5. Required Fields for Goose Plugin

Based on the plan.md (line 60-63) and existing goose.toml:

**Session metadata (line 1):**
- `working_dir` (string) - Absolute path to project directory
- `description` (string) - Optional session description
- Other fields: not specified in docs, need verification

**Message events (subsequent lines):**
- `role` (string) - "user", "assistant", "system", "tool"
- `content` (array or string) - Message content blocks
- `timestamp` (string) - ISO 8601 timestamp

**Acceptable values:**
- `working_dir`: Must be absolute path string (e.g., "/home/user/project")
- `role`: Standard message role enum values
- `content`: Can be array of content blocks or string

## Implementation Gap

**What works:**
- ✅ Plugin schema accepts `method = "field"` with `field = "working_dir"`
- ✅ First-line reading works for session ID extraction
- ✅ Envelope unwrapping infrastructure exists

**What doesn't work:**
- ❌ JSONL parser does NOT extract project from first-line metadata
- ❌ Current goose.toml config will silently fail (project will be None)
- ❌ No error message - just falls back to None

## Acceptance Criteria Status

- [x] Schema requirements documented
- [x] Project detection mechanism understood
- [ ] **NOT READY** to create config with correct field values - implementation missing

## Next Steps

To properly support the goose plugin (and other JSONL agents with first-line metadata):

1. Implement project field extraction in jsonl.rs:
   - Read first line during `detect_sessions()` or `parse()`
   - Extract field specified in `plugin.parser.project.field`
   - Store in `SessionInfo.metadata` or directly set on ParseContext

2. Implement model field extraction similarly:
   - Read first line for model information
   - Apply `plugin.parser.model` configuration

3. Add tests with real goose format fixtures:
   - Verify `working_dir` extraction works
   - Test fallback when field is missing
   - Test with various working_dir formats

## Verification Needed

Before shipping goose plugin:
1. ✅ Plugin TOML schema documented
2. ❌ Actual goose log format NOT verified (no local install)
3. ❌ Real field names NOT confirmed against ~/.local/share/goose/sessions/
4. ❌ Implementation NOT complete for field-based project detection

## References

- `plugins/goose.toml` - Current plugin configuration
- `src/plugin.rs` lines 443-460 - ProjectDetection enum
- `src/scraper/mod.rs` lines 730-771 - detect_project implementation
- `src/parser/jsonl.rs` lines 44-129 - Envelope unwrapping
- `src/parser/jsonl.rs` lines 713-850 - Session detection
- `src/parser/json_array.rs` lines 176-181 - Working field detection example
- `docs/plan.md` lines 60-63 - Goose format specification
