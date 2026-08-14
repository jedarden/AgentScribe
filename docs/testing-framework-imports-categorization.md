# Test Framework Imports Categorization — AgentScribe

**Generated**: 2026-08-13
**Source**: Extracted imports from 65 test files
**Total Imports Analyzed**: 570 use statements

---

## Executive Summary

AgentScribe uses **only standard Rust testing infrastructure** with minimal external test utilities. **No advanced testing frameworks** (criterion, proptest, rstest, mockall, async testing frameworks) are present in the codebase.

**Key Finding**: All testing is done through built-in `#[test]` and `#[cfg(test)]` modules with `tempfile` as the only external test dependency.

---

## Framework Categories

### 1. Criterion Imports (Benchmarking)

**Status**: ❌ **NOT FOUND**

- **criterion**: 0 occurrences
- **criterion::**: 0 occurrences
- Files using criterion: 0

**Analysis**: No benchmarking framework is used. Performance tests use manual `Instant::now()` measurements instead (found in `tests/integration_tests.rs`).

---

### 2. PropTest Imports (Property-Based Testing)

**Status**: ❌ **NOT FOUND**

- **proptest**: 0 occurrences
- **proptest::**: 0 occurrences
- Files using proptest: 0

**Analysis**: No property-based testing framework. Edge cases are covered through manual test data and fixtures.

---

### 3. Rstest Imports (Parameterized Testing)

**Status**: ❌ **NOT FOUND**

- **rstest**: 0 occurrences
- **rstest::**: 0 occurrences
- Files using rstest: 0

**Analysis**: No parameterized testing framework. Tests with multiple scenarios use manual duplication or helper functions.

---

### 4. Mockall Imports (Mocking Framework)

**Status**: ❌ **NOT FOUND**

- **mockall**: 0 occurrences
- **mockall::**: 0 occurrences
- Files using mockall: 0

**Analysis**: No mocking framework. Tests use real implementations with temporary directories (`tempfile`) for isolation.

---

### 5. Async Test Framework Imports

#### tokio::test

**Status**: ❌ **NOT FOUND**

- **tokio::test**: 0 occurrences
- Files using tokio::test: 0

**Analysis**: Async tests use standard `#[test]` with `.await` calls, not the tokio::test macro.

#### async-std::async_test

**Status**: ❌ **NOT FOUND**

- **async_std::test**: 0 occurrences
- **async_std::async_test**: 0 occurrences
- Files using async-std test: 0

**Analysis**: No async-std testing framework detected.

---

### 6. async_trait Imports

**Status**: ❌ **NOT FOUND**

- **async_trait**: 0 occurrences
- **async_trait::**: 0 occurrences
- Files using async_trait: 0

**Analysis**: No async trait testing patterns. Async functionality is tested directly with `.await`.

---

### 7. Other Testing-Related Imports

#### tempfile (Test Isolation)

**Status**: ✅ **HEAVILY USED** — 17 imports across 15 files

**Import Types**:
- `use tempfile::TempDir;` — 11 files
- `use tempfile::NamedTempFile;` — 5 files
- `use tempfile::tempdir;` — 1 file

**Files Using tempfile**:

1. `src/analytics.rs` — TempDir
2. `src/annotations.rs` — TempDir
3. `src/capacity.rs` — TempDir
4. `src/enrichment/config_change_tracker.rs` — TempDir
5. `src/index.rs` — TempDir
6. `src/parser/aider_input.rs` — NamedTempFile
7. `src/parser/markdown.rs` — NamedTempFile, TempDir
8. `src/parser/sqlite.rs` — NamedTempFile
9. `src/projects.rs` — tempdir
10. `src/scraper/companion.rs` — NamedTempFile
11. `src/scraper/state.rs` — NamedTempFile
12. `src/search.rs` — TempDir
13. `src/vector.rs` — TempDir
14. `tests/aider_glob_discovery_test.rs` — TempDir
15. `tests/aider_toml_glob_validation_test.rs` — TempDir
16. `tests/render_tests.rs` — TempDir
17. `tests/aider_input_scrape_test.rs` — (No tempfile import, uses test helpers)

**Pattern**: `tempfile` is used for:
- Creating isolated test directories (`TempDir`)
- Temporary file creation (`NamedTempFile`)
- Test data isolation and cleanup

---

#### Standard Library Testing Imports

**std::assert\*\***: Not directly imported (macros are built-in)

**std::path and std::fs for test fixtures**:
- `use std::fs;` — 25 files (for reading test fixtures, creating test data)
- `use std::path::{Path, PathBuf};` — 30+ files (for test file paths)

**Files using std::fs for testing**:
1. `src/config.rs` — Configuration test data
2. `src/daemon.rs` — Daemon integration tests
3. `src/enrichment/config_change_tracker.rs` — Test file I/O
4. `src/gc.rs` — Garbage collection tests
5. `src/parser/json_array.rs` — JSON test fixtures
6. `src/parser/jsonl.rs` — JSONL test data
7. `src/parser/markdown.rs` — Markdown test fixtures
8. `src/plugin.rs` — Plugin TOML test files
9. `src/projects.rs` — Project path tests
10. `src/rules.rs` — Rule file tests
11. `src/scraper/mod.rs` — Scraper test data
12. `src/tags.rs` — Tag test fixtures
13. `tests/aider_glob_discovery_test.rs` — Glob pattern tests
14. `tests/context_tests.rs` — Context test data
15. `tests/daemon_mcp.rs` — MCP daemon tests
16. `tests/integration_tests.rs` — Full pipeline test data
17. `tests/parent_session_tests.rs` — Parent session test files
18. `tests/phase6_tests.rs` — Phase 6 feature tests
19. `tests/pulse_report_tests.rs` — Report generation tests
20. `tests/render_tests.rs` — Rendering test fixtures
21. `tests/test_helpers.rs` — Test fixture utilities
22. `tests/zero_write_tests.rs` — Write guard tests

---

#### chrono (Test Time Manipulation)

**Status**: ✅ **WIDELY USED** — 55+ files

**Common Test Patterns**:
- `use chrono::{DateTime, Utc};` — Setting test timestamps
- `use chrono::Duration;` — Time-based test assertions
- `use chrono::Utc;` — Current time in tests

**Usage in Tests**:
- Setting event timestamps: `Utc::now()`, `Utc.with_ymd_and_hms(...)`
- Time-based assertions: `Duration::days(1)`, `Duration::hours(24)`
- Test data creation: `DateTime::parse_from_rfc3339(...)`

---

#### tokio (Async Runtime in Tests)

**Status**: ⚠️ **LIMITED USE** — 3 files

**Import Types**:
- `use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};` — Async I/O in tests
- `use tokio::net::UnixListener;` — Unix socket tests
- `use tokio::task;` — Async task spawning in tests
- `use tokio::sync::{mpsc, Mutex};` — Async synchronization tests
- `use tokio::time::sleep;` — Async timing tests

**Files Using tokio**:
1. `src/mcp.rs` — MCP server async tests
2. `src/transcription.rs` — Transcription async tests
3. `tests/daemon_mcp.rs` — MCP daemon integration tests

**Pattern**: Tokio is used for async runtime in integration tests, but not via `tokio::test` macro.

---

#### Testing-Specific Crate Imports

**tantivy (Search Index Testing)**:
- `use tantivy::schema::*;` — Schema validation tests
- `use tantivy::query::*;` — Query testing
- `use tantivy::collector::TopDocs;` — Search result tests
- `use tantivy::collector::Count;` — Count assertion tests

**serde (Test Data Serialization)**:
- `use serde::{Deserialize, Serialize};` — Test data structures
- `use serde_json::{json, Value};` — JSON test data

**regex (Pattern Testing)**:
- `use regex::Regex;` — Pattern validation tests
- `use std::sync::LazyLock;` — Compiled regex for tests

---

## Test Import Patterns

### Most Common Test Imports (by frequency)

1. **`use super::*;`** — 50+ files (access to module being tested)
2. **`use std::path::{Path, PathBuf};`** — 30+ files (test file paths)
3. **`use std::fs;`** — 25 files (test fixture I/O)
4. **`use chrono::{DateTime, Utc};`** — 40+ files (test timestamps)
5. **`use serde::{Deserialize, Serialize};`** — 35+ files (test data)
6. **`use tempfile::TempDir;`** — 11 files (test isolation)

### Test Organization Patterns

**Integration Test Files** (17 files in `tests/`):
- Import via `use agentscribe::...;`
- Use `tempfile::TempDir` for isolation
- Import test helpers: `use super::*;` (within tests/)

**Unit Test Modules** (58 `#[cfg(test)]` modules):
- Import via `use super::*;` (access to parent module)
- Import via `use crate::...;` (access to other modules)
- Use `tempfile` for temporary test data

---

## Interesting Findings

### 1. No Advanced Testing Frameworks

**Zero usage** of:
- criterion (benchmarking)
- proptest (property-based testing)
- rstest (parameterized testing)
- mockall (mocking)
- tokio::test (async test attributes)
- async_trait (async trait testing)

**Implication**: The codebase relies on:
- Manual test duplication for multiple scenarios
- Manual performance measurements with `Instant::now()`
- Real implementations instead of mocks
- Standard `#[test]` with `.await` for async tests

### 2. Heavy tempfile Usage

**17 tempfile imports** across 15 files indicate:
- Strong emphasis on test isolation
- No shared state between tests
- Automatic cleanup of test artifacts
- Filesystem-based testing (scraper, parser, state persistence)

### 3. Minimal External Test Dependencies

Only **one external test crate**: `tempfile`

**Comparison with typical Rust projects**:
- `tempfile` ✅ (1/1 projects)
- `pretty_assertions` ❌ (not imported, likely in dev-dependencies only)
- `criterion` ❌ (no benchmarking)
- `proptest` ❌ (no property testing)
- `mockall` ❌ (no mocking)

### 4. Test Data Management

**Fixture-based approach**:
- `tests/fixtures/` directory (mentioned in docs)
- `std::fs` for reading test fixtures
- `tempfile` for creating test data
- `chrono` for setting test timestamps

### 5. Integration Test Pattern

**Full pipeline testing**:
- `tests/integration_tests.rs` — scrape → index → search
- Real implementations (no mocks)
- Temporary directories (tempfile)
- Performance assertions (Instant::now())

---

## Import Distribution by File Type

### Source Files with `#[cfg(test)]` (58 files)

**Average imports per file**: 8-12 use statements

**Most common imports**:
1. `use super::*;` — access to module under test
2. `use tempfile::TempDir;` — test isolation
3. `use chrono::Utc;` — test timestamps
4. `use std::fs;` — test fixture I/O

### Integration Test Files (17 files)

**Average imports per file**: 6-10 use statements

**Most common imports**:
1. `use agentscribe::...;` — crate imports
2. `use tempfile::TempDir;` — test isolation
3. `use std::fs;` — test fixture setup
4. `use std::path::PathBuf;` — test paths

---

## Comparison: Available vs. Used

### In dev-dependencies (from docs/testing-frameworks-analysis.md)

| Crate | In Cargo.toml | Imported in Tests |
|-------|---------------|-------------------|
| tempfile | ✅ 3.14 | ✅ 17 imports |
| pretty_assertions | ✅ 1.4 | ❌ 0 imports (macro-only) |
| filetime | ✅ 0.2 | ❌ 0 imports (used in tests, not tracked) |

### NOT in dev-dependencies (and NOT used)

| Crate | Usage |
|-------|-------|
| criterion | 0 (manual benchmarking) |
| proptest | 0 (manual test data) |
| rstest | 0 (manual test duplication) |
| mockall | 0 (real implementations) |
| tokio::test | 0 (standard `#[test]` + `.await`) |
| async_trait | 0 (no async trait tests) |

---

## Recommendations (from import analysis)

### Keep Current Approach

1. **tempfile usage** — Excellent for test isolation
2. **Standard `#[test]`** — Simple, fast, no framework overhead
3. **Real implementations** — No mock complexity, tests are trustworthy

### Consider Adding

1. **criterion** — If benchmarking becomes regular (replace `Instant::now()`)
   - Current: Manual timing in `tests/integration_tests.rs`
   - Benefit: Statistical analysis, regression detection

2. **rstest** — For parameterized tests
   - Current: Manual test duplication (e.g., multiple parser test cases)
   - Benefit: Reduce test duplication, data-driven tests

3. **pretty_assertions** — For better diffs
   - Already in dev-dependencies but not imported
   - May be used via macro expansion (invisible to import extraction)

### Skip Adding

1. **mockall** — Current tempfile approach works well
2. **proptest** — Manual edge case coverage is sufficient
3. **tokio::test** — Standard `#[test]` + `.await` is simpler
4. **async_trait** — No async trait testing patterns needed

---

## Conclusion

AgentScribe's testing infrastructure is **minimal and pragmatic**:
- **Zero advanced testing frameworks** — Only built-in testing
- **One external test dependency** — `tempfile` (17 imports)
- **570 total imports** across 65 test files
- **Standard patterns** — `#[test]`, `#[cfg(test)]`, `tempfile::TempDir`

The import analysis confirms the earlier findings: AgentScribe achieves comprehensive test coverage (1,878 test functions) without framework complexity. The testing strategy prioritizes simplicity, fast execution, and maintainability over specialized testing capabilities.

**Key insight**: The codebase demonstrates that sophisticated testing doesn't require sophisticated frameworks. Good test design (isolation via tempfile, fixture-based data, integration coverage) matters more than framework features.
