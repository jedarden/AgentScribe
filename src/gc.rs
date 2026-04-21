//! Garbage collection for old sessions.
//!
//! Removes normalized session files, sidecar files, and Tantivy index entries
//! for sessions older than a configured threshold.

use crate::error::{AgentScribeError, Result};
use crate::index::IndexManager;
use crate::scraper::Scraper;
use chrono::{Duration, Utc};
use serde::Serialize;
use std::fs;
use std::path::Path;

/// Result of a garbage collection run.
#[derive(Debug, Serialize)]
pub struct GcResult {
    /// Number of sessions deleted
    pub sessions_deleted: usize,
    /// Total bytes reclaimed from file deletion
    pub bytes_reclaimed: u64,
    /// Sessions that would be deleted (dry-run)
    pub candidate_sessions: Vec<String>,
    /// Whether this was a dry run
    pub dry_run: bool,
}

/// Sidecar file suffixes to clean up alongside session files.
const SIDECAR_SUFFIXES: &[&str] = &[".anti-patterns.jsonl", ".summary.jsonl"];

/// Parse a human-readable duration string (e.g., "30d", "12w", "6mo").
///
/// Returns a `chrono::Duration`.
pub fn parse_duration(s: &str) -> Result<Duration> {
    let s = s.trim();
    let (num_str, unit) = s.split_at(
        s.find(|c: char| !c.is_ascii_digit())
            .ok_or_else(|| AgentScribeError::Config(format!("invalid duration: {}", s)))?,
    );
    let num: i64 = num_str
        .parse()
        .map_err(|_| AgentScribeError::Config(format!("invalid duration number: {}", num_str)))?;

    let duration = match unit {
        "d" | "days" => Duration::days(num),
        "w" | "weeks" => Duration::weeks(num),
        "mo" | "months" => {
            // Approximate: 30 days per month
            Duration::days(num * 30)
        }
        "h" | "hours" => Duration::hours(num),
        _ => {
            return Err(AgentScribeError::Config(format!(
                "unknown duration unit: {}",
                unit
            )))
        }
    };

    Ok(duration)
}

/// Collect sidecar file paths for a given session.
///
/// Session ID format: `<plugin>/<session_id>`
/// Files live at: `{sessions_dir}/{plugin}/{session_id}.jsonl`
/// Sidecars at:   `{sessions_dir}/{plugin}/{session_id}.anti-patterns.jsonl`
///                `{sessions_dir}/{plugin}/{session_id}.summary.jsonl`
fn session_file_paths(sessions_dir: &Path, session_id: &str) -> Vec<std::path::PathBuf> {
    let parts: Vec<&str> = session_id.splitn(2, '/').collect();
    if parts.len() != 2 {
        return Vec::new();
    }

    let plugin_dir = sessions_dir.join(parts[0]);
    let stem = parts[1];
    let mut paths = Vec::new();

    // Main session file
    paths.push(plugin_dir.join(format!("{}.jsonl", stem)));

    // Sidecar files
    for suffix in SIDECAR_SUFFIXES {
        paths.push(plugin_dir.join(format!("{}{}", stem, suffix)));
    }

    paths
}

/// Compute total size of files that exist at the given paths.
fn compute_file_size(paths: &[std::path::PathBuf]) -> u64 {
    let mut total: u64 = 0;
    for path in paths {
        if path.exists() {
            if let Ok(meta) = fs::metadata(path) {
                total += meta.len();
            }
        }
    }
    total
}

/// Delete files at the given paths. Returns the number of files actually deleted.
fn delete_files(paths: &[std::path::PathBuf]) -> usize {
    let mut deleted = 0;
    for path in paths {
        if path.exists() && fs::remove_file(path).is_ok() {
            deleted += 1;
        }
    }
    deleted
}

/// Run garbage collection on old sessions.
///
/// Deletes session files, sidecar files, and Tantivy index entries for sessions
/// older than `max_age`. If `dry_run` is true, only reports what would be deleted.
///
/// `max_age` is the duration threshold — sessions whose first event timestamp is
/// older than `now - max_age` will be collected.
pub fn run_gc(data_dir: &Path, max_age: Duration, dry_run: bool) -> Result<GcResult> {
    let mut scraper = Scraper::new(data_dir.to_path_buf())?;
    scraper.load_plugins()?;

    let cutoff = Utc::now() - max_age;
    let all_sessions = scraper.all_sessions()?;

    let sessions_dir = data_dir.join("sessions");
    let mut candidate_sessions = Vec::new();
    let mut bytes_reclaimed: u64 = 0;

    // First pass: identify sessions older than the cutoff
    for session_id in &all_sessions {
        // Read the first event to get the session timestamp
        let events = match scraper.read_session(session_id) {
            Ok(events) => events,
            Err(_) => {
                // If we can't read the session, skip it
                continue;
            }
        };

        if events.is_empty() {
            continue;
        }

        let session_ts = events[0].ts;
        if session_ts < cutoff {
            let paths = session_file_paths(&sessions_dir, session_id);
            let size = compute_file_size(&paths);
            bytes_reclaimed += size;
            candidate_sessions.push(session_id.clone());
        }
    }

    if dry_run {
        return Ok(GcResult {
            sessions_deleted: 0,
            bytes_reclaimed,
            candidate_sessions,
            dry_run: true,
        });
    }

    // Second pass: delete files and index entries
    let mut index_manager = IndexManager::open(data_dir)?;
    index_manager.begin_write()?;

    let mut sessions_deleted = 0;
    for session_id in &candidate_sessions {
        // Delete files
        let paths = session_file_paths(&sessions_dir, session_id);
        delete_files(&paths);

        // Delete from Tantivy index
        if let Err(e) = index_manager.delete_session(session_id) {
            eprintln!(
                "Warning: failed to delete session {} from index: {}",
                session_id, e
            );
        }

        sessions_deleted += 1;
    }

    // Commit index changes
    index_manager.finish()?;

    // Optimize: remove unused segment files from the index
    if let Err(e) = index_manager.optimize() {
        eprintln!("Warning: index optimization failed: {}", e);
    }

    // Clean up empty plugin directories
    if let Ok(entries) = fs::read_dir(&sessions_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Check if directory is empty (only contains leftover files we may have missed)
                if let Ok(contents) = fs::read_dir(&path) {
                    if contents.count() == 0 {
                        let _ = fs::remove_dir(&path);
                    }
                }
            }
        }
    }

    Ok(GcResult {
        sessions_deleted,
        bytes_reclaimed,
        candidate_sessions,
        dry_run: false,
    })
}

/// Format a GcResult for human-readable output.
pub fn format_human(result: &GcResult) -> String {
    let mut lines = Vec::new();

    if result.dry_run {
        lines.push("Dry run — no files were deleted.".to_string());
        lines.push(format!(
            "Would delete {} session(s) ({})",
            result.candidate_sessions.len(),
            format_bytes(result.bytes_reclaimed)
        ));
    } else {
        lines.push(format!(
            "Deleted {} session(s), reclaimed {}",
            result.sessions_deleted,
            format_bytes(result.bytes_reclaimed)
        ));
    }

    for session_id in &result.candidate_sessions {
        lines.push(format!("  - {}", session_id));
    }

    lines.join("\n")
}

fn format_bytes(bytes: u64) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_parse_duration_days() {
        assert_eq!(parse_duration("30d").unwrap(), Duration::days(30));
    }

    #[test]
    fn test_parse_duration_weeks() {
        assert_eq!(parse_duration("12w").unwrap(), Duration::weeks(12));
    }

    #[test]
    fn test_parse_duration_months() {
        assert_eq!(parse_duration("6mo").unwrap(), Duration::days(180));
    }

    #[test]
    fn test_parse_duration_hours() {
        assert_eq!(parse_duration("48h").unwrap(), Duration::hours(48));
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("10x").is_err());
    }

    #[test]
    fn test_session_file_paths() {
        let sessions_dir = Path::new("/tmp/sessions");
        let paths = session_file_paths(sessions_dir, "claude-code/20250322-123456");

        assert_eq!(paths.len(), 3);
        assert!(paths[0]
            .to_str()
            .unwrap()
            .ends_with("20250322-123456.jsonl"));
        assert!(paths[1]
            .to_str()
            .unwrap()
            .ends_with("20250322-123456.anti-patterns.jsonl"));
        assert!(paths[2]
            .to_str()
            .unwrap()
            .ends_with("20250322-123456.summary.jsonl"));
    }

    #[test]
    fn test_gc_dry_run_no_sessions() {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path();

        // Initialize basic structure
        fs::create_dir_all(data_dir.join("sessions")).unwrap();
        fs::create_dir_all(data_dir.join("plugins")).unwrap();
        fs::create_dir_all(data_dir.join("index")).unwrap();
        fs::create_dir_all(data_dir.join("state")).unwrap();

        let result = run_gc(data_dir, Duration::days(30), true).unwrap();
        assert!(result.dry_run);
        assert_eq!(result.sessions_deleted, 0);
        assert_eq!(result.candidate_sessions.len(), 0);
    }
}
