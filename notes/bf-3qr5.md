# Aider Analytics JSONL Format Schema

**Bead:** bf-3qr5  
**Source:** [sample-analytics.jsonl](https://github.com/Aider-AI/aider/blob/main/aider/website/assets/sample-analytics.jsonl)

## Format

**JSONL** - One JSON object per line, UTF-8 encoded.

## Standard Fields (Present in All Events)

| Field | Type | Description | Example |
|-------|------|-------------|---------|
| `event` | string | Event type/name | `"message_send"`, `"launched"`, `"exit"` |
| `time` | integer | Unix timestamp (seconds) | `1754753991` |
| `user_id` | string | Anonymous user UUID | `"c42c4e6b-f054-44d7-ae1f-6726cc41da88"` |
| `properties` | object | Event-specific properties (varies by event type) | See below |

## Event Types

### Lifecycle Events

#### `launched`
Fired when aider starts up.
```json
{"event": "launched", "properties": {}, "user_id": "...", "time": 1754753991}
```

#### `exit`
Fired when aider exits.
```json
{"event": "exit", "properties": {"reason": "Completed main CLI coder.run"}, "user_id": "...", "time": 1754753991}
```
**Exit reasons:**
- `"Completed main CLI coder.run"`
- `"Completed --message"`
- `"Completed lint/test/commit"`
- `"GUI session ended"`
- `"/exit"`
- `"Exit flag set"`
- `"Showed repo map"`
- `"Control-C"`
- `"Unknown edit format"`

### Session Events

#### `cli session`
Start of a CLI session (interactive mode).
```json
{"event": "cli session", "properties": {
  "main_model": "gemini/gemini-2.5-pro",
  "weak_model": "gemini/gemini-2.5-flash-lite",
  "editor_model": "gemini/gemini-2.5-pro",
  "edit_format": "diff-fenced"
}, "user_id": "...", "time": 1754755056}
```

#### `gui session`
Start of a GUI session.
```json
{"event": "gui session", "properties": {}, "user_id": "...", "time": 1754753991}
```

### Repository Events

#### `repo`
Repository information.
```json
{"event": "repo", "properties": {"num_files": 630}, "user_id": "...", "time": 1754755056}
```

#### `no-repo`
No repository detected.
```json
{"event": "no-repo", "properties": {}, "user_id": "...", "time": 1754754234}
```

### Configuration Events

#### `auto_commits`
Auto-commit setting.
```json
{"event": "auto_commits", "properties": {"enabled": true}, "user_id": "...", "time": 1754755056}
```

#### `model warning`
Warning about missing/invalid models.
```json
{"event": "model warning", "properties": {
  "main_model": "None",
  "weak_model": "None",
  "editor_model": "None"
}, "user_id": "...", "time": 1754754234}
```

### Message Events

#### `message_send_starting`
Message send initiated (before API call).
```json
{"event": "message_send_starting", "properties": {}, "user_id": "...", "time": 1754761389}
```

#### `message_send`
Message completed (after API response).
```json
{"event": "message_send", "properties": {
  "main_model": "gpt-5",
  "weak_model": "gemini/gemini-2.5-flash-lite",
  "editor_model": "gpt-5",
  "edit_format": "diff",
  "prompt_tokens": 15724,
  "completion_tokens": 107,
  "total_tokens": 17532,
  "cost": 0.02285125,
  "total_cost": 0.4392675
}, "user_id": "...", "time": 1754942076}
```

**Key fields in `message_send`:**
- **Model tracking:** `main_model`, `weak_model`, `editor_model`
- **Edit format:** `edit_format` (e.g., `"diff"`, `"whole"`, `"ask"`, `"diff-fenced"`)
- **Token usage:** `prompt_tokens`, `completion_tokens`, `total_tokens`
- **Cost tracking:** `cost` (this message), `total_cost` (cumulative)

### Command Events

#### `command_ask`
User issued `/ask` command.
```json
{"event": "command_ask", "properties": {}, "user_id": "...", "time": 1754933194}
```

#### `command_add`
User issued `/add` command.
```json
{"event": "command_add", "properties": {}, "user_id": "...", "time": 1754933511}
```

#### `command_clear`
User issued `/clear` command.
```json
{"event": "command_clear", "properties": {}, "user_id": "...", "time": 1754935909}
```

#### `command_code`
User issued `/code` command.
```json
{"event": "command_code", "properties": {}, "user_id": "...", "time": 1754936019}
```

#### `command_commit`
User issued `/commit` command.
```json
{"event": "command_commit", "properties": {}, "user_id": "...", "time": 1752552856}
```

#### `command_edit`
User issued `/edit` command.
```json
{"event": "command_edit", "properties": {}, "user_id": "...", "time": 1754933195}
```

#### `command_exit`
User issued `/exit` command.
```json
{"event": "command_exit", "properties": {}, "user_id": "...", "time": 1755099935}
```

#### `command_model`
User changed model.
```json
{"event": "command_model", "properties": {}, "user_id": "...", "time": 1754934338}
```

#### `command_paste`
User issued `/paste` command.
```json
{"event": "command_paste", "properties": {}, "user_id": "...", "time": 1754935845}
```

#### `command_reasoning-effort`
User changed reasoning effort.
```json
{"event": "command_reasoning-effort", "properties": {}, "user_id": "...", "time": 1754938934}
```

#### `command_run`
User issued `/run` command.
```json
{"event": "command_run", "properties": {}, "user_id": "...", "time": 1753766585}
```

#### `command_undo`
User issued `/undo` command.
```json
{"event": "command_undo", "properties": {}, "user_id": "...", "time": 1755005241}
```

#### `command_drop`
User issued `/drop` command.
```json
{"event": "command_drop", "properties": {}, "user_id": "...", "time": 1770821112}
```

#### `command_web`
User issued `/web` command.
```json
{"event": "command_web", "properties": {}, "user_id": "...", "time": 1770820702}
```

#### `command_chat-mode`
User changed chat mode.
```json
{"event": "command_chat-mode", "properties": {}, "user_id": "...", "time": 1755082533}
```

#### `command_ok`
User issued `/ok` command.
```json
{"event": "command_ok", "properties": {}, "user_id": "...", "time": 1773705893}
```

## Key Fields for Analytics Companion

For **AgentScribe's aider analytics companion** (bead bf-xp9w), the most valuable fields are:

1. **Session grouping:** `user_id` + sequence of `launched`/`exit` events
2. **Timestamp:** `time` (Unix timestamp)  
3. **Models used:** `main_model`, `weak_model`, `editor_model` in `cli session` and `message_send` events
4. **Token metrics:** `prompt_tokens`, `completion_tokens`, `total_tokens` from `message_send`
5. **Cost metrics:** `cost`, `total_cost` from `message_send`
6. **Edit format:** `edit_format` (affects token usage patterns)
7. **Repository size:** `num_files` from `repo` event

## Model Naming Conventions

Models appear in various formats:
- `"gpt-5"` - OpenAI GPT-5
- `"gpt-5.2"` - OpenAI GPT-5.2  
- `"gpt-5.2-codex"` - GPT-5.2 Codex variant
- `"gpt-5.3-codex"` - GPT-5.3 Codex
- `"gpt-5.4"` - GPT-5.4
- `"o3-pro"` - OpenAI o3-pro
- `"openai/gpt-5.2"` - Prefixed format
- `"gemini/gemini-2.5-pro"` - Google Gemini 2.5 Pro
- `"gemini/gemini-2.5-flash-lite"` - Gemini Flash Lite
- `"gemini/gemini-3-flash-preview"` - Gemini 3 Flash Preview
- `"gemini/gemini-3-pro-preview"` - Gemini 3 Pro Preview
- `"gemini/REDACTED"` - Redacted/sensitive model name

## File Path

**Default location:** `~/.aider.analytics.jsonl`

---

**Acceptance Criteria Met:**
- ✅ Schema documented with field names, types, and example values
- ✅ Key fields identified: model names (`main_model`, `weak_model`, `editor_model`), token counts (`prompt_tokens`, `completion_tokens`, `total_tokens`), timestamps (`time`), session identifiers (`user_id`)
- ✅ Format confirmed as JSONL (one JSON object per line)
