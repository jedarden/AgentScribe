//! Recurring problem detection
//!
//! Analyzes error fingerprints across sessions to find problems that recur
//! frequently. Groups by fingerprint, counts occurrences, and shows affected
//! projects, agents, and links to the most recent fix session.

use crate::enrichment::outcome::{detect_outcome, OutcomeConfig};
use crate::error::Result;
use crate::event::{Event, Role, SessionManifest};
use crate::scraper::Scraper;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Options for recurring problem detection
pub struct RecurringOptions {
    /// Only consider sessions after this timestamp
    pub since: DateTime<Utc>,
    /// Minimum occurrence count to report
    pub threshold: usize,
}

/// A single occurrence of an error fingerprint within a session
#[derive(Debug, Clone, Serialize)]
struct FingerprintOccurrence {
    session_id: String,
    source_agent: String,
    project: Option<String>,
    timestamp: DateTime<Utc>,
    outcome: Option<String>,
    /// Number of events in this session carrying this fingerprint
    event_count: usize,
}

/// Aggregated data for a recurring fingerprint
#[derive(Debug, Clone, Serialize)]
pub struct RecurringProblem {
    /// The normalized error fingerprint
    pub fingerprint: String,
    /// How many sessions contain this fingerprint
    pub session_count: usize,
    /// Total number of events with this fingerprint across all sessions
    pub event_count: usize,
    /// Projects affected by this problem
    pub projects: Vec<String>,
    /// Agents (source types) that encountered this problem
    pub agents: Vec<String>,
    /// Agents that successfully fixed this problem
    pub fix_agents: Vec<String>,
    /// Timestamp of the most recent occurrence
    pub last_seen: DateTime<Utc>,
    /// Session ID of the most recent fix (outcome=success) session
    pub last_fix_session: Option<String>,
    /// Timestamp of the last fix
    pub last_fix_at: Option<DateTime<Utc>>,
}

/// JSON output wrapper
#[derive(Debug, Serialize)]
pub struct RecurringOutput {
    pub since: DateTime<Utc>,
    pub threshold: usize,
    pub problems: Vec<RecurringProblem>,
}

/// Execute recurring problem detection
pub fn detect_recurring(
    data_dir: &Path,
    opts: &RecurringOptions,
) -> Result<RecurringOutput> {
    let mut scraper = Scraper::new(data_dir.to_path_buf())?;
    scraper.load_plugins()?;

    let all_sessions = scraper.all_sessions()?;

    // fingerprint -> list of occurrences
    let mut fingerprint_map: HashMap<String, Vec<FingerprintOccurrence>> = HashMap::new();

    for session_id in &all_sessions {
        let events = match scraper.read_session(session_id) {
            Ok(e) => e,
            Err(_) => continue,
        };

        if events.is_empty() {
            continue;
        }

        // Determine session metadata from events
        let first_ts = events[0].ts;
        if first_ts < opts.since {
            continue;
        }

        let source_agent = events[0].source_agent.clone();
        let project = events.iter().find_map(|e| e.project.clone());

        // Detect session outcome using the enrichment signal-scoring system
        let manifest = build_manifest_from_events(&events, session_id, &source_agent);
        let outcome = detect_outcome(&events, &manifest, &OutcomeConfig::default());
        let outcome_str = outcome.as_str().to_string();

        // Collect fingerprints and their per-session event counts
        let mut fp_event_counts: HashMap<String, usize> = HashMap::new();
        let mut fp_latest_ts: HashMap<String, DateTime<Utc>> = HashMap::new();

        for event in &events {
            for fp in &event.error_fingerprints {
                if fp.is_empty() {
                    continue;
                }
                *fp_event_counts.entry(fp.clone()).or_insert(0) += 1;
                fp_latest_ts
                    .entry(fp.clone())
                    .and_modify(|ts| {
                        if event.ts > *ts {
                            *ts = event.ts;
                        }
                    })
                    .or_insert(event.ts);
            }
        }

        for (fp, event_count) in fp_event_counts {
            let ts = fp_latest_ts[&fp];
            fingerprint_map.entry(fp).or_default().push(FingerprintOccurrence {
                session_id: session_id.clone(),
                source_agent: source_agent.clone(),
                project: project.clone(),
                timestamp: ts,
                outcome: Some(outcome_str.clone()),
                event_count,
            });
        }
    }

    // Aggregate into RecurringProblem entries
    let mut problems: Vec<RecurringProblem> = fingerprint_map
        .into_iter()
        .filter(|(_, occurrences)| occurrences.len() >= opts.threshold)
        .map(|(fingerprint, occurrences)| {
            let session_count = occurrences.len();
            let event_count: usize = occurrences.iter().map(|o| o.event_count).sum();

            let mut projects: Vec<String> = occurrences
                .iter()
                .filter_map(|o| o.project.clone())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            projects.sort();

            let agents: Vec<String> = occurrences
                .iter()
                .map(|o| o.source_agent.clone())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();

            // Agents that successfully fixed the problem
            let fix_agents: Vec<String> = occurrences
                .iter()
                .filter(|o| o.outcome.as_deref() == Some("success"))
                .map(|o| o.source_agent.clone())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();

            let last_seen = occurrences
                .iter()
                .map(|o| o.timestamp)
                .max()
                .unwrap_or_else(Utc::now);

            // Find the most recent session where the problem was resolved
            let last_fix = occurrences
                .iter()
                .filter(|o| o.outcome.as_deref() == Some("success"))
                .max_by_key(|o| o.timestamp);

            RecurringProblem {
                fingerprint,
                session_count,
                event_count,
                projects,
                agents,
                fix_agents,
                last_seen,
                last_fix_session: last_fix.map(|o| o.session_id.clone()),
                last_fix_at: last_fix.map(|o| o.timestamp),
            }
        })
        .collect();

    // Sort by frequency descending, then by last_seen descending
    problems.sort_by(|a, b| {
        b.session_count
            .cmp(&a.session_count)
            .then_with(|| b.last_seen.cmp(&a.last_seen))
    });

    Ok(RecurringOutput {
        since: opts.since,
        threshold: opts.threshold,
        problems,
    })
}

/// Build a SessionManifest from events for outcome detection.
fn build_manifest_from_events(
    events: &[Event],
    session_id: &str,
    source_agent: &str,
) -> SessionManifest {
    let mut manifest = SessionManifest::new(session_id.to_string(), source_agent.to_string());

    if let Some(first) = events.first() {
        manifest.started = first.ts;
    }
    if let Some(last) = events.last() {
        manifest.ended = Some(last.ts);
    }

    manifest.turns = events.len() as u32;

    // Collect files touched from events
    let mut files: HashSet<String> = HashSet::new();
    for event in events {
        for fp in &event.file_paths {
            files.insert(fp.clone());
        }
    }
    manifest.files_touched = files.into_iter().collect();

    // Detect model from events
    manifest.model = events.iter().find_map(|e| e.model.clone());

    manifest
}

/// Format recurring problems as a human-readable table
pub fn format_human(output: &RecurringOutput) -> String {
    if output.problems.is_empty() {
        return format!(
            "No recurring problems found (threshold: {}, since: {})\n",
            output.threshold,
            output.since.format("%Y-%m-%d")
        );
    }

    let mut lines = Vec::new();

    lines.push(format!(
        "Recurring problems (threshold: {}, since: {})\n",
        output.threshold,
        output.since.format("%Y-%m-%d")
    ));

    for (i, problem) in output.problems.iter().enumerate() {
        lines.push(format!(
            "{}. {} ({} sessions, {} events)",
            i + 1,
            truncate_fingerprint(&problem.fingerprint, 80),
            problem.session_count,
            problem.event_count,
        ));

        lines.push(format!("   Projects: {}", format_list(&problem.projects)));
        lines.push(format!("   Agents:   {}", format_list(&problem.agents)));

        if !problem.fix_agents.is_empty() {
            lines.push(format!("   Fixed by: {}", format_list(&problem.fix_agents)));
        }

        lines.push(format!(
            "   Last seen: {}",
            problem.last_seen.format("%Y-%m-%d %H:%M")
        ));

        if let Some(ref session_id) = problem.last_fix_session {
            lines.push(format!(
                "   Last fix:  {} ({})",
                session_id,
                problem.last_fix_at.unwrap().format("%Y-%m-%d")
            ));
        } else {
            lines.push("   Last fix:  (none)".to_string());
        }

        lines.push(String::new());
    }

    lines.join("\n")
}

/// Truncate a fingerprint for display, keeping the error type prefix intact.
fn truncate_fingerprint(fp: &str, max_len: usize) -> String {
    if fp.len() <= max_len {
        return fp.to_string();
    }
    // Try to split at the first colon to preserve the error type
    if let Some(colon_pos) = fp.find(':') {
        let prefix = &fp[..=colon_pos];
        let remaining = max_len.saturating_sub(prefix.len() + 6); // 3 for "..." + 3 safety margin
        if remaining > 10 {
            return format!("{}{}...", prefix, &fp[colon_pos + 1..colon_pos + 1 + remaining]);
        }
    }
    format!("{}...", &fp[..max_len.saturating_sub(3)])
}

/// Format a list of items for human-readable output.
fn format_list(items: &[String]) -> String {
    match items.len() {
        0 => "(none)".to_string(),
        1 => items[0].clone(),
        2 => format!("{} and {}", items[0], items[1]),
        3 => format!("{}, {}, and {}", items[0], items[1], items[2]),
        n if n <= 5 => {
            let last = items.last().unwrap();
            let rest = &items[..n - 1];
            format!("{}, and {}", rest.join(", "), last)
        }
        n => {
            let shown = &items[..3];
            format!("{}, ... ({} total)", shown.join(", "), n)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use std::path::PathBuf;

    fn make_event(
        ts: DateTime<Utc>,
        session_id: &str,
        role: Role,
        content: &str,
        fingerprints: Vec<&str>,
    ) -> Event {
        Event::new(
            ts,
            session_id.to_string(),
            "test-agent".to_string(),
            role,
            content.to_string(),
        )
        .with_error_fingerprints(fingerprints.into_iter().map(String::from).collect())
    }

    fn make_scraper_with_sessions(
        sessions: HashMap<String, Vec<Event>>,
    ) -> (tempfile::TempDir, PathBuf, Scraper) {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join(".agentscribe");

        // Create the plugin and sessions directories
        std::fs::create_dir_all(data_dir.join("plugins")).unwrap();
        std::fs::create_dir_all(data_dir.join("sessions/test-agent")).unwrap();
        std::fs::create_dir_all(data_dir.join("state")).unwrap();

        // Write session files
        for (session_id, events) in &sessions {
            let path = data_dir
                .join("sessions/test-agent")
                .join(format!("{}.jsonl", session_id));
            let content: Vec<String> = events
                .iter()
                .map(|e| e.to_jsonl().unwrap())
                .collect();
            std::fs::write(path, content.join("\n")).unwrap();
        }

        // Create a minimal plugin
        let plugin_dir = data_dir.join("plugins");
        std::fs::write(
            plugin_dir.join("test-agent.toml"),
            r#"
[plugin]
name = "test-agent"
version = "0.1.0"

[source]
format = "jsonl"
paths = ["/tmp/test-agent-logs"]

[parser]
timestamp = "ts"
role = "role"
content = "content"
"#,
        )
        .unwrap();

        let scraper = Scraper::new(data_dir.clone()).unwrap();
        (temp, data_dir, scraper)
    }

    #[test]
    fn test_format_list() {
        assert_eq!(format_list(&[]), "(none)");
        assert_eq!(format_list(&["foo".to_string()]), "foo");
        assert_eq!(
            format_list(&["foo".to_string(), "bar".to_string()]),
            "foo and bar"
        );
        assert_eq!(
            format_list(&["a".to_string(), "b".to_string(), "c".to_string()]),
            "a, b, and c"
        );
    }

    #[test]
    fn test_truncate_fingerprint() {
        assert_eq!(
            truncate_fingerprint("ConnectionError:Connection refused to host:port", 80),
            "ConnectionError:Connection refused to host:port"
        );
        assert_eq!(
            truncate_fingerprint(
                "SomeVeryLongErrorType:this is a very long message that should be truncated",
                40
            ),
            "SomeVeryLongErrorType:this is a ve..."
        );
    }

    #[test]
    fn test_detect_recurring_groups_fingerprints() {
        let now = Utc::now();
        let sessions = HashMap::from([
            (
                "session-1".to_string(),
                vec![
                    make_event(now - Duration::days(10), "test-agent/session-1", Role::User, "fix this", vec!["ErrorType:connection refused"]),
                    make_event(now - Duration::days(10), "test-agent/session-1", Role::Assistant, "done", vec![]),
                    make_event(now - Duration::days(10), "test-agent/session-1", Role::User, "thanks, that worked!", vec![]),
                ],
            ),
            (
                "session-2".to_string(),
                vec![
                    make_event(now - Duration::days(5), "test-agent/session-2", Role::User, "same error", vec!["ErrorType:connection refused"]),
                    make_event(now - Duration::days(5), "test-agent/session-2", Role::User, "thanks!", vec![]),
                ],
            ),
            (
                "session-3".to_string(),
                vec![
                    make_event(now - Duration::days(2), "test-agent/session-3", Role::User, "again", vec!["ErrorType:connection refused"]),
                ],
            ),
            // Different fingerprint — should not be grouped
            (
                "session-4".to_string(),
                vec![
                    make_event(now - Duration::days(1), "test-agent/session-4", Role::User, "other error", vec!["OtherError:something else"]),
                ],
            ),
        ]);

        let (_temp, data_dir, mut scraper) = make_scraper_with_sessions(sessions);
        scraper.load_plugins().unwrap();

        let opts = RecurringOptions {
            since: now - Duration::days(30),
            threshold: 3,
        };

        let output = detect_recurring(&data_dir, &opts).unwrap();

        assert_eq!(output.problems.len(), 1);
        assert_eq!(output.problems[0].fingerprint, "ErrorType:connection refused");
        assert_eq!(output.problems[0].session_count, 3);
    }

    #[test]
    fn test_detect_recurring_event_count() {
        let now = Utc::now();
        let sessions = HashMap::from([
            (
                "session-1".to_string(),
                vec![
                    make_event(now - Duration::days(5), "test-agent/session-1", Role::ToolResult, "err1", vec!["Err:X"]),
                    make_event(now - Duration::days(5), "test-agent/session-1", Role::ToolResult, "err2", vec!["Err:X"]),
                    make_event(now - Duration::days(5), "test-agent/session-1", Role::User, "thanks!", vec![]),
                ],
            ),
            (
                "session-2".to_string(),
                vec![
                    make_event(now - Duration::days(3), "test-agent/session-2", Role::ToolResult, "err", vec!["Err:X"]),
                    make_event(now - Duration::days(3), "test-agent/session-2", Role::User, "thanks!", vec![]),
                ],
            ),
            (
                "session-3".to_string(),
                vec![
                    make_event(now - Duration::days(1), "test-agent/session-3", Role::ToolResult, "err", vec!["Err:X"]),
                    make_event(now - Duration::days(1), "test-agent/session-3", Role::ToolResult, "err", vec!["Err:X"]),
                    make_event(now - Duration::days(1), "test-agent/session-3", Role::ToolResult, "err", vec!["Err:X"]),
                ],
            ),
        ]);

        let (_temp, data_dir, mut scraper) = make_scraper_with_sessions(sessions);
        scraper.load_plugins().unwrap();

        let opts = RecurringOptions {
            since: now - Duration::days(30),
            threshold: 3,
        };

        let output = detect_recurring(&data_dir, &opts).unwrap();

        assert_eq!(output.problems.len(), 1);
        assert_eq!(output.problems[0].session_count, 3);
        assert_eq!(output.problems[0].event_count, 6); // 2 + 1 + 3
    }

    #[test]
    fn test_detect_recurring_threshold_filter() {
        let now = Utc::now();
        let sessions = HashMap::from([
            (
                "session-1".to_string(),
                vec![make_event(now, "test-agent/session-1", Role::User, "err", vec!["E:A"])],
            ),
            (
                "session-2".to_string(),
                vec![make_event(now, "test-agent/session-2", Role::User, "err", vec!["E:A"])],
            ),
        ]);

        let (_temp, data_dir, mut scraper) = make_scraper_with_sessions(sessions);
        scraper.load_plugins().unwrap();

        let opts = RecurringOptions {
            since: now - Duration::days(30),
            threshold: 3,
        };

        let output = detect_recurring(&data_dir, &opts).unwrap();
        assert!(output.problems.is_empty());
    }

    #[test]
    fn test_detect_recurring_fix_agents() {
        let now = Utc::now();
        let sessions = HashMap::from([
            (
                "session-1".to_string(),
                vec![
                    make_event(now - Duration::days(10), "test-agent/session-1", Role::User, "fix this error", vec!["Err:foo"]),
                    make_event(now - Duration::days(10), "test-agent/session-1", Role::Assistant, "fixed it", vec![]),
                    make_event(now - Duration::days(10), "test-agent/session-1", Role::User, "thanks, that worked!", vec![]),
                ],
            ),
            (
                "session-2".to_string(),
                vec![
                    make_event(now - Duration::days(5), "test-agent/session-2", Role::User, "same error", vec!["Err:foo"]),
                    make_event(now - Duration::days(5), "test-agent/session-2", Role::Assistant, "fixed it", vec![]),
                    make_event(now - Duration::days(5), "test-agent/session-2", Role::User, "great, works now!", vec![]),
                ],
            ),
            (
                "session-3".to_string(),
                vec![
                    make_event(now - Duration::days(2), "test-agent/session-3", Role::User, "again", vec!["Err:foo"]),
                    // No resolution — this should not count as a fix
                ],
            ),
        ]);

        let (_temp, data_dir, mut scraper) = make_scraper_with_sessions(sessions);
        scraper.load_plugins().unwrap();

        let opts = RecurringOptions {
            since: now - Duration::days(30),
            threshold: 3,
        };

        let output = detect_recurring(&data_dir, &opts).unwrap();

        assert_eq!(output.problems.len(), 1);
        assert_eq!(output.problems[0].fix_agents, vec!["test-agent"]);
        assert!(output.problems[0].last_fix_session.is_some());
        // Last fix should be session-2 (day 5), not session-3 (day 2, unresolved)
        assert!(output.problems[0].last_fix_session.as_ref().unwrap().contains("session-2"));
    }

    #[test]
    fn test_detect_recurring_time_filter() {
        let now = Utc::now();
        let sessions = HashMap::from([
            (
                "session-1".to_string(),
                vec![make_event(now - Duration::days(60), "test-agent/session-1", Role::User, "old", vec!["Err:A"])],
            ),
            (
                "session-2".to_string(),
                vec![make_event(now - Duration::days(5), "test-agent/session-2", Role::User, "new", vec!["Err:A"])],
            ),
            (
                "session-3".to_string(),
                vec![make_event(now - Duration::days(3), "test-agent/session-3", Role::User, "new", vec!["Err:A"])],
            ),
        ]);

        let (_temp, data_dir, mut scraper) = make_scraper_with_sessions(sessions);
        scraper.load_plugins().unwrap();

        let opts = RecurringOptions {
            since: now - Duration::days(10),
            threshold: 3,
        };

        let output = detect_recurring(&data_dir, &opts).unwrap();

        // Only 2 sessions within the window, below threshold
        assert!(output.problems.is_empty());
    }
}
