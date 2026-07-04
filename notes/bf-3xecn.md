# Audit Report: build_manifest_from_events Call Sites Missing 6th Argument

## Function Signature (CONFIRMED)

**Location:** `src/index.rs:588-595`

```rust
pub fn build_manifest_from_events(
    events: &[Event],
    session_id: &str,
    source_agent: &str,
    project: Option<&str>,
    model: Option<&str>,
    parent_session_id: Option<&str>,  // 6th parameter
) -> SessionManifest
```

The function takes **exactly 6 parameters**. The last parameter `parent_session_id: Option<&str>` is the parameter missing from many call sites.

---

## Call Sites Requiring Fixes

### 1. src/recurring.rs:97 - **SEVERE** (missing 3 arguments)
```rust
let manifest = build_manifest_from_events(&events, session_id, &source_agent);
```
**Missing:** `project`, `model`, `parent_session_id`

Note: `project` is available on line 94 but not passed. This call site needs 3 additional arguments, not just 1.

---

### 2. src/cli.rs:1415-1421 - Missing parent_session_id
```rust
let manifest = crate::index::build_manifest_from_events(
    &events,
    &session_id,
    &agent_name,
    first.project.as_deref(),
    first.model.as_deref(),
);  // <-- Missing parent_session_id argument
```

---

### 3. src/cli.rs:2652-2658 - Missing parent_session_id
```rust
let manifest = crate::index::build_manifest_from_events(
    &events,
    sid,
    agent,
    events.first().and_then(|e| e.project.as_deref()),
    events.first().and_then(|e| e.model.as_deref()),
);  // <-- Missing parent_session_id argument
```

---

### 4. src/cli.rs:2689-2695 - Missing parent_session_id
```rust
let manifest = crate::index::build_manifest_from_events(
    &events,
    &sid,
    agent,
    events.first().and_then(|e| e.project.as_deref()),
    events.first().and_then(|e| e.model.as_deref()),
);  // <-- Missing parent_session_id argument
```

---

### 5. src/cli.rs:3355-3361 - Missing parent_session_id
```rust
let manifest = crate::index::build_manifest_from_events(
    &events,
    &session_id,
    parts[0],
    events.first().and_then(|e| e.project.as_deref()),
    events.first().and_then(|e| e.model.as_deref()),
);  // <-- Missing parent_session_id argument
```

---

### 6. src/cli.rs:3432-3438 - Missing parent_session_id
```rust
let manifest = crate::index::build_manifest_from_events(
    &events,
    &session_id,
    agent,
    events.first().and_then(|e| e.project.as_deref()),
    events.first().and_then(|e| e.model.as_deref()),
);  // <-- Missing parent_session_id argument
```

---

## Test Files (src/index.rs) - All Missing parent_session_id

All test functions in `src/index.rs` use the following pattern (5 arguments only):

```rust
let manifest = build_manifest_from_events(&events, "test/X", "claude", None, None);
```

**Test call sites:**
- Line 1025: `test_build_manifest_from_events_basic`
- Line 1044: `test_build_manifest_from_events_empty`
- Line 1073: `test_build_manifest_from_events_files_deduped`
- Line 1100: `test_build_manifest_from_events_timestamps`
- Line 1226: `test_build_manifest_from_events_clips_duration`
- Line 1249: `test_build_manifest_from_events_includes_outcome`
- Line 1292: `test_build_manifest_from_events_includes_outcome_implicit_success`
- Line 1334: `test_build_manifest_from_events_propagates_project_and_model`
- Line 1359: `test_build_manifest_from_events_project_and_model_override_none`
- Line 1404: `test_build_manifest_from_events_defaults_to_running_if_no_outcome`
- Line 1420: `test_build_manifest_from_events_treats_unknown_as_running`
- Line 1464: `test_build_manifest_from_events_running_if_early_termination`
- Line 1585: `test_build_manifest_from_events_ends_at_last_event`
- Line 1632: `test_build_manifest_from_events_running_state_propagates`

All 14 test call sites need `None` added as the 6th argument.

---

## Call Site That Is CORRECT

**src/scraper/mod.rs:220** - This call correctly passes all 6 arguments:
```rust
build_manifest_from_events(events, session_id, source_agent, project, model, None);
```

---

## Summary

| File | Line Count | Severity |
|------|-----------|----------|
| src/recurring.rs | 1 | Severe (missing 3 args) |
| src/cli.rs | 5 | Normal (missing 1 arg) |
| src/index.rs (tests) | 14 | Normal (missing 1 arg) |
| **Total** | **20** | - |

All call sites need to be updated to pass the 6th argument `parent_session_id: Option<&str>`. Most production code should probably pass `None` unless there's parent session tracking logic in place.
