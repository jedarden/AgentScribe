# AgentScribe Community Plugin Examples

This directory contains community-contributed and example plugins for coding agents that are not bundled with AgentScribe by default. Use these as a starting point when adding support for a new agent.

## Available Examples

| File | Agent | Format | Notes |
|------|-------|--------|-------|
| `plugin_template.toml` | — | JSONL | Blank template with every field documented |
| `copilot-chat.toml` | GitHub Copilot Chat | JSONL | VS Code extension (≥ 0.20) |
| `continue-dev.toml` | Continue.dev | JSONL | Open-source AI code assistant |
| `sqlite-agent.toml` | — | SQLite | Example plugin for SQLite-based agents (Cursor/Windsurf pattern) |

The bundled plugins (Claude Code, Aider, OpenCode, Codex, Cursor, Windsurf) live in `plugins/` and are installed automatically by `agentscribe config init`.

## Quick Install

```bash
# Install a single example plugin
cp examples/copilot-chat.toml ~/.agentscribe/plugins/copilot-chat.toml

# Validate the plugin definition
agentscribe plugins validate ~/.agentscribe/plugins/copilot-chat.toml

# Dry-run to confirm file discovery and field mapping
agentscribe scrape --plugin copilot-chat --dry-run

# Full scrape
agentscribe scrape --plugin copilot-chat
```

## Writing a New Plugin

See [`plugins/BUILDING_PLUGINS.md`](../plugins/BUILDING_PLUGINS.md) for the complete field reference and a step-by-step walkthrough.

The shortest path to a working plugin:

1. Copy `plugin_template.toml` and rename it to `<agent-name>.toml`.
2. Set `paths` to the glob pattern(s) where your agent stores its logs.
3. Set `format` to `jsonl`, `markdown`, `json-tree`, or `sqlite`.
4. Map `timestamp`, `role`, and `content` to the correct field paths.
5. Run `agentscribe plugins validate` and `agentscribe scrape --dry-run` to verify.

## Contributing a Plugin

If you write a plugin for an agent not yet covered, consider contributing it back so others can benefit.

### Contribution Checklist

- [ ] Plugin validates cleanly: `agentscribe plugins validate <file>`
- [ ] Dry-run shows correct session and event counts
- [ ] `name` is lowercase, alphanumeric + hyphens (e.g., `my-agent`)
- [ ] `version` starts at `"1.0"`
- [ ] Paths include all common platform locations (Linux, macOS, Windows if applicable)
- [ ] Comments explain any non-obvious field choices or version dependencies
- [ ] No hardcoded absolute paths specific to your machine

### How to Submit

1. Fork the [AgentScribe repository](https://github.com/your-org/agentscribe).
2. Place your plugin file in `examples/<agent-name>.toml`.
3. Add a row to the table in this README.
4. Open a pull request with a short description of the agent and how you tested it.

The maintainers will review for correctness, path coverage across platforms, and field mapping quality before merging.

### Versioning

Increment `version` in your plugin when you change field mappings or source paths. AgentScribe uses the version to detect when existing files need to be re-scraped.

```toml
[plugin]
name    = "my-agent"
version = "1.1"   # bumped after adding token field mapping
```
