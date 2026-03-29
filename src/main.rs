//! AgentScribe - Archive, search, and learn from coding agent conversations
//!
//! A Rust CLI binary that scrapes conversation logs from multiple coding agent types,
//! normalizes them into a canonical format, and stores them as flat files.

#![allow(clippy::too_many_arguments)]

mod analytics;
mod cli;
mod config;
mod daemon;
mod digest;
mod enrichment;
mod error;
mod event;
mod gc;
mod index;
mod mcp;
mod parser;
mod plugin;
mod recurring;
mod rules;
mod scraper;
mod search;
mod shell_hook;
mod tags;

use cli::run;
use error::Result;

fn main() -> Result<()> {
    run()
}
