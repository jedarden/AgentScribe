# AgentScribe Test Files Catalog

Generated: 2026-08-12
Project: /home/coding/AgentScribe

## Summary

- **Total test files found:** 51
- **Integration test files:** 17 (in `/tests` directory)
- **Unit test files (with embedded tests):** 34 (in `/src` directory)
- **Dedicated unit test files:** 2

---

## Integration Test Files (`/tests` directory)

These files contain integration tests and test helpers:

1. `/home/coding/AgentScribe/tests/aider_glob_discovery_test.rs` - Aider glob discovery pattern tests
2. `/home/coding/AgentScribe/tests/aider_input_scrape_test.rs` - Aider input history scraping tests
3. `/home/coding/AgentScribe/tests/aider_toml_glob_validation_test.rs` - Aider TOML validation tests
4. `/home/coding/AgentScribe/tests/context_tests.rs` - Context search and priming tests
5. `/home/coding/AgentScribe/tests/daemon_mcp.rs` - Daemon MCP server integration tests
6. `/home/coding/AgentScribe/tests/integration_tests.rs` - General integration test suite
7. `/home/coding/AgentScribe/tests/main_session_parent_tests.rs` - Main session parent relationship tests
8. `/home/coding/AgentScribe/tests/parent_session_tests.rs` - Parent session relationship tests
9. `/home/coding/AgentScribe/tests/phase6_tests.rs` - Phase 6 feature tests (analytics, rules, digest)
10. `/home/coding/AgentScribe/tests/pulse_report_tests.rs` - Quarterly pulse report tests
11. `/home/coding/AgentScribe/tests/render_tests.rs` - HTML/Markdown rendering tests
12. `/home/coding/AgentScribe/tests/subagent_integration_test.rs` - Subagent integration tests
13. `/home/coding/AgentScribe/tests/subagent_parent_session_unit_tests.rs` - Subagent parent session unit tests
14. `/home/coding/AgentScribe/tests/subagent_spawning_integration_tests.rs` - Subagent spawning integration tests
15. `/home/coding/AgentScribe/tests/test_helpers.rs` - Test helper utilities and fixtures
16. `/home/coding/AgentScribe/tests/transcription_tests.rs` - Audio transcription and PII redaction tests
17. `/home/coding/AgentScribe/tests/zero_write_tests.rs` - Zero-write behavior tests

---

## Dedicated Unit Test Files

### Parser Module
1. `/home/coding/AgentScribe/src/parser/jsonl/jsonl_subagent_test.rs` - JSONL subagent event parsing tests

### Project Root
2. `/home/coding/AgentScribe/test_timestamps.rs` - Timestamp handling tests

---

## Source Files with Embedded Unit Tests

These 34 files in `/src` contain `#[cfg(test)]` modules with unit tests:

### Core Modules
1. `/home/coding/AgentScribe/src/analytics.rs` - Analytics and metrics tests
2. `/home/coding/AgentScribe/src/annotations.rs` - Annotation handling tests
3. `/home/coding/AgentScribe/src/capacity.rs` - Capacity utilization tests
4. `/home/coding/AgentScribe/src/config.rs` - Configuration parsing and validation tests
5. `/home/coding/AgentScribe/src/daemon.rs` - Daemon lifecycle tests
6. `/home/coding/AgentScribe/src/digest.rs` - Weekly digest generation tests
7. `/home/coding/AgentScribe/src/embedding.rs` - Embedding model tests (stub)
8. `/home/coding/AgentScribe/src/event.rs` - Canonical event schema tests
9. `/home/coding/AgentScribe/src/file_knowledge.rs` - File knowledge map tests
10. `/home/coding/AgentScribe/src/gc.rs` - Garbage collection tests
11. `/home/coding/AgentScribe/src/index.rs` - Tantivy index schema and operations tests
12. `/home/coding/AgentScribe/src/mcp.rs` - MCP server tests
13. `/home/coding/AgentScribe/src/plugin.rs` - Plugin manifest parsing and validation tests
14. `/home/coding/AgentScribe/src/projects.rs` - Project detection tests
15. `/home/coding/AgentScribe/src/pulse_report.rs` - Quarterly report generation tests
16. `/home/coding/AgentScribe/src/recurring.rs` - Recurring problem detection tests
17. `/home/coding/AgentScribe/src/redaction.rs` - PII redaction pattern tests
18. `/home/coding/AgentScribe/src/reflect.rs` - Reflection and self-modification tests
19. `/home/coding/AgentScribe/src/render.rs` - Session rendering tests
20. `/home/coding/AgentScribe/src/rules.rs` - Auto-generated project rules tests
21. `/home/coding/AgentScribe/src/search.rs` - Search query execution and ranking tests
22. `/home/coding/AgentScribe/src/shell_hook.rs` - Shell hook generation tests
23. `/home/coding/AgentScribe/src/tags.rs` - Tag extraction tests
24. `/home/coding/AgentScribe/src/transcription.rs` - Transcription queue and retry tests
25. `/home/coding/AgentScribe/src/vector.rs` - Vector index tests (stub - non-functional)
26. `/home/coding/AgentScribe/src/write_guard.rs` - Write guard synchronization tests

### Enrichment Modules
27. `/home/coding/AgentScribe/src/enrichment/antipatterns.rs` - Anti-pattern detection tests
28. `/home/coding/AgentScribe/src/enrichment/behavioral_signals.rs` - Behavioral signal extraction tests
29. `/home/coding/AgentScribe/src/enrichment/code_artifacts.rs` - Code artifact extraction tests
30. `/home/coding/AgentScribe/src/enrichment/config_change_tracker.rs` - Config change tracking tests
31. `/home/coding/AgentScribe/src/enrichment/errors.rs` - Error fingerprinting tests
32. `/home/coding/AgentScribe/src/enrichment/git.rs` - Git commit correlation tests
33. `/home/coding/AgentScribe/src/enrichment/outcome.rs` - Outcome detection tests
34. `/home/coding/AgentScribe/src/enrichment/solution.rs` - Solution extraction tests
35. `/home/coding/AgentScribe/src/enrichment/summary.rs` - Summary generation tests

### Parser Modules
36. `/home/coding/AgentScribe/src/parser/aider_input.rs` - Aider input history parser tests
37. `/home/coding/AgentScribe/src/parser/json_array.rs` - JSON array format parser tests
38. `/home/coding/AgentScribe/src/parser/jsonl.rs` - JSONL format parser tests
39. `/home/coding/AgentScribe/src/parser/json_tree.rs` - JSON tree format parser tests
40. `/home/coding/AgentScribe/src/parser/markdown.rs` - Markdown format parser tests
41. `/home/coding/AgentScribe/src/parser/mod.rs` - Parser module tests
42. `/home/coding/AgentScribe/src/parser/sqlite.rs` - SQLite format parser tests

### Scraper Modules
43. `/home/coding/AgentScribe/src/scraper/companion.rs` - Companion index tests
44. `/home/coding/AgentScribe/src/scraper/file_path_extractor.rs` - File path extraction tests
45. `/home/coding/AgentScribe/src/scraper/mod.rs` - Scraper orchestration tests
46. `/home/coding/AgentScribe/src/scraper/state.rs` - Scrape state persistence tests

---

## Test Statistics

- **Total integration test files:** 17
- **Total unit test locations:** 34 source files with embedded tests + 2 dedicated unit test files = 36
- **Total test files:** 51 (Note: Some files may be counted in both categories if they have both integration tests and embedded unit tests)
- **Total Rust files in project:** 70
- **Test coverage ratio:** ~73% of Rust files contain tests

---

## Running All Tests

To run all tests in the AgentScribe codebase:

```bash
# Run all tests (unit + integration)
cargo test

# Run only unit tests (skip integration tests in /tests directory)
cargo test --lib

# Run only integration tests
cargo test --test '*'

# Run tests with output
cargo test -- --nocapture

# Run tests in parallel
cargo test -- --test-threads=4
```

---

## Notes

1. **Stub Implementation:** Several test files cover stub/non-functional implementations:
   - `src/embedding.rs` - Embedding pipeline (stub)
   - `src/vector.rs` - Vector index (non-functional stub due to turbovec BLAS linking issues)

2. **Test Helpers:** The `/tests/test_helpers.rs` file provides common utilities and fixtures used across multiple integration tests.

3. **Test Organization:**
   - Unit tests are embedded in the same files as the code they test (using `#[cfg(test)]` modules)
   - Integration tests are separate files in the `/tests` directory
   - Some dedicated unit test files exist for specific subsystems (e.g., `jsonl_subagent_test.rs`)

4. **Special Test Files:**
   - `test_timestamps.rs` at project root - appears to be a standalone timestamp validation test
   - `daemon_mcp.rs` - MCP server integration tests
   - `phase6_tests.rs` - Tests for Phase 6 features (analytics, recurring problems, auto-generated rules, weekly digest)
