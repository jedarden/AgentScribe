//! Anti-pattern detection.
//!
//! Analyzes failed/abandoned sessions for rejection windows and
//! links to successful alternatives via error fingerprint matching.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::event::{Event, Role, SessionManifest};
use crate::scraper::Scraper;

/// A detected anti-pattern with its rejection window and alternatives.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiPattern {
    /// Description of the anti-pattern
    pub pattern: String,
    /// Error fingerprints associated with this anti-pattern
    pub error_fingerprints: Vec<String>,
    /// Session IDs that exhibited this anti-pattern
    pub session_ids: Vec<String>,
    /// Session IDs that successfully resolved the same error
    pub alternative_session_ids: Vec<String>,
}

/// Detect anti-patterns in a session.
///
/// For failed/abandoned sessions, identifies rejection windows (repeated
/// failed attempts) and links to successful alternatives via error fingerprint matching.
pub fn detect_antipatterns(
    events: &[Event],
    manifest: &SessionManifest,
    outcome: Option<&str>,
    scraper: &Scraper,
) -> Vec<AntiPattern> {
    // Only detect anti-patterns for failed/abandoned sessions
    match outcome {
        Some(o) if o == "failure" || o == "abandoned" => {}
        _ => return Vec::new(),
    }

    let mut patterns = Vec::new();

    // Detect rejection windows (repeated attempts without success)
    if let Some(window) = find_rejection_window(events) {
        // Collect error fingerprints from the rejection window
        let mut fps = Vec::new();
        for event in &events[window.start..window.end] {
            for fp in &event.error_fingerprints {
                if !fps.contains(fp) {
                    fps.push(fp.clone());
                }
            }
        }

        // Find successful alternatives by matching error fingerprints
        let alternatives = find_alternatives(&fps, manifest.session_id.as_str(), scraper);

        patterns.push(AntiPattern {
            pattern: format!(
                "Rejection window: {} attempts over {} events without resolution",
                window.attempts, window.end - window.start
            ),
            error_fingerprints: fps,
            session_ids: vec![manifest.session_id.clone()],
            alternative_session_ids: alternatives,
        });
    }

    // Detect error escalation (errors get worse over time)
    if let Some(escalation) = detect_escalation(events) {
        let mut fps = Vec::new();
        for event in &events[escalation.start..=escalation.end] {
            for fp in &event.error_fingerprints {
                if !fps.contains(fp) {
                    fps.push(fp.clone());
                }
            }
        }

        let alternatives = find_alternatives(&fps, manifest.session_id.as_str(), scraper);

        patterns.push(AntiPattern {
            pattern: format!(
                "Error escalation: {} distinct error types in {} events",
                escalation.error_types, escalation.end - escalation.start + 1
            ),
            error_fingerprints: fps,
            session_ids: vec![manifest.session_id.clone()],
            alternative_session_ids: alternatives,
        });
    }

    patterns
}

/// A rejection window: a sequence of events where the agent repeatedly
/// tried and failed to resolve an issue.
struct RejectionWindow {
    start: usize,
    end: usize,
    attempts: usize,
}

/// Find a rejection window in the events.
///
/// A rejection window is detected when there are multiple consecutive
/// error-producing tool calls without user confirmation of success.
fn find_rejection_window(events: &[Event]) -> Option<RejectionWindow> {
    let mut attempts = 0;
    let mut start = None;
    let mut end = 0;
    let mut consecutive_errors = 0;

    for (i, event) in events.iter().enumerate() {
        match event.role {
            Role::ToolResult => {
                if has_error_signal(&event.content) {
                    consecutive_errors += 1;
                    if start.is_none() {
                        start = Some(i.saturating_sub(5).max(0));
                    }
                    end = i;
                } else {
                    consecutive_errors = 0;
                }
            }
            Role::ToolCall => {
                if consecutive_errors > 0 {
                    attempts += 1;
                }
            }
            Role::User => {
                // User saying something positive resets the window
                if has_positive_signal(&event.content) {
                    if attempts >= 2 {
                        // End of a successful recovery - not an anti-pattern
                        return None;
                    }
                    start = None;
                    attempts = 0;
                    consecutive_errors = 0;
                }
            }
            _ => {}
        }
    }

    // Need at least 2 failed attempts to be an anti-pattern
    if attempts >= 2 && start.is_some() {
        Some(RejectionWindow {
            start: start.unwrap(),
            end: end + 1,
            attempts,
        })
    } else {
        None
    }
}

/// Error escalation data.
struct ErrorEscalation {
    start: usize,
    end: usize,
    error_types: usize,
}

/// Detect error escalation: increasing error diversity over time.
fn detect_escalation(events: &[Event]) -> Option<ErrorEscalation> {
    let mut error_fps: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut start = None;
    let mut end = 0;
    let mut max_types = 0;

    for (i, event) in events.iter().enumerate() {
        if event.role == Role::ToolResult && !event.error_fingerprints.is_empty() {
            if start.is_none() {
                start = Some(i);
            }
            for fp in &event.error_fingerprints {
                error_fps.insert(fp.clone());
            }
            end = i;
            max_types = error_fps.len();
        }
    }

    // Escalation if we see 3+ distinct error types
    if max_types >= 3 && start.is_some() {
        Some(ErrorEscalation {
            start: start.unwrap(),
            end,
            error_types: max_types,
        })
    } else {
        None
    }
}

/// Check if content has an error signal.
fn has_error_signal(content: &str) -> bool {
    let lower = content.to_lowercase();
    lower.contains("error")
        || lower.contains("failed")
        || lower.contains("exit code 1")
        || lower.contains("panic")
        || lower.contains("fatal")
        || lower.contains("exception")
        || lower.contains("traceback")
}

/// Check if content has a positive/success signal from user.
fn has_positive_signal(content: &str) -> bool {
    let lower = content.to_lowercase();
    lower.contains("thanks")
        || lower.contains("works")
        || lower.contains("great")
        || lower.contains("perfect")
        || lower.contains("done")
        || lower.contains("looks good")
}

/// Find successful sessions that resolved the same error fingerprints.
fn find_alternatives(
    error_fps: &[String],
    current_session: &str,
    scraper: &Scraper,
) -> Vec<String> {
    if error_fps.is_empty() {
        return Vec::new();
    }

    let mut alternatives = Vec::new();

    // List all sessions and check for matching fingerprints
    let plugins = scraper.plugin_manager().names();
    for plugin in &plugins {
        if let Ok(sessions) = scraper.list_sessions(plugin) {
            for session_id in sessions {
                if session_id == current_session {
                    continue;
                }
                if let Ok(events) = scraper.read_session(&session_id) {
                    // Check if this session has matching fingerprints AND a positive outcome
                    let has_match = events.iter().any(|e| {
                        e.error_fingerprints
                            .iter()
                            .any(|fp| error_fps.contains(fp))
                    });

                    // Check for success signal in the last few events
                    let has_success = events
                        .iter()
                        .rev()
                        .take(5)
                        .any(|e| e.role == Role::User && has_positive_signal(&e.content));

                    if has_match && has_success {
                        alternatives.push(session_id);
                    }
                }
            }
        }
    }

    // Limit to 5 alternatives
    alternatives.truncate(5);
    alternatives
}

/// Write anti-patterns to a sidecar file.
pub fn write_antipatterns_sidecar(
    data_dir: &Path,
    session_id: &str,
    patterns: &[AntiPattern],
) -> std::io::Result<()> {
    if patterns.is_empty() {
        return Ok(());
    }

    let session_dir = data_dir.join("sessions");
    // Extract plugin prefix from session_id
    let parts: Vec<&str> = session_id.splitn(2, '/').collect();
    if parts.len() != 2 {
        return Ok(());
    }

    let plugin_dir = session_dir.join(parts[0]);
    std::fs::create_dir_all(&plugin_dir)?;

    let sidecar_path = plugin_dir.join(format!("{}.anti-patterns.jsonl", parts[1]));
    let mut file = std::fs::File::create(&sidecar_path)?;

    for pattern in patterns {
        let line = serde_json::to_string(pattern)?;
        writeln!(file, "{}", line)?;
    }

    Ok(())
}

use std::io::Write;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use crate::config::Config;
    use chrono::Utc;

    fn make_event(role: Role, content: &str, tool: Option<&str>, fps: Vec<&str>) -> Event {
        let mut e = Event::new(Utc::now(), "test/1".into(), "test".into(), role, content.into());
        e.tool = tool.map(|s| s.to_string());
        e.error_fingerprints = fps.iter().map(|s| s.to_string()).collect();
        e
    }

    fn make_manifest() -> SessionManifest {
        let mut m = SessionManifest::new("test/1".into(), "test".into());
        m.turns = 5;
        m
    }

    #[test]
    fn test_no_antipatterns_for_success() {
        let events = vec![
            make_event(Role::User, "fix bug", None, vec![]),
            make_event(Role::Assistant, "fixed", None, vec![]),
            make_event(Role::User, "thanks", None, vec![]),
        ];
        let manifest = make_manifest();
        // Create a minimal scraper for testing
        let temp = tempfile::tempdir().unwrap();
        let scraper = Scraper::new(temp.path().to_path_buf()).unwrap();
        let patterns = detect_antipatterns(&events, &manifest, Some("success"), &scraper);
        assert!(patterns.is_empty());
    }

    #[test]
    fn test_rejection_window_detected() {
        let events = vec![
            make_event(Role::User, "fix the build", None, vec![]),
            make_event(Role::Assistant, "trying fix 1", None, vec![]),
            make_event(Role::ToolCall, "cargo build", Some("Bash"), vec![]),
            make_event(Role::ToolResult, "error: missing import", None, vec!["missing import"]),
            make_event(Role::ToolCall, "cargo build", Some("Bash"), vec![]),
            make_event(Role::ToolResult, "error: cannot find module", None, vec!["cannot find module"]),
            make_event(Role::ToolCall, "cargo build", Some("Bash"), vec![]),
            make_event(Role::ToolResult, "error: type mismatch", None, vec!["type mismatch"]),
            make_event(Role::Assistant, "I couldn't fix this", None, vec![]),
        ];
        let manifest = make_manifest();
        let temp = tempfile::tempdir().unwrap();
        let scraper = Scraper::new(temp.path().to_path_buf()).unwrap();
        let patterns = detect_antipatterns(&events, &manifest, Some("failure"), &scraper);
        assert!(!patterns.is_empty());
        assert!(patterns[0].pattern.contains("Rejection window"));
    }

    #[test]
    fn test_escalation_detected() {
        let events = vec![
            make_event(Role::ToolResult, "error: connection refused", None, vec!["connection refused"]),
            make_event(Role::ToolResult, "error: timeout", None, vec!["timeout"]),
            make_event(Role::ToolResult, "error: auth failed", None, vec!["auth failed"]),
        ];
        let manifest = make_manifest();
        let temp = tempfile::tempdir().unwrap();
        let scraper = Scraper::new(temp.path().to_path_buf()).unwrap();
        let patterns = detect_antipatterns(&events, &manifest, Some("failure"), &scraper);
        assert!(patterns.iter().any(|p| p.pattern.contains("Error escalation")));
    }

    #[test]
    fn test_has_error_signal() {
        assert!(has_error_signal("error: something failed"));
        assert!(has_error_signal("exit code 1"));
        assert!(has_error_signal("panic at 'unreachable'"));
        assert!(!has_error_signal("all tests passed"));
    }

    #[test]
    fn test_has_positive_signal() {
        assert!(has_positive_signal("thanks, that works"));
        assert!(has_positive_signal("perfect!"));
        assert!(has_positive_signal("looks good to me"));
        assert!(!has_positive_signal("still broken"));
    }

    #[test]
    fn test_write_sidecar() {
        let temp = tempfile::tempdir().unwrap();
        let patterns = vec![AntiPattern {
            pattern: "test pattern".to_string(),
            error_fingerprints: vec!["test_fp".to_string()],
            session_ids: vec!["test/1".to_string()],
            alternative_session_ids: vec![],
        }];

        write_antipatterns_sidecar(temp.path(), "test/1", &patterns).unwrap();

        let sidecar = temp.path().join("sessions/test/1.anti-patterns.jsonl");
        assert!(sidecar.exists());
        let content = std::fs::read_to_string(&sidecar).unwrap();
        assert!(content.contains("test pattern"));
    }
}
