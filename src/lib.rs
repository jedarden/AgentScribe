//! AgentScribe library — exposes modules for integration testing and external use.

#![allow(clippy::too_many_arguments)]

pub mod analytics;
pub mod capacity;
pub mod cli;
pub mod config;
pub mod daemon;
pub mod digest;
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
pub mod render;
pub mod rules;
pub mod scraper;
pub mod search;
pub mod shell_hook;
pub mod tags;
pub mod transcription;
pub mod write_guard;
