# Search Results: 'coding/AgentScribe' Occurrences

**Pattern Searched:** `coding/AgentScribe`
**Search Scope:** All `*.toml`, `*.md`, `*.rs` files in /home/coding/AgentScribe
**Total Occurrences:** 294
**Date:** 2026-08-23

## Executive Summary

**Finding:** The pattern `'coding/AgentScribe'` does **NOT** appear in this codebase as a repository URL.

All occurrences of `'coding/AgentScribe'` in this codebase are **absolute file paths** to the local repository at `/home/coding/AgentScribe/`. These are NOT repository URLs.

## Analysis

### Repository URL Check
Ran targeted search for common repository URL patterns:
- `github.com/coding/AgentScribe` - ❌ Not found
- `gitlab.com/coding/AgentScribe` - ❌ Not found
- `git.ardenone.com/coding/AgentScribe` - ❌ Not found
- Any `https://` or `http://` URLs with 'coding/AgentScribe' - ❌ Not found

**Result:** No repository URLs contain this pattern.

### What the Pattern Actually Is

All 294 matches are **absolute file paths** in the format:
```
/home/coding/AgentScribe/
```

This is the legitimate local repository path on this machine.

## Distribution by File

| File | Count | Type |
|------|-------|------|
| `extract_imports.rs` | 67 | Rust test script |
| `TEST_FILES_CATALOG.md` | 66 | Test catalog documentation |
| `TEST_IMPORTS.md` | 65 | Test import documentation |
| `docs/test-imports-analysis.md` | 20 | Test analysis documentation |
| `agentscr-e3ec00f3-findings.md` | 12 | This file (self-reference) |
| `docs/test/empty-index.md` | 9 | Test documentation |
| `LOOKUP_TEST_REFERENCE.md` | 7 | Reference documentation |
| Various notes/ files | 19 | Project notes |
| Various docs/ files | 28 | Documentation |
| `src/cli.rs` | 2 | Source code comments |
| `CLI_ENTRY_POINT.md` | 2 | Documentation |
| Other scattered files | 7 | Various |

## Representative Examples

### Example 1: Test Import Script (extract_imports.rs)
```rust
let files = vec![
    "/home/coding/AgentScribe/tests/aider_glob_discovery_test.rs",
    "/home/coding/AgentScribe/tests/aider_input_scrape_test.rs",
    // ... more test file paths
];
```
**Type:** Legitimate file path array for test discovery

### Example 2: Documentation Files
```markdown
## /home/coding/AgentScribe/src/analytics.rs
## /home/coding/AgentScribe/src/annotations.rs
```
**Type:** Section headers referencing source file locations

### Example 3: Source Code Comments
```rust
/// **CLI Definition Location:** This enum variant (lines 81-208) in `/home/coding/AgentScribe/src/cli.rs`
/// **Internal Options Struct:** `SearchOptions` struct in `/home/coding/AgentScribe/src/search.rs`
```
**Type:** Documentation comments pointing to file locations

### Example 4: Bead/Project Notes
```markdown
- **Location:** `/home/coding/AgentScribe/src/parser/aider_input.rs`
- **Location:** `/home/coding/AgentScribe/tests/aider_input_scrape_test.rs`
```
**Type:** Project tracking notes referencing file locations

## Conclusion

**No changes required** for the `'coding/AgentScribe'` pattern, as it does not appear as a repository URL in this codebase.

All matches are **legitimate file path references** to the local repository at `/home/coding/AgentScribe/`, which is the correct and expected usage for:

1. **Test file discovery** - Scripts that enumerate test files
2. **Documentation** - References to source code locations
3. **Project notes** - Tracking files and modules
4. **Comments** - Pointing to related code sections

These file paths are machine-specific (tied to `/home/coding/`) and would need to be adjusted if the repository were cloned to a different location, but they are **not incorrect repository URLs** that need fixing.

## Next Steps

**None required.** This search was a cataloging task. No actionable issues were found.

If this repository were to be moved or cloned to a different location, the absolute paths would need to be updated, but that is a separate concern from fixing incorrect repository URLs.
