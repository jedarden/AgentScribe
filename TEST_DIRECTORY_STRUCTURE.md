# AgentScribe Test Directory Structure

## Overview

AgentScribe uses a hybrid testing approach with both integration tests in a dedicated `tests/` directory and embedded unit tests co-located with source code.

## Root Test Directory: `/tests/`

The primary test directory is located at the repository root and contains integration and end-to-end tests.

### Test Files

| File | Purpose |
|------|---------|
| `integration_tests.rs` | Main integration test suite |
| `context_tests.rs` | Context command tests |
| `daemon_mcp.rs` | Daemon mode and MCP server tests |
| `render_tests.rs` | HTML/Markdown rendering tests |
| `transcription_tests.rs` | Audio transcription tests |
| `pulse_report_tests.rs` | Quarterly reporting tests |
| `phase6_tests.rs` | Phase 6 feature tests |
| `parent_session_tests.rs` | Parent session handling tests |
| `main_session_parent_tests.rs` | Main session parent tests |
| `subagent_integration_test.rs` | Subagent integration tests |
| `subagent_parent_session_unit_tests.rs` | Subagent parent session unit tests |
| `subagent_spawning_integration_tests.rs` | Subagent spawning tests |
| `aider_glob_discovery_test.rs` | Aider glob pattern discovery tests |
| `aider_toml_glob_validation_test.rs` | Aider TOML validation tests |
| `aider_input_scrape_test.rs` | Aider input history scraping tests |
| `zero_write_tests.rs` | Zero-write operation tests |
| `test_helpers.rs` | Shared test utilities and helpers |

### Test Fixtures: `/tests/fixtures/`

Test fixtures are organized by agent type and use case:

#### Agent-Specific Fixtures

| Directory | Agent Type | Purpose |
|-----------|-----------|---------|
| `fixtures/claude-code/` | Claude Code | JSONL session files and metadata |
| `fixtures/aider/` | Aider | Markdown chat history and input history |
| `fixtures/codex/` | Codex | JSONL rollout files with envelope unwrapping |
| `fixtures/opencode/` | OpenCode | JSON tree sessions, messages, and parts |
| `fixtures/cursor/` | Cursor | SQLite state.vscdb test databases |
| `fixtures/windsurf/` | Windsurf | SQLite state.vscdb test databases |
| `fixtures/pi/` | Pi | JSONL session files |
| `fixtures/gemini-cli/` | Gemini CLI | JSON array logs and checkpoints |
| `fixtures/goose/` | Goose | JSONL session files |

#### Specialized Fixtures

| Directory | Purpose |
|-----------|---------|
| `fixtures/envelope/` | Envelope unwrapping test cases |
| `fixtures/edge_cases/` | Edge case scenarios (malformed data, truncation, etc.) |
| `fixtures/aider_input/` | Aider input history parsing |

## Embedded Unit Tests

### Co-located Tests Pattern

Unit tests are embedded directly in source files using Rust's `#[cfg(test)]` attribute. This keeps tests close to the code they test.

#### Statistics

- **Total files with embedded tests**: 48
- **Approach**: Tests defined in `#[cfg(test)]` modules within source files

#### Key Files with Embedded Tests

**Core functionality**:
- `src/capacity.rs` - Capacity tracking tests
- `src/config.rs` - Configuration parsing tests
- `src/index.rs` - Tantivy indexing tests
- `src/search.rs` - Search functionality tests
- `src/plugin.rs` - Plugin system tests
- `src/daemon.rs` - Daemon mode tests

**Analytics and reporting**:
- `src/analytics.rs` - Analytics computation tests
- `src/recurring.rs` - Recurring problem detection tests
- `src/pulse_report.rs` - Quarterly reporting tests
- `src/digest.rs` - Weekly digest tests

**Data processing**:
- `src/tags.rs` - Tag extraction tests
- `src/transcription.rs` - Transcription tests
- `src/redaction.rs` - PII redaction tests
- `src/embedding.rs` - Embedding pipeline tests

**Parser modules**:
- `src/parser/mod.rs` - Parser interface tests
- `src/parser/jsonl/` - JSONL format parser tests

**Scraping and enrichment**:
- `src/scraper/mod.rs` - Scraper framework tests
- `src/enrichment/` - Enrichment pipeline tests (in module files)

### Standalone Test Files in Source

| File | Purpose |
|------|---------|
| `src/parser/jsonl/jsonl_subagent_test.rs` | JSONL subagent parsing tests |

## Test Organization Strategy

### Separation of Concerns

1. **Integration tests** (`tests/`): Test end-to-end workflows, CLI commands, and cross-module interactions
2. **Unit tests** (embedded in `src/`): Test individual functions, modules, and internal logic
3. **Fixtures** (`tests/fixtures/`): Provide standardized test data for reproducible testing

### Benefits

- **Co-located unit tests**: Easy to find and maintain alongside the code they test
- **Centralized integration tests**: Clear separation for end-to-end testing
- **Organized fixtures**: Agent-specific fixtures make it easy to add test coverage for new agent types
- **Shared utilities**: `test_helpers.rs` provides common test utilities across the test suite

## Test Discovery

Rust's test framework automatically discovers:

1. **Integration tests**: All files in `tests/` directory
2. **Unit tests**: All `#[cfg(test)]` modules in source files
3. **Documentation tests**: Examples in doc comments (if present)

## Running Tests

```bash
# Run all tests
cargo test

# Run only integration tests
cargo test --test integration_tests

# Run only unit tests in a specific file
cargo test --lib capacity

# Run with output
cargo test -- --nocapture

# Run tests in a specific module
cargo test capacity::tests::test_function_name
```

## Test Data Management

- **Fixture isolation**: Each agent type has its own fixture directory
- **Real data samples**: Fixtures are captured from real agent installations (sanitized)
- **Edge case coverage**: Dedicated `edge_cases/` directory for malformed data testing
- **Incremental testing**: Fixtures support testing incremental scraping scenarios

## Summary

AgentScribe uses a well-organized test structure:

1. **Primary test directory**: `/tests/` with integration tests and fixtures
2. **Embedded unit tests**: 48 source files with `#[cfg(test)]` modules
3. **Fixture organization**: Agent-specific fixtures under `tests/fixtures/`
4. **Test helpers**: Shared utilities in `tests/test_helpers.rs`

This structure provides clear separation between integration and unit testing while keeping tests close to the code they test for maintainability.