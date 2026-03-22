# Configuration Reference

AgentScribe reads configuration from `~/.agentscribe/config.toml`. On systems that support XDG, this can also be placed at `$XDG_CONFIG_DIR/agentscribe/config.toml`. Override with `--config <path>` or the `AGENTSCRIBE_CONFIG` environment variable.

Initialize the default config with:

```bash
agentscribe config init
```

---

## `[general]`

General settings.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `data_dir` | string | `~/.agentscribe` | Root data directory. All session files, the index, plugins, and state are stored here. Supports `~` expansion. |
| `log_level` | string | `"info"` | Log verbosity. One of: `error`, `warn`, `info`, `debug`, `trace`. Affects stderr output from CLI commands. |

```toml
[general]
data_dir = "~/.agentscribe"
log_level = "info"
```

---

## `[scrape]`

Controls how log files are discovered and processed.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `debounce_seconds` | integer | `5` | Seconds to wait after a file change before scraping (daemon mode). Prevents thrashing during active agent sessions. |
| `max_session_age_days` | integer | `0` | Ignore sessions older than N days. `0` means no limit — process all sessions. Useful for large historical logs where you only care about recent activity. |

```toml
[scrape]
debounce_seconds = 5
max_session_age_days = 0
```

---

## `[index]`

Tantivy full-text index settings.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `tantivy_heap_size_mb` | integer | `50` | Memory budget in MB for the Tantivy `IndexWriter`. Higher values speed up indexing at the cost of more RAM. Recommended: 50-200 MB depending on available memory. |

```toml
[index]
tantivy_heap_size_mb = 50
```

---

## `[search]`

Default search behavior.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `default_max_results` | integer | `10` | Default number of results returned when `--max-results` is not specified. |
| `default_snippet_length` | integer | `200` | Default maximum character length of content snippets when `--snippet-length` is not specified. Set to `0` to omit snippets by default. |

```toml
[search]
default_max_results = 10
default_snippet_length = 200
```

---

## `[outcome]`

Weights for the outcome detection enrichment pipeline. These control how sessions are classified as `success`, `failure`, `abandoned`, or `unknown` based on signal scores.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `user_satisfaction` | integer | `30` | Weight for user satisfaction signals (e.g., "thanks", "LGTM", "looks good"). |
| `user_frustration` | integer | `-25` | Weight for user frustration signals (e.g., "no", "wrong", "revert", "undo"). |
| `final_edit_write` | integer | `15` | Weight for sessions ending with an Edit/Write tool call (suggests productive output). |
| `final_error` | integer | `-20` | Weight for sessions ending with an error (tool_result with error exit code). |
| `unresolved_error` | integer | `-15` | Weight for sessions with unresolved errors in the last assistant turn. |
| `tool_success` | integer | `10` | Weight for successful tool calls (exit code 0) in the final turns. |
| `tool_failure` | integer | `-10` | Weight for failed tool calls (non-zero exit code) in the final turns. |
| `very_short_session` | integer | `-5` | Weight for sessions with very few turns (likely abandoned or trivial). |
| `help_without_resolution` | integer | `-10` | Weight for sessions where the agent provided help but the user didn't confirm resolution. |
| `task_completion` | integer | `25` | Weight for explicit task completion signals (e.g., "done", "finished", "committed"). |
| `success_threshold` | integer | `20` | Minimum score to classify a session as `success`. Sessions scoring above this are marked success. |
| `failure_threshold` | integer | `-20` | Maximum score to classify a session as `failure`. Sessions scoring below this are marked failure. Sessions between thresholds are `abandoned` or `unknown`. |
| `min_turns` | integer | `3` | Minimum number of turns before outcome classification is applied. Very short sessions below this are always classified as `unknown`. |

```toml
[outcome]
user_satisfaction = 30
user_frustration = -25
final_edit_write = 15
final_error = -20
unresolved_error = -15
tool_success = 10
tool_failure = -10
very_short_session = -5
help_without_resolution = -10
task_completion = 25
success_threshold = 20
failure_threshold = -20
min_turns = 3
```

---

## `[cost]`

Model pricing for cost estimation in analytics and digests.

### `[cost.models]`

Per-model pricing in USD per 1M tokens. Add entries for each model you use. Models not listed here are excluded from cost estimates.

```toml
[cost.models]
"claude-sonnet-4-20250514" = { input = 3.0, output = 15.0 }
"claude-opus-4-20250514" = { input = 15.0, output = 75.0 }
"gpt-4o" = { input = 2.5, output = 10.0 }
"gpt-4o-mini" = { input = 0.15, output = 0.6 }
"deepseek-v3" = { input = 0.27, output = 1.1 }
```

Each model entry has:

| Key | Type | Description |
|-----|------|-------------|
| `input` | float | Cost per 1M input tokens (USD). |
| `output` | float | Cost per 1M output tokens (USD). |

---

## Complete Example

```toml
[general]
data_dir = "~/.agentscribe"
log_level = "info"

[scrape]
debounce_seconds = 5
max_session_age_days = 0

[index]
tantivy_heap_size_mb = 50

[search]
default_max_results = 10
default_snippet_length = 200

[outcome]
user_satisfaction = 30
user_frustration = -25
final_edit_write = 15
final_error = -20
unresolved_error = -15
tool_success = 10
tool_failure = -10
very_short_session = -5
help_without_resolution = -10
task_completion = 25
success_threshold = 20
failure_threshold = -20
min_turns = 3

[cost.models]
"claude-sonnet-4-20250514" = { input = 3.0, output = 15.0 }
"gpt-4o" = { input = 2.5, output = 10.0 }
```

---

## Setting Values from the CLI

Use `agentscribe config set` to change individual values:

```bash
agentscribe config set general.log_level debug
agentscribe config set scrape.debounce_seconds 10
agentscribe config set index.tantivy_heap_size_mb 100
agentscribe config set search.default_max_results 20
```

Use `agentscribe config get` to read values:

```bash
agentscribe config get general.log_level
agentscribe config get scrape.debounce_seconds
```

Use `agentscribe config show` to display the full configuration.
