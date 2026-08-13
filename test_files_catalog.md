# AgentScribe Test Files Catalog

Generated: 2026-08-13

## Summary

- **Total standalone test files**: 19
- **Source files with test modules**: 47
- **Total test-containing files**: 66

## Standalone Test Files (19)

Files in `tests/` directory or matching `*test*.rs` pattern:

| File | Size | Module |
|------|------|--------|
| tests/integration_tests.rs | 69K | integration_tests |
| tests/main_session_parent_tests.rs | 14K | main_session_parent_tests |
| tests/parent_session_tests.rs | 26K | parent_session_tests |
| tests/subagent_spawning_integration_tests.rs | 23K | subagent_spawning_integration_tests |
| tests/subagent_parent_session_unit_tests.rs | 21K | subagent_parent_session_unit_tests |
| tests/phase6_tests.rs | 38K | phase6_tests |
| tests/pulse_report_tests.rs | 24K | pulse_report_tests |
| tests/test_helpers.rs | 19K | test_helpers |
| tests/context_tests.rs | 9.8K | context_tests |
| tests/daemon_mcp.rs | 21K | daemon_mcp |
| tests/transcription_tests.rs | 21K | transcription_tests |
| tests/aider_input_scrape_test.rs | 7.2K | aider_input_scrape_test |
| tests/aider_toml_glob_validation_test.rs | 7.4K | aider_toml_glob_validation_test |
| tests/aider_glob_discovery_test.rs | 4.3K | aider_glob_discovery_test |
| tests/render_tests.rs | 4.5K | render_tests |
| tests/subagent_integration_test.rs | 6.3K | subagent_integration_test |
| tests/zero_write_tests.rs | 6.0K | zero_write_tests |
| test_timestamps.rs | 2.1K | test_timestamps |
| src/parser/jsonl/jsonl_subagent_test.rs | 15K | jsonl_subagent_test |

**Total standalone test files: 363.3 KB**

## Source Files with Test Modules (47)

Files in `src/` containing `#[cfg(test)]` modules:

| File | Size | Module |
|------|------|--------|
| src/search.rs | 216K | search |
| src/daemon.rs | 61K | daemon |
| src/pulse_report.rs | 60K | pulse_report |
| src/index.rs | 60K | index |
| src/capacity.rs | 57K | capacity |
| src/scraper/mod.rs | 54K | scraper |
| src/analytics.rs | 44K | analytics |
| src/parser/jsonl.rs | 153K | parser::jsonl |
| src/parser/mod.rs | 39K | parser |
| src/parser/sqlite.rs | 33K | parser::sqlite |
| src/vector.rs | 33K | vector |
| src/mcp.rs | 34K | mcp |
| src/transcription.rs | 34K | transcription |
| src/rules.rs | 29K | rules |
| src/reflect.rs | 28K | reflect |
| src/redaction.rs | 28K | redaction |
| src/parser/json_array.rs | 26K | parser::json_array |
| src/recurring.rs | 25K | recurring |
| src/file_knowledge.rs | 24K | file_knowledge |
| src/tags.rs | 22K | tags |
| src/parser/markdown.rs | 23K | parser::markdown |
| src/digest.rs | 21K | digest |
| src/plugin.rs | 21K | plugin |
| src/projects.rs | 21K | projects |
| src/config.rs | 18K | config |
| src/enrichment/errors.rs | 15K | enrichment::errors |
| src/enrichment/antipatterns.rs | 15K | enrichment::antipatterns |
| src/annotations.rs | 15K | annotations |
| src/render.rs | 17K | render |
| src/enrichment/behavioral_signals.rs | 16K | enrichment::behavioral_signals |
| src/enrichment/config_change_tracker.rs | 16K | enrichment::config_change_tracker |
| src/enrichment/outcome.rs | 15K | enrichment::outcome |
| src/scraper/state.rs | 19K | scraper::state |
| src/parser/json_tree.rs | 13K | parser::json_tree |
| src/embedding.rs | 12K | embedding |
| src/shell_hook.rs | 9.2K | shell_hook |
| src/event.rs | 8.8K | event |
| src/enrichment/git.rs | 8.8K | enrichment::git |
| src/gc.rs | 9.6K | gc |
| src/parser/aider_input.rs | 9.3K | parser::aider_input |
| src/enrichment/code_artifacts.rs | 6.9K | enrichment::code_artifacts |
| src/enrichment/solution.rs | 6.9K | enrichment::solution |
| src/enrichment/summary.rs | 6.8K | enrichment::summary |
| src/scraper/file_path_extractor.rs | 11K | scraper::file_path_extractor |
| src/scraper/companion.rs | 10K | scraper::companion |
| src/write_guard.rs | 5.0K | write_guard |

**Total source files with test modules: 1.1 MB**

## Test Coverage by Module Area

### Core Functionality
- **Search** (216K) - Largest test coverage
- **Index** (60K) - Tantivy integration
- **Analytics** (44K) - Session analytics and metrics
- **Daemon** (61K) - Background process tests

### Parser & Scraping
- **JSONL Parser** (153K) - Core JSONL parsing
- **Parser Module** (39K) - Format-agnostic tests
- **SQLite Parser** (33K) - Cursor/Windsurf support
- **Markdown Parser** (23K) - Aider log parsing
- **JSON Array Parser** (26K) - Gemini CLI support
- **Scraper** (54K) - Scraping orchestration
- **Scraper State** (19K) - State persistence

### Enrichment & Intelligence
- **Errors** (15K) - Error fingerprinting
- **Antipatterns** (15K) - Failed pattern detection
- **Outcome** (15K) - Session outcome classification
- **Behavioral Signals** (16K) - Behavior analysis
- **Code Artifacts** (6.9K) - Code extraction
- **Solution** (6.9K) - Solution extraction
- **Git** (8.8K) - Git integration

### Special Features
- **Pulse Report** (60K) - Quarterly reporting
- **Capacity** (57K) - Claude Code capacity tracking
- **Transcription** (34K) - Whisper integration
- **Vector** (33K) - Semantic search (STUB)
- **MCP** (34K) - MCP server
- **Rules** (29K) - Auto-generated rules
- **File Knowledge** (24K) - File-level session mapping
- **Recurring** (25K) - Recurring problem detection
- **Digest** (21K) - Weekly digest
- **Context** (9.8K) - Pre-task context
- **Render** (17K) - HTML/Markdown rendering
- **Redaction** (28K) - PII redaction
- **Reflect** (28K) - Reflection utilities

### Integration Tests
- **Integration Tests** (69K) - Comprehensive integration
- **Parent Session Tests** (26K) - Session hierarchy
- **Subagent Tests** (21K-23K) - Subagent spawning
- **Phase 6 Tests** (38K) - Analytics features
- **Pulse Report Tests** (24K) - Report generation
- **Context Tests** (9.8K) - Context generation
- **Daemon MCP Tests** (21K) - Daemon + MCP integration
- **Transcription Tests** (21K) - Audio transcription

### Plugin-Specific Tests
- **Aider Input Scrape** (7.2K)
- **Aider TOML Glob Validation** (7.4K)
- **Aider Glob Discovery** (4.3K)

### Utility Tests
- **Test Helpers** (19K) - Test utilities
- **Timestamp Tests** (2.1K)
- **Zero Write Tests** (6.0K) - Crash safety tests
- **Render Tests** (4.5K)
