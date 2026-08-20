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
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let data_dir = std::path::PathBuf::from("~/.agentscribe");
//! let results = search::execute_search(
//!     &data_dir,
//!     &SearchOptions {
//!         query: Some("database connection".to_string()),
//!         ..Default::default()
//!     }
//! )?;
//! # Ok(())
//! # }
//! ```

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
