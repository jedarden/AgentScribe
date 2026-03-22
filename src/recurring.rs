//! Recurring problem detection
//!
//! Analyzes error fingerprints across sessions to find problems that recur
//! frequently. Groups by fingerprint, counts occurrences, and shows affected
//! projects, agents, and links to the most recent fix session.

use crate::error::Result;
use crate::event::Event;
use crate::scraper::Scraper;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

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

        // Determine session outcome from the last event's outcome or manifest
        // We look at the last event for a summary/outcome hint
        let session_outcome = detect_session_outcome(&events);

        // Collect unique fingerprints per session to avoid double-counting
        let mut seen_fingerprints: HashMap<String, DateTime<Utc>> = HashMap::new();

        for event in &events {
            for fp in &event.error_fingerprints {
                if fp.is_empty() {
                    continue;
                }
                // Track the latest timestamp for this fingerprint in this session
                seen_fingerprints
                    .entry(fp.clone())
                    .and_modify(|ts| {
                        if event.ts > *ts {
                            *ts = event.ts;
                        }
                    })
                    .or_insert(event.ts);
            }
        }

        for (fp, ts) in seen_fingerprints {
            fingerprint_map.entry(fp).or_default().push(FingerprintOccurrence {
                session_id: session_id.clone(),
                source_agent: source_agent.clone(),
                project: project.clone(),
                timestamp: ts,
                outcome: session_outcome.clone(),
            });
        }
    }

    // Aggregate into RecurringProblem entries
    let mut problems: Vec<RecurringProblem> = fingerprint_map
        .into_iter()
        .filter(|(_, occurrences)| occurrences.len() >= opts.threshold)
        .map(|(fingerprint, occurrences)| {
            let session_count = occurrences.len();
            let mut projects: Vec<String> = occurrences
                .iter()
                .filter_map(|o| o.project.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            projects.sort();

            let mut agents: Vec<String> = occurrences
                .iter()
                .map(|o| o.source_agent.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            agents.sort();

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
                event_count: session_count, // one occurrence tracked per session
                projects,
                agents,
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

/// Detect session outcome from events.
/// Looks at the SessionManifest-like data embedded in events or infers from content.
fn detect_session_outcome(events: &[Event]) -> Option<String> {
    // Try to find an outcome from the last few events' content patterns
    let last = events.last()?;

    // Check if any event contains outcome-related keywords
    // This is a best-effort heuristic since outcomes are stored in manifests, not events
    let last_content = last.content.to_lowercase();

    // Check the enrichment outcome detection patterns
    if last_content.contains("success") || last_content.contains("completed") {
        return Some("success".to_string());
    }
    if last_content.contains("abandoned") || last_content.contains("cancelled") {
        return Some("abandoned".to_string());
    }
    if last_content.contains("error") || last_content.contains("failed") {
        return Some("failure".to_string());
    }

    None
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
            "{}. {} ({} sessions)",
            i + 1,
            problem.fingerprint,
            problem.session_count
        ));

        lines.push(format!("   Projects: {}", comma_join(&problem.projects)));
        lines.push(format!("   Agents:   {}", comma_join(&problem.agents)));
        lines.push(format!("   Last seen: {}", problem.last_seen.format("%Y-%m-%d")));

        if let Some(ref session_id) = problem.last_fix_session {
            lines.push(format!("   Last fix: {} ({})", session_id,
                problem.last_fix_at.unwrap().format("%Y-%m-%d")));
        } else {
            lines.push("   Last fix: (none)".to_string());
        }

        lines.push(String::new());
    }

    lines.join("\n")
}

fn comma_join(items: &[String]) -> String {
    if items.is_empty() {
        "(unknown)".to_string()
    } else if items.len() == 1 {
        items[0].clone()
    } else {
        format!("{}, ... ({} total)", items[0], items.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_session_outcome_success() {
        let event = Event::new(
            Utc::now(),
            "test/1".to_string(),
            "test".to_string(),
            crate::event::Role::Assistant,
            "The task was completed successfully.".to_string(),
        );
        let result = detect_session_outcome(&[event]);
        assert_eq!(result, Some("success".to_string()));
    }

    #[test]
    fn test_detect_session_outcome_failure() {
        let event = Event::new(
            Utc::now(),
            "test/1".to_string(),
            "test".to_string(),
            crate::event::Role::Assistant,
            "The task failed with an error.".to_string(),
        );
        let result = detect_session_outcome(&[event]);
        assert_eq!(result, Some("failure".to_string()));
    }

    #[test]
    fn test_detect_session_outcome_none() {
        let event = Event::new(
            Utc::now(),
            "test/1".to_string(),
            "test".to_string(),
            crate::event::Role::User,
            "Fix the bug.".to_string(),
        );
        let result = detect_session_outcome(&[event]);
        assert_eq!(result, None);
    }

    #[test]
    fn test_comma_join() {
        assert_eq!(comma_join(&[]), "(unknown)");
        assert_eq!(comma_join(&["foo".to_string()]), "foo");
        assert_eq!(comma_join(&["a".to_string(), "b".to_string()]), "a, ... (2 total)");
    }
}
