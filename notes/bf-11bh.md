# Bead bf-11bh: WIRE Decision for aider_input.rs

## Decision: WIRE

**Decision Date:** 2026-07-04

**Choice:** WIRE (integrate into scrape path) rather than DELETE.

## Reasoning

### FormatParser Trait Constraints
The `FormatParser` trait signature is:
```rust
fn parse(&self, source_path: &Path, plugin: &Plugin) -> Result<Vec<Event>>;
```

This signature cannot be changed without affecting all parsers and the scrape pipeline.

### Current State
- `src/parser/aider_input.rs` (278 lines) is fully implemented with `AiderInputHistory`
- `src/parser/markdown.rs` has `parse_content_with_input_history`, `enrich_with_input_history`, and test coverage
- The wiring already exists at the parser level but is not exercised through the scrape path

### Why WIRE is NOT a Contortion
The integration mechanism is straightforward:
1. In `MarkdownParser::parse` (markdown.rs:234), we have access to `source_path: &Path`
2. Auto-discover a sibling `.aider.input.history` file in the same directory as `source_path`
3. Attempt `AiderInputHistory::load_from_file` on discovery
4. Call `parse_content_with_input_history` if successful, fallback to `parse_content` on failure
5. This is a **contained, best-effort discovery pattern** entirely within `MarkdownParser`
6. **Zero changes** to the `FormatParser` trait or scrape pipeline

### Why DELETE Would Be Wrong
DELETE would discard:
- 278 lines of fully implemented, tested code
- Already-working enrichment logic
- Valuable timestamp enrichment for user events (per-message granularity vs session-start only)

DELETE is only appropriate if wiring requires restructuring the trait or adding complexity to the scrape path. **It does not.**

## Implementation Mechanism

**Location:** `MarkdownParser::parse` in `src/parser/markdown.rs:234`

**Approach:**
```rust
// After reading content
let input_history = source_path
    .parent()
    .map(|dir| dir.join(".aider.input.history"))
    .and_then(|path| AiderInputHistory::load_from_file(&path).ok());

if let Some(ref history) = input_history {
    Self::parse_content_with_input_history(content, source_path, session_id, plugin, Some(history))
} else {
    Self::parse_content(content, source_path, session_id, plugin)
}
```

**Best-effort:** Missing or unreadable `.aider.input.history` files simply fall back to no enrichment.

**Dead-code markers to remove:**
- `#[allow(dead_code)]` on `parse_content_with_input_history`
- `#[allow(dead_code)]` on `enrich_with_input_history`
- `#[allow(dead_code)]` on `parse_markdown_with_input_history`

## Child Dependencies

This decision unblocks:
- **Child 2 (bf-4kpq):** Execute the WIRE implementation
- **Child 3 (bf-1i9x):** Add scrape-path test and verify

## Related Beads

- **Parent:** bf-2epr (aider plugin hardening)
- **Child 1:** bf-12ko (investigation - still open, no findings recorded)
- **Child 2:** bf-4kpq (execute decision)
- **Child 3:** bf-1i9x (finalize with test)
