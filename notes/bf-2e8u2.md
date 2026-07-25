# Test Failure Analysis: test_multiple_subagents_same_parent

## Test: `test_multiple_subagents_same_parent`

### Expected Behavior
The test creates 4 source files:
- 1 parent session: `sessions/claude-code/parent-shared-123.jsonl`
- 3 subagent sessions in a nested structure:
  - `sessions/claude-code/parent-shared-123/subagents/agent-000.jsonl`
  - `sessions/claude-code/parent-shared-123/subagents/agent-001.jsonl`
  - `sessions/claude-code/parent-shared-123/subagents/agent-002.jsonl`

The test expects the scraper to:
1. Detect that the 3 subagent files have `parent_session_id = "parent-shared-123"`
2. Write these sessions to a nested output structure preserving the hierarchy
3. Find exactly 4 sessions total (1 parent + 3 subagents)

### Actual Behavior
The test finds 7 sessions instead of 4:
```
DEBUG: Found 7 sessions:
  claude-code/parent-shared-123
  claude-code/agent-002
  claude-code/parent-shared-123/subagents/agent-002
  claude-code/parent-shared-123/subagents/agent-000
  claude-code/parent-shared-123/subagents/agent-001
  claude-code/agent-000
  claude-code/agent-001
```

### Root Cause Identified

The bug is in the `parent_session_id` extraction logic in `src/parser/jsonl.rs` (lines 469-502):

```rust
let parent_session_id = source_path
    .components()
    .collect::<Vec<_>>()
    .iter()
    .position(|c| c.as_os_str() == "subagents")
    .and_then(|subagents_idx| {
        if subagents_idx >= 2 {
            let components: Vec<_> = source_path.components().collect();
            let parent_idx = subagents_idx - 1;
            
            // Check if there's a "projects" component somewhere before the parent session
            let has_projects_before_parent = components[..parent_idx]
                .iter()
                .any(|c| c.as_os_str() == "projects");
            
            if has_projects_before_parent {
                components.get(parent_idx)...
            } else {
                None  // ← This path is taken!
            }
        } else { None }
    });
```

**The Problem:** The code requires a "projects" directory in the path structure, but the test creates files under `/tmp/.../sessions/claude-code/...` where there is NO "projects" directory!

Test path: `/tmp/.../sessions/claude-code/parent-shared-123/subagents/agent-000.jsonl`

The path components are:
- Various tmp directories
- **"sessions"** ← NOT "projects"!
- "claude-code"
- "parent-shared-123"
- "subagents"
- "agent-000.jsonl"

Because there's no "projects" component, `has_projects_before_parent = false`, and the function returns `None` for `parent_session_id`.

**When `parent_session_id = None`:**
The subagent files are treated as MAIN sessions and written to a flat structure:
```rust
// scraper/mod.rs:551-554
else {
    self.sessions_dir
        .join(&plugin.plugin.name)
        .join(format!("{}.jsonl", session_info.session_id))
}
// Outputs: sessions/claude-code/agent-000.jsonl
```

This explains why `claude-code/agent-000`, `claude-code/agent-001`, and `claude-code/agent-002` exist as standalone sessions!

### Why Are There ALSO Sessions in the "subagents" Path?

The test output also shows sessions at:
- `claude-code/parent-shared-123/subagents/agent-002`
- `claude-code/parent-shared-123/subagents/agent-000`  
- `claude-code/parent-shared-123/subagents/agent-001`

This suggests that either:
1. The files are being processed twice (once with parent_session_id detected, once without)
2. There are old test artifacts from previous runs
3. The glob pattern is matching files in both source and output directories

### The Fix

The `parent_session_id` detection logic needs to be fixed to handle test directory structures that don't have a "projects" directory. Options:

1. **Remove the "projects" requirement** - Just check for the path pattern `.../parent-uuid/subagents/agent-id.jsonl`
2. **Make the test use "projects" directory** - Update test to create files at `/tmp/.../projects/.../sessions/...`
3. **Make the "projects" check optional** - Only require it when running in production mode

The test expectations suggest that option 1 or 2 is intended - the test clearly expects subagent sessions to be detected and written to a nested structure.

### File Locations
- Test code: `tests/parent_session_tests.rs:294-383`
- Bug location: `src/parser/jsonl.rs:469-502` (parent_session_id extraction)
- Writing logic: `src/scraper/mod.rs:544-555`
