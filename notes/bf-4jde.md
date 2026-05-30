# Fix bf-4jde: Subagent Default Exclusion

## Issue
The plan specified that sub-agents should be "excluded by default" but the mechanism didn't support opt-in.

## Root Cause
The `paths` glob pattern `~/.claude/projects/*/*.jsonl` only matched 2 directory levels, while subagent files are at 3 levels (`<project>/<session-uuid>/subagents/agent-*.jsonl`). This meant subagents were already excluded by the glob being too shallow, making the `exclude = ["*/subagents/*"]` entry redundant and the "opt-in" mechanism non-functional.

## Fix
Changed glob pattern to `~/.claude/projects/**/*.jsonl` (using `**` for recursive matching) so that:
1. Subagent paths are included by the glob
2. `exclude = ["*/subagents/*"]` excludes them by default
3. Opt-in is possible by overriding `exclude` in user config or custom plugins

## Files Changed
- `plugins/claude-code.toml`: Updated paths glob
- `docs/plan.md`: Updated example to match
