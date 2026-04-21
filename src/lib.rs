//! AgentScribe library — exposes modules for integration testing and external use.

#![allow(clippy::too_many_arguments)]

pub mod analytics;
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
pub mod recurring;
pub mod rules;
pub mod scraper;
pub mod search;
pub mod shell_hook;
pub mod tags;
