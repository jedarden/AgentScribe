# AgentScribe Test Directory Structure

## Overview
AgentScribe uses a **hybrid test organization** with both a dedicated test directory and embedded unit tests within source files.

## Root Test Directory
**Location**: `/tests/` (at project root)

### Integration and Unit Test Files
The main test directory contains 17 test files covering various components:

- `integration_tests.rs` (70KB) - Primary integration test suite
- `daemon_mcp.rs` (20KB) - Daemon and MCP server tests
- `phase6_tests.rs` (38KB) - Phase 6 feature tests
- `pulse_report_tests.rs` (23KB) - Quarterly report generation tests
- `transcription_tests.rs` (20KB) - Audio transcription and PII redaction tests
- `parent_session_tests.rs` (25KB) - Parent session relationship tests
- `subagent_spawning_integration_tests.rs` (22KB) - Subagent spawning tests
- `subagent_parent_session_unit_tests.rs` (20KB) - Subagent parent session unit tests
- `main_session_parent_tests.rs` (13KB) - Main session parent tests
- `context_tests.rs` (9KB) - Context command tests
- `subagent_integration_test.rs` (6KB) - Subagent integration tests
- `aider_input_scrape_test.rs` (7KB) - Aider input parsing tests
- `aider_glob_discovery_test.rs` (4KB) - Aider glob discovery tests
- `aider_toml_glob_validation_test.rs` (7KB) - Aider TOML validation tests
- `render_tests.rs` (4KB) - HTML/Markdown rendering tests
- `zero_write_tests.rs` (6KB) - Edge case tests
- `test_helpers.rs` (18KB) - Shared test utilities

### Test Fixtures
**Location**: `/tests/fixtures/`

Contains test data fixtures for different agent types:
- `aider/` - Aider chat history fixtures
- `aider_input/` - Aider input history fixtures
- `claude-code/` - Claude Code JSONL fixtures
- `codex/` - Codex rollout fixtures
- `cursor/` - Cursor SQLite fixtures
- `windsurf/` - Windsurf SQLite fixtures
- `opencode/` - OpenCode JSON fixtures (session/, message/, part/)
- `pi/` - Pi agent fixtures
- `goose/` - Goose agent fixtures
- `envelope/` - Envelope unwrapping test fixtures
- `edge_cases/` - Edge case test fixtures

## Embedded Unit Tests
**Pattern**: Co-located unit tests in source files using `#[cfg(test)]` modules

**Files with embedded tests** (42+ source files):
- Core modules: `analytics.rs`, `config.rs`, `capacity.rs`, `daemon.rs`, `digest.rs`, `embedding.rs`, `gc.rs`, `index.rs`, `search.rs`, `tags.rs`
- Enrichment: `enrichment/*.rs` (all modules)
- Parsers: `parser/*.rs` (all parser modules)
- Scraping: `scraper/*.rs` (state, file_path_extractor, companion)
- Other: `annotations.rs`, `event.rs`, `file_knowledge.rs`, `mcp.rs`, `plugin.rs`, `projects.rs`, `pulse_report.rs`, `redaction.rs`, `reflect.rs`, `render.rs`, `recurring.rs`, `rules.rs`, `shell_hook.rs`, `transcription.rs`, `vector.rs`, `write_guard.rs`

**Test module pattern**:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_example() {
        // test code
    }
}
```

## Exception: Co-located Test File
**Location**: `/src/parser/jsonl/jsonl_subagent_test.rs`

Single test file in the source tree (not in `/tests/`):
- Tests JSONL subagent parsing and session detection
- 14KB, contains unit tests for the jsonl parser's subagent functionality

## Test Organization Philosophy

### Integration Tests (`/tests/`)
- **Scope**: Cross-module integration, end-to-end workflows
- **Dependencies**: Full environment setup with fixtures
- **Examples**: Full scrape pipelines, daemon lifecycle, multi-agent scenarios

### Embedded Unit Tests (source files)
- **Scope**: Module-specific unit tests, private API testing
- **Dependencies**: Minimal setup, test utilities only
- **Examples**: Individual parser functions, data structure validation, algorithm correctness

## Running Tests

```bash
# Run all tests
cargo test

# Run integration tests only
cargo test --test '*'

# Run specific test module
cargo test integration_tests

# Run embedded tests in specific source file
cargo test analytics::tests

# Run with output
cargo test -- --nocapture

# Run tests in parallel (default)
cargo test --test '*' -- --test-threads=10
```

## Test Documentation References
- `TEST_DIRECTORY_STRUCTURE.md` (this file)
- `TEST_FRAMEWORK_PATTERNS.md` - Testing patterns and utilities
- `LOOKUP_TEST_REFERENCE.md` - Lookup test catalog
- `META_ROUTING_TEST_STRUCTURE.md` - Meta routing case handling tests
- `aider_input_test_catalog.md` - Aider input test catalog
- `aider_input_test_environment_setup.md` - Aider test environment setup

## Summary
AgentScribe uses a **comprehensive hybrid test structure**:
- **Dedicated test directory** for integration and cross-module tests
- **Embedded unit tests** for 42+ source modules using `#[cfg(test)]`
- **One exception** where a test file lives alongside source code
- **Fixture library** covering all major agent log formats
- **Shared test helpers** in `/tests/test_helpers.rs` for common utilities

This structure ensures both comprehensive integration coverage and modular unit testing while keeping tests discoverable and maintainable.
