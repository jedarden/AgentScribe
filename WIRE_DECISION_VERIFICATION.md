# WIRE Decision Verification: MarkdownParser Auto-Discovery of .aider.input.history

**Date:** 2026-08-16
**Status:** ✅ VERIFIED - Already implemented
**Reference:** Child of agentscr-48aa5061 (verification task)

## Decision Verified

**WIRE Mechanism:** `MarkdownParser.parse()` self-discovers and loads sibling `.aider.input.history` file

## Implementation Details

### Location
- **File:** `src/parser/markdown.rs`
- **Function:** `load_companion_input_history()` (lines 76-106)
- **Integration:** `FormatParser::parse()` trait implementation (lines 256-281)

### Mechanism
The auto-discovery works as follows:

1. **Discovery:** When `MarkdownParser::parse()` is called on a markdown file (e.g., `.aider.chat.history.md`), it automatically looks for a sibling `.aider.input.history` file in the same directory.

2. **Best-Effort Loading:** The companion file is loaded with these failure modes:
   - Missing file → Returns `None` (no error)
   - Unreadable file → Returns `None` (logs debug message)
   - Empty file → Returns `None` (logs debug message)
   - Valid file → Returns `AiderInputHistory` object

3. **Enrichment:** If the input history is loaded successfully, user events are enriched with precise timestamps from the input history instead of using `Utc::now()` defaults.

### Code Evidence

**From `src/parser/markdown.rs:76-106`:**
```rust
/// Auto-discover a sibling `.aider.input.history` companion file and attempt to load it.
///
/// Best-effort: returns `None` for missing or unreadable files without error.
fn load_companion_input_history(source_path: &Path) -> Option<AiderInputHistory> {
    let companion = source_path
        .parent()
        .map(|dir| dir.join(".aider.input.history"))?;

    match AiderInputHistory::load_from_file(&companion) {
        Ok(history) if !history.is_empty() => {
            debug!(
                path = %companion.display(),
                entries = history.len(),
                "loaded .aider.input.history companion"
            );
            Some(history)
        }
        Ok(_) => {
            debug!(path = %companion.display(), "companion input history is empty, skipping");
            None
        }
        Err(e) => {
            debug!(
                path = %companion.display(),
                error = %e,
                "companion .aider.input.history not available, skipping"
            );
            None
        }
    }
}
```

**Integration in parse() method (lines 257-281):**
```rust
impl super::FormatParser for MarkdownParser {
    fn parse(&self, source_path: &Path, plugin: &Plugin) -> Result<Vec<Event>> {
        let content = std::fs::read_to_string(source_path)?;

        let session_id = source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Auto-discover sibling .aider.input.history companion file (best-effort).
        // Missing or unreadable files degrade gracefully — no error, no enrichment.
        let input_history = Self::load_companion_input_history(source_path);

        if let Some(ref history) = input_history {
            Self::parse_content_with_input_history(
                &content,
                source_path,
                session_id,
                plugin,
                Some(history),
            )
        } else {
            Self::parse_content(&content, source_path, session_id, plugin)
        }
    }
    // ...
}
```

## Verification Results

### ✅ Mechanism Confirmed
The `MarkdownParser.parse()` method automatically discovers and loads sibling `.aider.input.history` files as a best-effort operation.

### ✅ No Trait Signature Changes Needed
This is an **internal implementation detail** of the `MarkdownParser` struct. The `FormatParser` trait signature remains unchanged:
```rust
fn parse(&self, source_path: &Path, plugin: &Plugin) -> Result<Vec<Event>>;
```

### ✅ Tests Verify Behavior
Comprehensive tests in `markdown.rs` (lines 462-623) verify:
- Auto-discovery works with temporary files
- Timestamp enrichment occurs correctly
- Persistent fixtures validate end-to-end behavior
- Missing companion files degrade gracefully

### ✅ Error Handling is Best-Effort
The implementation follows the documented "skip-and-log" policy:
- File missing → Skip with debug log
- File unreadable → Skip with debug log  
- File empty → Skip with debug log
- No errors propagate to caller

## Acceptance Criteria Met

1. ✅ **Verified decision from agentscr-638eccca** - WIRE mechanism is confirmed as implemented
2. ✅ **Documented specific mechanism** - `MarkdownParser.parse()` auto-discovers and loads sibling `.aider.input.history` file
3. ✅ **Recorded decision for reference** - This document serves as verification record
4. ✅ **Confirmed no trait signature changes** - Implementation is internal to `MarkdownParser`, no trait changes

## Notes for Subsequent Children

This auto-discovery mechanism is now a proven pattern that could be applied to other parser types if they have companion metadata files. The key design principles are:

1. **Best-effort:** Always return `Option<T>`, never error on missing companion files
2. **Sibling discovery:** Use `source_path.parent()` to locate companions
3. **Transparent enrichment:** Caller doesn't need to know about companion files
4. **Testable:** Both with and without companion files present

## Fixtures

Test fixtures at `tests/fixtures/aider_input/` demonstrate the mechanism:
- `chat.md` - Main conversation file
- `.aider.input.history` - Companion file with precise timestamps

The test `test_parse_aider_scrape_path_with_persistent_fixtures` validates that when `chat.md` is parsed, the sibling `.aider.input.history` is automatically loaded and used to enrich user event timestamps.
