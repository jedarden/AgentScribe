# Testing Framework and Conventions

## Overview

AgentScribe uses **Rust's built-in test framework** with no additional testing frameworks. The codebase follows standard Rust testing patterns organized into two main categories:

1. **Unit tests** - Inline test modules within source files (`src/*.rs`)
2. **Integration tests** - Separate test files in the `tests/` directory

## Test Framework

### Primary Framework
- **Rust built-in testing**: `cargo test` runs all tests
- **No external test frameworks**: No criterion, proptest, or similar libraries
- **Standard assertions**: `assert!`, `assert_eq!`, `assert_ne!`, `unwrap()`, `expect()`

### Development Dependencies
The following testing utilities are available in `Cargo.toml`:

```toml
[dev-dependencies]
tempfile = "3.14"              # Temporary file/directory creation
pretty_assertions = "1.4"      # Better diff output in assertion failures
filetime = "0.2"               # File time manipulation for testing
```

## Test Organization

### Unit Tests (`src/`)

Unit tests are embedded directly within source files using the `#[cfg(test)]` attribute:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_name() {
        // Test implementation
        assert_eq!(result, expected);
    }
}
```

**Patterns:**
- **47 source files** contain inline test modules
- Test modules are named `mod tests`
- Tests use `use super::*` to access parent module items
- Functions are named with `test_` prefix followed by descriptive name

**Example from `src/event.rs`:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_from_str() {
        assert_eq!(Role::from_str("user"), Some(Role::User));
        assert_eq!(Role::from_str("invalid"), None);
    }

    #[test]
    fn test_event_jsonl_roundtrip() {
        let event = Event::new(/* ... */);
        let jsonl = event.to_jsonl().unwrap();
        let parsed = Event::from_jsonl(&jsonl).unwrap();
        assert_eq!(parsed.session_id, event.session_id);
    }
}
```

### Integration Tests (`tests/`)

Integration tests are separate files in the `tests/` directory that test the full pipeline:

**Structure:**
- **200+ test functions** across multiple integration test files
- Each file focuses on a specific subsystem or feature
- Tests use fixture data from `tests/fixtures/`

**Major integration test files:**
- `integration_tests.rs` - End-to-end pipeline tests (scrape, index, search)
- `context_tests.rs` - Context command tests
- `pulse_report_tests.rs` - Quarterly analytics report tests
- `render_tests.rs` - Session rendering tests
- `parent_session_tests.rs` - Parent-child session relationship tests
- `subagent_*.rs` - Subagent detection and handling tests
- `daemon_mcp.rs` - Daemon and MCP server tests
- `phase6_tests.rs` - Analytics and enrichment feature tests

## Naming Conventions

### Test Function Names

Test functions follow a descriptive pattern that indicates:
1. What is being tested
2. The expected behavior or outcome
3. Any specific conditions

**Patterns:**

```rust
// Unit tests - simple and direct
fn test_role_from_str()              // Testing a specific function
fn test_event_jsonl_roundtrip()      // Testing round-trip serialization

// Integration tests - more descriptive
fn test_scrape_claude_code_sessions()           // Testing agent-specific scraping
fn test_full_pipeline_end_to_end()              // Testing complete workflow
fn test_outcome_detection_success_session()     // Testing specific outcome
fn test_search_latency_under_50ms()             // Performance requirements
fn test_cross_session_error_fingerprint_correlation()  // Cross-session features
```

### Test File Names

Integration test files are named descriptively based on what they test:

```
tests/
├── integration_tests.rs           # Main end-to-end tests
├── context_tests.rs               # Context command tests
├── pulse_report_tests.rs         # Pulse report functionality
├── render_tests.rs                # Session rendering tests
├── parent_session_tests.rs        # Parent-child session tests
├── subagent_integration_test.rs   # Subagent integration tests
├── daemon_mcp.rs                  # Daemon and MCP tests
├── aider_*.rs                     # Aider-specific tests
└── test_helpers.rs                # Shared test utilities
```

## Test Utilities and Helpers

### test_helpers.rs Module

The `tests/test_helpers.rs` file provides reusable test infrastructure:

**Key helper functions:**

```rust
/// Set up a temporary directory with standard AgentScribe layout
pub fn setup_temp_directory() -> tempfile::TempDir

/// Create a configured claude-code plugin for testing
pub fn create_claude_code_plugin(base_path: &Path) -> Plugin

/// Create a minimal parser for simple JSONL tests
pub fn create_simple_parser() -> Parser

/// Create a basic test plugin for simple JSONL parsing
pub fn create_test_plugin() -> Plugin

/// Create a plugin with envelope routing configured
pub fn create_envelope_plugin() -> Plugin

/// Helper for testing meta routing fixture lines
pub fn assert_meta_routing_returns_empty(
    fixture_line: &str,
    line_number: usize,
    assertion_message: &str,
)
```

### Common Test Setup Patterns

**Temporary directory creation:**
```rust
fn make_data_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    fs::create_dir_all(dir.path().join("plugins")).unwrap();
    fs::create_dir_all(dir.path().join("sessions")).unwrap();
    fs::create_dir_all(dir.path().join("state")).unwrap();
    dir
}
```

**Fixture directory access:**
```rust
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}
```

## Test Fixtures

### Fixture Directory Structure

```
tests/fixtures/
├── aider/              # Aider chat history fixtures
├── aider_input/        # Aider input history fixtures
├── claude-code/        # Claude Code JSONL fixtures
├── codex/              # Codex rollout fixtures
├── cursor/             # Cursor SQLite fixtures
├── envelope/           # Envelope routing fixtures
├── goose/              # Goose session fixtures
├── opencode/           # OpenCode session fixtures
├── pi/                 # Pi agent fixtures
├── windsurf/           # Windsurf fixtures
├── edge_cases/         # Edge case test data
└── subagent_test.jsonl # Subagent detection fixtures
```

### Fixture Usage

Fixtures are used to test parser implementations against real-world data:

```rust
#[test]
fn test_scrape_claude_code_sessions() {
    let fixtures = fixtures_dir().join("claude-code");
    let plugin = jsonl_plugin("claude-code", &format!("{}/**/*.jsonl", fixtures.display()));
    
    let scraper = Scraper::new(/* ... */).unwrap();
    let results = scraper.scrape().unwrap();
    
    assert!(results.sessions_scraped > 0);
}
```

## Test Coverage by Type

### 1. Unit Tests (Source Files)
- **47 source files** with inline test modules
- Focus on single-function behavior
- Fast execution (<1ms per test typically)
- No external dependencies (filesystem, network)

### 2. Integration Tests
- **200+ test functions** across multiple test files
- **End-to-end pipeline tests**: scrape → index → search → verify
- **Agent-specific tests**: Claude Code, Aider, Codex, OpenCode, Cursor, Windsurf
- **Feature tests**: outcome detection, error fingerprinting, solution extraction
- **Performance tests**: 1000-session scrape <300s, search <50ms

### 3. Edge Case Tests
- Truncated files
- Unicode content
- Empty sessions
- Invalid JSON
- Malformed timestamps

## Running Tests

### Run All Tests
```bash
cargo test
```

### Run Specific Test File
```bash
cargo test --test integration_tests
cargo test --test context_tests
```

### Run Specific Test Function
```bash
cargo test test_scrape_claude_code_sessions
```

### Run Tests with Output
```bash
cargo test -- --nocapture    # Show println! output
cargo test -- --show-output  # Show test output
```

### Run Only Unit Tests (in source files)
```bash
cargo test --lib
```

### Run Only Integration Tests
```bash
cargo test --tests
```

## Test Documentation Patterns

### Module Documentation

Test files include module-level documentation explaining what they test:

```rust
//! Context command tests
//!
//! Tests the `context_pack` function which provides pre-task priming for agent workers.
//! Validates:
//!   - Empty index returns empty output
//!   - Non-empty index returns formatted block
//!   - Token budget truncates correctly
//!   - --json output is valid JSON
//!   - File path extraction from task descriptions
```

### Test Helper Documentation

Test helper functions include comprehensive documentation:

```rust
/// Set up a temporary directory with the standard AgentScribe layout
///
/// Creates a temporary directory with the following structure:
/// - `.agentscribe/plugins/` - For plugin definitions
/// - `.agentscribe/sessions/` - For normalized session files
/// - `.agentscribe/index/` - For search indices
/// - `.agentscribe/state/` - For scrape state
///
/// # Returns
///
/// A `tempfile::TempDir` that will be automatically cleaned up when dropped.
///
/// # Example
///
/// ```ignore
/// let temp_dir = setup_temp_directory();
/// let data_dir = temp_dir.path().join(".agentscribe");
/// // Use data_dir for testing
/// ```
pub fn setup_temp_directory() -> tempfile::TempDir
```

## Performance Testing

Some integration tests include performance requirements:

```rust
#[test]
fn test_scrape_1000_sessions_under_60s() {
    let start = Instant::now();
    // ... scrape 1000 sessions ...
    let duration = start.elapsed();
    assert!(duration.as_secs() < 60, "Scrape took {:?}", duration);
}

#[test]
fn test_search_latency_under_50ms() {
    // ... setup index ...
    let start = Instant::now();
    let results = execute_search(/* ... */).unwrap();
    let duration = start.elapsed();
    assert!(duration.as_millis() < 50, "Search took {:?}", duration);
}
```

## Memory Testing

Integration tests can monitor RSS (Resident Set Size) on Linux:

```rust
/// Read RSS (Resident Set Size) in kilobytes from /proc/self/status.
/// Returns None on non-Linux platforms.
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
    {
        None
    }
}
```

## Common Test Patterns

### Pattern 1: Plugin Creation
```rust
fn jsonl_plugin(name: &str, glob: &str) -> Plugin {
    Plugin {
        plugin: PluginMeta {
            name: name.to_string(),
            version: "1.0".to_string(),
        },
        source: Source {
            paths: vec![glob.to_string()],
            exclude: vec![],
            format: LogFormat::Jsonl,
            session_detection: SessionDetection::OneFilePerSession {
                session_id_from: SessionIdSource::Filename,
            },
            // ... additional configuration
        },
        // ... parser configuration
    }
}
```

### Pattern 2: Round-trip Serialization
```rust
#[test]
fn test_event_jsonl_roundtrip() {
    let event = Event::new(/* ... */);
    let jsonl = event.to_jsonl().unwrap();
    let parsed = Event::from_jsonl(&jsonl).unwrap();
    assert_eq!(parsed.session_id, event.session_id);
}
```

### Pattern 3: Empty Index Behavior
```rust
#[test]
fn test_context_empty_index() {
    let data_dir = make_data_dir();
    // Create empty index
    let result = context_pack(data_dir.path(), "implement auth feature", 3000, None);
    assert!(result.is_ok());
    let output = result.unwrap().format_text();
    assert!(output.contains("No prior context found") || output.is_empty());
}
```

## No Property-Based Testing

AgentScribe does **not** use property-based testing frameworks like:
- `proptest`
- `quickcheck`
- `hypothesis` (Python)

All tests are example-based rather than property-based.

## Continuous Integration

Tests run on CI via `cargo test` as part of the standard build process. The project uses:
- **Cargo's built-in test runner**
- **Standard test assertions** (`assert!`, `assert_eq!`)
- **Pretty assertions** for better failure messages (`pretty_assertions` crate)

## Summary

**Test Framework**: Rust built-in testing (`cargo test`)  
**Test Organization**: 47 source files with inline tests + 10+ integration test files  
**Total Test Count**: 200+ test functions  
**Testing Dependencies**: tempfile, pretty_assertions, filetime  
**Test Utilities**: `tests/test_helpers.rs` with reusable fixtures and helpers  
**Fixture Organization**: `tests/fixtures/` by agent type and feature  
**Naming Convention**: `test_<what>_<condition>_<expected>()` pattern  
**No Property Tests**: All tests are example-based, no proptest/quickcheck

---

*Generated: 2026-08-12*  
*AgentScribe Testing Framework Documentation*
