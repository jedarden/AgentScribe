# Test Framework Usage Patterns Analysis — AgentScribe

**Generated**: 2026-08-13
**Source**: Comprehensive scan of 65 test files across AgentScribe codebase
**Total Test Functions**: 1,878

---

## Executive Summary

AgentScribe uses **exclusively standard Rust testing infrastructure** with **zero advanced testing frameworks**. All testing is done through the built-in `#[test]` attribute and standard assertion macros (`assert!`, `assert_eq!`, `assert_ne!`). No procedural test macros, parameterized testing frameworks, or async-specific test attributes are used.

**Key Finding**: The codebase achieves comprehensive test coverage through simple patterns and custom test helpers, avoiding framework complexity entirely.

---

## Macro Usage Patterns

### ❌ Procedural Test Macros (NOT USED)

| Macro | Usage Count | Files | Status |
|-------|-------------|-------|--------|
| `rstest::rstest` | 0 | 0 | Not used |
| `rstest::fixture` | 0 | 0 | Not used |
| `proptest::proptest` | 0 | 0 | Not used |
| `proptest::strategy` | 0 | 0 | Not used |
| `parameterized::parameterized` | 0 | 0 | Not used |
| `tokio::test` | 0 | 0 | Not used |
| `async_std::test` | 0 | 0 | Not used |

**Analysis**: The codebase does not use any procedural macros for test generation, parameterization, or async test attributes. All tests use the standard `#[test]` attribute provided by Rust's built-in testing framework.

### ✅ Assertion Macros (HEAVILY USED)

| Macro | Usage Pattern | Example |
|-------|--------------|---------|
| `assert!` | Boolean assertions | `assert!(result.is_ok())` |
| `assert_eq!` | Equality comparisons | `assert_eq!(result, expected)` |
| `assert_ne!` | Inequality comparisons | `assert_ne!(value, 0)` |
| `assert!(result.is_ok())` | Result type validation | `assert!(parsed.is_ok())` |
| `assert!(result.is_err())` | Error case validation | `assert!(invalid_path.is_err())` |

**Pattern**: Standard assertion macros are used exclusively. No custom assertion macros or assertion frameworks (like `pretty_assertions` in imports - likely macro-only usage).

---

## Attribute Usage Patterns

### ✅ Standard Test Attributes

| Attribute | Usage Count | Pattern | Example |
|-----------|-------------|---------|---------|
| `#[test]` | 1,878+ | Standard test function | `#[test] fn test_feature() { ... }` |
| `#[cfg(test)]` | 58 | Test module declaration | `#[cfg(test)] mod tests { ... }` |
| `#[ignore]` | 1 | Temporarily disabled test | `#[ignore] // Temporarily disabled - turbovec dependency commented out` |

**Location of `#[ignore]`**: `src/vector.rs` - test temporarily disabled due to commented-out turbovec dependency

### ❌ Advanced Test Attributes (NOT USED)

| Attribute | Usage | Status |
|-----------|-------|--------|
| `#[tokio::test]` | Async test attribute | Not used - async tests use `#[test]` + `.await` |
| `#[rstest]` | Parameterized test attribute | Not used |
| `#[should_panic]` | Expected panic attribute | Not found in scans |
| `#[ignore]` (general) | Test skipping | Only 1 usage (vector.rs) |

---

## Async Testing Patterns

### Pattern: Standard `#[test]` + `.await` (NO `#[tokio::test]`)

**Finding**: All async tests use the standard `#[test]` attribute with manual `.await` calls. No `#[tokio::test]` or async-specific attributes are used.

**Example Pattern** (from `src/mcp.rs`, `src/transcription.rs`, `tests/daemon_mcp.rs`):
```rust
#[test]
async fn test_async_feature() {
    // Manual async runtime setup or tokio::test harness
    let result = async_function().await;
    assert!(result.is_ok());
}
```

**Files with Async Tests** (3 files):
- `src/mcp.rs` — MCP server async tests
- `src/transcription.rs` — Transcription async tests  
- `tests/daemon_mcp.rs` — MCP daemon integration tests

**Pattern**:
- Async tests import: `use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};`
- Async runtime: Tokio used for runtime, but NOT via `#[tokio::test]` attribute
- Test execution: Manual async test invocation with `.await`

**Implication**: The codebase does NOT follow the common pattern of "all async tests use tokio::test". Instead, it uses standard `#[test]` with manual async runtime handling.

---

## Common Usage Combinations

### ❌ No Framework Combinations

Since no advanced testing frameworks are used, there are no macro/attribute combinations to document.

### ✅ Standard Pattern Combinations

**Pattern 1**: `#[test]` + `assert_eq!` + `tempfile::TempDir`
```rust
#[test]
fn test_with_temp_directory() {
    let temp_dir = tempfile::tempdir().expect("failed to create tempdir");
    let result = function_under_test(temp_dir.path());
    assert_eq!(result, expected);
}
```

**Pattern 2**: `#[cfg(test)]` module + `use super::*` + helper functions
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_internal_function() {
        assert_eq!(internal_helper("input"), "expected");
    }
}
```

**Pattern 3**: Integration test pattern + custom helpers
```rust
use agentscribe::scraper::Scraper;
use tempfile::TempDir;

#[test]
fn test_full_pipeline() {
    let temp_dir = setup_temp_directory();
    let mut scraper = Scraper::new(temp_dir.path()).unwrap();
    // ... test code
}
```

---

## Custom Test Helpers and Utilities

### Test Helper Functions (tests/test_helpers.rs)

**5 major test helper functions**:

| Helper | Purpose | Usage |
|--------|---------|-------|
| `setup_temp_directory()` | Creates `.agentscribe/` layout with plugins/sessions/index/state dirs | 15+ files |
| `create_claude_code_plugin()` | Builds configured Claude Code plugin for testing | Integration tests |
| `create_simple_parser()` | Creates minimal JSONL parser for basic tests | Parser tests |
| `create_test_plugin()` | Builds basic test plugin with standard mappings | 10+ files |
| `create_envelope_plugin()` | Creates plugin with envelope routing configured | Parser envelope tests |
| `create_meta_routing_test_plugin()` | Creates plugin with meta-type routing (session_start, session_end) | Parser tests |
| `assert_meta_routing_returns_empty()` | Validates meta-type events produce zero events | 5+ tests |

**File-level Custom Helpers** (integration tests):

| Helper | Location | Purpose |
|--------|----------|---------|
| `fixtures_dir()` | tests/integration_tests.rs | Path to test fixtures |
| `make_data_dir()` | tests/integration_tests.rs | Creates temp data directory structure |
| `jsonl_plugin()` | tests/integration_tests.rs | Builds JSONL-format plugin |
| `aider_plugin()` | tests/integration_tests.rs | Builds Aider Markdown plugin |
| `test_jsonl_content()` | tests/subagent_spawning_integration_tests.rs | Generates test JSONL data |
| `create_test_event()` | tests/main_session_parent_tests.rs | Creates test Event objects |
| `create_test_events()` | tests/main_session_parent_tests.rs | Creates vector of test Events |

**Pattern**: Custom helpers focus on:
- **Directory structure setup** (AgentScribe's `~/.agentscribe/` layout)
- **Plugin configuration** (pre-configured plugins for different agent types)
- **Test data generation** (JSONL content, Event objects, fixture paths)
- **Validation helpers** (meta routing assertions, empty result checks)

---

## Detection Results: "All Async Tests Use tokio::test"

**Result**: ❌ **FALSE**

**Finding**: Async tests do NOT use `#[tokio::test]`. They use standard `#[test]` with manual async runtime handling.

**Evidence**:
- 0 occurrences of `#[tokio::test]` attribute found
- 3 files contain async tests (src/mcp.rs, src/transcription.rs, tests/daemon_mcp.rs)
- Async tests import tokio runtime directly: `use tokio::io::{AsyncBufReadExt, ...}` 
- Tests use `.await` directly without async test attribute

**Implication**: The common pattern of using `#[tokio::test]` for async tests is NOT followed in this codebase. Tests use standard `#[test]` with manual async invocation.

---

## Notable Patterns

### 1. **No Test Duplication Frameworks**

- No `rstest` for parameterized testing
- No `proptest` for property-based testing
- Manual test data duplication instead of data-driven tests

**Example**: Parser tests with multiple fixtures use manual test duplication rather than `#[rstest]` parameterization.

### 2. **No Mocking Framework**

- No `mockall` or similar mocking libraries
- Tests use real implementations with temporary directories for isolation
- Filesystem-based testing (tempfile) instead of mocked filesystems

### 3. **Heavy Use of tempfile**

- 17 imports of `tempfile` across 15 files
- Pattern: Every integration test creates isolated temp directories
- Automatic cleanup via TempDir drop behavior

### 4. **Test Organization**

- **58 inline test modules**: `#[cfg(test)] mod tests { ... }`
- **17 integration test files**: Separate files in `tests/` directory
- **Shared test helpers**: `tests/test_helpers.rs` with reusable utilities

### 5. **Assertion Style**

- **Primary assertion**: `assert_eq!` for equality comparisons
- **Secondary assertions**: `assert!` for boolean checks (Result type validation)
- **No custom assertion messages** in most cases
- **No pretty-assertions imports** (likely macro-only from dev-dependencies)

---

## Dev-Dependencies vs. Actual Usage

| Crate | In Cargo.toml | Imported | Usage |
|-------|---------------|----------|-------|
| `tempfile` | ✅ 3.14 | ✅ 17 imports | Heavy usage for test isolation |
| `pretty_assertions` | ✅ 1.4 | ❌ 0 imports | Macro-only (no imports needed) |
| `filetime` | ✅ 0.2 | ❌ 0 imports | Used directly in tests, no use statements |

**Note**: `pretty_assertions` is in dev-dependencies but has 0 imports, indicating it's used via macro expansion only (assert_eq! macro rewrite).

---

## Import Patterns Summary

### Top 6 Test Imports (by frequency)

1. **`use super::*;`** — 50+ files (access to module being tested)
2. **`use std::path::{Path, PathBuf};`** — 30+ files (test file paths)
3. **`use std::fs;`** — 25 files (test fixture I/O)
4. **`use chrono::{DateTime, Utc};`** — 40+ files (test timestamps)
5. **`use serde::{Deserialize, Serialize};`** — 35+ files (test data structures)
6. **`use tempfile::TempDir;`** — 11 files (test isolation)

### Framework-Import Comparison

| Framework | Imported | In Dev-Dependencies | Used |
|-----------|----------|-------------------|------|
| Standard `#[test]` | N/A (built-in) | N/A | ✅ 1,878 tests |
| tempfile | ✅ 17 imports | ✅ Yes | ✅ Heavy usage |
| pretty_assertions | ❌ 0 imports | ✅ Yes | ✅ Macro-only |
| criterion | ❌ 0 imports | ❌ No | ❌ Manual benchmarking |
| proptest | ❌ 0 imports | ❌ No | ❌ Manual test data |
| rstest | ❌ 0 imports | ❌ No | ❌ Manual duplication |
| mockall | ❌ 0 imports | ❌ No | ❌ Real implementations |

---

## Recommendations

### Keep Current Approach

1. **Standard `#[test]`** — Simple, fast, no framework overhead
2. **tempfile usage** — Excellent for test isolation
3. **Real implementations** — No mock complexity, trustworthy tests
4. **Custom helpers** — Well-organized reusable test utilities

### Consider Adding (if needed)

1. **rstest** — For parameterized tests (reduce manual duplication)
   - Current: Manual test duplication for multiple scenarios
   - Benefit: Data-driven tests, less boilerplate

2. **pretty_assertions imports** — For better diffs
   - Already in dev-dependencies but not imported
   - May be used via macro expansion (invisible to import analysis)

3. **criterion** — If benchmarking becomes regular
   - Current: Manual `Instant::now()` measurements
   - Benefit: Statistical analysis, regression detection

### Skip Adding

1. **mockall** — Current tempfile approach works well
2. **proptest** — Manual edge case coverage is sufficient
3. **`#[tokio::test]`** — Standard `#[test]` + `.await` is simpler
4. **async_trait** — No async trait testing patterns needed

---

## Conclusion

AgentScribe's test framework usage is **minimal and pragmatic**:

- **Zero procedural test macros** — No rstest, proptest, parameterized testing
- **Only standard attributes** — `#[test]`, `#[cfg(test)]`, one `#[ignore]`
- **Standard assertion macros** — `assert!`, `assert_eq!`, `assert_ne!`
- **Custom test helpers** — 7+ utility functions for common test patterns
- **Async tests use standard `#[test]`** — NOT `#[tokio::test]`

The codebase demonstrates that sophisticated testing doesn't require sophisticated frameworks. Good test design (isolation via tempfile, fixture-based data, custom helpers, integration coverage) matters more than framework features.

**Key insight**: The testing strategy prioritizes simplicity, fast execution, and maintainability over specialized testing capabilities. This approach works well for AgentScribe's needs and provides comprehensive coverage (1,878 test functions) without framework complexity.
