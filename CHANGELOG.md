# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- MCP server over Unix socket with four tools: `agentscribe_search` (full-text and faceted search), `agentscribe_status` (plugin list, session counts, daemon state, index stats), `agentscribe_blame` (bidirectional git commit ↔ session linking), and `agentscribe_file` (chronological session list for a file path)
- `context` subcommand for pre-task priming queries, packing relevant past sessions into a token budget for agent workers
- `render` subcommand to export sessions as HTML or Markdown for documentation and review
- Semantic vector search via `embed` subcommand (build, rebuild, stats, missing commands) with 4-bit quantized indexes using turbovec; `agentscribe search --semantic` and `--hybrid` flags combine embedding-based similarity with BM25 ranking
- `pulse-report` subcommand to generate quarterly "State of AI Coding" reports summarizing activity, patterns, and trends
- `capacity` subcommand showing per-account Claude Code utilization over 5-hour and 7-day rolling windows
- `transcribe` subcommand with Whisper model support and automatic PII redaction for audio-to-text workflows
- `annotate` subcommand for session tagging with sidecar JSON storage (`<session_id>.annotations.json`)
- `reflect` subcommand to export sessions with behavioral metadata for NEEDLE-style fleet performance analysis

## [0.1.0] - 2026-03-26

### Added
- Initial release of AgentScribe
- Multi-agent log scraping (Claude Code, Aider, Codex, Cursor, Windsurf, OpenCode)
- Full-text search via Tantivy index
- Session enrichment pipeline: outcomes, solutions, errors, anti-patterns
- Background daemon with file-system watcher
- Shell hook integration for auto-querying on command failure (bash, zsh, fish)
- Shell completion generation: `agentscribe completions bash|zsh|fish`
- Agent analytics and cross-agent performance comparison
- Recurring problem detection via error fingerprinting
- Rules distillation into agent-specific rules files (CLAUDE.md, .cursorrules, .aider.conf.yml)
- Activity digest generation
- Plugin system for custom agent log formats (TOML definitions)
- Bundled SQLite parser plugins for Cursor and Windsurf
- Garbage collection for old sessions
- Pre-built binaries for Linux x86_64/aarch64 and macOS x86_64/aarch64
- Install script (`install.sh`) with automatic platform detection

[Unreleased]: https://github.com/coding/AgentScribe/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/coding/AgentScribe/releases/tag/v0.1.0
