# Daemon Log Rotation Investigation

**Date:** 2026-08-20  
**Task:** Investigate daemon.log tracing setup and design rotation approach  
**Bead:** agentscr-4c114280  
**Parent:** agentscr-e9e20b3b

---

## Current State

### Log File Location
- **Path:** `~/.agentscribe/daemon.log` (configurable via `data_dir`)
- **Constants:** `src/daemon.rs:36` defines `LOG_FILE = "daemon.log"`

### Current Implementation

**Location:** `src/daemon.rs:950-1017` (`init_file_logging_with_config`)

The daemon uses `tracing-appender`'s pre-built rolling appenders:

```rust
let appender = match rotation_mode.as_str() {
    "daily" => {
        let a = tracing_appender::rolling::daily(log_dir, log_name);
        BoxMakeWriter::new(a)
    }
    "hourly" => {
        let a = tracing_appender::rolling::hourly(log_dir, log_name);
        BoxMakeWriter::new(a)
    }
    "never" => {
        let a = tracing_appender::rolling::never(log_dir, log_name);
        BoxMakeWriter::new(a)
    }
    _ => { /* defaults to daily */ }
};
```

**Supported modes:**
- `"daily"` - Rotates at midnight
- `"hourly"` - Rotates every hour
- `"never"` - No rotation

**Retention:** `cleanup_old_logs()` (lines 1219-1278) removes old log files, keeping only the most recent `log_retention_count` files.

### Configuration Structure

**Location:** `src/config.rs:129-176` (`DaemonConfig`)

```rust
pub struct DaemonConfig {
    pub log_rotation: String,              // Default: "size" (NOTE: not implemented)
    pub log_max_size_bytes: u64,          // Default: 10MB
    pub log_retention_count: usize,        // Default: 7 files
    // ... other fields
}
```

**IMPORTANT MISMATCH:** The config default is `"size"` (line 154), but the implementation only recognizes `"daily"`, `"hourly"`, and `"never"`. The `"size"` mode falls through to the default case and gets treated as `"daily"`.

---

## Problem Analysis

### The Real-World Incident (from ADR-1 context)

The daemon.log grew to **43MB without rotation** over 26 days. This happened because:

1. **Time-based rotation alone doesn't bound single-file size**
   - Daily rotation means 24 hours of logs in one file
   - A busy daemon (high scrape activity, errors, debug logging) can generate tens of MB in a day
   - The incident shows ~1.6MB/day average

2. **Size limit exists in config but isn't enforced**
   - `log_max_size_bytes` defaults to 10MB
   - This field is never read or used by `init_file_logging_with_config`
   - Only time-based modes are implemented

3. **`cleanup_old_logs` has the right logic but runs too late**
   - It's called AFTER `tracing::subscriber::set_global_default()` succeeds
   - By that point, the current log file has already grown unbounded
   - Cleanup only works on rotated files, not the active one

---

## Recommended Approach

### Decision: Use `tracing_appender::rolling::RollingFileAppender` Directly

**Chosen approach:** Size-based rolling rotation with configurable size limit and retention count.

**Rationale:**

1. **Burst protection:** Daemons experience variable load. A single debugging session or error storm can generate megabytes of logs in minutes. Size-based rotation caps this regardless of time.

2. **Predictable disk usage:** With size-based rotation (10MB default) and retention count (7 files), maximum disk usage is bounded at ~70MB regardless of daemon uptime or activity level.

3. **Simpler mental model:** "Rotate at 10MB" is clearer than "Rotate daily at midnight, but also check if file exceeds 10MB" (hybrid approach).

4. **Industry standard:** Most production daemons (nginx, systemd-journald with size limits, Docker containers) use size-based rotation for the same reason.

### Implementation Plan

**Replace the existing appender creation in `init_file_logging_with_config`:**

```rust
use tracing_appender::rolling::{RollingFileAppender, Rotation};

// Determine rotation interval for file naming
let rotation = match cfg.daemon.log_rotation.as_str() {
    "hourly" => Rotation::HOURLY,
    "never" | _ => Rotation::DAILY,  // Default for naming
};

// Create size-based rolling appender
let appender = RollingFileAppender::new(
    rotation,                           // Time suffix for rotated files
    log_dir,                            // Directory for logs
    log_name,                           // Base name ("daemon")
    cfg.daemon.log_max_size_bytes,      // Rotate when exceeding this size
);

// Wrap in BoxMakeWriter
let writer = BoxMakeWriter::new(appender);
```

**Cleanup remains the same:** `cleanup_old_logs()` already correctly removes old files based on `log_retention_count`. It just needs to run periodically (already does on daemon startup).

### Configuration Changes

**Update `src/config.rs` documentation (lines 137-141):**

```rust
/// Log rotation mode for file naming (default: "daily")
/// - "daily": Time-based suffix in rotated files (daemon.log.2026-03-16)
/// - "hourly": Time-based suffix with hour (daemon.log.2026-03-16-14)
/// - "never": No time-based naming (uses counter instead)
/// NOTE: Rotation is triggered by SIZE, not time. This setting only
/// affects the filename suffix of rotated files.
#[serde(default = "default_log_rotation")]
pub log_rotation: String,
```

**Update default to match reality (line 154):**

```rust
fn default_log_rotation() -> String {
    "daily".to_string()  // Changed from "size" to "daily"
}
```

### Behavior Changes

**Before (current):**
- Daily rotation at midnight
- File size unbounded within a day
- 43MB file possible (as demonstrated)

**After (proposed):**
- Rotation when file exceeds 10MB (configurable)
- Time suffix from `log_rotation` setting for organization
- Maximum disk usage: `log_max_size_bytes × log_retention_count`
- With defaults: 10MB × 7 files = 70MB maximum

---

## Alternative Considered

### Hybrid Rotation (daily + size)

**Approach:** Rotate at midnight AND when exceeding size limit.

**Rejected because:**
- Adds complexity without meaningful benefit
- Size-based alone already bounds disk usage
- Midnight rotation adds minor value (organizing logs by calendar day) but can be achieved via time suffix in size-based rotation
- Hybrid is not natively supported by `tracing-appender`'s pre-built appenders, would require custom wrapper logic

---

## Implementation Notes

### No Code Changes in This Bead

Per acceptance criteria, this bead is **investigation and design only**. Implementation will be tracked in a follow-up bead.

### Testing Strategy

When implementing:

1. **Unit tests:** Verify `RollingFileAppender` creation with different size limits
2. **Integration test:** Generate logs exceeding `log_max_size_bytes` and verify rotation occurs
3. **Cleanup test:** Verify `cleanup_old_logs` respects retention count
4. **Config validation:** Add test that warns if `log_rotation` is "size" (old invalid value)

### Migration Path

For users with existing `daemon.log` files:

1. Existing file continues to be used until it exceeds size limit
2. No data loss or renaming required
3. New rotated files will use time-based suffixes going forward

---

## References

- **tracing-appender docs:** https://docs.rs/tracing-appender/latest/tracing_appender/rolling/
- **ADR-1 context:** Lines 16-40 describe the 43MB incident and crash-safe state persistence needs
- **Parent bead:** agentscr-e9e20b3b references ADR-1 and daemon.log setup investigation
