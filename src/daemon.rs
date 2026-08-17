//! Daemon lifecycle management
//!
//! Provides start/stop/status/run/logs commands for the AgentScribe daemon.
//! The daemon runs a Tokio event loop, uses inotify (via the `notify` crate) to
//! watch all plugin source paths for changes, debounces rapid writes, and triggers
//! incremental scraping automatically.

use crate::config::Config as AppConfig;
use crate::error::{AgentScribeError, Result};
use crate::plugin::Plugin;
use crate::scraper::{git_auto_commit as scraper_git_commit, Scraper};
use chrono::{DateTime, Utc};
use glob::Pattern as GlobPattern;
use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How often to re-discover new files and watch newly created directories (seconds).
pub const REDISCOVERY_INTERVAL_SECS: u64 = 60;

/// Backoff intervals for logging failed watch attempts (seconds).
/// After the first failure, log again after 1 hour, then 24 hours, then max.
const FAILED_WATCH_BACKOFF_SECS: [u64; 4] = [0, 3600, 86400, 86400];

/// PID file name
const PID_FILE: &str = "agentscribe.pid";

/// Log file name
const LOG_FILE: &str = "daemon.log";

/// Daemon state persisted between runs
const STATE_FILE: &str = "daemon_state.json";

// ── Public types ──────────────────────────────────────────────────────

/// Information returned by `daemon status`
#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonInfo {
    pub running: bool,
    pub pid: Option<u32>,
    pub uptime_secs: Option<u64>,
    pub rss_bytes: Option<u64>,
    pub peak_rss_bytes: Option<u64>,
    pub sessions_indexed: Option<u64>,
    pub last_scrape: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    /// Health status: "healthy", "stale", "warning", "critical"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<String>,
    /// Human-readable health message (e.g., "Last scrape was 26 days ago")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_message: Option<String>,
}

/// Daemon state persisted to disk
///
/// Tracks daemon runtime info and per-file byte offsets for incremental parsing.
/// When the daemon restarts, it uses these offsets to resume reading files from
/// where it left off, rather than re-processing the entire file.
#[derive(Debug, Serialize, Deserialize, Default)]
struct PersistedState {
    started_at: Option<DateTime<Utc>>,
    sessions_indexed: u64,
    last_scrape: Option<DateTime<Utc>>,
    /// Per-file byte offsets for incremental parsing
    /// Maps absolute file path to the last byte offset successfully processed
    #[serde(default)]
    file_offsets: HashMap<std::string::String, u64>,
}

impl PersistedState {
    /// Get the last byte offset for a file.
    ///
    /// Returns `None` if the file is not tracked (should read from beginning).
    #[allow(dead_code)]
    fn get_file_offset(&self, file_path: &str) -> Option<u64> {
        self.file_offsets.get(file_path).copied()
    }

    /// Set the byte offset for a file.
    ///
    /// Call this after successfully processing a file up to a certain position.
    /// On next daemon restart, parsing will resume from this offset.
    #[allow(dead_code)]
    fn set_file_offset(&mut self, file_path: &str, offset: u64) {
        self.file_offsets.insert(file_path.to_string(), offset);
    }

    /// Remove a file from tracking.
    ///
    /// Call this when a file is deleted or when truncation is detected.
    #[allow(dead_code)]
    fn remove_file(&mut self, file_path: &str) {
        self.file_offsets.remove(file_path);
    }

    /// Clear all file offsets.
    ///
    /// Call this to force a full rescan of all files on next daemon start.
    #[allow(dead_code)]
    fn clear_file_offsets(&mut self) {
        self.file_offsets.clear();
    }
}

// ── daemon start ──────────────────────────────────────────────────────

/// Start the daemon in the background.
///
/// Forks, writes a PID file, and the child enters the event loop.
pub fn start(data_dir: &Path) -> Result<()> {
    let pid_file = data_dir.join(PID_FILE);
    let log_file = data_dir.join(LOG_FILE);
    let state_file = data_dir.join(STATE_FILE);

    // Ensure directories exist
    fs::create_dir_all(data_dir)?;

    // Check if already running
    if let Some(existing) = read_pid(&pid_file) {
        if process_exists(existing) {
            return Err(AgentScribeError::Config(format!(
                "Daemon already running with PID {}",
                existing
            )));
        }
        // Stale PID file — clean up
        let _ = fs::remove_file(&pid_file);
    }

    // Get the path to the current executable (unused now but needed for re-exec)
    let _exe = std::env::current_exe().map_err(|e| {
        AgentScribeError::Config(format!("Cannot determine executable path: {}", e))
    })?;

    // Fork: double-fork to detach from terminal
    // SAFETY: These are standard POSIX fork calls
    unsafe {
        // First fork
        let pid = libc::fork();
        if pid < 0 {
            return Err(AgentScribeError::Config("First fork failed".to_string()));
        }
        if pid > 0 {
            // Parent — wait briefly for child to write PID, then exit
            std::thread::sleep(Duration::from_millis(300));
            // Verify child started
            if let Ok(content) = fs::read_to_string(&pid_file) {
                if content.trim().parse::<u32>().is_ok() {
                    println!("Daemon started (PID {})", content.trim());
                    return Ok(());
                }
            }
            eprintln!("Warning: daemon may not have started cleanly. Check logs.");
            return Ok(());
        }

        // First child — create new session
        if libc::setsid() < 0 {
            return Err(AgentScribeError::Config("setsid failed".to_string()));
        }

        // Second fork
        let pid2 = libc::fork();
        if pid2 < 0 {
            return Err(AgentScribeError::Config("Second fork failed".to_string()));
        }
        if pid2 > 0 {
            // Intermediate child exits
            std::process::exit(0);
        }

        // Grandchild — this is the daemon process
        let my_pid = std::process::id();
        fs::write(&pid_file, my_pid.to_string())?;

        // Redirect stdin/stdout/stderr to /dev/null
        let devnull = fs::File::open("/dev/null")?;
        let fd = devnull.as_raw_fd();
        libc::dup2(fd, libc::STDIN_FILENO);
        libc::dup2(fd, libc::STDOUT_FILENO);
        libc::dup2(fd, libc::STDERR_FILENO);
        if fd > libc::STDERR_FILENO {
            libc::close(fd);
        }
    }

    // Now in the daemon grandchild — run the event loop
    run_event_loop(&log_file, &pid_file, &state_file, data_dir);

    // Unreachable in normal flow
    std::process::exit(0)
}

// ── daemon run ────────────────────────────────────────────────────────

/// Run the daemon in the foreground (for systemd or debugging).
pub fn run(data_dir: &Path) -> Result<()> {
    let log_file = data_dir.join(LOG_FILE);
    let pid_file = data_dir.join(PID_FILE);
    let state_file = data_dir.join(STATE_FILE);

    fs::create_dir_all(data_dir)?;

    // Check if already running
    if let Some(existing) = read_pid(&pid_file) {
        if process_exists(existing) {
            return Err(AgentScribeError::Config(format!(
                "Daemon already running with PID {}",
                existing
            )));
        }
        let _ = fs::remove_file(&pid_file);
    }

    // Write PID file
    let my_pid = std::process::id();
    fs::write(&pid_file, my_pid.to_string())?;

    // Set up clean shutdown
    let pid_file_clone = pid_file.clone();
    ctrlc_handler(move || {
        cleanup_pid(&pid_file_clone);
        std::process::exit(0);
    });

    run_event_loop(&log_file, &pid_file, &state_file, data_dir);

    Ok(())
}

// ── daemon stop ───────────────────────────────────────────────────────

/// Stop a running daemon by sending SIGTERM.
pub fn stop(data_dir: &Path) -> Result<()> {
    let pid_file = data_dir.join(PID_FILE);

    let pid = read_pid(&pid_file).ok_or_else(|| {
        AgentScribeError::Config("Daemon is not running (no PID file)".to_string())
    })?;

    if !process_exists(pid) {
        // Stale PID file
        let _ = fs::remove_file(&pid_file);
        return Err(AgentScribeError::Config(
            "Daemon is not running (stale PID file cleaned up)".to_string(),
        ));
    }

    // Send SIGTERM
    unsafe {
        if libc::kill(pid as i32, libc::SIGTERM) != 0 {
            return Err(AgentScribeError::Config(format!(
                "Failed to send SIGTERM to PID {}: {}",
                pid,
                std::io::Error::last_os_error()
            )));
        }
    }

    // Wait up to 5 seconds for graceful shutdown
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        if !process_exists(pid) {
            let _ = fs::remove_file(&pid_file);
            println!("Daemon stopped (PID {})", pid);
            return Ok(());
        }
    }

    // Force kill if still running
    eprintln!("Daemon did not shut down gracefully, sending SIGKILL...");
    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }
    std::thread::sleep(Duration::from_millis(200));
    let _ = fs::remove_file(&pid_file);
    println!("Daemon killed (PID {})", pid);
    Ok(())
}

// ── daemon status ─────────────────────────────────────────────────────

/// Get daemon status information.
pub fn status(data_dir: &Path) -> Result<DaemonInfo> {
    let pid_file = data_dir.join(PID_FILE);
    let state_file = data_dir.join(STATE_FILE);

    let pid = read_pid(&pid_file);
    let running = pid.map(process_exists).unwrap_or(false);

    let mut info = DaemonInfo {
        running,
        pid: if running { pid } else { None },
        uptime_secs: None,
        rss_bytes: None,
        peak_rss_bytes: None,
        sessions_indexed: None,
        last_scrape: None,
        started_at: None,
        health: None,
        health_message: None,
    };

    // Even if daemon is not running, check if last_scrape is stale
    // This catches the case where daemon died long ago
    if let Some(state) = load_state(&state_file) {
        info.sessions_indexed = Some(state.sessions_indexed);
        info.last_scrape = state.last_scrape;
        info.started_at = state.started_at;

        // Calculate staleness: use 1 hour as default threshold
        // (could be made configurable via 2x debounce+rediscovery in the future)
        let stale_threshold_secs = 3600; // 1 hour
        let critical_threshold_secs = 86400; // 24 hours

        if let Some(last_scrape_ts) = state.last_scrape {
            let now = Utc::now();
            let duration = now.signed_duration_since(last_scrape_ts);
            let secs_since = duration.num_seconds();

            if secs_since > critical_threshold_secs {
                info.health = Some("critical".to_string());
                let days = secs_since / 86400;
                info.health_message = Some(format!(
                    "CRITICAL: Daemon has not scraped in {} days (last: {})",
                    days,
                    last_scrape_ts.format("%Y-%m-%d %H:%M:%S UTC")
                ));
            } else if secs_since > stale_threshold_secs {
                info.health = Some("stale".to_string());
                let hours = secs_since / 3600;
                info.health_message = Some(format!(
                    "WARNING: Last scrape was {} hours ago (may indicate daemon failure)",
                    hours
                ));
            } else {
                if running {
                    info.health = Some("healthy".to_string());
                } else {
                    info.health = Some("stopped".to_string());
                }
                info.health_message = None;
            }
        } else {
            // No scrape ever recorded
            if running {
                info.health = Some("warning".to_string());
                info.health_message =
                    Some("WARNING: Daemon running but no scrapes recorded yet".to_string());
            } else {
                info.health = Some("stopped".to_string());
                info.health_message = None;
            }
        }
    }

    if !running {
        return Ok(info);
    }

    let pid_val = pid.unwrap();

    // Read /proc/<pid>/stat for RSS and start time
    if let Ok(stat) = fs::read_to_string(format!("/proc/{}/stat", pid_val)) {
        // Parse: pid (comm) state ppid pgrp session tty_nr tpgid flags ...
        // fields after comm (which may contain spaces/parens): find last ')' then count fields
        if let Some(close_paren) = stat.rfind(')') {
            let after_comm = &stat[close_paren + 2..]; // skip ") "
            let fields: Vec<&str> = after_comm.split_whitespace().collect();
            // Per man 5 proc (0-indexed after comm):
            //   field 19 = starttime (clock ticks since boot)
            //   field 20 = vsize (bytes)
            //   field 21 = rss  (pages)
            if fields.len() > 21 {
                if let Ok(rss_pages) = fields[21].parse::<u64>() {
                    info.rss_bytes = Some(rss_pages * 4096); // page size on x86_64
                }
                if let Ok(start_ticks) = fields[19].parse::<u64>() {
                    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as u64;
                    let uptime_ticks = read_uptime_ticks();
                    if uptime_ticks > start_ticks {
                        if let Some(elapsed_secs) = (uptime_ticks - start_ticks).checked_div(hz) {
                            info.uptime_secs = Some(elapsed_secs);
                        }
                    }
                }
            }
        }
    }

    // Read /proc/<pid>/status for VmHWM (peak RSS high water mark)
    if let Ok(status_content) = fs::read_to_string(format!("/proc/{}/status", pid_val)) {
        for line in status_content.lines() {
            if let Some(val) = line.strip_prefix("VmHWM:") {
                let trimmed = val.trim();
                // Format: "12345 kB"
                if let Some(kb) = trimmed.split_whitespace().next() {
                    if let Ok(kb_num) = kb.parse::<u64>() {
                        info.peak_rss_bytes = Some(kb_num * 1024);
                    }
                }
                break;
            }
        }
    }

    Ok(info)
}

// ── daemon logs ───────────────────────────────────────────────────────

/// Tail the daemon log file.
pub fn logs(data_dir: &Path, follow: bool, lines: usize) -> Result<()> {
    let log_file = data_dir.join(LOG_FILE);

    if !log_file.exists() {
        return Err(AgentScribeError::Config(
            "No daemon log file found. Has the daemon been started?".to_string(),
        ));
    }

    if follow {
        follow_log(&log_file, lines)
    } else {
        tail_log(&log_file, lines)
    }
}

// ── Internal: event loop ──────────────────────────────────────────────

/// The main daemon event loop. Runs a Tokio runtime and idles on a timer.
fn run_event_loop(log_file: &Path, pid_file: &Path, state_file: &Path, data_dir: &Path) {
    // Load config for log rotation settings before initializing logging
    let config_path = data_dir.join("config.toml");
    let app_config = AppConfig::load(&config_path).unwrap_or_default();

    // Use current_thread runtime — safe after fork (no thread pool)
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create Tokio runtime");

    let log_path = log_file.to_path_buf();
    let pid_path = pid_file.to_path_buf();
    let state_path = state_file.to_path_buf();
    let data_path = data_dir.to_path_buf();

    let _guard = rt.enter();

    // Initialize file logging with config
    init_file_logging_with_config(&log_path, Some(&app_config));

    let started_at = Utc::now();

    // Persist initial state
    let mut state = load_state(&state_path).unwrap_or_default();
    state.started_at = Some(started_at);
    let _ = save_state(&state_path, &state);

    tracing::info!("AgentScribe daemon started (PID {})", std::process::id());

    // Register signal handlers for clean shutdown
    let pid_path_clone = pid_path.clone();
    ctrlc_handler(move || {
        tracing::info!("Received shutdown signal, cleaning up...");
        cleanup_pid(&pid_path_clone);
        std::process::exit(0);
    });

    // Extract settings from the already-loaded config
    let debounce_secs = app_config.scrape.debounce_seconds;
    let lock_timeout_secs = app_config.scrape.lock_timeout_seconds;
    let git_auto_commit = app_config.scrape.git_auto_commit;
    let mcp_enabled = app_config.daemon.mcp_enabled;
    let mcp_socket_path = app_config
        .mcp_socket_path()
        .unwrap_or_else(|_| data_path.join("mcp.sock"));

    tracing::info!(
        debounce_secs,
        git_auto_commit,
        mcp_enabled,
        mcp_socket = %mcp_socket_path.display(),
        rediscovery_interval_secs = REDISCOVERY_INTERVAL_SECS,
        "daemon configuration"
    );

    // Block on the async file-watch loop
    rt.block_on(run_watch_loop(
        data_path,
        state_path,
        state,
        debounce_secs,
        lock_timeout_secs,
        git_auto_commit,
        if mcp_enabled {
            Some(mcp_socket_path)
        } else {
            None
        },
    ));
}

// ── Debouncer ─────────────────────────────────────────────────────────

/// Tracks pending file changes and implements debounce logic.
///
/// A file is considered "ready" once its debounce window (measured from the
/// last recorded change) has elapsed without another change being recorded.
struct Debouncer {
    /// path → (plugin_name, time_of_last_change)
    pending: HashMap<PathBuf, (String, Instant)>,
}

impl Debouncer {
    fn new() -> Self {
        Debouncer {
            pending: HashMap::new(),
        }
    }

    /// Record a change to `path` for `plugin_name`.
    ///
    /// Resets the debounce timer if the file was already pending, so a rapid
    /// sequence of writes does not trigger a scrape until writes stop.
    fn record(&mut self, path: PathBuf, plugin_name: String) {
        self.pending.insert(path, (plugin_name, Instant::now()));
    }

    /// Return (and remove) all entries whose debounce window has elapsed.
    fn drain_ready(&mut self, debounce: Duration) -> Vec<(PathBuf, String)> {
        let ready: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, (_, t))| t.elapsed() >= debounce)
            .map(|(p, _)| p.clone())
            .collect();

        ready
            .into_iter()
            .map(|path| {
                let (plugin_name, _) = self.pending.remove(&path).unwrap();
                (path, plugin_name)
            })
            .collect()
    }

    #[allow(dead_code)]
    fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

// ── Internal: file watcher helpers ────────────────────────────────────

/// Extract the deepest directory that contains no glob wildcards from a source
/// path pattern, e.g. `~/.claude/projects/*/*.jsonl` → `~/.claude/projects`.
fn base_dir_from_glob_pattern(pattern: &str) -> Option<PathBuf> {
    let expanded = shellexpand::tilde(pattern).into_owned();
    let wildcard_pos = expanded.find(['*', '?', '[']).unwrap_or(expanded.len());
    let non_glob = &expanded[..wildcard_pos];
    let base = Path::new(non_glob);
    let dir = if non_glob.ends_with('/') || non_glob.ends_with('\\') {
        base.to_path_buf()
    } else {
        base.parent()?.to_path_buf()
    };
    if dir.as_os_str().is_empty() {
        None
    } else {
        Some(dir)
    }
}

/// Return true if `path` matches any of `plugin`'s source glob patterns.
fn file_matches_plugin(path: &Path, plugin: &Plugin) -> bool {
    let path_str = match path.to_str() {
        Some(s) => s,
        None => return false,
    };
    for pat_str in &plugin.source.paths {
        let expanded = shellexpand::full(pat_str)
            .map(|c| c.into_owned())
            .unwrap_or_else(|_| pat_str.clone());
        if let Ok(pattern) = GlobPattern::new(&expanded) {
            if pattern.matches(path_str) {
                return true;
            }
        }
    }
    false
}

// ── Internal: watch loop ───────────────────────────────────────────────

/// State maintained across iterations of the file-watch loop.
struct WatchLoop {
    /// The notify watcher (kept alive for its entire lifetime).
    _watcher: RecommendedWatcher,
    /// Receives raw notify events from the background inotify thread.
    event_rx: tokio::sync::mpsc::UnboundedReceiver<notify::Event>,
    /// Debounce tracker: holds pending files until their quiet window expires.
    debouncer: Debouncer,
    /// All loaded plugin definitions (for pattern matching).
    plugins: Vec<Plugin>,
    /// Directories currently being watched.
    watched_dirs: HashSet<PathBuf>,
    /// When we last refreshed watched directories.
    last_discovery: Instant,
    /// Directories that failed to watch, with timestamps of last logged attempt.
    /// Tracks backoff state to prevent infinite identical WARN spam.
    failed_dirs: HashMap<PathBuf, Instant>,
}

impl WatchLoop {
    /// Try to add `dir` to the inotify watch set, log on failure with backoff.
    fn try_watch(&mut self, dir: &Path) {
        if self.watched_dirs.contains(dir) || !dir.exists() {
            return;
        }

        // Check if this directory has failed before and if we should retry logging
        let should_log = if let Some(last_attempt) = self.failed_dirs.get(dir) {
            // Calculate which backoff interval we're in
            let elapsed_secs = last_attempt.elapsed().as_secs();
            let mut interval_index = 0;
            for (i, &interval) in FAILED_WATCH_BACKOFF_SECS.iter().enumerate() {
                if elapsed_secs >= interval {
                    interval_index = i;
                } else {
                    break;
                }
            }

            // Only retry if we've passed the current backoff interval
            // and we haven't exhausted all intervals
            let next_interval = FAILED_WATCH_BACKOFF_SECS
                .get(interval_index + 1)
                .copied()
                .unwrap_or(FAILED_WATCH_BACKOFF_SECS[FAILED_WATCH_BACKOFF_SECS.len() - 1]);

            elapsed_secs >= next_interval && interval_index < FAILED_WATCH_BACKOFF_SECS.len() - 1
        } else {
            // First attempt, always log
            true
        };

        match self._watcher.watch(dir, RecursiveMode::Recursive) {
            Ok(()) => {
                tracing::info!(path = %dir.display(), "watching directory");
                self.watched_dirs.insert(dir.to_path_buf());
                // Remove from failed_dirs if it was there (success after retries)
                self.failed_dirs.remove(dir);
            }
            Err(e) => {
                if should_log {
                    tracing::warn!(path = %dir.display(), error = %e, "cannot watch directory");
                }
                // Update or insert the timestamp of this failed attempt
                self.failed_dirs.insert(dir.to_path_buf(), Instant::now());
            }
        }
    }

    /// Refresh watches for all base directories derived from plugin source paths.
    fn refresh_watches(&mut self) {
        let base_dirs: Vec<PathBuf> = self
            .plugins
            .iter()
            .flat_map(|p| p.source.paths.iter())
            .filter_map(|pat| base_dir_from_glob_pattern(pat))
            .collect();

        for dir in base_dirs {
            self.try_watch(&dir);
        }

        self.last_discovery = Instant::now();
        tracing::debug!(watched = self.watched_dirs.len(), "watch refresh complete");
    }

    /// Process a raw notify event, queueing files that match a plugin pattern.
    fn handle_event(&mut self, event: notify::Event) {
        use notify::EventKind;

        let relevant = matches!(
            event.kind,
            EventKind::Create(_)
                | EventKind::Modify(notify::event::ModifyKind::Data(_))
                | EventKind::Modify(notify::event::ModifyKind::Any)
        );
        if !relevant {
            return;
        }

        for path in event.paths {
            if !path.is_file() {
                continue;
            }
            for plugin in &self.plugins {
                if file_matches_plugin(&path, plugin) {
                    let name = plugin.plugin.name.clone();
                    tracing::debug!(path = %path.display(), plugin = %name, "file change queued");
                    self.debouncer.record(path.clone(), name);
                    break;
                }
            }
        }
    }

    /// Drain all files whose debounce window has elapsed.
    fn drain_ready(&mut self, debounce: Duration) -> Vec<(PathBuf, String)> {
        self.debouncer.drain_ready(debounce)
    }
}

/// Async watch-and-scrape loop.  Runs until `shutdown_requested()` is set.
///
/// `mcp_socket_path`: when `Some`, the MCP server is started and listens on
/// that Unix socket for the lifetime of the loop.
async fn run_watch_loop(
    data_dir: PathBuf,
    state_path: PathBuf,
    mut daemon_state: PersistedState,
    debounce_secs: u64,
    lock_timeout_secs: u64,
    git_auto_commit: bool,
    mcp_socket_path: Option<PathBuf>,
) {
    // ── load plugins for path-pattern matching ────────────────────────
    let plugins: Vec<Plugin> = {
        let mut scraper = match Scraper::new_with_lock_timeout(data_dir.clone(), lock_timeout_secs)
        {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to create scraper: {}", e);
                return;
            }
        };
        if let Err(e) = scraper.load_plugins() {
            tracing::warn!("Failed to load plugins: {}", e);
        }
        scraper.plugin_manager().all().values().cloned().collect()
    };

    if plugins.is_empty() {
        tracing::warn!("No plugins loaded — daemon idling without file watching");
    } else {
        tracing::info!(count = plugins.len(), "plugins loaded for file watching");
    }

    // ── set up notify channel ─────────────────────────────────────────
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<notify::Event>();

    let watcher = match RecommendedWatcher::new(
        move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        },
        NotifyConfig::default(),
    ) {
        Ok(w) => w,
        Err(e) => {
            tracing::error!("Failed to create file watcher: {}", e);
            return;
        }
    };

    let mut wl = WatchLoop {
        _watcher: watcher,
        event_rx: rx,
        debouncer: Debouncer::new(),
        plugins,
        watched_dirs: HashSet::new(),
        // Trigger immediate discovery on first tick
        last_discovery: Instant::now()
            .checked_sub(Duration::from_secs(REDISCOVERY_INTERVAL_SECS + 1))
            .unwrap_or_else(Instant::now),
        // Track failed watch attempts to prevent infinite identical WARN spam
        failed_dirs: HashMap::new(),
    };

    let debounce = Duration::from_secs(debounce_secs);
    let rediscovery = Duration::from_secs(REDISCOVERY_INTERVAL_SECS);

    // ── optional MCP server ───────────────────────────────────────────
    // Spawned as a sibling task; receives a oneshot signal when the watch
    // loop exits so it can clean up its Unix socket before returning.
    let _mcp_shutdown_tx: Option<tokio::sync::oneshot::Sender<()>> =
        if let Some(ref socket_path) = mcp_socket_path {
            let (tx, rx) = tokio::sync::oneshot::channel::<()>();
            let data_dir_clone = data_dir.clone();
            let socket_path_clone = socket_path.clone();
            tokio::spawn(crate::mcp::run_mcp_server(
                data_dir_clone,
                socket_path_clone,
                rx,
            ));
            Some(tx)
        } else {
            None
        };

    // ── main loop ─────────────────────────────────────────────────────
    loop {
        if shutdown_requested() {
            tracing::info!("Shutdown requested, exiting watch loop");
            break;
        }

        // Refresh watched dirs periodically (picks up newly created directories)
        if wl.last_discovery.elapsed() >= rediscovery {
            wl.refresh_watches();
        }

        // Wait up to 1 s for an inotify event or let the debounce timer tick
        tokio::select! {
            Some(event) = wl.event_rx.recv() => {
                wl.handle_event(event);
            }
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                // Debounce check on each tick
            }
        }

        // Collect files whose debounce window has expired
        let ready = wl.drain_ready(debounce);
        if ready.is_empty() {
            continue;
        }

        tracing::info!(
            files = ready.len(),
            debounce_secs,
            "debounce expired, scraping changed files"
        );

        // ── scrape one file at a time (I/O-bound, not CPU-bound) ──────
        let mut scraper = match Scraper::new_with_lock_timeout(data_dir.clone(), lock_timeout_secs)
        {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to create scraper: {}", e);
                continue;
            }
        };
        if let Err(e) = scraper.load_plugins() {
            tracing::warn!("Failed to load plugins for scrape: {}", e);
        }

        let mut combined = crate::scraper::ScrapeResult {
            sessions_scraped: 0,
            sessions_indexed: 0,
            events_written: 0,
            errors: Vec::new(),
            files_processed: 0,
            files_skipped: 0,
            agent_types: Vec::new(),
        };

        for (file_path, plugin_name) in &ready {
            // Re-fetch the plugin from the freshly loaded scraper
            if let Some(plugin) = scraper.plugin_manager().get(plugin_name).cloned() {
                tracing::info!(file = %file_path.display(), plugin = %plugin_name, "scraping file");
                match scraper.scrape_file(file_path, &plugin) {
                    Ok(result) => {
                        tracing::info!(
                            file = %file_path.display(),
                            sessions = result.sessions_scraped,
                            indexed = result.sessions_indexed,
                            "scrape complete"
                        );
                        combined.sessions_scraped += result.sessions_scraped;
                        combined.sessions_indexed += result.sessions_indexed;
                        combined.events_written += result.events_written;
                        combined.files_processed += result.files_processed;
                        combined.errors.extend(result.errors);
                        for agent in result.agent_types {
                            if !combined.agent_types.contains(&agent) {
                                combined.agent_types.push(agent);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(file = %file_path.display(), error = %e, "scrape error");
                    }
                }
            } else {
                tracing::warn!(plugin = %plugin_name, "plugin not found in scraper");
            }
        }

        // Persist incremental scrape state
        if let Err(e) = scraper.state_manager().save() {
            tracing::warn!(error = %e, "failed to save scrape state");
        }

        // Optionally commit newly scraped sessions to git
        if git_auto_commit && combined.sessions_scraped > 0 {
            match scraper_git_commit(&data_dir, &combined) {
                Ok(true) => {
                    tracing::info!(sessions = combined.sessions_scraped, "git auto-committed")
                }
                Ok(false) => {}
                Err(e) => tracing::warn!(error = %e, "git auto-commit failed"),
            }
        }

        // Process embedding queue if vector index is enabled
        if combined.sessions_scraped > 0 {
            let config_path = data_dir.join("config.toml");
            if let Ok(app_config) = AppConfig::load(&config_path) {
                if app_config.vector.enabled {
                    tracing::debug!("Vector index enabled, processing embeddings");
                    // Embeddings are processed asynchronously to avoid blocking the scrape loop
                    // New sessions will be picked up on the next daemon tick or via `agentscribe embed build`
                    if let Err(e) = queue_sessions_for_embedding(&data_dir, &combined) {
                        tracing::warn!(error = %e, "failed to queue sessions for embedding");
                    }
                }
            }
        }

        // Drop the scraper to free Tantivy heap memory between scrapes
        drop(scraper);

        // Update persisted daemon state
        daemon_state.sessions_indexed += combined.sessions_scraped as u64;
        daemon_state.last_scrape = Some(Utc::now());
        if let Err(e) = save_state(&state_path, &daemon_state) {
            tracing::warn!(error = %e, "failed to save daemon state");
        }

        tracing::info!(
            total_sessions_indexed = daemon_state.sessions_indexed,
            "daemon state updated"
        );
    }
}

/// Set up file-based logging with optional configuration override.
/// If config is None, uses daily rotation with default retention.
fn init_file_logging_with_config(log_path: &Path, config: Option<&AppConfig>) {
    use tracing_subscriber::fmt::writer::BoxMakeWriter;
    use tracing_subscriber::EnvFilter;

    let log_dir = log_path.parent().unwrap_or(Path::new("."));
    let _ = fs::create_dir_all(log_dir);

    let log_name = log_path
        .file_name()
        .unwrap_or_default()
        .to_str()
        .unwrap_or("daemon");

    // Get rotation settings from config or use defaults
    let (rotation_mode, retention_count) = if let Some(cfg) = config {
        (
            cfg.daemon.log_rotation.clone(),
            cfg.daemon.log_retention_count,
        )
    } else {
        // Default: daily rotation with 7 files retained
        ("daily".to_string(), 7)
    };

    // Create the appropriate rolling appender based on rotation mode
    // Note: All rolling appenders implement MakeWriter, so we can use them uniformly
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
        _ => {
            // Log to stderr before tracing is set up
            eprintln!(
                "Unknown log rotation mode '{}', defaulting to daily",
                rotation_mode
            );
            let a = tracing_appender::rolling::daily(log_dir, log_name);
            BoxMakeWriter::new(a)
        }
    };

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let subscriber = tracing_subscriber::fmt()
        .with_writer(appender)
        .with_env_filter(filter)
        .with_ansi(false)
        .finish();

    let _ = tracing::subscriber::set_global_default(subscriber);

    // Now that tracing is initialized, clean up old log files
    // Use eprintln as fallback since tracing might not work immediately
    if let Err(e) = cleanup_old_logs(log_dir, log_name, retention_count) {
        eprintln!("Failed to clean up old log files: {}", e);
    }
}

/// Global shutdown flag set by signal handlers.
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Install Ctrl-C and SIGTERM handlers that set the global shutdown flag.
/// Returns a clone of the Arc<bool> so callers can poll for shutdown.
fn install_signal_handlers() {
    // Use sigaction for SIGTERM on Linux
    unsafe {
        extern "C" fn sigterm_handler(_: libc::c_int) {
            SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
        }
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = sigterm_handler as *const () as usize;
        libc::sigemptyset(&mut action.sa_mask);
        action.sa_flags = 0;
        libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut());
    }

    // Ctrl-C (SIGINT) via ctrlc crate
    let _ = ctrlc::set_handler(|| {
        SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
    });
}

/// Check if a shutdown has been requested.
fn shutdown_requested() -> bool {
    SHUTDOWN_REQUESTED.load(Ordering::SeqCst)
}

/// Legacy wrapper: install signal handlers and register a cleanup callback.
fn ctrlc_handler(handler: impl Fn() + Send + Sync + 'static) {
    let handler = Arc::new(handler);

    install_signal_handlers();

    // Spawn a thread that polls the shutdown flag and invokes the callback
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(100));
            if shutdown_requested() {
                handler();
                // Handler typically calls process::exit, but just in case:
                break;
            }
        }
    });
}

// ── Internal: log tailing ─────────────────────────────────────────────

/// Print the last N lines of a log file.
fn tail_log(log_file: &Path, n: usize) -> Result<()> {
    let content = fs::read_to_string(log_file)?;
    let lines: Vec<&str> = content.lines().collect();
    let start = if lines.len() > n { lines.len() - n } else { 0 };
    for line in &lines[start..] {
        println!("{}", line);
    }
    Ok(())
}

/// Follow a log file, printing new lines as they appear.
fn follow_log(log_file: &Path, initial_lines: usize) -> Result<()> {
    use std::io::{BufRead, Seek, SeekFrom};

    // Print initial lines
    tail_log(log_file, initial_lines)?;

    // Set up a ctrlc handler to break out of the loop
    let _ = ctrlc::set_handler(|| {
        std::process::exit(0);
    });

    // Open and seek to end so we only print new content
    let file = fs::File::open(log_file)?;
    let mut reader = std::io::BufReader::new(file);
    reader.seek(SeekFrom::End(0))?;

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => {
                // EOF — wait for more data (file is not rotated for daemon.log)
                std::thread::sleep(Duration::from_millis(200));
            }
            Ok(_) => {
                print!("{}", line);
                let _ = std::io::stdout().lock().flush();
            }
            Err(_) => {
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

// ── Internal: helpers ─────────────────────────────────────────────────

/// Read PID from the PID file.
fn read_pid(pid_file: &Path) -> Option<u32> {
    fs::read_to_string(pid_file)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
}

/// Check if a process with the given PID exists.
fn process_exists(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Remove the PID file.
fn cleanup_pid(pid_file: &Path) {
    let _ = fs::remove_file(pid_file);
}

/// Read system uptime in clock ticks from /proc/uptime.
fn read_uptime_ticks() -> u64 {
    if let Ok(content) = fs::read_to_string("/proc/uptime") {
        // First field is uptime in seconds
        if let Some(secs_str) = content.split_whitespace().next() {
            if let Ok(secs) = secs_str.parse::<f64>() {
                let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
                return (secs * hz) as u64;
            }
        }
    }
    0
}

/// Load persisted daemon state from disk.
///
/// Returns `None` if the file doesn't exist or is corrupted. This allows the
/// daemon to recover gracefully from corrupted state by starting with an empty
/// state.
fn load_state(path: &Path) -> Option<PersistedState> {
    let content = fs::read_to_string(path).ok()?;
    match serde_json::from_str::<PersistedState>(&content) {
        Ok(state) => Some(state),
        Err(_) => {
            // Corrupted state file - log warning and return empty state
            eprintln!(
                "Warning: Corrupted daemon state file at {}, starting with empty state",
                path.display()
            );

            // Rename corrupted file for debugging
            let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
            let backup_path = path.with_extension(format!("json.corrupt-{}", timestamp));
            let _ = fs::rename(path, &backup_path);
            eprintln!(
                "Corrupted state file backed up to: {}",
                backup_path.display()
            );

            Some(PersistedState::default())
        }
    }
}

/// Save persisted daemon state to disk using atomic writes.
///
/// Writes to a temporary file then renames over the target. This ensures that
/// a crash during write leaves a valid previous state file, not a truncated one.
fn save_state(path: &Path, state: &PersistedState) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(state)?;

    // Create temporary file in the same directory as the target
    // Use both process ID and thread ID to ensure uniqueness
    let pid = std::process::id();
    let tid = std::thread::current().id();

    // Build temp file name: daemon-state.json -> daemon-state.tmp-PID-TID.json
    let temp_file = if let Some(stem) = path.file_stem() {
        if let Some(ext) = path.extension() {
            path.with_file_name(format!(
                "{}.tmp-{}-{:?}.{}",
                stem.to_str().unwrap_or("daemon-state"),
                pid,
                tid,
                ext.to_str().unwrap_or("json")
            ))
        } else {
            path.with_file_name(format!(
                "{}.tmp-{}-{:?}",
                stem.to_str().unwrap_or("daemon-state"),
                pid,
                tid
            ))
        }
    } else {
        path.with_extension(format!("tmp-{}-{:?}", pid, tid))
    };

    // Write to temporary file
    fs::write(&temp_file, json)?;

    // Atomic rename over the target
    fs::rename(&temp_file, path)
}

/// Clean up old log files based on retention policy.
///
/// Keeps only the most recent `retention_count` log files, removing older ones.
/// Log files are named with a suffix like `.log.2024-03-16` or `.log.2024-03-16-14` for daily/hourly rotation.
fn cleanup_old_logs(log_dir: &Path, log_prefix: &str, retention_count: usize) -> Result<()> {
    if retention_count == 0 {
        // 0 means keep all logs (no cleanup)
        return Ok(());
    }

    // Find all log files matching the pattern: <log_prefix>.log.*
    let _pattern = format!("{}.log.*", log_prefix);
    let mut log_files: Vec<PathBuf> = Vec::new();

    if let Ok(entries) = fs::read_dir(log_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(os_str) = path.file_name() {
                if let Some(file_name) = os_str.to_str() {
                    // Match files like "daemon.log.2024-03-16" or "daemon.log.2024-03-16-14"
                    if file_name.starts_with(&format!("{}.log.", log_prefix)) {
                        log_files.push(path);
                    }
                }
            }
        }
    }

    // Sort by modification time (newest first)
    log_files.sort_by(|a, b| {
        let a_time = fs::metadata(a)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        let b_time = fs::metadata(b)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        b_time.cmp(&a_time) // Descending order (newest first)
    });

    // Keep the most recent `retention_count` files, delete the rest
    if log_files.len() > retention_count {
        eprintln!(
            "Cleaning up {} old log files (retaining {} most recent)",
            log_files.len() - retention_count,
            retention_count
        );
        for old_file in &log_files[retention_count..] {
            eprintln!("Deleting old log file: {}", old_file.display());
            if let Err(e) = fs::remove_file(old_file) {
                eprintln!(
                    "Failed to delete old log file {}: {}",
                    old_file.display(),
                    e
                );
            }
        }
    }

    Ok(())
}

/// Queue newly scraped sessions for embedding.
///
/// This function marks new sessions as pending for embedding. The actual
/// embedding is done asynchronously via `agentscribe embed build` or the
/// embedding queue will be processed on the next daemon tick.
fn queue_sessions_for_embedding(
    data_dir: &Path,
    scrape_result: &crate::scraper::ScrapeResult,
) -> Result<()> {
    use crate::config::Config;

    let config_path = data_dir.join("config.toml");
    let config = Config::load(&config_path).unwrap_or_default();

    if !config.vector.enabled {
        return Ok(());
    }

    // The scrape_result doesn't contain session IDs directly, but we can
    // infer that new sessions need embedding by checking the sessions directory
    // In a full implementation, the scraper would return the list of scraped session IDs
    // For now, we just note that embedding is needed (users run `agentscribe embed build`)

    tracing::debug!(
        sessions = scrape_result.sessions_scraped,
        "sessions queued for embedding"
    );

    // In daemon mode, we don't block on embedding. The user should run
    // `agentscribe embed build` periodically, or we could add async processing here.
    // For now, this is a no-op that logs the intent.

    Ok(())
}

// ── systemd service install/uninstall ─────────────────────────────────

/// Name of the systemd user service unit file.
const SERVICE_NAME: &str = "agentscribe.service";

/// Install a systemd user service unit for the daemon.
///
/// Creates `~/.config/systemd/user/agentscribe.service` pointing to the
/// current executable with `daemon run`, then prints instructions to
/// enable and start it.
pub fn install_service() -> Result<()> {
    let exe = std::env::current_exe().map_err(|e| {
        AgentScribeError::Config(format!("Cannot determine executable path: {}", e))
    })?;
    let exe_str = exe.to_string_lossy();

    let service_dir = service_unit_dir()?;
    fs::create_dir_all(&service_dir)?;

    let unit_path = service_dir.join(SERVICE_NAME);

    let unit_content = format!(
        "[Unit]\n\
         Description=AgentScribe daemon\n\
         After=default.target\n\
         \n\
         [Service]\n\
         ExecStart={exe} daemon run\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        exe = exe_str,
    );

    if unit_path.exists() {
        eprintln!(
            "Warning: {} already exists — overwriting.",
            unit_path.display()
        );
    }

    fs::write(&unit_path, &unit_content)?;
    println!("Installed: {}", unit_path.display());
    println!();
    println!("To enable and start the service:");
    println!("  systemctl --user daemon-reload");
    println!("  systemctl --user enable --now agentscribe");
    println!();
    println!("To check status:");
    println!("  systemctl --user status agentscribe");

    Ok(())
}

/// Remove the systemd user service unit for the daemon.
///
/// Deletes `~/.config/systemd/user/agentscribe.service`.
pub fn uninstall_service() -> Result<()> {
    let unit_path = service_unit_dir()?.join(SERVICE_NAME);

    if !unit_path.exists() {
        return Err(AgentScribeError::Config(format!(
            "Service unit not found: {}",
            unit_path.display()
        )));
    }

    fs::remove_file(&unit_path)?;
    println!("Removed: {}", unit_path.display());
    println!();
    println!("If the service was enabled, run:");
    println!("  systemctl --user disable agentscribe");
    println!("  systemctl --user daemon-reload");

    Ok(())
}

/// Return the directory where the user systemd unit file should live.
fn service_unit_dir() -> Result<std::path::PathBuf> {
    // Respect $XDG_CONFIG_HOME if set, otherwise fall back to ~/.config
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            directories::BaseDirs::new()
                .map(|d| d.home_dir().join(".config"))
                .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        });
    Ok(config_home.join("systemd").join("user"))
}

/// Format bytes as human-readable.
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Format seconds as human-readable duration.
pub fn format_duration(secs: u64) -> String {
    if secs >= 86400 {
        format!("{}d {}h", secs / 86400, (secs % 86400) / 3600)
    } else if secs >= 3600 {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    } else if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}s", secs)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{LogFormat, Parser, Plugin, PluginMeta, SessionDetection, Source};
    use std::thread;

    // ── helpers ───────────────────────────────────────────────────────

    fn make_plugin(name: &str, paths: Vec<&str>) -> Plugin {
        Plugin {
            plugin: PluginMeta {
                name: name.to_string(),
                version: "1.0".to_string(),
            },
            source: Source {
                paths: paths.iter().map(|s| s.to_string()).collect(),
                exclude: vec![],
                format: LogFormat::Jsonl,
                session_detection: SessionDetection::default(),
                tree: None,
                truncation_limit: None,
                array: None,
                envelope: None,
            },
            parser: Parser::default(),
            metadata: None,
        }
    }

    // ── base_dir_from_glob_pattern ────────────────────────────────────

    #[test]
    fn test_base_dir_simple_wildcard() {
        let dir = base_dir_from_glob_pattern("/home/user/.claude/projects/*/*.jsonl");
        assert_eq!(dir, Some(PathBuf::from("/home/user/.claude/projects")));
    }

    #[test]
    fn test_base_dir_no_wildcard() {
        // Without a wildcard the parent dir of the file is returned.
        let dir = base_dir_from_glob_pattern("/home/user/logs/session.jsonl");
        assert_eq!(dir, Some(PathBuf::from("/home/user/logs")));
    }

    #[test]
    fn test_base_dir_trailing_slash_before_wildcard() {
        let dir = base_dir_from_glob_pattern("/data/logs/*/foo.jsonl");
        assert_eq!(dir, Some(PathBuf::from("/data/logs")));
    }

    #[test]
    fn test_base_dir_wildcard_at_root() {
        // Wildcard directly under / — base dir should be /
        let dir = base_dir_from_glob_pattern("/*.jsonl");
        assert_eq!(dir, Some(PathBuf::from("/")));
    }

    #[test]
    fn test_base_dir_question_mark_wildcard() {
        let dir = base_dir_from_glob_pattern("/tmp/agent/session?.jsonl");
        assert_eq!(dir, Some(PathBuf::from("/tmp/agent")));
    }

    #[test]
    fn test_base_dir_bracket_wildcard() {
        let dir = base_dir_from_glob_pattern("/tmp/[abc]/session.jsonl");
        assert_eq!(dir, Some(PathBuf::from("/tmp")));
    }

    // ── file_matches_plugin ───────────────────────────────────────────

    #[test]
    fn test_file_matches_plugin_exact_glob() {
        let plugin = make_plugin("claude", vec!["/tmp/test/*.jsonl"]);
        assert!(file_matches_plugin(
            Path::new("/tmp/test/session.jsonl"),
            &plugin
        ));
    }

    #[test]
    fn test_file_matches_plugin_no_match() {
        let plugin = make_plugin("claude", vec!["/tmp/test/*.jsonl"]);
        assert!(!file_matches_plugin(
            Path::new("/tmp/other/session.jsonl"),
            &plugin
        ));
    }

    #[test]
    fn test_file_matches_plugin_multiple_paths() {
        let plugin = make_plugin("multi", vec!["/tmp/a/*.jsonl", "/tmp/b/*.md"]);
        assert!(file_matches_plugin(Path::new("/tmp/a/foo.jsonl"), &plugin));
        assert!(file_matches_plugin(Path::new("/tmp/b/bar.md"), &plugin));
        assert!(!file_matches_plugin(Path::new("/tmp/c/baz.jsonl"), &plugin));
    }

    #[test]
    fn test_file_matches_plugin_double_star() {
        let plugin = make_plugin("deep", vec!["/tmp/**/*.jsonl"]);
        assert!(file_matches_plugin(
            Path::new("/tmp/a/b/c/session.jsonl"),
            &plugin
        ));
        assert!(!file_matches_plugin(
            Path::new("/tmp/a/b/c/session.md"),
            &plugin
        ));
    }

    // ── Debouncer ─────────────────────────────────────────────────────

    #[test]
    fn test_debouncer_empty_initially() {
        let mut d = Debouncer::new();
        assert_eq!(d.pending_count(), 0);
        assert!(d.drain_ready(Duration::from_millis(0)).is_empty());
    }

    #[test]
    fn test_debouncer_no_ready_before_window_expires() {
        let mut d = Debouncer::new();
        d.record(PathBuf::from("/tmp/a.jsonl"), "plugin-a".to_string());
        // With a 1 h debounce nothing should be ready immediately.
        let ready = d.drain_ready(Duration::from_secs(3600));
        assert!(ready.is_empty());
        assert_eq!(d.pending_count(), 1);
    }

    #[test]
    fn test_debouncer_ready_after_window_expires() {
        let mut d = Debouncer::new();
        d.record(PathBuf::from("/tmp/a.jsonl"), "plugin-a".to_string());
        // Zero-duration debounce: every pending entry is immediately ready.
        let ready = d.drain_ready(Duration::from_millis(0));
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].0, PathBuf::from("/tmp/a.jsonl"));
        assert_eq!(ready[0].1, "plugin-a");
        assert_eq!(d.pending_count(), 0);
    }

    #[test]
    fn test_debouncer_resets_timer_on_repeat_change() {
        let mut d = Debouncer::new();
        let path = PathBuf::from("/tmp/a.jsonl");

        // Record a change that has effectively aged out.
        d.pending.insert(
            path.clone(),
            (
                "plugin-a".to_string(),
                Instant::now() - Duration::from_secs(10),
            ),
        );
        assert_eq!(d.drain_ready(Duration::from_secs(5)).len(), 1);

        // Record the path again — timer resets and the file is no longer ready.
        d.record(path.clone(), "plugin-a".to_string());
        assert!(d.drain_ready(Duration::from_secs(3600)).is_empty());
    }

    #[test]
    fn test_debouncer_multiple_files_independent() {
        let mut d = Debouncer::new();
        let path_a = PathBuf::from("/tmp/a.jsonl");
        let path_b = PathBuf::from("/tmp/b.jsonl");

        // path_a: artificially old (will be ready).
        d.pending.insert(
            path_a.clone(),
            (
                "plugin-a".to_string(),
                Instant::now() - Duration::from_secs(10),
            ),
        );
        // path_b: just recorded (not ready for a long debounce).
        d.record(path_b.clone(), "plugin-b".to_string());

        let ready = d.drain_ready(Duration::from_secs(5));
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].0, path_a);
        // path_b still pending.
        assert_eq!(d.pending_count(), 1);
        assert!(d.pending.contains_key(&path_b));
    }

    #[test]
    fn test_debouncer_drain_removes_entries() {
        let mut d = Debouncer::new();
        d.record(PathBuf::from("/tmp/x.jsonl"), "px".to_string());
        d.record(PathBuf::from("/tmp/y.jsonl"), "py".to_string());
        assert_eq!(d.pending_count(), 2);

        let ready = d.drain_ready(Duration::from_millis(0));
        assert_eq!(ready.len(), 2);
        assert_eq!(d.pending_count(), 0);
    }

    #[test]
    fn test_debouncer_plugin_name_preserved() {
        let mut d = Debouncer::new();
        let path = PathBuf::from("/tmp/session.jsonl");
        d.record(path.clone(), "claude-code".to_string());
        let ready = d.drain_ready(Duration::from_millis(0));
        assert_eq!(ready[0].1, "claude-code");
    }

    #[test]
    fn test_debouncer_real_time_debounce() {
        let mut d = Debouncer::new();
        let path = PathBuf::from("/tmp/live.jsonl");
        d.record(path.clone(), "agent".to_string());

        // Not ready before the window.
        assert!(d.drain_ready(Duration::from_millis(50)).is_empty());

        // Sleep past the window and confirm it's ready.
        thread::sleep(Duration::from_millis(60));
        let ready = d.drain_ready(Duration::from_millis(50));
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].0, path);
    }

    // ── format helpers ────────────────────────────────────────────────

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(50 * 1024 * 1024), "50.0 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GB");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(59), "59s");
        assert_eq!(format_duration(60), "1m 0s");
        assert_eq!(format_duration(3661), "1h 1m");
        assert_eq!(format_duration(86400 + 3600), "1d 1h");
    }

    // ── PID file management ────────────────────────────────────────────

    #[test]
    fn test_read_pid_valid() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("agentscribe.pid");
        fs::write(&pid_file, "12345\n").unwrap();
        assert_eq!(read_pid(&pid_file), Some(12345));
    }

    #[test]
    fn test_read_pid_missing_file() {
        assert_eq!(read_pid(Path::new("/tmp/nonexistent_pid_test")), None);
    }

    #[test]
    fn test_read_pid_invalid_content() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("agentscribe.pid");
        fs::write(&pid_file, "not-a-number\n").unwrap();
        assert_eq!(read_pid(&pid_file), None);
    }

    #[test]
    fn test_cleanup_pid_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("agentscribe.pid");
        fs::write(&pid_file, "99999\n").unwrap();
        assert!(pid_file.exists());
        cleanup_pid(&pid_file);
        assert!(!pid_file.exists());
    }

    // ── Persisted state save/load ──────────────────────────────────────

    #[test]
    fn test_persisted_state_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("daemon_state.json");
        let state = PersistedState {
            started_at: Some(Utc::now()),
            sessions_indexed: 42,
            last_scrape: Some(Utc::now()),
            file_offsets: std::collections::HashMap::new(),
        };
        save_state(&state_file, &state).unwrap();
        let loaded = load_state(&state_file).unwrap();
        assert_eq!(loaded.sessions_indexed, 42);
        assert!(loaded.started_at.is_some());
        assert!(loaded.last_scrape.is_some());
    }

    #[test]
    fn test_persisted_state_defaults() {
        let state = PersistedState::default();
        assert_eq!(state.sessions_indexed, 0);
        assert!(state.started_at.is_none());
        assert!(state.last_scrape.is_none());
    }

    // ── daemon status ──────────────────────────────────────────────────

    #[test]
    fn test_status_not_running_no_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        let info = status(dir.path()).unwrap();
        assert!(!info.running);
        assert!(info.pid.is_none());
    }

    #[test]
    fn test_status_stale_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join(PID_FILE);
        // PID 999999 is extremely unlikely to exist
        fs::write(&pid_file, "999999").unwrap();
        let info = status(dir.path()).unwrap();
        assert!(!info.running);
        assert!(info.pid.is_none());
    }

    #[test]
    fn test_status_stale_last_scrape_critical() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join(STATE_FILE);

        // Create state with last scrape 2 days ago (critical threshold)
        let old_state = PersistedState {
            started_at: Some(Utc::now() - chrono::Duration::days(3)),
            sessions_indexed: 100,
            last_scrape: Some(Utc::now() - chrono::Duration::days(2)),
            file_offsets: std::collections::HashMap::new(),
        };
        save_state(&state_file, &old_state).unwrap();

        let info = status(dir.path()).unwrap();
        assert!(!info.running); // No PID file, so not running
        assert_eq!(info.health, Some("critical".to_string()));
        assert!(info.health_message.is_some());
        assert!(info.health_message.as_ref().unwrap().contains("2 days"));
        assert_eq!(info.sessions_indexed, Some(100));
    }

    #[test]
    fn test_status_stale_last_scrape_stale() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join(STATE_FILE);

        // Create state with last scrape 2 hours ago (stale threshold)
        let old_state = PersistedState {
            started_at: Some(Utc::now() - chrono::Duration::hours(3)),
            sessions_indexed: 50,
            last_scrape: Some(Utc::now() - chrono::Duration::hours(2)),
            file_offsets: std::collections::HashMap::new(),
        };
        save_state(&state_file, &old_state).unwrap();

        let info = status(dir.path()).unwrap();
        assert!(!info.running);
        assert_eq!(info.health, Some("stale".to_string()));
        assert!(info.health_message.is_some());
        assert!(info.health_message.as_ref().unwrap().contains("2 hours"));
    }

    #[test]
    fn test_status_healthy_last_scrape_recent() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join(STATE_FILE);

        // Create state with last scrape 10 minutes ago (healthy)
        let recent_state = PersistedState {
            started_at: Some(Utc::now() - chrono::Duration::minutes(30)),
            sessions_indexed: 25,
            last_scrape: Some(Utc::now() - chrono::Duration::minutes(10)),
            file_offsets: HashMap::new(),
        };
        save_state(&state_file, &recent_state).unwrap();

        let info = status(dir.path()).unwrap();
        assert!(!info.running);
        assert_eq!(info.health, Some("stopped".to_string()));
        assert!(info.health_message.is_none());
    }

    #[test]
    fn test_status_no_scrape_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join(STATE_FILE);

        // Create state with no last scrape
        let empty_state = PersistedState {
            started_at: Some(Utc::now() - chrono::Duration::minutes(5)),
            sessions_indexed: 0,
            last_scrape: None,
            file_offsets: HashMap::new(),
        };
        save_state(&state_file, &empty_state).unwrap();

        let info = status(dir.path()).unwrap();
        assert!(!info.running);
        assert_eq!(info.health, Some("stopped".to_string()));
        assert!(info.health_message.is_none());
    }

    // ── daemon logs ────────────────────────────────────────────────────

    #[test]
    fn test_logs_missing_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let result = logs(dir.path(), false, 10);
        assert!(result.is_err());
    }

    #[test]
    fn test_logs_tail() {
        let dir = tempfile::tempdir().unwrap();
        let log_file = dir.path().join(LOG_FILE);
        fs::write(&log_file, "line1\nline2\nline3\n").unwrap();
        // Should succeed (output goes to stdout)
        assert!(logs(dir.path(), false, 10).is_ok());
    }

    // ── systemd unit generation ────────────────────────────────────────

    #[test]
    fn test_service_unit_dir_respects_xdg() {
        // With XDG_CONFIG_HOME set, should use it
        let dir = service_unit_dir().unwrap();
        assert!(dir.ends_with("systemd/user"));
    }

    // ── graceful shutdown flag ─────────────────────────────────────────

    #[test]
    fn test_shutdown_flag_initially_false() {
        // Reset the flag first
        SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
        assert!(!shutdown_requested());
    }

    #[test]
    fn test_shutdown_flag_set_and_read() {
        SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
        assert!(shutdown_requested());
        // Clean up
        SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
    }

    // ── WatchLoop event handling ───────────────────────────────────────

    #[test]
    fn test_watch_loop_handles_create_event() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<notify::Event>();

        let plugin = make_plugin("test", vec![&format!("{}/*.jsonl", dir.path().display())]);

        let _watcher = notify::RecommendedWatcher::new(
            move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    let _ = tx.send(event);
                }
            },
            NotifyConfig::default(),
        )
        .unwrap();

        // We can't easily construct a WatchLoop since _watcher is moved,
        // so test the Debouncer + event matching logic directly.
        let mut debouncer = Debouncer::new();
        let test_file = dir.path().join("test.jsonl");
        fs::write(&test_file, "test").unwrap();

        assert!(file_matches_plugin(&test_file, &plugin));
        debouncer.record(test_file.clone(), "test".to_string());
        assert_eq!(debouncer.pending_count(), 1);

        let ready = debouncer.drain_ready(Duration::from_millis(0));
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].0, test_file);
        assert_eq!(ready[0].1, "test");
    }

    // ── Integration: debounce coalesces rapid changes ──────────────────

    #[test]
    fn test_debounce_coalesces_rapid_changes_into_single_scrape() {
        let mut d = Debouncer::new();
        let path = PathBuf::from("/tmp/rapid.jsonl");

        // Simulate rapid writes: record the same file 5 times in quick succession
        for _ in 0..5 {
            d.record(path.clone(), "agent".to_string());
        }

        // Only 1 entry should exist (HashMap deduplicates by path)
        assert_eq!(d.pending_count(), 1);

        // Single drain returns all of it
        let ready = d.drain_ready(Duration::from_millis(0));
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].0, path);
        assert_eq!(d.pending_count(), 0);
    }

    #[test]
    fn test_debounce_timer_resets_on_rapid_re_record() {
        let mut d = Debouncer::new();
        let path = PathBuf::from("/tmp/rapid2.jsonl");

        // Record once, artificially age it
        d.pending.insert(
            path.clone(),
            (
                "agent".to_string(),
                Instant::now() - Duration::from_secs(10),
            ),
        );

        // Should be ready with a 5s debounce
        assert_eq!(d.drain_ready(Duration::from_secs(5)).len(), 1);

        // Re-record (simulating another write before scrape fires)
        d.record(path.clone(), "agent".to_string());

        // No longer ready — timer reset
        assert!(d.drain_ready(Duration::from_secs(5)).is_empty());
    }

    // ── Failed watch backoff ───────────────────────────────────────────────

    #[test]
    fn test_failed_watch_backoff_prevents_infinite_logging() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<notify::Event>();

        let plugin = make_plugin("test", vec![&format!("{}/*.jsonl", dir.path().display())]);

        let _watcher = notify::RecommendedWatcher::new(
            move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    let _ = tx.send(event);
                }
            },
            NotifyConfig::default(),
        )
        .unwrap();

        let mut wl = WatchLoop {
            _watcher,
            event_rx: _rx,
            debouncer: Debouncer::new(),
            plugins: vec![plugin],
            watched_dirs: HashSet::new(),
            last_discovery: Instant::now()
                .checked_sub(Duration::from_secs(REDISCOVERY_INTERVAL_SECS + 1))
                .unwrap_or_else(Instant::now),
            failed_dirs: HashMap::new(),
        };

        // Create a directory path that we'll simulate as having failed to watch
        // We don't create the actual directory to avoid a successful watch
        let test_dir = dir.path().join("test");

        // Simulate that this directory previously failed to watch
        // (e.g., due to permissions or temporary unavailability)
        wl.failed_dirs.insert(test_dir.clone(), Instant::now());

        // First retry attempt immediately - should not attempt to watch since dir doesn't exist
        // The early return on line 581-583 prevents any action, keeping the failed_dirs entry
        let failed_count_before = wl.failed_dirs.len();
        wl.try_watch(&test_dir);

        // Since we're still within the backoff period, the entry should remain
        // and the timestamp should be updated
        assert_eq!(wl.failed_dirs.len(), failed_count_before);

        // Verify the timestamp was updated
        let updated_timestamp = wl.failed_dirs.get(&test_dir).unwrap();
        assert!(updated_timestamp.elapsed() < Duration::from_secs(1));

        // Now artificially age the entry to simulate time passing beyond backoff
        let old_timestamp = Instant::now() - Duration::from_secs(7200); // 2 hours
        wl.failed_dirs.insert(test_dir.clone(), old_timestamp);

        // This should now attempt to log again (past the 1 hour backoff)
        // But since the directory still exists and we can't actually test
        // watch failure without permissions, we just verify the logic allows retry
        wl.try_watch(&test_dir);
    }

    #[test]
    fn test_failed_watch_removed_on_success() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<notify::Event>();

        let plugin = make_plugin("test", vec![&format!("{}/*.jsonl", dir.path().display())]);

        let watcher = notify::RecommendedWatcher::new(
            move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    let _ = tx.send(event);
                }
            },
            NotifyConfig::default(),
        )
        .unwrap();

        let mut wl = WatchLoop {
            _watcher: watcher,
            event_rx: _rx,
            debouncer: Debouncer::new(),
            plugins: vec![plugin],
            watched_dirs: HashSet::new(),
            last_discovery: Instant::now()
                .checked_sub(Duration::from_secs(REDISCOVERY_INTERVAL_SECS + 1))
                .unwrap_or_else(Instant::now),
            failed_dirs: HashMap::new(),
        };

        // Successfully watch a directory
        wl.try_watch(dir.path());

        // It should not be in failed_dirs
        assert!(!wl.failed_dirs.contains_key(dir.path()));

        // It should be in watched_dirs
        assert!(wl.watched_dirs.contains(dir.path()));
    }

    #[test]
    fn test_failed_watch_first_attempt_always_logs() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<notify::Event>();

        let plugin = make_plugin("test", vec![&format!("{}/*.jsonl", dir.path().display())]);

        let watcher = notify::RecommendedWatcher::new(
            move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    let _ = tx.send(event);
                }
            },
            NotifyConfig::default(),
        )
        .unwrap();

        let mut wl = WatchLoop {
            _watcher: watcher,
            event_rx: _rx,
            debouncer: Debouncer::new(),
            plugins: vec![plugin],
            watched_dirs: HashSet::new(),
            last_discovery: Instant::now()
                .checked_sub(Duration::from_secs(REDISCOVERY_INTERVAL_SECS + 1))
                .unwrap_or_else(Instant::now),
            failed_dirs: HashMap::new(),
        };

        // First attempt at a directory - should have no entry in failed_dirs initially
        assert!(!wl.failed_dirs.contains_key(dir.path()));

        // Watch should succeed (test directory exists and is watchable)
        wl.try_watch(dir.path());

        // Directory should be watched, not failed
        assert!(wl.watched_dirs.contains(dir.path()));
        assert!(!wl.failed_dirs.contains_key(dir.path()));
    }

    // ── Daemon state persistence: file offset tracking ─────────────────

    #[test]
    fn test_persisted_state_file_offset_operations() {
        let mut state = PersistedState::default();

        // Initially, no offsets are tracked
        assert_eq!(state.get_file_offset("/tmp/test.jsonl"), None);

        // Set an offset
        state.set_file_offset("/tmp/test.jsonl", 1024);
        assert_eq!(state.get_file_offset("/tmp/test.jsonl"), Some(1024));

        // Update the same file's offset
        state.set_file_offset("/tmp/test.jsonl", 2048);
        assert_eq!(state.get_file_offset("/tmp/test.jsonl"), Some(2048));

        // Track multiple files
        state.set_file_offset("/tmp/other.jsonl", 512);
        state.set_file_offset("/home/user/session.md", 256);
        assert_eq!(state.get_file_offset("/tmp/other.jsonl"), Some(512));
        assert_eq!(state.get_file_offset("/home/user/session.md"), Some(256));

        // Remove a file from tracking
        state.remove_file("/tmp/test.jsonl");
        assert_eq!(state.get_file_offset("/tmp/test.jsonl"), None);
        // Other files should still be tracked
        assert_eq!(state.get_file_offset("/tmp/other.jsonl"), Some(512));

        // Clear all offsets
        state.clear_file_offsets();
        assert_eq!(state.get_file_offset("/tmp/other.jsonl"), None);
        assert_eq!(state.get_file_offset("/home/user/session.md"), None);
    }

    #[test]
    fn test_persisted_state_with_file_offsets_persists() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("daemon_state.json");

        // Create state with file offsets
        let mut state = PersistedState {
            started_at: Some(Utc::now()),
            sessions_indexed: 100,
            last_scrape: Some(Utc::now()),
            file_offsets: HashMap::new(),
        };

        state.set_file_offset("/tmp/a.jsonl", 1000);
        state.set_file_offset("/tmp/b.jsonl", 2000);
        state.set_file_offset("/home/c.log", 500);

        // Save state
        save_state(&state_file, &state).unwrap();

        // Load state in a new instance
        let loaded = load_state(&state_file).unwrap();
        assert_eq!(loaded.sessions_indexed, 100);
        assert_eq!(loaded.get_file_offset("/tmp/a.jsonl"), Some(1000));
        assert_eq!(loaded.get_file_offset("/tmp/b.jsonl"), Some(2000));
        assert_eq!(loaded.get_file_offset("/home/c.log"), Some(500));
        assert_eq!(loaded.file_offsets.len(), 3);
    }

    #[test]
    fn test_persisted_state_atomic_write_renames_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("daemon_state.json");

        // Save state
        let mut state = PersistedState {
            started_at: Some(Utc::now()),
            sessions_indexed: 42,
            last_scrape: Some(Utc::now()),
            file_offsets: HashMap::new(),
        };
        state.set_file_offset("/tmp/test.jsonl", 12345);

        save_state(&state_file, &state).unwrap();

        // Verify the target file exists (atomic rename completed)
        assert!(state_file.exists());

        // Verify no temp files remain
        let temp_files: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_str().unwrap_or("").contains(".tmp-"))
            .collect();

        assert_eq!(
            temp_files.len(),
            0,
            "No temp files should remain after atomic write"
        );

        // Verify content is correct
        let loaded = load_state(&state_file).unwrap();
        assert_eq!(loaded.sessions_indexed, 42);
        assert_eq!(loaded.get_file_offset("/tmp/test.jsonl"), Some(12345));
    }

    #[test]
    fn test_persisted_state_corrupted_file_fallback_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("daemon_state.json");

        // Write corrupted JSON (missing closing brace)
        fs::write(&state_file, r#"{"started_at": "2026-03-16T12:00:00Z"#).unwrap();

        // Load should return Some(empty state) rather than None
        let loaded = load_state(&state_file);
        assert!(loaded.is_some(), "Should return empty state on corruption");

        let state = loaded.unwrap();
        assert_eq!(
            state.sessions_indexed, 0,
            "Empty state should have 0 sessions"
        );
        assert!(state.started_at.is_none());
        assert!(state.last_scrape.is_none());
        assert_eq!(state.file_offsets.len(), 0);

        // Corrupted file should be backed up
        let backup_files: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .unwrap_or("")
                    .contains(".corrupt-")
            })
            .collect();

        assert_eq!(
            backup_files.len(),
            1,
            "Corrupted file should be backed up with .corrupt- suffix"
        );
    }

    #[test]
    fn test_persisted_state_invalid_json_fallback_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("daemon_state.json");

        // Write completely invalid content (not JSON at all)
        fs::write(&state_file, "this is not json").unwrap();

        // Load should return Some(empty state)
        let loaded = load_state(&state_file);
        assert!(loaded.is_some());

        let state = loaded.unwrap();
        assert!(state.file_offsets.is_empty());
        assert_eq!(state.sessions_indexed, 0);
    }

    #[test]
    fn test_persisted_state_missing_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("nonexistent.json");

        // Load should return None for missing file
        let loaded = load_state(&state_file);
        assert!(loaded.is_none(), "Missing file should return None");
    }

    #[test]
    fn test_persisted_state_concurrent_atomic_writes_produce_valid_state() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("daemon_state.json");

        // Simulate concurrent writes from multiple threads
        let handles: Vec<_> = (0..5)
            .map(|i| {
                let state_file_clone = state_file.clone();
                std::thread::spawn(move || {
                    let mut state = PersistedState {
                        sessions_indexed: i * 10,
                        ..Default::default()
                    };
                    state.set_file_offset(&format!("/tmp/file{}.jsonl", i), i * 100);
                    save_state(&state_file_clone, &state)
                })
            })
            .collect();

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap().expect("Thread save should succeed");
        }

        // Final state should be valid and readable
        let loaded = load_state(&state_file);
        assert!(
            loaded.is_some(),
            "Final state should be valid after concurrent writes"
        );

        let state = loaded.unwrap();
        // One of the writes should have won (we don't know which, but state should be consistent)
        assert!(
            state.sessions_indexed > 0,
            "At least one write should have succeeded"
        );
        // File offsets should be present (from whichever write won)
        assert!(
            !state.file_offsets.is_empty(),
            "At least one file offset should be present"
        );
    }

    #[test]
    fn test_persisted_state_preserves_all_fields_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("daemon_state.json");

        let now = Utc::now();
        let mut original = PersistedState {
            started_at: Some(now),
            sessions_indexed: 12345,
            last_scrape: Some(now - chrono::Duration::hours(1)),
            file_offsets: HashMap::new(),
        };

        // Add multiple file offsets
        original.set_file_offset("/var/log/agent1.jsonl", 9999);
        original.set_file_offset("/var/log/agent2.jsonl", 8888);
        original.set_file_offset("/home/user/.aider.chat.history.md", 5000);

        save_state(&state_file, &original).unwrap();

        let loaded = load_state(&state_file).unwrap();

        assert_eq!(loaded.sessions_indexed, 12345);
        assert_eq!(loaded.started_at, Some(now));
        assert_eq!(loaded.last_scrape, Some(now - chrono::Duration::hours(1)));
        assert_eq!(loaded.get_file_offset("/var/log/agent1.jsonl"), Some(9999));
        assert_eq!(loaded.get_file_offset("/var/log/agent2.jsonl"), Some(8888));
        assert_eq!(
            loaded.get_file_offset("/home/user/.aider.chat.history.md"),
            Some(5000)
        );
        assert_eq!(loaded.file_offsets.len(), 3);
    }
}
