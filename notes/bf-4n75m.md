# Bead bf-4n75m: Fix broken build - annotations.rs module

## Issue
The build was failing because src/annotations.rs was declared in src/lib.rs but was never created (E0583). However, upon investigation, the file src/annotations.rs was found to exist but was untracked in git (shown as `??` in git status).

## Fixes Applied

### 1. Fixed test code in src/annotations.rs (lines 400, 422)
Changed test calls from:
```rust
let annotation = new_annotation(
    "bug-fix".to_string(),
    Some("Fixed parsing error".to_string()),
    "human".to_string(),  // ❌ String passed
);
```

To:
```rust
let annotation = new_annotation(
    "bug-fix".to_string(),
    Some("Fixed parsing error".to_string()),
    Some("human".to_string()),  // ✅ Option<String> expected
);
```

### 2. Fixed test code in src/index.rs (line 1045)
Changed from 7 arguments to 6 arguments to match function signature:
```rust
// Before (7 args - incorrect)
let manifest = build_manifest_from_events(&[], "test/2", "aider", None, None, None, None);

// After (6 args - correct)  
let manifest = build_manifest_from_events(&[], "test/2", "aider", None, None, None);
```

## Verification
- ✅ cargo check succeeds (BUILD SUCCESS)
- ✅ cargo fmt --check passes (Formatting OK)
- ❌ cargo build fails with linker errors (pre-existing OpenSSL dependency issues)
- ❌ cargo test fails due to pre-existing compilation errors in other modules (parser, scraper, plugin)

## Pre-existing Issues (NOT fixed in this bead)
The following modules have compilation errors that should be filed separately:
- src/parser/sqlite.rs:547 - missing field `array` in Source initializer
- src/parser/markdown.rs:518 - FormatParser trait not in scope  
- src/plugin.rs:625 - missing field `array` in Source initializer
- src/scraper/file_path_extractor.rs:321 - missing fields `array`, `envelope`
- src/scraper/mod.rs:933, 1005, 1112, 1162, 1356 - missing fields in Source initializers

These errors existed before this bead and are unrelated to the annotations.rs module.

## Implementation Details
The src/annotations.rs module provides:
- `Annotation` struct with fields: tag, note, created_at, created_by
- `new_annotation()` - factory function with timestamp
- `add_annotation()` - append-only write to sidecar JSON files
- `load_annotations()` - read annotations from sidecar
- `remove_annotation()` - delete annotation by tag, removes file when empty
- Sidecar file path: `~/.agentscribe/sessions/<agent>/<id>.annotations.json`
- Complete test suite with 10 unit tests

The module now compiles successfully and all its tests should pass once the pre-existing errors in other modules are fixed.
