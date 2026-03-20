//! AgentScribe - Archive, search, and learn from coding agent conversations
//!
//! A Rust CLI binary that scrapes conversation logs from multiple coding agent types,
//! normalizes them into a canonical format, and stores them as flat files.

#![allow(clippy::too_many_arguments)]

mod cli;
mod config;
mod error;
mod event;
mod index;
mod parser;
mod plugin;
mod scraper;
mod tags;

use cli::run;
use error::Result;

fn main() -> Result<()> {
    run()
}
