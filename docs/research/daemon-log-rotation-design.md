# Daemon Log Rotation — Design Decision

**Bead:** agentscr-4c114280
**Date:** 2026-08-16
**Status:** Design Complete — Implementation Pending

## Problem Statement

The daemon's `daemon.log` file grows unbounded, risking disk-space exhaustion. The current code has a critical gap between configured behavior and actual implementation:

**Config declares (`src/config.rs`):**
- Default rotation: `"size"` 
- Fields: `log_rotation`, `log_max_size_bytes`, `log_retention_count`
- Documents modes: `"size"`, `"daily"`, `"hourly"`, `"daily+size"`

**Code implements (`src/daemon.rs`):**
- Default rotation: `"daily"` (hardcoded, ignores config)
- Only implements: `"daily"`, `"hourly"`, `"never"`
- **Missing**: `"size"` mode (the config default!)
- **Missing**: `"daily+size"` hybrid mode
- **Unused**: `log_max_size_bytes` field

## Current Implementation Analysis

### Location: `src/daemon.rs:951-1017` (`init_file_logging_with_config`)

```rust
// Get rotation settings from config or use defaults
let (rotation_mode, retention_count) = if let Some(cfg) = config {
    (
        cfg.daemon.log_rotation.clone(),  // ← Config says "size"
        cfg.daemon.log_retention_count,
    )
} else {
    ("daily".to_string(), 7)
};

// Create the appropriate rolling appender
let appender = match rotation_mode.as_str() {
    "daily" => { /* ... */ }
    "hourly" => { /* ... */ }
    "never" => { /* ... */ }
    _ => {
        eprintln!("Unknown log rotation mode '{}', defaulting to daily", rotation_mode);
        // Falls back to daily — size mode is lost!
    }
};
```

### Issues

1. **Config default is ignored**: Config defaults to `"size"`, but code falls back to `"daily"` for unknown modes
2. **No size-based rotation**: `tracing_appender::rolling::daily/hourly/never` are time-based only
3. **`log_max_size_bytes` field is unused**: Config has a 10MB default that's never read
4. **No hybrid mode**: `"daily+size"` isn't implemented

## Design Decision: Use `tracing-appender`'s `RollingFileAppender`

### Chosen Approach: `tracing-appender` with Size-Based Rotation

**Primary mode:** Size-based rotation (when `log_max_size_bytes` is exceeded)  
**Secondary mode:** Time-based rotation (daily/hourly, optional)  
**Hybrid mode:** Both time and size triggers ( `"daily+size"` )

### Rationale

1. **No new dependencies**: Already using `tracing-appender` v0.2
2. **Size-based is safer**: Prevents unbounded growth from sudden log spikes (e.g., error loops)
3. **Config intent aligns**: Config already defaults to `"size"` mode
4. **Minimal code changes**: ~50 lines to implement missing modes

### Implementation Strategy

#### Option A: `tracing-appender::non_blocking` + Custom Size Check

**How it works:**
- Use `tracing_appender::rolling::daily()` as base
- Wrap in `tracing_appender::non_blocking()` 
- Add a background task that checks file size every N seconds
- When size exceeds `log_max_size_bytes`, trigger rotation by renaming current file

**Pros:**
- Leverages existing time-based rotation
- Size check is cheap (one `stat()` call)
- Rotation happens outside write path (no blocking)

**Cons:**
- Custom rotation logic (not using tracing-appender's built-in)
- Time window between size check and rotation can still exceed limit

#### Option B: `tracing_appender::rolling::RollingFileAppender` (RECOMMENDED)

**How it works:**
- `tracing-appender` provides `RollingFileAppender` directly
- Configure with `Rotation::HOURLY` / `DAILY` for time-based
- For size-based, use custom `Rotation` trigger or manual file rotation

**Pros:**
- Standard tracing-appender pattern
- Time-based rotation built-in and reliable
- Clear separation of concerns

**Cons:**
- `RollingFileAppender` doesn't support size-based rotation directly
- Need to implement size-trigger manually (similar to Option A)

#### Option C: Custom Wrapper with Size Trigger (RECOMMENDED)

**How it works:**
- Create a custom writer wrapper that checks file size on each write
- When size exceeds limit, close current file and open new one with timestamp suffix
- Use `tracing_appender::non_blocking` to avoid blocking writes

**Pros:**
- Size limit is enforced immediately (no time window)
- Simple and predictable behavior
- Works with any time-based mode

**Cons:**
- Slight overhead on each write (one file size check)
- Custom implementation required

### Final Design: Hybrid Approach

I recommend **Option C** with the following implementation:

```rust
// In init_file_logging_with_config():

let appender = match rotation_mode.as_str() {
    "size" => {
        // Size-based rotation only
        let appender = SizeBasedAppender::new(
            log_dir,
            log_name,
            app_config.daemon.log_max_size_bytes,
        );
        BoxMakeWriter::new(appender)
    }
    "daily" => {
        // Time-based rotation at midnight
        let appender = tracing_appender::rolling::daily(log_dir, log_name);
        BoxMakeWriter::new(appender)
    }
    "hourly" => {
        // Time-based rotation every hour
        let appender = tracing_appender::rolling::hourly(log_dir, log_name);
        BoxMakeWriter::new(appender)
    }
    "daily+size" => {
        // Hybrid: rotate at midnight OR when exceeding size limit
        let appender = HybridAppender::new(
            log_dir,
            log_name,
            app_config.daemon.log_max_size_bytes,
        );
        BoxMakeWriter::new(appender)
    }
    "never" => {
        // No rotation
        let appender = tracing_appender::rolling::never(log_dir, log_name);
        BoxMakeWriter::new(appender)
    }
    _ => {
        eprintln!("Unknown log rotation mode '{}', defaulting to daily", rotation_mode);
        let appender = tracing_appender::rolling::daily(log_dir, log_name);
        BoxMakeWriter::new(appender)
    }
};
```

### New Appender Implementations

#### `SizeBasedAppender`

```rust
struct SizeBasedAppender {
    log_dir: PathBuf,
    log_name: String,
    max_size_bytes: u64,
    current_file: Arc<Mutex<fs::File>>,
    current_size: Arc<AtomicU64>,
}

impl io::Write for SizeBasedAppender {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut file = self.current_file.lock().unwrap();
        
        // Check if we need to rotate before writing
        let current_size = self.current_size.load(Ordering::Relaxed);
        if current_size + buf.len() as u64 > self.max_size_bytes {
            self.rotate(&mut file)?;
        }
        
        let n = file.write(buf)?;
        self.current_size.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
    
    fn flush(&mut self) -> io::Result<()> {
        self.current_file.lock().unwrap().flush()
    }
}
```

#### `HybridAppender` (for `"daily+size"`)

Similar to `SizeBasedAppender`, but also checks time-based rotation trigger (can wrap a `tracing_appender::rolling::daily` appender and trigger rotation if size limit is hit).

### Configuration Changes (None Required)

The existing `config.toml` structure already supports this:

```toml
[daemon]
log_rotation = "size"           # "daily" | "hourly" | "size" | "daily+size" | "never"
log_max_size_bytes = 10485760   # 10MB default
log_retention_count = 7          # Keep last 7 rotated files
```

No config changes needed — just implement what's already declared.

### File Naming Convention

**Size-based rotation:**
- Pattern: `daemon.log`, `daemon.log.2026-08-16-14-30-45`, `daemon.log.2026-08-16-15-45-20`
- Timestamp suffix indicates when rotation occurred

**Time-based (existing):**
- Pattern: `daemon.log.2026-08-16`, `daemon.log.2026-08-16-14`

**Hybrid:**
- Pattern: Same as size-based (timestamped when rotation triggered)

### Retention Policy

Existing `cleanup_old_logs()` function (lines 1219-1278) already handles retention correctly:
- Scans directory for `<log_prefix>.log.*` files
- Sorts by modification time (newest first)
- Keeps most recent `retention_count` files
- Deletes older files

**No changes needed to retention logic.**

## Implementation Plan

1. **Add custom appender types** to `src/daemon.rs`:
   - `SizeBasedAppender` struct
   - `HybridAppender` struct
   - Implement `io::Write` and `MakeWriter` traits

2. **Update `init_file_logging_with_config()`**:
   - Implement `"size"` mode (currently missing)
   - Implement `"daily+size"` hybrid mode (currently missing)
   - Remove hardcoded `"daily"` fallback for unknown modes

3. **Update config default** (optional):
   - Keep `"size"` as default in `config.rs`
   - Or change to `"daily+size"` for safest default

4. **Tests**:
   - Unit test for size-based rotation threshold
   - Unit test for hybrid mode (both triggers)
   - Integration test with real log writes

## Alternatives Considered

### External Rotation (logrotate)

**Rejected** because:
- Adds external dependency on system configuration
- Not cross-platform (Linux-specific)
- Requires root/sudo for system-wide logrotate config
- AgentScribe users may not have permission to configure

### Custom Rotation in Background Thread

**Rejected** because:
- More complex than write-path size check
- Time window between size check and rotation
- Additional thread overhead

### Time-Based Only (Current State)

**Rejected** because:
- Doesn't prevent disk-space exhaustion from error loops
- Config already declares size-based mode as default
- ADR-1 context shows 43MB log file grew in 26 days (time-based rotation would have limited this, but size-based is safer)

## Migration Notes

**For existing deployments:**
- Default mode changes from `"daily"` (actual) to `"size"` (configured)
- Existing `daemon.log` files are not affected
- New rotation only applies to writes after daemon restart
- No data loss — old log files remain readable

**Config migration:** None needed — config structure unchanged.

## References

- ADR-1: 2026-07-20 — Crash-safe, self-healing persistence for daemon scrape state
- `src/daemon.rs:951-1017` — Current `init_file_logging_with_config()` implementation
- `src/config.rs:129-176` — `DaemonConfig` struct with rotation settings
- `tracing-appender` docs: https://docs.rs/tracing-appender/latest/tracing_appender/
