//! Daemon lifecycle management
//!
//! Provides start/stop/status/run/logs commands for the AgentScribe daemon.
//! The daemon runs a Tokio event loop, writes a PID file for lifecycle tracking,
//! and logs to a file at ~/.agentscribe/daemon.log.

use crate::error::{AgentScribeError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Default poll interval for the daemon idle loop (seconds)
const POLL_INTERVAL_SECS: u64 = 30;

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
}

/// Daemon state persisted to disk
#[derive(Debug, Serialize, Deserialize, Default)]
struct PersistedState {
    started_at: Option<DateTime<Utc>>,
    sessions_indexed: u64,
    last_scrape: Option<DateTime<Utc>>,
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
    let _exe = std::env::current_exe()
        .map_err(|e| AgentScribeError::Config(format!("Cannot determine executable path: {}", e)))?;

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
    run_event_loop(&log_file, &pid_file, &state_file);

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

    run_event_loop(&log_file, &pid_file, &state_file);

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
    };

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
                    if hz > 0 {
                        let uptime_ticks = read_uptime_ticks();
                        if uptime_ticks > start_ticks {
                            let elapsed_secs = (uptime_ticks - start_ticks) / hz;
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

    // Load persisted state
    if let Some(state) = load_state(&state_file) {
        info.sessions_indexed = Some(state.sessions_indexed);
        info.last_scrape = state.last_scrape;
        info.started_at = state.started_at;
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
fn run_event_loop(log_file: &Path, pid_file: &Path, state_file: &Path) {
    // Use current_thread runtime — safe after fork (no thread pool)
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create Tokio runtime");

    let log_path = log_file.to_path_buf();
    let pid_path = pid_file.to_path_buf();
    let state_path = state_file.to_path_buf();

    let _guard = rt.enter();

    // Initialize file logging
    init_file_logging(&log_path);

    let started_at = Utc::now();

    // Persist initial state
    let mut state = load_state(&state_path).unwrap_or_default();
    state.started_at = Some(started_at);
    let _ = save_state(&state_path, &state);

    tracing::info!("AgentScribe daemon started (PID {})", std::process::id());
    tracing::info!("Poll interval: {}s", POLL_INTERVAL_SECS);

    let start = Instant::now();

    // Register signal handlers for clean shutdown
    let pid_path_clone = pid_path.clone();
    ctrlc_handler(move || {
        tracing::info!("Received shutdown signal, cleaning up...");
        cleanup_pid(&pid_path_clone);
        std::process::exit(0);
    });

    // Block on the async event loop
    rt.block_on(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(POLL_INTERVAL_SECS));
        loop {
            interval.tick().await;
            if shutdown_requested() {
                tracing::info!("Shutdown requested, exiting event loop");
                break;
            }
            let _elapsed = start.elapsed();
            tracing::debug!("daemon heartbeat (elapsed: {:?})", _elapsed);
        }
    });
}

/// Set up a file-based tracing subscriber that writes to the given path.
fn init_file_logging(log_path: &Path) {
    use tracing_subscriber::fmt::writer::BoxMakeWriter;
    use tracing_subscriber::EnvFilter;

    let log_dir = log_path.parent().unwrap_or(Path::new("."));
    let _ = fs::create_dir_all(log_dir);

    let file_appender = tracing_appender::rolling::never(
        log_dir,
        log_path.file_name().unwrap_or_default().to_str().unwrap_or("daemon.log"),
    );

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let subscriber = tracing_subscriber::fmt()
        .with_writer(BoxMakeWriter::new(file_appender))
        .with_env_filter(filter)
        .with_ansi(false)
        .finish();

    let _ = tracing::subscriber::set_global_default(subscriber);
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
fn load_state(path: &Path) -> Option<PersistedState> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Save persisted daemon state to disk.
fn save_state(path: &Path, state: &PersistedState) -> std::io::Result<()> {
    let json = serde_json::to_string(state)?;
    fs::write(path, json)
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
    let exe = std::env::current_exe()
        .map_err(|e| AgentScribeError::Config(format!("Cannot determine executable path: {}", e)))?;
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
