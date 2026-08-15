# Behavioral Sidecar File Naming Convention

## Summary

Verified the actual file naming convention used for behavioral signals sidecar files in AgentScribe.

## Code Implementation

The code consistently uses the `.behavioral.json` suffix format:

**File:** `src/enrichment/behavioral_signals.rs`

- **Line 272-273:** Documentation comment states:
  ```rust
  /// The sidecar is stored as `sessions/<agent>/<session_id>.behavioral.json`
  /// alongside the session JSONL.
  ```

- **Line 288:** `write_behavioral_sidecar()` function:
  ```rust
  let sidecar_path = plugin_dir.join(format!("{}.behavioral.json", parts[1]));
  ```

- **Line 308:** `load_behavioral_signals()` function:
  ```rust
  .join(format!("{}.behavioral.json", parts[1]));
  ```

- **Line 357:** `read_behavioral_signals()` function:
  ```rust
  .join(format!("{}.behavioral.json", parts[1]));
  ```

## Test Confirmation

**File:** `tests/behavioral_signals_integration_tests.rs`

- **Line 301:** Test explicitly verifies this naming pattern:
  ```rust
  let sidecar_path = data_dir
      .path()
      .join("sessions/claude-code")
      .join("test-session-123.behavioral.json");
  assert!(sidecar_path.exists());
  ```

## Naming Convention

**Correct pattern:** `<session_id>.behavioral.json`

**Examples:**
- `sessions/claude-code/abc123-def456.behavioral.json`
- `sessions/aider/session-0.behavioral.json`

**Session ID format:** `<agent>/<session-id>`

**Full path pattern:**
```
~/.agentscribe/sessions/<agent>/<session_id>.behavioral.json
```

## Verification Results

1. **Code uses:** `.behavioral.json` suffix (with leading dot)
2. **Code does NOT use:** `behavioral_signals.json` pattern
3. **Actual files:** No behavioral sidecar files currently exist in the sessions directory (only `.jsonl` session files)
4. **Consistency:** All code references and tests use the same `.behavioral.json` pattern

## Conclusion

The implemented and documented file naming convention is:
- **Suffix:** `.behavioral.json`
- **Not `behavioral_signals.json`**

This convention is consistent across:
- Implementation code
- Test code
- Documentation comments

No discrepancies found between implementation and documentation.
