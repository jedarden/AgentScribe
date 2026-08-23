# Daemon Log Rotation Design Analysis

**Date:** 2026-08-23
**Bead:** agentscr-4c114280
**Status:** Investigation complete - implementation decision made

## Current Setup

### Implementation Location
- **File:** `src/daemon.rs`, function `init_file_logging_with_config()` (lines 950-1017)
- **Config:** `src/config.rs`, struct `DaemonConfig` (lines 129-176)

### How It Works Now

The daemon uses `tracing_appender::rolling` with three time-based rotation modes:

1. **`"daily"`** (default in code) - Rotates at midnight
2. **`"hourly"`** - Rotates every hour  
3. **`"never"`** - No rotation, single file grows unbounded

Log files are named with date suffixes:
- Daily: `daemon.log.2024-03-16`
- Hourly: `daemon.log.2024-03-16-14`

### Configuration (Declared vs. Implemented)

The `DaemonConfig` struct documents these options:

```toml
[daemon]
log_rotation = "daily"  # or "hourly", "never", "size", "daily+size"
log_max_size_bytes = 10485760  # 10MB default
log_retention_count = 7
```

**However**, the actual code only implements `"daily"`, `"hourly"`, and `"never"`.

### Retention Cleanup

The `cleanup_old_logs()` function (daemon.rs:1219-1278) handles retention:
- Scans log directory for files matching pattern `<log_prefix>.log.*`
- Sorts by modification time (newest first)
- Keeps the most recent `retention_count` files
- Deletes older files

This works correctly for the implemented time-based rotation modes.

## Problem Statement

According to ADR-1 context (agentscr-4c114280 parent bead), the daemon log grew unbounded to **43MB** because:

1. The config documentation claims support for `"size"` and `"daily+size"` rotation
2. These modes are **not actually implemented** in the code
3. Users who set `log_rotation = "size"` expecting size-based rotation get `"daily"` instead (with a warning)

### Why This Matters

- **Time-based rotation alone is insufficient** for daemon logs:
  - A busy daemon can generate hundreds of MB in a single day
  - Daily rotation doesn't bound maximum file size
  - Disk space exhaustion is a real risk

- **Size-based rotation is the documented intent**:
  - The config comments explicitly recommend `"size"` mode
  - The `log_max_size_bytes` field exists but is unused
  - Users reading the config are misled about what works

## Investigation: tracing-appender Capabilities

I investigated whether `tracing_appender` crate natively supports size-based rotation.

### Findings

The `tracing_appender::rolling` module only provides:
- `daily()` - Time-based, midnight rotation
- `hourly()` - Time-based, hourly rotation  
- `never()` - No rotation

**No size-based rotation is built into tracing-appender.**

To implement size-based rotation, we have two options:

### Option 1: Custom Wrapper Appender
Create a custom `MakeWriter` that wraps the rolling appender and checks file size before each write, triggering rotation when size exceeds threshold.

**Pros:**
- Works with existing tracing infrastructure
- No new external dependencies
- Consistent with current architecture

**Cons:**
- More complex implementation (~100-150 lines)
- Need to handle race conditions (concurrent writes)
- Must test carefully to avoid log loss during rotation

### Option 2: Switch to Different Crate
Use a crate like `rolling-file` that supports size-based rotation natively.

**Pros:**
- Well-tested, maintained implementation
- Less custom code to maintain
- Built-in size limits

**Cons:**
- New dependency (adds ~50-100KB to binary)
- Potential API incompatibilities
- Migration cost from tracing-appender

## Recommended Approach: Custom Size-Based Rotation

**Decision:** Implement **Option 1** (custom wrapper appender)

### Rationale

1. **No new dependencies** - Aligns with project's minimal-dependency philosophy
2. **Consistent with existing pattern** - Already using custom appenders for time-based rotation
3. **Full control** - Can implement exact semantics needed (size-only, daily+size hybrid)
4. **Low risk** - Isolated to logging subsystem, doesn't affect indexing/scraping

### Implementation Plan

#### Phase 1: Size-Checking Wrapper (Core)

Create `src/logging.rs` with a custom `SizeRollingAppender`:

```rust
pub struct SizeRollingAppender {
    base_dir: PathBuf,
    log_name: String,
    max_size_bytes: u64,
    current_file: Option<File>,
    current_size: AtomicU64,
}

impl SizeRollingAppender {
    fn new(base_dir: PathBuf, log_name: String, max_size_bytes: u64) -> Self {
        // On construction, find the most recent log file
        // Check its size, use it if under limit, else rotate
    }
    
    fn check_and_rotate(&self) -> io::Result<()> {
        // Before each write, check if adding content would exceed limit
        // If yes: close current, rename with timestamp, open new
    }
}
```

#### Phase 2: Update Rotation Mode Handling

Modify `init_file_logging_with_config()` in `src/daemon.rs`:

```rust
let appender = match rotation_mode.as_str() {
    "size" => {
        let a = SizeRollingAppender::new(
            log_dir.to_path_buf(),
            log_name.to_string(),
            cfg.daemon.log_max_size_bytes,
        );
        BoxMakeWriter::new(a)
    }
    "daily+size" => {
        // Hybrid: check both time AND size
        let a = HybridRollingAppender::new(/* ... */);
        BoxMakeWriter::new(a)
    }
    // existing modes...
};
```

#### Phase 3: Hybrid Mode (daily+size)

Create a hybrid appender that:
- Checks for time-based rotation (midnight) on first write after midnight
- Checks for size-based rotation before every write
- Rotates if EITHER condition is met

#### Phase 4: Testing

Add comprehensive tests:
- Size-based rotation triggers exactly at threshold
- Hybrid mode respects both time and size constraints
- No log loss during rotation (atomic rename)
- Retention cleanup works for all rotation modes
- Concurrent writes don't cause corruption

### Configuration Examples

After implementation, users can configure:

```toml
# Size-based only (recommended for production)
[daemon]
log_rotation = "size"
log_max_size_bytes = 10485760  # 10MB
log_retention_count = 7

# Hybrid: rotate at midnight OR when exceeding 10MB
[daemon]
log_rotation = "daily+size"
log_max_size_bytes = 10485760
log_retention_count = 30  # Keep more files with daily rotation
```

### Backward Compatibility

- Existing `"daily"`, `"hourly"`, `"never"` modes continue unchanged
- Default remains `"daily"` (safe default)
- No breaking changes to config schema

## File Size Semantics

### Rotation Trigger

When `log_rotation = "size"`:
- Rotation occurs **before** a write that would cause the file to exceed `log_max_size_bytes`
- Check is: `current_size + write_size > max_size_bytes`
- If true, rotate first, then write to new file

### File Naming

Size-based rotated files use timestamp suffix:
- `daemon.log.20240316-143022` (format: `.YYYYMMDD-HHMMSS`)
- Ensures uniqueness and sortability

### Edge Cases

- **Concurrent writes:** Use `AtomicU64` for size tracking to avoid race conditions
- **Empty write after rotation:** Check file size on open, not just tracking
- **Disk full during rotation:** Fail gracefully, log to stderr as fallback

## Alternatives Considered

### Alternative A: Use `rolling-file` Crate

**Rejected because:**
- Adds external dependency (~50-100KB)
- Unknown long-term maintenance status
- API may not align with tracing-subscriber expectations
- Current tracing_appender integration works well

### Alternative B: External Logrotate

**Rejected because:**
- Requires system-level configuration (not portable)
- Daemon should manage its own logs self-sufficiently
- Adds deployment complexity
- Doesn't help with size-based rotation within the daemon

### Alternative C: Do Nothing, Live with Daily Rotation

**Rejected because:**
- Doesn't solve the 43MB log file problem from ADR-1
- High-volume daemons can still exhaust disk
- Config documentation promises size-based rotation
- User expectation mismatch leads to support burden

## Acceptance Criteria for Implementation

1. ✅ `"size"` mode rotates when file exceeds `log_max_size_bytes`
2. ✅ `"daily+size"` mode rotates on midnight OR size exceeded (whichever first)
3. ✅ No log loss during rotation (atomic rename)
4. ✅ Concurrent writes handled safely (no corruption)
5. ✅ Retention cleanup works for all rotation modes
6. ✅ Comprehensive test coverage
7. ✅ Backward compatible with existing `"daily"`, `"hourly"`, `"never"` modes

## Next Steps

This bead (agentscr-4c114280) covers **investigation and design only**.

Implementation should be tracked as a **separate follow-up bead** that:
1. Creates `src/logging.rs` with custom appender
2. Updates `init_file_logging_with_config()` in `src/daemon.rs`
3. Adds tests in `src/logging.rs` and `src/daemon.rs`
4. Updates docs/plan.md with new rotation capabilities
5. Runs `cargo clippy` and `cargo test` before closing

## References

- ADR-1 context: agentscr-4c114280 parent bead
- Current implementation: `src/daemon.rs:950-1017`
- Config structure: `src/config.rs:129-176`
- Retention cleanup: `src/daemon.rs:1219-1278`
- tracing-appender docs: https://docs.rs/tracing-appender/
