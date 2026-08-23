//! AgentScribe — Archive, index, and extract intelligence from AI coding agent conversations
//!
//! AgentScribe is a Rust library and CLI tool that scrapes conversation logs from multiple
//! coding agents (Claude Code, Aider, OpenCode, Codex, Cursor, Windsurf), normalizes them
//! into a unified searchable format, and distills actionable intelligence from accumulated
//! agent knowledge.
//!
//! # Architecture Overview
//!
//! AgentScribe follows a pipeline architecture:
//!
//! 1. **Scraping** ([`scraper`]): Plugin-based log discovery and parsing
//!    - Declarative TOML plugin definitions for each agent type
//!    - Format-specific parsers: JSONL, Markdown, JSON-tree, JSON-array, SQLite
//!    - Session boundary detection and incremental tailing
//!    - Error resilience: skip bad lines, log warnings, continue processing
//!
//! 2. **Normalization** ([`event`], [`parser`]): Canonical event schema
//!    - All agent formats → unified [`Event`] structure
//!    - Field mapping via plugin TOML or envelope unwrapping
//!    - Role attribution: user, assistant, tool_call, tool_result, system
//!    - Metadata extraction: project path, model name, file references
//!
//! 3. **Storage** ([`scraper`]): Flat files as source of truth
//!    - Normalized sessions as JSONL: `~/.agentscribe/sessions/<agent>/<id>.jsonl`
//!    - Tantivy BM25 index: full-text search with faceted filters
//!    - Optional vector index (turbovec): semantic search (currently stubbed)
//!    - Crash-safe incremental scrape state tracking
//!
//! 4. **Enrichment** ([`enrichment`]): Intelligence extraction
//!    - Outcome detection: success, failure, abandoned, unknown
//!    - Error fingerprinting: normalize errors → searchable patterns
//!    - Solution extraction: resolution window → fix summary
//!    - Anti-pattern detection: failed approaches → avoid list
//!    - Code artifact extraction: fenced code blocks → separate index
//!    - Git blame linking: commits ↔ sessions bidirectionally
//!
//! 5. **Query** ([`search`]): Multi-mode retrieval interface
//!    - Full-text BM25 search with fuzzy matching
//!    - Error lookup: normalized fingerprint matching
//!    - Anti-pattern search: what not to do
//!    - Code search: language-specific artifact retrieval
//!    - More-like-this: Tantivy similarity queries
//!    - File knowledge: all sessions touching a path
//!
//! 6. **Analytics** ([`analytics`], [`pulse_report`], [`capacity`]): Cross-cutting insights
//!    - Agent effectiveness: success rates, turns per outcome, cost efficiency
//!    - Recurring problems: error patterns solved 3+ times
//!    - Weekly digest: automated activity summary
//!    - Capacity utilization: Claude Code 5h/7d rolling windows
//!    - Quarterly reports: State of AI Coding analytics
//!
//! # Module Organization
//!
//! ## Core Subsystems
//!
//! - **[`config`]**: Global configuration (`config.toml`) and data directory management
//! - **[`plugin`]**: Plugin manifest schema and validation
//! - **[`parser`]**: Format-specific parsers (JsonlParser, MarkdownParser, etc.) and import statement parsing
//! - **[`scraper`]**: Log discovery, incremental scraping, state tracking
//! - **[`index`]**: Tantivy schema, document management, indexing operations
//! - **[`search`]**: Query execution, result packing, context budget optimization
//!
//! ## Enrichment & Intelligence
//!
//! - **[`enrichment`]**: Outcome detection, solution extraction, anti-patterns
//! - **[`analytics`]**: Agent metrics, problem-type classification, cost estimation
//! - **[`recurring`]**: Repeated problem detection
//! - **[`file_knowledge`]**: File → sessions reverse index
//! - **[`rules`]**: Auto-generated CLAUDE.md/.cursorrules from patterns
//! - **[`pulse_report`]**: Quarterly analytics reports
//! - **[`capacity`]**: Claude Code utilization tracking
//!
//! ## Data Processing
//!
//! - **[`event`]**: Canonical event schema and normalization
//! - **[`transcription`]**: Whisper audio transcription with PII redaction
//! - **[`redaction`]**: PII pattern matching and removal
//! - **[`render`]**: Session export to HTML/Markdown
//!
//! ## Daemon & Integration
//!
//! - **[`daemon`]**: Long-running background process with file watching
//! - **[`mcp`]**: Model Context Protocol server (optional)
//! - **[`shell_hook`]**: Search-on-error integration for bash/zsh/fish
//! - **[`cli`]**: Command-line interface (Clap subcommands)
//!
//! ## Utilities
//!
//! - **[`error`]**: Error types and Result wrapper
//! - **[`utils`]**: Shared helper functions
//! - **[`write_guard`]**: Concurrent write protection
//! - **[`gc`]**: Session garbage collection and index compaction
//! - **[`doctor`]**: Health checks and diagnostics
//!
//! # When to Use This Library vs CLI
//!
//! **Use the library** when you need to:
//! - Integrate AgentScribe into a Rust application
//! - Write integration tests against the data layer
//! - Build custom tools on top of AgentScribe's corpus
//! - Programmatically query sessions from your own code
//!
//! **Use the CLI** (`agentscribe`) when you:
//! - Need ad-hoc search from the terminal
//! - Want daemon-mode background scraping
//! - Run periodic analytics or reports
//! - Generate shell hooks or completions
//!
//! # Data Model
//!
//! ## Canonical Event
//!
//! Every conversation turn from every agent normalizes to this schema:
//!
//! ```text
//! {
//!   "ts": "2026-03-16T12:00:00Z",
//!   "session_id": "claude-code/83f5a4e7",
//!   "source_agent": "claude-code",
//!   "project": "/home/coding/myproject",
//!   "role": "user|assistant|tool_call|tool_result|system",
//!   "content": "the text content",
//!   "tool": null,
//!   "tokens": {"input": 1200, "output": 450},
//!   "tags": ["git", "migration", "postgres"],
//!   "file_paths": ["/home/coding/myproject/src/auth.rs"],
//!   "error_fingerprints": ["ConnectionRefusedError:{host}:{port}"]
//! }
//! ```
//!
//! ## Session Manifest
//!
//! Each session has a manifest entry with summary, outcome, and metadata:
//!
//! ```text
//! {
//!   "session_id": "claude-code/83f5a4e7",
//!   "source_agent": "claude-code",
//!   "project": "/home/coding/myproject",
//!   "started": "2026-03-16T10:00:00Z",
//!   "ended": "2026-03-16T10:45:00Z",
//!   "turns": 42,
//!   "summary": "Migrated Postgres schema from v3 to v4...",
//!   "solution_summary": "ALTER TABLE users ADD COLUMN...",
//!   "outcome": "success|failure|abandoned|unknown",
//!   "tags": ["postgres", "migration", "schema"],
//!   "files_touched": ["db/migrations/004.sql"],
//!   "git_commits": ["a1b2c3d"],
//!   "error_fingerprints": ["ConnectionRefusedError:{host}:{port}"],
//!   "model": "claude-sonnet-4-20250514"
//! }
//! ```
//!
//! # Example: Library Usage
//!
//! ```no_run
//! use agentscribe::{search, search::SearchOptions};
//! use std::path::PathBuf;
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let data_dir = PathBuf::from("~/.agentscribe");
//!
//!     // Search for past solutions to a database connection problem
//!     let results = search::execute_search(
//!         &data_dir,
//!         &SearchOptions {
//!             query: Some("database connection timeout".to_string()),
//!             outcome: Some("success".to_string()),
//!             solution_only: true,
//!             max_results: 3,
//!             ..Default::default()
//!         }
//!     )?;
//!
//!     for session in results.results {
//!         println!("Session: {}", session.session_id);
//!         println!("Summary: {}", session.summary);
//!         if let Some(solution) = session.solution_summary {
//!             println!("Solution: {}", solution);
//!         }
//!     }
//!
//!     Ok(())
//! }
//! ```
//!
//! # Example: CLI Usage
//!
//! ```bash
//! # Search for past error solutions
//! agentscribe search --error "ENOSPC" --outcome success --json
//!
//! # Get file knowledge
//! agentscribe file src/auth/middleware.rs
//!
//! # Generate weekly digest
//! agentscribe digest --since 7d
//!
//! # Start daemon for continuous scraping
//! agentscribe daemon start
//! ```
//!
//! # Design Principles
//!
//! - **CLI-first, MCP-also**: The CLI is the primary interface; MCP is optional
//! - **Flat files first**: All data is plain text (JSONL + Markdown); indexes are rebuildable
//! - **Git-native**: Append-only JSONL and Markdown are diff-friendly
//! - **Incremental**: Scraping tracks offsets for fast re-runs
//! - **Agent-readable**: Search output is structured JSON for programmatic use
//! - **No external dependencies**: Core scraping/indexing/search work offline
//! - **Non-invasive**: Read-only access to agent logs
//! - **Pluggable**: New agent types added via TOML, not code changes
//! - **Low footprint**: Daemon idles under 20MB RSS, scraping under 50MB
//!
//! # Further Reading
//!
//! - [CLI Reference](https://github.com/jedarden/AgentScribe/blob/main/docs/cli-reference.md) — Detailed help for every command
//! - [Plugin Building Guide](https://github.com/jedarden/AgentScribe/blob/main/plugins/BUILDING_PLUGINS.md) — How to write custom plugins
//! - [Implementation Plan](https://github.com/jedarden/AgentScribe/blob/main/docs/plan.md) — Full architecture and phase breakdown

#![allow(clippy::too_many_arguments)]

pub mod analytics;
pub mod annotations;
pub mod capacity;
pub mod cli;
pub mod config;
pub mod daemon;
pub mod digest;
pub mod doctor;
pub mod embedding;
pub mod enrichment;
pub mod error;
pub mod event;
pub mod file_knowledge;
pub mod gc;
pub mod index;
pub mod mcp;
pub mod parser;
pub mod plugin;
pub mod projects;
pub mod pulse_report;
pub mod recurring;
pub mod redaction;
pub mod reflect;
pub mod render;
pub mod rules;
pub mod scraper;
pub mod search;
pub mod shell_hook;
pub mod tags;
pub mod transcription;
pub mod utils;
pub mod vector;
pub mod write_guard;
