# Fix bf-4jde: Align claude-code.toml with plan spec for subagent exclusion

## Problem
The plan specified that sub-agents should be excluded by default:
- Sub-agents location: `<session-uuid>/subagents/agent-<id>.jsonl`
- Plan spec: `exclude = ["*/subagents/*"]`

But `plugins/claude-code.toml` had:
- `paths` including explicit subagent glob
- `exclude = []` (empty, no exclusion)

## Fix Applied
Updated `plugins/claude-code.toml`:
```toml
[source]
paths = ["~/.claude/projects/*/*.jsonl"]  # Removed explicit subagents path
exclude = ["*/subagents/*"]               # Now excludes subagents by default
```

This aligns with the plan's specification (line 104 of plan.md).

## Why This Matters
- The glob `~/.claude/projects/*/*.jsonl` matches both main sessions AND subagent files
- The exclude pattern `*/subagents/*` filters out subagent files by default
- Users can still opt-in by modifying their local config to remove the exclude pattern
