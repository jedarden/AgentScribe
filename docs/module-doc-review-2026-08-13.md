# Module-Level Documentation Review
**Date:** 2026-08-13
**Reviewed by:** Automated review task
**Scope:** All Rust modules in `/home/coding/AgentScribe/src/`

## Summary

Reviewed 30+ Rust source files for module-level documentation quality. Found 7 issues requiring attention, ranging from minimal documentation to incorrect content.

---

## Issues Found

### 🔴 CRITICAL: Incorrect module documentation

**File:** `src/parser/mod.rs` (lines 1-80)
**Issue:** Module documentation describes Rust import parsing (`Import`, `ImportType`, `ImportParser`) but this module actually contains format parsers for different log formats (JSONL, Markdown, JSON-tree, SQLite, JSON-array).

**Evidence:**
```rust
//! # Import Types
//!
//! This module provides types for working with Rust import statements:
//!
//! - **[`Import`]**: Lightweight struct representing a single import statement with path,
//!   type, and line number information
//! - **[`ImportType`]**: Enum representing the three kinds of Rust import statements
//!   (`use`, `extern crate`, and `mod`)
```

**Expected:** Documentation should describe:
- The format parser implementations (JsonlParser, MarkdownParser, etc.)
- How each parser normalizes events to the canonical schema
- When to use each parser type

**Impact:** HIGH - This documentation is actively misleading. Anyone reading this module doc will be completely confused about what the module actually does.

**Recommendation:** Replace the entire module doc with:
```rust
//! Parser implementations for different log formats
//!
//! This module provides format-specific parsers that normalize raw agent logs
//! into the canonical event schema. Each parser handles one log format:
//!
//! - **[`JsonlParser`]**: JSONL format (Claude Code, Codex, Goose)
//! - **[`MarkdownParser`]**: Markdown format (Aider)
//! - **[`JsonTreeParser`]**: JSON tree format (OpenCode legacy)
//! - **[`JsonArrayParser`]**: JSON array format (Gemini CLI)
//! - **[`SqliteParser`]**: SQLite format (Cursor, Windsurf)
//!
//! # When to Use
//!
//! These parsers are used internally by the scraper plugin system. Each plugin
//! specifies its format in the `[source] format` field, and the corresponding
//! parser is instantiated to normalize events.
//!
//! # Parser Behavior
//!
//! All parsers:
//! - Read source files in streaming fashion
//! - Extract timestamps, roles, content, and metadata
//! - Normalize to the canonical [`Event`](crate::event::Event) schema
//! - Handle errors gracefully (skip bad lines, log warnings)
```

---

### 🟡 MAJOR: Insufficient module documentation

#### Issue 1: lib.rs
**File:** `src/lib.rs` (lines 1-36)
**Issue:** Only one line of documentation for the entire library crate.

**Current:**
```rust
//! AgentScribe library — exposes modules for integration testing and external use.
```

**Problems:**
- No explanation of what the library provides
- No overview of the architecture or module organization
- No examples or usage guidance
- No explanation of when to use this library vs. the CLI
- Inconsistent with other modules that have rich documentation

**Recommendation:** Expand to include:
```rust
//! AgentScribe — Archive, index, and extract intelligence from AI coding agent conversations
//!
//! This library provides the core functionality for scraping conversation logs from
//! multiple AI coding agents (Claude Code, Aider, OpenCode, Codex, Cursor, Windsurf),
//! normalizing them into a unified searchable format.
//!
//! # Architecture
//!
//! AgentScribe is organized into several subsystems:
//!
//! - **Scraping** ([`scraper`]): Plugin-based log discovery and parsing
//! - **Indexing** ([`index`]): Tantivy full-text search index
//! - **Search** ([`search`]): Query interface with multiple modes
//! - **Analytics** ([`analytics`]): Cross-agent performance metrics
//! - **Enrichment** ([`enrichment`]): Outcome detection, error fingerprinting, anti-patterns
//!
//! # When to Use This Library
//!
//! Use this library when you need to:
//! - Integrate AgentScribe into a Rust application
//! - Write integration tests
//! - Build custom tools on top of AgentScribe's data
//!
//! For command-line usage, prefer the `agentscribe` CLI binary.
//!
//! # Example
//!
//! ```no_run
//! use agentscribe::{search, search::SearchOptions};
//!
//! let results = search::execute_search(
//!     &data_dir,
//!     &SearchOptions {
//!         query: Some("database connection".to_string()),
//!         ..Default::default()
//!     }
//! )?;
//! ```
```

---

#### Issue 2: scraper/mod.rs
**File:** `src/scraper/mod.rs` (lines 1-4)
**Issue:** Extremely terse documentation for a critical orchestration module.

**Current:**
```rust
//! Scraping orchestration
//!
//! Coordinates plugin loading, file discovery, parsing, and state management.
```

**Problems:**
- Doesn't explain the scraping process or data flow
- No examples
- Doesn't explain when to use it
- Missing important details about incremental scraping, state management, error handling

**Recommendation:** Expand to match the detail level of `config.rs` or `event.rs`:
```rust
//! Scraping orchestration — plugin loading, file discovery, parsing, and indexing
//!
//! This module coordinates the entire scraping pipeline:
//!
//! # Scraping Pipeline
//!
//! 1. **Plugin Loading**: Load plugin definitions from `~/.agentscribe/plugins/`
//! 2. **File Discovery**: Expand glob patterns, find log files
//! 3. **Session Detection**: Identify session boundaries (one-file-per-session, delimiter, etc.)
//! 4. **Parsing**: Format-specific parsers normalize to canonical [`Event`](crate::event::Event) schema
//! 5. **State Management**: Track byte offsets for incremental scraping
//! 6. **Indexing**: Update Tantivy search index with new sessions
//!
//! # Incremental Scraping
//!
//! Scrape state is tracked per-source-file:
//! - JSONL: byte offset (`last_byte_offset`)
//! - Markdown: delimiter offset (`last_delimiter_offset`)
//! - SQLite: mtime-based full reparse
//!
//! State is persisted in `state/scrape-state.json` and survives daemon restarts.
//!
//! # Error Handling
//!
//! Parser errors never block the entire scrape. Bad lines/files are skipped with warnings
//! and reported in the scrape summary.
//!
//! # When to Use
//!
//! - **CLI**: `agentscribe scrape` invokes the scraper directly
//! - **Daemon**: Background scraping on file changes
//! - **Tests**: Integration tests use [`Scraper::new_with_lock_timeout()`]
```

---

#### Issue 3: index.rs
**File:** `src/index.rs` (lines 1-4)
**Issue:** Minimal documentation for the core search index schema.

**Current:**
```rust
//! Tantivy index schema and document builder
//!
//! Defines the full-text search index schema and provides functions to build
//! Tantivy documents from normalized session events and manifests.
```

**Problems:**
- Doesn't list the indexed fields or their purposes
- No explanation of document structure (session vs. code_artifact)
- Missing the ADR-2 decision about content field indexing vs. storage
- No examples

**Recommendation:** Expand with field documentation:
```rust
//! Tantivy full-text search index schema and document builder
//!
//! This module defines the search index schema and provides functions to build
//! Tantivy documents from normalized session events and manifests.
//!
//! # Index Structure
//!
//! One Tantivy index stores both sessions and code artifacts:
//!
//! ## Session Documents (`doc_type: "session"`)
//!
//! - **content** (indexed, not stored): Full conversation with role prefixes
//! - **summary** (indexed, stored): One-line session summary
//! - **solution_summary** (indexed, stored): Extracted resolution for successful sessions
//! - **session_id**: Unique identifier (`<agent>/<id>`)
//! - **source_agent**: Plugin name (claude-code, aider, etc.)
//! - **project**: Absolute project path
//! - **outcome**: success | failure | abandoned | unknown
//! - **tags**: Technology tags, languages, tools
//! - **error_fingerprint**: Normalized error patterns
//! - **timestamp**: Session start time
//! - **turn_count**: Number of conversational turns
//!
//! ## Code Artifact Documents (`doc_type: "code_artifact"`)
//!
//! - **code_content** (indexed, stored): Extracted code block
//! - **code_language**: rust, python, typescript, etc.
//! - **code_file_path**: File path if known
//! - **code_is_final**: Whether this was the final applied version
//!
//! # Storage Policy (ADR-2)
//!
//! The `content` field is indexed but NOT stored to avoid duplicating the full
//! conversation text. Raw content is re-read from `sessions/*.jsonl` when needed
//! (snippets, more-like-this, analytics). Only short display fields are stored.
```

---

### 🟢 MINOR: Documentation could be improved

#### Issue 4: daemon.rs
**File:** `src/daemon.rs` (lines 1-7)
**Issue:** Acceptable but could be more comprehensive.

**Current:** 7 lines covering basic purpose
**Recommendation:** Add sections on:
- Daemon lifecycle (start, run, stop, status)
- File watching behavior
- Debounce logic
- Health monitoring
- When to use daemon vs. CLI

---

#### Issue 5: tags.rs
**File:** `src/tags.rs` (lines 1-6)
**Issue:** Terse but functional for a simple utility module.

**Current:** 6 lines explaining the three-tier pipeline
**Status:** ACCEPTABLE - Clear enough for the module's complexity

---

#### Issue 6: Other modules with minimal but adequate docs

These modules have brief documentation that is sufficient for their scope:
- `render.rs`: "Session rendering for HTML and Markdown export" - adequate for a narrow-purpose module
- `gc.rs`: "Garbage collection for old sessions" - adequate
- `embedding.rs`: "Embedding model clients for vector index" - adequate
- `mcp.rs`: 12 lines explaining MCP server and tools - adequate

---

## ✅ Excellent Examples

The following modules have exemplary documentation that others should emulate:

### config.rs
- Clear purpose statement
- Configuration file example
- Data directory structure
- Environment variables
- Defaults explanation
- Validation notes

### event.rs
- Core concepts section
- Data flow explanation
- Canonical format rationale
- Examples
- Detailed enum documentation

### error.rs
- Error categories
- Handling strategy (skip-and-log for parser errors, fail-fast for config)
- Examples

### plugin.rs
- Plugin structure with TOML example
- Bundled plugins list
- Custom plugin instructions
- Validation details

### analytics.rs
- Bullet list of capabilities
- Problem type classification explanation
- Cost estimation notes
- CLI examples

### search.rs
- Search modes with examples
- Filtering options
- Output modes
- Context budgeting explanation

### recurring.rs, rules.rs, digest.rs, pulse_report.rs
- Clear purpose statements
- Output format descriptions
- Appropriate detail level

---

## Recommendations by Priority

### High Priority
1. **Fix `src/parser/mod.rs`** - Replace import-related docs with format parser documentation
2. **Expand `src/lib.rs`** - Add architecture overview and usage guidance for the library crate
3. **Expand `src/scraper/mod.rs`** - Add scraping pipeline, incremental behavior, and error handling details
4. **Expand `src/index.rs`** - Document the schema fields and storage policy

### Medium Priority
5. **Improve `src/daemon.rs`** - Add daemon lifecycle and health monitoring sections
6. **Consider `src/tags.rs`** - Add examples for the three-tier pipeline

### Low Priority
7. **Standardize format** - Consider adding a "When to Use" section to all module docs
8. **Add examples** - More modules could benefit from code examples (especially `lib.rs` and `index.rs`)

---

## Formatting Notes

All reviewed modules follow Rust documentation conventions correctly:
- Proper use of `//!` for module-level docs
- `///` for item-level docs
- Markdown formatting with headings, code blocks, and lists
- Cross-references using `[`name`]` syntax

No formatting issues were found.

---

## Consistency Analysis

### Tone
Most modules use a professional, informative tone. Some are more concise (`tags.rs`, `gc.rs`) while others are more verbose (`event.rs`, `plugin.rs`). This variance is appropriate given the differing complexity of the modules.

### Style
- Well-documented modules use sections with `#` headings
- Code examples use `no_run` or `ignore` attributes appropriately
- Bullet lists are used for feature enumeration
- CLI examples show bash commands with output

### Terminology
Consistent use of terms across modules:
- "canonical event schema" / "canonical format"
- "session" / "event" / "manifest"
- "Tantivy index" / "BM25"
- "plugin" / "parser" / "scraper"

No confusing or inconsistent terminology was found.

---

## Conclusion

Overall, the module-level documentation in AgentScribe is **good** with several **excellent** examples. The critical issue in `parser/mod.rs` (incorrect documentation about Rust imports) should be fixed immediately. Expanding the minimal documentation in `lib.rs`, `scraper/mod.rs`, and `index.rs` would significantly improve the library's usability for new contributors and integrators.

The codebase shows a strong documentation culture—most modules are well-documented with clear explanations of purpose, examples, and usage guidance. Addressing the identified issues would bring all modules up to the same high standard.
