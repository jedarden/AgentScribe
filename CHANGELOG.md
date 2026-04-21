# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
