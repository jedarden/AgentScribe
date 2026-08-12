# Test Framework and Patterns — AgentScribe Codebase

## Test Framework

**AgentScribe uses the built-in Rust `#[test]` framework** — no third-party test frameworks like criterion, proptest, or custom test harnesses are used.

### Core Test Framework
- **Attribute:** `#[test]` on functions in `tests/` directory and inline `#[cfg(test)]` modules in `src/`
- **Runner:** Standard `cargo test` (built into Rust)
- **No async test attributes:** Tests do not use `#[tokio::test]` — async tests use manual runtime blocking instead

### Dev Dependencies (from Cargo.toml)
```toml
[dev-dependencies]
tempfile = "3.14"              # Temporary directories for isolated test environments
pretty_assertions = "1.4"     # Better diff output for assert failures (but not actively used)
filetime = "0.2"              # File timestamp manipulation for incremental scrape tests
```

## Test Organization

### 1. Integration Tests (`tests/` directory)
Integration tests test the full pipeline: scrape → index → search. Each file is a separate integration test binary compiled by Cargo.

**Test files:**
- `integration_tests.rs` — Full pipeline: scrape (Claude Code, Aider, Codex, OpenCode), index, search, enrichment validation
- `context_tests.rs` — Phase 7 `context` subcommand (search + rules + file knowledge)
- `phase6_tests.rs` — Analytics, recurring problems, rules generation, weekly digest
- `pulse_report_tests.rs` — Quarterly reports (quarter parsing, monthly breakdowns, HTML rendering)
- `transcription_tests.rs` — Whisper transcription with PII redaction (job queue, retry logic)
- `render_tests.rs` — Session HTML/Markdown export
- `daemon_mcp.rs` — Daemon mode + MCP server integration
- `zero_write_tests.rs` — ADR-2 crash-safe state persistence invariant enforcement
- `aider_glob_discovery_test.rs` — Aider plugin recursive glob discovery
- `aider_toml_glob_validation_test.rs` — Aider TOML config validation
- `aider_input_scrape_test.rs` — Aider `.input.history` companion enrichment
- `subagent_*.rs` — Claude Code subagent session linking tests

### 2. Inline Unit Tests (`src/` modules)
Each `src/*.rs` file can have a `#[cfg(test)]` module with unit tests for that module's functions.

**Example from `src/analytics.rs`:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_debug() {
        let (primary, _secondary) = classify_problem_type(
            "fix the bug error crash",
            &["ErrorType:connection refused".to_string()],
            &[],
        );
        assert_eq!(primary, ProblemType::Debug);
    }
}
```

**Modules with inline tests:** 442 tests found across `src/` (analytics, capacity, rules, recurring, pulse_report, render, annotations, projects, reflect, mcp, and more)

### 3. Test Helper Module (`tests/test_helpers.rs`)
Reusable test infrastructure for setting up temp directories, configured plugins, and common scenarios.

**Key functions:**
- `setup_temp_directory()` — Creates temp dir with `.agentscribe/` subdirs (plugins, sessions, index, state)
- `create_claude_code_plugin()` — Pre-configured Claude Code plugin for testing
- Plugin builders for Aider, OpenCode, Codex patterns

## Common Test Patterns

### 1. Temp Directory Setup (Isolation)
Every test creates an isolated temporary directory using `tempfile::TempDir`. The temp dir is automatically cleaned up when dropped.

```rust
fn make_data_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    fs::create_dir_all(dir.path().join("plugins")).unwrap();
    fs::create_dir_all(dir.path().join("sessions")).unwrap();
    fs::create_dir_all(dir.path().join("state")).unwrap();
    dir
}
```

### 2. Plugin Builders (Test Fixtures)
Tests use helper functions to build configured `Plugin` objects for different agent formats.

```rust
fn jsonl_plugin(name: &str, glob: &str) -> Plugin {
    Plugin {
        plugin: PluginMeta { name: name.to_string(), version: "1.0".to_string() },
        source: Source {
            paths: vec![glob.to_string()],
            format: LogFormat::Jsonl,
            session_detection: SessionDetection::OneFilePerSession { ... },
            ...
        },
        ...
    }
}
```

### 3. Assertion Style

**Standard assertions** (most common):
```rust
assert_eq!(result.files_processed, 5);
assert!(result.sessions_scraped > 0, "no sessions scraped");
assert!(!sessions.is_empty(), "no sessions found");
```

**Multiline assertions:**
```rust
assert_eq!(
    top.source_agent,
    "claude-code",
    "top result should be claude-code session"
);
```

**Error handling with `.expect()`:**
```rust
let mut scraper = Scraper::new(data_dir.path().to_path_buf()).expect("scraper init");
scraper.scrape_plugin(&plugin).expect("scrape failed");
```

### 4. Fixture-Based Tests
Test fixtures live in `tests/fixtures/<agent>/` with real-format sample sessions from each agent type.

**Fixture directories:**
- `tests/fixtures/claude-code/` — Claude Code JSONL session samples
- `tests/fixtures/aider/` — Aider Markdown chat histories
- `tests/fixtures/codex/` — Codex rollout JSONL files (envelope format)
- `tests/fixtures/opencode/` — OpenCode JSON-tree session/message/part files
- `tests/fixtures/cursor/` — Cursor SQLite state dumps
- `tests/fixtures/windsurf/` — Windsurf SQLite state dumps
- `tests/fixtures/pi/` — Pi agent JSONL sessions
- `tests/fixtures/gemini/` — Gemini CLI JSON-array logs
- `tests/fixtures/goose/` — Goose JSONL sessions
- `tests/fixtures/edge_cases/` — Truncated files, Unicode, empty sessions, malformed JSON
- `tests/fixtures/envelope/` — Envelope unwrapping test fixtures

### 5. Performance Regression Tests
Some tests enforce performance budgets (scrape <300s for 1000 sessions, search <50ms latency, RSS <250MB during scrape).

```rust
#[test]
fn test_scrape_1000_sessions_under_60s() {
    let start = Instant::now();
    // ... generate and scrape 1000 sessions
    let duration = start.elapsed();
    assert!(duration.as_secs() < 60, "scrape took {:?}", duration);
}
```

### 6. Memory Budget Tests
Memory usage is measured on Linux via `/proc/self/status` (VmRSS field).

```rust
fn current_rss_kb() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let status = fs::read_to_string("/proc/self/status").ok()?;
        for line in status.lines() {
            if line.starts_with("VmRSS:") {
                let kb: u64 = line.split_whitespace().nth(1).and_then(|s| s.parse().ok())?;
                return Some(kb);
            }
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    { None }
}
```

### 7. File Timestamp Manipulation
Tests use the `filetime` crate to set file mtime for incremental scrape testing.

```rust
use filetime::FileTime;
ft = FileTime::from_unix_time(timestamp, 0);
filetime::set_file_mtime(path, ft).unwrap();
```

## No Property-Based Testing
AgentScribe does **not** use proptest or other property-based testing frameworks. All tests are example-based (specific inputs → specific outputs).

## No Custom Test Harness
Tests run with the standard `cargo test` harness. No custom test binaries or frameworks (no criterion benchmarks, no custom runner).

## Test Execution

### Run all tests:
```bash
cargo test
```

### Run specific test file:
```bash
cargo test --test integration_tests
```

### Run specific test function:
```bash
cargo test test_scrape_claude_code_sessions
```

### Run tests with output:
```bash
cargo test -- --nocapture
```

### Run tests in release mode (for performance tests):
```bash
cargo test --release
```

## Summary

**Test Framework:** Built-in Rust `#[test]` (cargo test)

**Test Organization:**
- Integration tests in `tests/` directory (full pipeline tests)
- Unit tests inline in `src/` modules (module-level tests)
- Shared helpers in `tests/test_helpers.rs`

**Common Patterns:**
1. Temp directory isolation with `tempfile::TempDir`
2. Plugin builder helpers for fixture setup
3. Standard `assert_eq!`, `assert!` assertions
4. Fixture-based tests with real-format samples
5. Performance regression tests (time/memory budgets)
6. File timestamp manipulation for incremental testing
7. Error propagation with `.expect()` for test failures

**No async test attributes** — tests that need async use manual runtime blocking (though most integration tests appear synchronous)
