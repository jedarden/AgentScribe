# Test Imports Analysis - AgentScribe

**Analysis Date:** N/A

## Summary

- **Total Test Files Analyzed:** 19
- **Import Categories Found:** 3

## Framework Usage

| Framework | Files Using It | Total Imports |
|-----------|----------------|---------------|
| local_crate | 16 | 66 |
| external_dep | 14 | 49 |
| stdlib | 14 | 28 |

## Detailed Analysis by Framework

### external_dep

- **Files affected:** 14
- **Total imports:** 49

**Import patterns:**

  - ``
  - `        let temp = tempfile::tempdir().unwrap();`
  - `    fn create_claude_code_plugin() -> Plugin {`
  - `    fn test_deeply_nested_subagent_path() {`
  - `    fn test_multiple_subagents_same_parent() {`
  - `    fn test_non_subagent_has_no_parent() {`
  - `    fn test_regular_session_no_subagent_suffix() {`
  - `    fn test_single_subagent_detection() {`
  - `    fn test_subagent_path_detection() {`
  - `    fn test_subagent_session_info_structure() {`
  - `    fn test_subagent_source_agent_suffix() {`
  - `    use super::*;`
  - `chrono::Utc`
  - `chrono::{Datelike, Timelike, Utc}`
  - `chrono::{Duration, Utc}`
  - `mod tests {`
  - `serde_json::{json, Value}`
  - `super::*`
  - `tantivy::Index`
  - `tantivy::query::TermQuery`
  - ... and 5 more

### local_crate

- **Files affected:** 16
- **Total imports:** 66

**Import patterns:**

  - `        use crate::parser::jsonl::JsonlParser;`
  - `        use crate::scraper::Scraper;`
  - `agentscribe::analytics::{self, AgentMetrics, AnalyticsOptions}`
  - `agentscribe::config::Config`
  - `agentscribe::config::{RedactionConfig, WhisperConfig}`
  - `agentscribe::daemon`
  - `agentscribe::digest::{self, DigestOptions}`
  - `agentscribe::enrichment::antipatterns`
  - `agentscribe::enrichment::outcome::OutcomeConfig`
  - `agentscribe::enrichment::{detect_outcome, enrich_events, extract_solution, generate_summary}`
  - `agentscribe::event::Role`
  - `agentscribe::event::{Event, Role, SessionManifest}`
  - `agentscribe::event::{Event, Role}`
  - `agentscribe::gc`
  - `agentscribe::index::IndexManager`
  - `agentscribe::index::build_manifest_from_events`
  - `agentscribe::index::build_schema`
  - `agentscribe::mcp`
  - `agentscribe::parser::{FormatParser, JsonTreeParser}`
  - `agentscribe::parser::{FormatParser, MarkdownParser}`
  - ... and 17 more

### stdlib

- **Files affected:** 14
- **Total imports:** 28

**Import patterns:**

  - `std::collections::HashMap`
  - `std::fs`
  - `std::io::Write`
  - `std::io::{BufRead, BufReader, Write}`
  - `std::os::unix::net::UnixStream`
  - `std::path::Path`
  - `std::path::PathBuf`
  - `std::path::{Path, PathBuf}`
  - `std::time::Duration`
  - `std::time::Instant`
