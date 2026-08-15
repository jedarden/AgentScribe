# Behavioral Sidecar File Naming Convention Verification

**Date:** 2025-08-15  
**Task:** Verify the actual file naming convention used for behavioral signals sidecar files.

## Findings

### Actual Implementation
The code uses the `.behavioral.json` file extension consistently:

**File:** `src/enrichment/behavioral_signals.rs`

**Line 288:** `write_behavioral_sidecar()`
```rust
let sidecar_path = plugin_dir.join(format!("{}.behavioral.json", parts[1]));
```

**Line 308:** `load_behavioral_signals()`
```rust
.join(format!("{}.behavioral.json", parts[1]));
```

**Line 357:** `read_behavioral_signals()`
```rust
.join(format!("{}.behavioral.json", parts[1]));
```

### Full File Path Pattern
The complete path pattern is:
```
~/.agentscribe/sessions/<agent>/<session-id>.behavioral.json
```

For example, for a Claude Code session with ID `a2379efa-0bbd-4cb3-aa52-48dd948fd66d`:
```
~/.agentscribe/sessions/claude-code/a2379efa-0bbd-4cb3-aa52-48dd948fd66d.behavioral.json
```

### Documentation Inconsistencies Found
There are **incorrect** references to `behavioral_signals.json` (with underscore) in documentation comments:

1. **Line 314** in `behavioral_signals.rs`:
   ```rust
   /// Read and parse behavioral_signals.json sidecar for a session.
   ```
   Should be: `.behavioral.json`

2. **Line 539** in `src/reflect.rs`:
   ```rust
   /// Returns sessions that have behavioral_signals.json sidecars available.
   ```
   Should be: `.behavioral.json`

3. **Line 272** in `behavioral_signals.rs` (CORRECT):
   ```rust
   /// The sidecar is stored as `sessions/<agent>/<session_id>.behavioral.json`
   ```

### Current State
No behavioral signals sidecar files currently exist on disk:
- No files matching `*.behavioral.json` found
- No files matching `behavioral_signals.json` found
- Only `.jsonl` session files are present

## Conclusion

**The correct naming convention is:** `<session-id>.behavioral.json`

The code implementation is consistent and uses the `.behavioral.json` extension throughout. However, there are documentation comments that incorrectly reference `behavioral_signals.json` which should be updated for consistency.

## Recommendation

Update the inconsistent documentation comments to use `.behavioral.json` instead of `behavioral_signals.json`:
1. Line 314 in `src/enrichment/behavioral_signals.rs`
2. Line 539 in `src/reflect.rs`

This will ensure documentation matches the actual implementation.
