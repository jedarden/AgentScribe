# AgentScribe Test Files Catalog

Generated: 2026-08-12
Repository: /home/coding/AgentScribe

## Summary Statistics
- **Total test files found**: 61 files
- **Dedicated test files**: 18 files (in tests/ directory)
- **Source files with embedded tests**: 43 files (in src/ directory)

## Test Files by Location

### Dedicated Test Directory (`tests/`)
Total: 18 files

1. `tests/aider_glob_discovery_test.rs` - Aider plugin glob discovery patterns
2. `tests/aider_input_scrape_test.rs` - Aider input history scraping
3. `tests/aider_toml_glob_validation_test.rs` - Aider TOML configuration validation
4. `tests/context_tests.rs` - Context search functionality tests
5. `tests/daemon_mcp.rs` - Daemon MCP server integration tests
6. `tests/integration_tests.rs` - Comprehensive integration test suite
7. `tests/main_session_parent_tests.rs` - Main session parent relationship tests
8. `tests/parent_session_tests.rs` - Parent session behavior tests
9. `tests/phase6_tests.rs` - Phase 6 feature tests (analytics, rules, digest)
10. `tests/pulse_report_tests.rs` - Quarterly pulse report generation tests
11. `tests/render_tests.rs` - HTML/Markdown rendering tests
12. `tests/subagent_integration_test.rs` - Subagent integration tests
13. `tests/subagent_parent_session_unit_tests.rs` - Subagent parent session unit tests
14. `tests/subagent_spawning_integration_tests.rs` - Subagent spawning integration tests
15. `tests/test_helpers.rs` - Test utility functions and helpers
16. `tests/transcription_tests.rs` - Audio transcription tests
17. `tests/zero_write_tests.rs` - Zero-write edge case tests

**Additional test-related file:**
18. `test_timestamps.rs` (root level) - Timestamp utility tests

### Source Files with Embedded Tests (`src/`)
Total: 43 files with `#[cfg(test)]` modules

#### Core Functionality
- `src/lib.rs` - Main library entry point
- `src/config.rs` - Configuration management tests
- `src/error.rs` - Error handling tests
- `src/event.rs` - Event system tests

#### Indexing and Search
- `src/index.rs` - Tantivy index management tests
- `src/search.rs` - Search functionality tests
- `src/vector.rs` - Vector index tests (currently stubbed)

#### Scraping and Parsing
- `src/plugin.rs` - Plugin system tests
- `src/parser/mod.rs` - Parser module tests
- `src/parser/jsonl.rs` - JSONL format parser tests
- `src/parser/json_array.rs` - JSON array format parser tests
- `src/parser/json_tree.rs` - JSON tree format parser tests
- `src/parser/markdown.rs` - Markdown format parser tests
- `src/parser/sqlite.rs` - SQLite format parser tests
- `src/parser/aider_input.rs` - Aider input history parser tests
- `src/parser/jsonl/jsonl_subagent_test.rs` - JSONL subagent tests

#### Scraper Components
- `src/scraper/mod.rs` - Main scraper logic tests
- `src/scraper/state.rs` - Scrape state management tests
- `src/scraper/companion.rs` - Companion index tests
- `src/scraper/file_path_extractor.rs` - File path extraction tests

#### Enrichment Pipeline
- `src/enrichment/antipatterns.rs` - Anti-pattern detection tests
- `src/enrichment/behavioral_signals.rs` - Behavioral signal analysis tests
- `src/enrichment/code_artifacts.rs` - Code artifact extraction tests
- `src/enrichment/config_change_tracker.rs` - Config change tracking tests
- `src/enrichment/errors.rs` - Error fingerprinting tests
- `src/enrichment/git.rs` - Git integration tests
- `src/enrichment/outcome.rs` - Outcome detection tests
- `src/enrichment/solution.rs` - Solution extraction tests
- `src/enrichment/summary.rs` - Summary generation tests

#### Analytics and Reporting
- `src/analytics.rs` - Analytics computation tests
- `src/recurring.rs` - Recurring problem detection tests
- `src/rules.rs` - Auto-generated rule extraction tests
- `src/digest.rs` - Weekly digest generation tests
- `src/pulse_report.rs` - Quarterly pulse report tests

#### Additional Features
- `src/annotations.rs` - Annotation system tests
- `src/capacity.rs` - Claude Code capacity utilization tests
- `src/daemon.rs` - Daemon mode tests
- `src/embedding.rs` - Embedding queue tests (currently stubbed)
- `src/file_knowledge.rs` - File knowledge map tests
- `src/gc.rs` - Garbage collection tests
- `src/mcp.rs` - MCP server tests
- `src/projects.rs` - Project management tests
- `src/redaction.rs` - PII redaction tests
- `src/reflect.rs` - Reflection utilities tests
- `src/render.rs` - Session rendering tests
- `src/shell_hook.rs` - Shell integration tests
- `src/tags.rs` - Tag extraction tests
- `src/transcription.rs` - Audio transcription tests
- `src/write_guard.rs` - Write guard tests

## Test Organization by Category

### Integration Tests (Primary)
- `tests/integration_tests.rs` - Main integration suite
- `tests/daemon_mcp.rs` - MCP server integration
- `tests/context_tests.rs` - Context search integration
- `tests/transcription_tests.rs` - Transcription integration
- `tests/subagent_integration_test.rs` - Subagent integration
- `tests/subagent_spawning_integration_tests.rs` - Subagent spawning
- `tests/phase6_tests.rs` - Phase 6 features integration

### Unit Tests (Embedded)
- All files in `src/` with `#[cfg(test)]` modules
- Tests are co-located with implementation for better maintenance

### Specialized Test Suites
- **Aider plugin tests**: 3 files (glob discovery, input scraping, TOML validation)
- **Subagent tests**: 3 files (integration, parent session unit tests, spawning)
- **Parent session tests**: 2 files (main session, parent sessions)
- **Rendering tests**: 2 files (render tests, pulse report rendering)

## Test Fixtures
Location: `tests/fixtures/`
- Contains sample data for testing different agent formats
- Used by integration tests to verify parsing and normalization

## Test Coverage Areas

1. **Plugin System**: Plugin loading, validation, format detection
2. **Scraping**: Incremental scraping, state management, file watching
3. **Parsing**: All supported formats (JSONL, Markdown, JSON-tree, SQLite, JSON-array)
4. **Indexing**: Tantivy BM25 indexing, vector indexing (stubbed)
5. **Search**: Keyword search, semantic search (stubbed), context search
6. **Enrichment**: Outcome detection, solution extraction, error fingerprinting
7. **Analytics**: Agent effectiveness, recurring problems, capacity utilization
8. **Integration**: End-to-end workflows, MCP server, daemon mode
9. **Special Features**: Transcription, rendering, shell hooks, file knowledge

## Notes
- Vector indexing tests are currently stubbed due to turbovec BLAS dependency issues
- Some files have `.bak` extensions (backup files) and are not counted in active tests
- Test helpers are centralized in `tests/test_helpers.rs`
- Integration test suite is comprehensive (~70K lines) covering major workflows
