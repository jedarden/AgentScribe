//! Doctor command for self-diagnosis of AgentScribe failure modes
//!
//! Performs health checks on the daemon, state files, index, and configuration
//! to catch problems that would otherwise require manual filesystem/log inspection.
//!
//! Based on the audit findings from 2026-07-20:
//! - Daemon process dead with stale PID file
//! - scrape-state.json corrupted
//! - IndexManager::open failing silently with sessions_indexed count present but no index directory
//! - mcp_enabled=false despite being the flagship feature

use crate::config::Config;
use crate::error::{AgentScribeError, Result};
use crate::event::ScrapeState;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Doctor check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    /// Check name (e.g., "daemon_alive", "state_file_parses")
    pub name: String,
    /// Pass/fail status
    pub passed: bool,
    /// Human-readable message (what was checked and what was found)
    pub message: String,
    /// Additional context (optional, for debugging)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// Severity: "info", "warning", "critical"
    pub severity: String,
}

/// Overall doctor output
#[derive(Debug, Serialize, Deserialize)]
pub struct DoctorOutput {
    /// Version of AgentScribe
    pub version: String,
    /// Data directory path
    pub data_dir: String,
    /// Overall health status: "healthy", "warning", "critical"
    pub health_status: String,
    /// Individual check results
    pub checks: Vec<CheckResult>,
    /// Timestamp when checks were run
    pub checked_at: DateTime<Utc>,
}

impl DoctorOutput {
    /// Create a new doctor output
    pub fn new(data_dir: PathBuf) -> Self {
        DoctorOutput {
            version: env!("CARGO_PKG_VERSION").to_string(),
            data_dir: data_dir.display().to_string(),
            health_status: "healthy".to_string(),
            checks: Vec::new(),
            checked_at: Utc::now(),
        }
    }

    /// Add a check result
    pub fn add_check(&mut self, check: CheckResult) {
        // Update overall health status based on check severity
        match check.severity.as_str() {
            "critical" if !check.passed => {
                if self.health_status != "critical" {
                    self.health_status = "critical".to_string();
                }
            }
            "warning" if !check.passed => {
                if self.health_status == "healthy" {
                    self.health_status = "warning".to_string();
                }
            }
            _ => {}
        }
        self.checks.push(check);
    }

    /// Count failed checks by severity
    pub fn failed_count(&self) -> usize {
        self.checks.iter().filter(|c| !c.passed).count()
    }
}

/// Run all doctor checks
pub fn run_doctor(data_dir: &Path, config: &Config, json: bool) -> Result<DoctorOutput> {
    let mut output = DoctorOutput::new(data_dir.to_path_buf());

    // Check 1: Daemon process alive
    output.add_check(check_daemon_alive(data_dir));

    // Check 2: last_scrape recent relative to debounce/rediscovery config
    output.add_check(check_last_scrape_recent(
        data_dir,
        config.scrape.debounce_seconds,
    ));

    // Check 3: scrape-state.json parses cleanly
    output.add_check(check_state_file_parses(data_dir));

    // Check 4: index/tantivy/ exists and doc count consistent with sessions/ file count
    output.add_check(check_index_consistent(data_dir));

    // Check 5: mcp_enabled set and socket path reachable (if enabled)
    output.add_check(check_mcp_config(data_dir, &config.daemon));

    if json {
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        print_human_readable(&output);
    }

    Ok(output)
}

/// Print human-readable output
fn print_human_readable(output: &DoctorOutput) {
    println!("AgentScribe v{} - Doctor Diagnostic Report", output.version);
    println!("Data directory: {}", output.data_dir);
    println!("Checked at: {}", output.checked_at.to_rfc3339());
    println!();

    // Overall status
    let status_symbol = match output.health_status.as_str() {
        "healthy" => "✅",
        "warning" => "⚠️ ",
        "critical" => "❌",
        _ => "?",
    };
    println!("Overall Status: {} {}", status_symbol, output.health_status);
    println!();

    // Individual checks
    for check in &output.checks {
        let status_symbol = if check.passed { "✅" } else { "❌" };
        println!("{} {} ({})", status_symbol, check.name, check.severity);
        println!("   {}", check.message);
        if let Some(ref ctx) = check.context {
            println!("   Context: {}", ctx);
        }
        println!();
    }

    // Summary
    let total = output.checks.len();
    let passed = output.checks.iter().filter(|c| c.passed).count();
    let failed = total - passed;

    println!("Summary: {}/{} checks passed", passed, total);
    if failed > 0 {
        println!("         {} checks failed", failed);
    }
}

/// Check 1: Daemon process alive (PID file vs actual process)
fn check_daemon_alive(data_dir: &Path) -> CheckResult {
    let pid_file = data_dir.join("agentscribe.pid");

    if !pid_file.exists() {
        return CheckResult {
            name: "daemon_alive".to_string(),
            passed: true,
            message: "Daemon is not running (no PID file)".to_string(),
            context: None,
            severity: "info".to_string(),
        };
    }

    // Read PID file
    let pid_str = match fs::read_to_string(&pid_file) {
        Ok(s) => s,
        Err(e) => {
            return CheckResult {
                name: "daemon_alive".to_string(),
                passed: false,
                message: format!("Cannot read PID file: {}", e),
                context: Some(format!("Path: {}", pid_file.display())),
                severity: "warning".to_string(),
            };
        }
    };

    let pid = match pid_str.trim().parse::<u32>() {
        Ok(p) => p,
        Err(_) => {
            return CheckResult {
                name: "daemon_alive".to_string(),
                passed: false,
                message: "PID file contains invalid data".to_string(),
                context: Some(format!("Content: {}", pid_str.trim())),
                severity: "warning".to_string(),
            };
        }
    };

    // Check if process is actually running
    unsafe {
        if libc::kill(pid as i32, 0) == 0 {
            CheckResult {
                name: "daemon_alive".to_string(),
                passed: true,
                message: format!("Daemon is running (PID {})", pid),
                context: None,
                severity: "info".to_string(),
            }
        } else {
            // Process doesn't exist - stale PID file
            CheckResult {
                name: "daemon_alive".to_string(),
                passed: false,
                message: format!("Daemon is dead (stale PID {} in PID file)", pid),
                context: Some("Run 'agentscribe daemon start' to restart".to_string()),
                severity: "critical".to_string(),
            }
        }
    }
}

/// Check 2: last_scrape recent relative to debounce/rediscovery config
fn check_last_scrape_recent(data_dir: &Path, debounce_seconds: u64) -> CheckResult {
    let state_file = data_dir.join("daemon_state.json");

    if !state_file.exists() {
        return CheckResult {
            name: "last_scrape_recent".to_string(),
            passed: true,
            message: "No daemon state file (daemon may never have started)".to_string(),
            context: None,
            severity: "info".to_string(),
        };
    }

    // Read daemon state
    let content = match fs::read_to_string(&state_file) {
        Ok(s) => s,
        Err(e) => {
            return CheckResult {
                name: "last_scrape_recent".to_string(),
                passed: false,
                message: format!("Cannot read daemon state: {}", e),
                context: Some(format!("Path: {}", state_file.display())),
                severity: "warning".to_string(),
            };
        }
    };

    // Parse as JSON to get last_scrape
    let last_scrape: Option<DateTime<Utc>> = serde_json::from_str(&content)
        .ok()
        .and_then(|v: serde_json::Value| {
            v.get("last_scrape")
                .and_then(|ts| ts.as_str())
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc))
        });

    let last_scrape = match last_scrape {
        Some(ts) => ts,
        None => {
            return CheckResult {
                name: "last_scrape_recent".to_string(),
                passed: true,
                message: "No scrape recorded yet".to_string(),
                context: None,
                severity: "info".to_string(),
            };
        }
    };

    let now = Utc::now();
    let elapsed = now.signed_duration_since(last_scrape);

    // Consider stale if > 1 hour (60 minutes) or > 10x debounce_seconds, whichever is greater
    let stale_threshold_secs = (60 * 60).max(debounce_seconds as i64 * 10);

    if elapsed.num_seconds() > stale_threshold_secs {
        CheckResult {
            name: "last_scrape_recent".to_string(),
            passed: false,
            message: format!(
                "Last scrape was {} ago (stale)",
                format_duration(elapsed)
            ),
            context: Some(format!(
                "Threshold: {} seconds | Consider restarting daemon",
                stale_threshold_secs
            )),
            severity: "critical".to_string(),
        }
    } else {
        CheckResult {
            name: "last_scrape_recent".to_string(),
            passed: true,
            message: format!("Last scrape was {} ago", format_duration(elapsed)),
            context: None,
            severity: "info".to_string(),
        }
    }
}

/// Check 3: scrape-state.json parses cleanly
fn check_state_file_parses(data_dir: &Path) -> CheckResult {
    let state_file = data_dir.join("state").join("scrape-state.json");

    if !state_file.exists() {
        return CheckResult {
            name: "state_file_parses".to_string(),
            passed: true,
            message: "No state file yet (will be created on first scrape)".to_string(),
            context: None,
            severity: "info".to_string(),
        };
    }

    // Try to parse the state file
    match parse_scrape_state(&state_file) {
        Ok(state) => {
            let source_count = state.sources.len();
            CheckResult {
                name: "state_file_parses".to_string(),
                passed: true,
                message: format!("State file parses cleanly ({} sources tracked)", source_count),
                context: None,
                severity: "info".to_string(),
            }
        }
        Err(e) => {
            // Check if quarantined file exists (ADR-1 recovery)
            let quarantine_dir = state_file.parent().unwrap();
            let has_quarantine = quarantine_dir
                .read_dir()
                .map(|entries| {
                    entries
                        .filter_map(|e| e.ok())
                        .any(|e| e.file_name().to_string_lossy().starts_with("scrape-state.json.corrupt-"))
                })
                .unwrap_or(false);

            let message = if has_quarantine {
                format!("State file corrupted and quarantined (started from empty state)")
            } else {
                format!("State file corrupted: {}", e)
            };

            CheckResult {
                name: "state_file_parses".to_string(),
                passed: false,
                message,
                context: Some(format!("Path: {}", state_file.display())),
                severity: "critical".to_string(),
            }
        }
    }
}

/// Parse scrape state from file
fn parse_scrape_state(path: &Path) -> Result<ScrapeState> {
    let file = fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    serde_json::from_reader(reader).map_err(|e| AgentScribeError::State(format!("Parse error: {}", e)))
}

/// Check 4: index/tantivy/ exists and doc count consistent with sessions/ file count
fn check_index_consistent(data_dir: &Path) -> CheckResult {
    let index_dir = data_dir.join("index").join("tantivy");
    let sessions_dir = data_dir.join("sessions");

    // Count session files
    let session_count = if sessions_dir.exists() {
        count_session_files(&sessions_dir)
    } else {
        0
    };

    if !index_dir.exists() {
        if session_count > 0 {
            return CheckResult {
                name: "index_consistent".to_string(),
                passed: false,
                message: format!(
                    "Index missing ({} session files exist but no index)",
                    session_count
                ),
                context: Some("Run 'agentscribe index rebuild' to build index".to_string()),
                severity: "critical".to_string(),
            };
        }
        return CheckResult {
            name: "index_consistent".to_string(),
            passed: true,
            message: "No index yet (no sessions to index)".to_string(),
            context: None,
            severity: "info".to_string(),
        };
    }

    // Try to open index and get doc count
    let index_doc_count = match tantivy::Index::open_in_dir(&index_dir) {
        Ok(index) => match index.reader() {
            Ok(reader) => reader.searcher().num_docs() as usize,
            Err(_) => {
                return CheckResult {
                    name: "index_consistent".to_string(),
                    passed: false,
                    message: "Index exists but cannot create reader".to_string(),
                    context: Some(format!("Path: {}", index_dir.display())),
                    severity: "critical".to_string(),
                };
            }
        },
        Err(e) => {
            return CheckResult {
                name: "index_consistent".to_string(),
                passed: false,
                message: format!("Index exists but cannot open: {}", e),
                context: Some(format!("Path: {}", index_dir.display())),
                severity: "critical".to_string(),
            };
        }
    };

    // Check if counts are roughly consistent (allow 10% tolerance for anti-patterns, code artifacts)
    let tolerance = (session_count as f64 * 0.1) as usize;
    let diff = if session_count > index_doc_count {
        session_count - index_doc_count
    } else {
        index_doc_count - session_count
    };

    if diff > tolerance && session_count > 0 {
        CheckResult {
            name: "index_consistent".to_string(),
            passed: false,
            message: format!(
                "Index document count ({}) doesn't match session file count ({})",
                index_doc_count, session_count
            ),
            context: Some(format!(
                "Difference: {} documents | Run 'agentscribe index rebuild'",
                diff
            )),
            severity: "warning".to_string(),
        }
    } else {
        CheckResult {
            name: "index_consistent".to_string(),
            passed: true,
            message: format!(
                "Index consistent ({} documents, {} session files)",
                index_doc_count, session_count
            ),
            context: None,
            severity: "info".to_string(),
        }
    }
}

/// Count session JSONL files recursively
fn count_session_files(sessions_dir: &Path) -> usize {
    fs::read_dir(sessions_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .map(|agent_dir| {
                    agent_dir
                        .path()
                        .read_dir()
                        .map(|entries| {
                            entries
                                .filter_map(|e| e.ok())
                                .filter(|e| {
                                    e.path()
                                        .extension()
                                        .and_then(|ext| ext.to_str())
                                        == Some("jsonl")
                                })
                                .count()
                        })
                        .unwrap_or(0)
                })
                .sum()
        })
        .unwrap_or(0)
}

/// Check 5: mcp_enabled set and socket path reachable (if enabled)
fn check_mcp_config(data_dir: &Path, daemon_config: &crate::config::DaemonConfig) -> CheckResult {
    if !daemon_config.mcp_enabled {
        return CheckResult {
            name: "mcp_config".to_string(),
            passed: false,
            message: "MCP server is disabled".to_string(),
            context: Some("MCP is the flagship 'agents query their own history' feature. Enable in config.toml with [daemon] mcp_enabled = true".to_string()),
            severity: "warning".to_string(),
        };
    }

    // MCP is enabled - check if socket path exists and is reachable
    let socket_path = daemon_config
        .mcp_socket_path
        .as_ref()
        .map(|p| PathBuf::from(p))
        .unwrap_or_else(|| data_dir.join("mcp.sock"));

    if socket_path.exists() {
        // Try to connect to the socket
        match std::os::unix::net::UnixStream::connect(&socket_path) {
            Ok(_) => CheckResult {
                name: "mcp_config".to_string(),
                passed: true,
                message: "MCP server enabled and socket reachable".to_string(),
                context: Some(format!("Socket: {}", socket_path.display())),
                severity: "info".to_string(),
            },
            Err(e) => CheckResult {
                name: "mcp_config".to_string(),
                passed: false,
                message: "MCP enabled but socket not reachable".to_string(),
                context: Some(format!(
                    "Socket: {} | Error: {} | Daemon may not be running",
                    socket_path.display(), e
                )),
                severity: "warning".to_string(),
            },
        }
    } else {
        CheckResult {
            name: "mcp_config".to_string(),
            passed: false,
            message: "MCP enabled but socket file doesn't exist".to_string(),
            context: Some(format!(
                "Socket: {} | Daemon may not be running or MCP not started",
                socket_path.display()
            )),
            severity: "warning".to_string(),
        }
    }
}

/// Format a Duration as a human-readable string
fn format_duration(duration: Duration) -> String {
    let secs = duration.num_seconds();
    if secs >= 86400 {
        format!("{}d", secs / 86400)
    } else if secs >= 3600 {
        format!("{}h", secs / 3600)
    } else if secs >= 60 {
        format!("{}m", secs / 60)
    } else {
        format!("{}s", secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_check_daemon_alive_no_pid_file() {
        let temp_dir = TempDir::new().unwrap();
        let result = check_daemon_alive(temp_dir.path());
        assert!(result.passed);
        assert_eq!(result.name, "daemon_alive");
    }

    #[test]
    fn test_check_state_file_parses_no_file() {
        let temp_dir = TempDir::new().unwrap();
        let result = check_state_file_parses(temp_dir.path());
        assert!(result.passed);
        assert_eq!(result.name, "state_file_parses");
    }

    #[test]
    fn test_check_index_consistent_no_index() {
        let temp_dir = TempDir::new().unwrap();
        let result = check_index_consistent(temp_dir.path());
        assert!(result.passed);
        assert_eq!(result.name, "index_consistent");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::seconds(30)), "30s");
        assert_eq!(format_duration(Duration::seconds(90)), "1m");
        assert_eq!(format_duration(Duration::seconds(7200)), "2h");
        assert_eq!(format_duration(Duration::seconds(172800)), "2d");
    }
}
