//! Reflection export API: structured behavioral data for reflection tooling.
//!
//! This module provides tools for exporting session data enriched with behavioral
//! signals suitable for reflection and auto-tuning of agent configurations.

use crate::enrichment::behavioral_signals::{load_behavioral_signals, BehavioralSignals};
use crate::event::{Event, SessionManifest};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Filter options for listing reflection sessions
#[derive(Debug, Clone, Default)]
pub struct ReflectFilter {
    /// Only include sessions starting after this timestamp
    pub since: Option<DateTime<Utc>>,
    /// Only include sessions from this project path
    pub project: Option<String>,
    /// Only include sessions with this outcome
    pub outcome: Option<String>,
    /// Only include sessions where agent modified config files
    pub modified_config_only: bool,
    /// Only include sessions where agent read config files
    pub read_config_only: bool,
    /// Maximum number of sessions to return
    pub limit: Option<usize>,
    /// Filter by annotation tags (AND logic: all tags must be present)
    pub tags: Vec<String>,
}

/// Structured behavioral session record for reflection analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectSession {
    /// Session ID (format: agent/session-id)
    pub session_id: String,
    /// Source agent name (plugin name)
    pub agent: String,
    /// Project path, if available
    pub project: Option<String>,
    /// Session start timestamp
    pub started: DateTime<Utc>,
    /// Session end timestamp, if available
    pub ended: Option<DateTime<Utc>>,
    /// Session duration in seconds
    pub duration_secs: u64,
    /// Session outcome (success, failure, abandoned, unknown)
    pub outcome: String,
    /// Generated summary
    pub summary: Option<String>,
    /// Session tags
    pub tags: Vec<String>,
    /// Model name, if available
    pub model: Option<String>,
    /// Tool call counts by tool name
    pub tool_call_counts: HashMap<String, u32>,
    /// Number of times files were re-read
    pub re_read_count: u32,
    /// Number of bash commands that failed
    pub bash_failure_count: u32,
    /// Config/memory files read in this session
    pub read_config_files: Vec<String>,
    /// Config/memory files modified in this session
    pub modified_config_files: Vec<String>,
    /// Error fingerprints found in this session
    pub error_fingerprints: Vec<String>,
    /// Anti-patterns detected (if any)
    pub anti_patterns: Vec<AntiPatternEntry>,
    /// Files touched in this session
    pub files_touched: Vec<String>,
    /// Config file changes observed after this session
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_changes_after: Option<Vec<ConfigChangeAfter>>,
    /// Parent session ID (for subagent sessions)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
}

/// Entry for an anti-pattern detected in a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiPatternEntry {
    /// Pattern name/description
    pub pattern: String,
}

/// Config file change observed after a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChangeAfter {
    /// File that was changed
    pub file: String,
    /// Minutes after session ended when change occurred
    pub minutes_after: u64,
}

/// Reflection session focused on behavioral signals from sidecar
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectionSession {
    /// Session ID (format: agent/session-id)
    pub session_id: String,
    /// Source agent name (plugin name)
    pub agent: String,
    /// Project path, if available
    pub project: Option<String>,
    /// Session start timestamp
    pub started: DateTime<Utc>,
    /// Session end timestamp, if available
    pub ended: Option<DateTime<Utc>>,
    /// Session duration in seconds
    pub duration_secs: u64,
    /// Session outcome (success, failure, abandoned, unknown)
    pub outcome: String,
    /// Model name, if available
    pub model: Option<String>,
    /// Tool call counts by tool name
    pub tool_call_counts: ToolCallCounts,
    /// Number of times files were re-read
    pub re_read_count: u32,
    /// Number of bash commands that failed
    pub bash_failure_count: u32,
    /// Config/memory files read in this session
    pub read_config_files: Vec<String>,
    /// Config/memory files modified in this session
    pub modified_config_files: Vec<String>,
    /// Files read more than once
    pub re_read_files: Vec<String>,
    /// Files edited more than once
    pub multi_edit_files: Vec<String>,
    /// Number of working directory switches
    pub cwd_switch_count: u32,
    /// Ratio: assistant turns / total turns
    pub assistant_turn_ratio: f32,
    /// Parent session ID (for subagent sessions)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
}

/// Tool call counts breakdown
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolCallCounts {
    /// Total tool calls in session
    pub total: u32,
    /// Counts by tool name
    pub by_name: HashMap<String, u32>,
}

/// Cross-session pattern summary for analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternSummary {
    /// Time range for this pattern analysis
    pub since: DateTime<Utc>,
    pub before: DateTime<Utc>,
    /// Total sessions analyzed
    pub total_sessions: usize,
    /// Total duration across all sessions (seconds)
    pub total_duration_secs: u64,
    /// Average session duration (seconds)
    pub avg_duration_secs: f64,
    /// Success rate (0.0 to 1.0)
    pub success_rate: f64,
    /// Most common tools used
    pub common_tools: Vec<PatternEntry>,
    /// Sessions with highest re-read counts
    pub top_re_read_sessions: Vec<SessionMetric>,
    /// Sessions with most bash failures
    pub top_bash_failure_sessions: Vec<SessionMetric>,
    /// Config files most frequently read
    pub config_read_patterns: Vec<PatternEntry>,
    /// Config files most frequently modified
    pub config_modify_patterns: Vec<PatternEntry>,
}

/// Session metric entry for top-N lists
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetric {
    /// Session ID
    pub session_id: String,
    /// Metric value
    pub value: u32,
    /// Agent name
    pub agent: String,
    /// Project path, if available
    pub project: Option<String>,
}

/// Cross-session behavioral pattern summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectPatterns {
    /// Time range for this pattern analysis
    pub since: DateTime<Utc>,
    pub before: DateTime<Utc>,
    /// Total sessions analyzed
    pub total_sessions: usize,
    /// Most common tool sequences preceding failures
    pub failure_tool_sequences: Vec<PatternEntry>,
    /// Files most frequently re-read before success
    pub re_read_before_success: Vec<PatternEntry>,
    /// Config files most frequently modified with outcomes
    pub config_modifications: Vec<ConfigModPattern>,
    /// Recurring error fingerprints and resolution rates
    pub error_patterns: Vec<ErrorPattern>,
}

/// A pattern entry with count and percentage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternEntry {
    /// Pattern description
    pub pattern: String,
    /// Occurrence count
    pub count: usize,
    /// Percentage of sessions with this pattern
    pub percent: f64,
}

/// Config modification pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigModPattern {
    /// Config file path
    pub file: String,
    /// Number of times modified
    pub count: usize,
    /// Outcomes that typically follow modification
    pub typical_outcomes: HashMap<String, usize>,
}

/// Error pattern with resolution statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPattern {
    /// Error fingerprint
    pub fingerprint: String,
    /// Number of occurrences
    pub count: usize,
    /// Number of sessions that ended successfully after this error
    pub resolved_successfully: usize,
    /// Resolution rate (0.0 to 1.0)
    pub resolution_rate: f64,
}

/// Export sessions with behavioral metadata for reflection analysis
pub fn export_reflect_sessions(
    data_dir: &Path,
    sessions: &[(String, Vec<Event>, SessionManifest)],
    filter: &ReflectFilter,
) -> Result<Vec<ReflectSession>, ReflectError> {
    let mut results = Vec::new();

    for (session_id, events, manifest) in sessions {
        // Apply project filter
        if let Some(ref proj) = filter.project {
            if manifest.project.as_deref() != Some(proj.as_str()) {
                continue;
            }
        }

        // Apply since filter
        if let Some(since) = filter.since {
            if manifest.started < since {
                continue;
            }
        }

        // Apply outcome filter
        if let Some(ref outcome_filter) = filter.outcome {
            if manifest.outcome.as_deref() != Some(outcome_filter.as_str()) {
                continue;
            }
        }

        // Load behavioral signals from sidecar
        let signals = load_behavioral_signals(data_dir, session_id).unwrap_or_default();

        // Apply config-based filters
        if filter.modified_config_only && signals.modified_config_files.is_empty() {
            continue;
        }
        if filter.read_config_only && signals.read_config_files.is_empty() {
            continue;
        }

        // Build the reflect session record
        let duration = signals.duration_secs;
        let ended = if duration > 0 {
            Some(manifest.started + Duration::seconds(duration as i64))
        } else {
            None
        };

        let outcome = manifest
            .outcome
            .clone()
            .unwrap_or_else(|| "unknown".to_string());

        // Collect error fingerprints from events
        let error_fingerprints: Vec<String> = events
            .iter()
            .flat_map(|e| e.error_fingerprints.clone())
            .collect();

        // Extract anti-patterns if available (not yet tracked on manifests)
        let anti_patterns: Vec<AntiPatternEntry> = Vec::new();

        // Build tool call counts
        let mut tool_call_counts = HashMap::new();
        for (tool, count) in &signals.tool_call_counts_by_name {
            tool_call_counts.insert(tool.clone(), *count);
        }

        let reflect_session = ReflectSession {
            session_id: session_id.clone(),
            agent: manifest.source_agent.clone(),
            project: manifest.project.clone(),
            started: manifest.started,
            ended,
            duration_secs: duration,
            outcome,
            summary: manifest.summary.clone(),
            tags: manifest.tags.clone(),
            model: manifest.model.clone(),
            tool_call_counts,
            re_read_count: signals.re_read_count,
            bash_failure_count: signals.bash_failure_count,
            read_config_files: signals.read_config_files,
            modified_config_files: signals.modified_config_files,
            error_fingerprints,
            anti_patterns,
            files_touched: manifest.files_touched.clone(),
            config_changes_after: None, // Populated by config_change_tracker if needed
            parent_session_id: manifest.parent_session_id.clone(),
        };

        results.push(reflect_session);
    }

    // Apply limit
    if let Some(limit) = filter.limit {
        results.truncate(limit);
    }

    results.sort_by_key(|s| s.started);
    results.reverse(); // Newest first

    Ok(results)
}

/// Analyze cross-session behavioral patterns
pub fn analyze_reflect_patterns(
    sessions: &[(String, Vec<Event>, SessionManifest)],
    since: DateTime<Utc>,
) -> Result<ReflectPatterns, ReflectError> {
    let total_sessions = sessions.len();
    if total_sessions == 0 {
        return Ok(ReflectPatterns {
            since,
            before: Utc::now(),
            total_sessions: 0,
            failure_tool_sequences: Vec::new(),
            re_read_before_success: Vec::new(),
            config_modifications: Vec::new(),
            error_patterns: Vec::new(),
        });
    }

    // Find latest timestamp
    let before = sessions
        .iter()
        .map(|(_, _, m)| m.started)
        .max()
        .unwrap_or_else(Utc::now);

    let mut failure_tool_seqs: HashMap<String, usize> = HashMap::new();
    let mut re_read_before_success: HashMap<String, usize> = HashMap::new();
    let mut config_mods: HashMap<String, ConfigModPattern> = HashMap::new();
    let mut error_patterns: HashMap<String, ErrorPattern> = HashMap::new();

    for (_session_id, events, manifest) in sessions {
        let outcome = manifest.outcome.as_deref().unwrap_or("unknown");

        // Analyze failure tool sequences
        if outcome == "failure" {
            if let Some(seq) = extract_tool_sequence(events, 3) {
                *failure_tool_seqs.entry(seq).or_insert(0) += 1;
            }
        }

        // Analyze re-reads before success
        if outcome == "success" {
            if let Ok(Some(signals)) = BehavioralSignals::try_from_events(events) {
                for file in &signals.re_read_files {
                    *re_read_before_success.entry(file.clone()).or_insert(0) += 1;
                }
            }
        }

        // Analyze config modifications
        if let Ok(Some(signals)) = BehavioralSignals::try_from_events(events) {
            for file in &signals.modified_config_files {
                let entry = config_mods.entry(file.clone()).or_insert(ConfigModPattern {
                    file: file.clone(),
                    count: 0,
                    typical_outcomes: HashMap::new(),
                });
                entry.count += 1;
                *entry
                    .typical_outcomes
                    .entry(outcome.to_string())
                    .or_insert(0) += 1;
            }
        }

        // Analyze error patterns
        for error in collect_error_fingerprints(events) {
            let entry = error_patterns.entry(error.clone()).or_insert(ErrorPattern {
                fingerprint: error,
                count: 0,
                resolved_successfully: 0,
                resolution_rate: 0.0,
            });
            entry.count += 1;
            if outcome == "success" {
                entry.resolved_successfully += 1;
            }
        }
    }

    // Calculate resolution rates
    for pattern in error_patterns.values_mut() {
        if pattern.count > 0 {
            pattern.resolution_rate = pattern.resolved_successfully as f64 / pattern.count as f64;
        }
    }

    // Convert to sorted vectors
    let mut failure_tool_sequences: Vec<_> = failure_tool_seqs
        .into_iter()
        .map(|(pattern, count)| PatternEntry {
            pattern,
            count,
            percent: (count as f64 / total_sessions as f64) * 100.0,
        })
        .collect();
    failure_tool_sequences.sort_by_key(|b| std::cmp::Reverse(b.count));

    let mut re_read_before_success_vec: Vec<_> = re_read_before_success
        .into_iter()
        .map(|(pattern, count)| PatternEntry {
            pattern,
            count,
            percent: (count as f64 / total_sessions as f64) * 100.0,
        })
        .collect();
    re_read_before_success_vec.sort_by_key(|b| std::cmp::Reverse(b.count));

    let mut config_modifications: Vec<_> = config_mods.into_values().collect();
    config_modifications.sort_by_key(|b| std::cmp::Reverse(b.count));

    let mut error_patterns_vec: Vec<_> = error_patterns.into_values().collect();
    error_patterns_vec.sort_by_key(|b| std::cmp::Reverse(b.count));

    Ok(ReflectPatterns {
        since,
        before,
        total_sessions,
        failure_tool_sequences,
        re_read_before_success: re_read_before_success_vec,
        config_modifications,
        error_patterns: error_patterns_vec,
    })
}

/// Extract the last N tool calls as a sequence string
fn extract_tool_sequence(events: &[Event], n: usize) -> Option<String> {
    let tools: Vec<&str> = events
        .iter()
        .filter_map(|e| e.tool.as_deref())
        .rev()
        .take(n)
        .collect();

    if tools.is_empty() {
        return None;
    }

    // Reverse back to chronological order
    let tools: Vec<_> = tools.into_iter().rev().collect();
    Some(tools.join(" → "))
}

/// Collect all unique error fingerprints from events
fn collect_error_fingerprints(events: &[Event]) -> Vec<String> {
    let mut fingerprints = std::collections::HashSet::new();
    for event in events {
        for fp in &event.error_fingerprints {
            fingerprints.insert(fp.clone());
        }
    }
    let mut vec: Vec<_> = fingerprints.into_iter().collect();
    vec.sort();
    vec
}

/// Parse a duration string (e.g., "7d", "30d", "1y") into a DateTime cutoff
pub fn parse_since_duration(duration_str: &str) -> Result<DateTime<Utc>, ReflectError> {
    let duration_str = duration_str.trim().to_lowercase();
    let (num_str, unit) = duration_str.split_at(
        duration_str
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(duration_str.len()),
    );

    let num: i64 = num_str
        .parse()
        .map_err(|_| ReflectError::InvalidDuration(duration_str.clone()))?;

    let duration = match unit {
        "s" | "sec" | "second" | "seconds" => Duration::seconds(num),
        "m" | "min" | "minute" | "minutes" => Duration::minutes(num),
        "h" | "hour" | "hours" => Duration::hours(num),
        "d" | "day" | "days" => Duration::days(num),
        "w" | "week" | "weeks" => Duration::weeks(num),
        "mo" | "month" | "months" => Duration::days(num * 30), // Approximate
        "y" | "year" | "years" => Duration::days(num * 365),   // Approximate
        _ => return Err(ReflectError::InvalidDuration(duration_str.clone())),
    };

    Ok(Utc::now() - duration)
}

/// Read behavioral signals from a session's sidecar file.
///
/// Returns Ok(None) if the sidecar doesn't exist yet (graceful degradation).
pub fn read_behavioral_signals(
    data_dir: &Path,
    session_id: &str,
) -> Result<Option<BehavioralSignals>, ReflectError> {
    Ok(load_behavioral_signals(data_dir, session_id))
}

/// List reflection sessions with behavioral signal data.
///
/// Returns sessions that have behavioral_signals.json sidecars available.
pub fn list_reflection_sessions(
    data_dir: &Path,
    sessions: &[(String, Vec<Event>, SessionManifest)],
    filter: &ReflectFilter,
) -> Result<Vec<ReflectionSession>, ReflectError> {
    let mut results = Vec::new();

    for (session_id, _events, manifest) in sessions {
        // Behavioral signals are read from the sidecar; the event stream itself
        // is not needed here.
        let signals = match read_behavioral_signals(data_dir, session_id)? {
            Some(s) => s,
            None => continue, // Skip sessions without sidecars
        };

        // Apply project filter
        if let Some(ref proj) = filter.project {
            if manifest.project.as_deref() != Some(proj.as_str()) {
                continue;
            }
        }

        // Apply since filter
        if let Some(since) = filter.since {
            if manifest.started < since {
                continue;
            }
        }

        // Apply outcome filter
        if let Some(ref outcome_filter) = filter.outcome {
            if manifest.outcome.as_deref() != Some(outcome_filter.as_str()) {
                continue;
            }
        }

        // Apply config-based filters
        if filter.modified_config_only && signals.modified_config_files.is_empty() {
            continue;
        }
        if filter.read_config_only && signals.read_config_files.is_empty() {
            continue;
        }

        // Build the reflection session record
        let duration = signals.duration_secs;
        let ended = if duration > 0 {
            Some(manifest.started + Duration::seconds(duration as i64))
        } else {
            None
        };

        let outcome = manifest
            .outcome
            .clone()
            .unwrap_or_else(|| "unknown".to_string());

        // Build tool call counts
        let tool_call_counts = ToolCallCounts {
            total: signals.tool_call_count,
            by_name: signals.tool_call_counts_by_name.clone(),
        };

        let reflect_session = ReflectionSession {
            session_id: session_id.clone(),
            agent: manifest.source_agent.clone(),
            project: manifest.project.clone(),
            started: manifest.started,
            ended,
            duration_secs: duration,
            outcome,
            model: manifest.model.clone(),
            tool_call_counts,
            re_read_count: signals.re_read_count,
            bash_failure_count: signals.bash_failure_count,
            read_config_files: signals.read_config_files,
            modified_config_files: signals.modified_config_files,
            re_read_files: signals.re_read_files,
            multi_edit_files: signals.multi_edit_files,
            cwd_switch_count: signals.cwd_switch_count,
            assistant_turn_ratio: signals.assistant_turn_ratio,
            parent_session_id: manifest.parent_session_id.clone(),
        };

        results.push(reflect_session);
    }

    // Apply limit
    if let Some(limit) = filter.limit {
        results.truncate(limit);
    }

    results.sort_by_key(|s| s.started);
    results.reverse(); // Newest first

    Ok(results)
}

/// Parse reflection sessions from the index.
///
/// This is a lightweight version that doesn't require loading full event streams.
/// It reads manifests and behavioral_signals sidecars only.
pub fn parse_sessions_from_index(
    data_dir: &Path,
    manifests: &[(String, SessionManifest)],
) -> Result<Vec<ReflectionSession>, ReflectError> {
    let mut results = Vec::new();

    for (session_id, manifest) in manifests {
        // Load behavioral signals from sidecar
        let signals = match read_behavioral_signals(data_dir, session_id)? {
            Some(s) => s,
            None => continue, // Skip sessions without sidecars
        };

        let duration = signals.duration_secs;
        let ended = if duration > 0 {
            Some(manifest.started + Duration::seconds(duration as i64))
        } else {
            None
        };

        let outcome = manifest
            .outcome
            .clone()
            .unwrap_or_else(|| "unknown".to_string());

        let tool_call_counts = ToolCallCounts {
            total: signals.tool_call_count,
            by_name: signals.tool_call_counts_by_name.clone(),
        };

        let reflect_session = ReflectionSession {
            session_id: session_id.clone(),
            agent: manifest.source_agent.clone(),
            project: manifest.project.clone(),
            started: manifest.started,
            ended,
            duration_secs: duration,
            outcome,
            model: manifest.model.clone(),
            tool_call_counts,
            re_read_count: signals.re_read_count,
            bash_failure_count: signals.bash_failure_count,
            read_config_files: signals.read_config_files,
            modified_config_files: signals.modified_config_files,
            re_read_files: signals.re_read_files,
            multi_edit_files: signals.multi_edit_files,
            cwd_switch_count: signals.cwd_switch_count,
            assistant_turn_ratio: signals.assistant_turn_ratio,
            parent_session_id: manifest.parent_session_id.clone(),
        };

        results.push(reflect_session);
    }

    results.sort_by_key(|s| s.started);
    results.reverse(); // Newest first

    Ok(results)
}

/// Reflection-specific errors
#[derive(Debug, thiserror::Error)]
pub enum ReflectError {
    #[error("Invalid duration format: {0}")]
    InvalidDuration(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

// Extension trait for constructing BehavioralSignals from events
trait BehavioralSignalsExt {
    fn try_from_events(events: &[Event]) -> Result<Option<BehavioralSignals>, ReflectError>;
}

impl BehavioralSignalsExt for BehavioralSignals {
    fn try_from_events(events: &[Event]) -> Result<Option<BehavioralSignals>, ReflectError> {
        // Import the compute function from behavioral_signals module
        use crate::enrichment::behavioral_signals::compute_behavioral_signals;
        Ok(Some(compute_behavioral_signals(events)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Role;

    #[test]
    fn test_parse_since_duration() {
        let now = Utc::now();
        let cutoff = parse_since_duration("7d").unwrap();
        let diff = now - cutoff;
        assert!(diff.num_days() >= 6 && diff.num_days() <= 8);
    }

    #[test]
    fn test_parse_since_duration_various() {
        parse_since_duration("1h").unwrap();
        parse_since_duration("30m").unwrap();
        parse_since_duration("1w").unwrap();
        parse_since_duration("1mo").unwrap();
        parse_since_duration("1y").unwrap();
    }

    #[test]
    fn test_invalid_duration() {
        assert!(parse_since_duration("invalid").is_err());
        assert!(parse_since_duration("7").is_err());
    }

    fn make_event(role: Role, tool: Option<&str>) -> Event {
        Event::new(
            Utc::now(),
            "test/1".to_string(),
            "claude".to_string(),
            role,
            "".to_string(),
        )
        .with_tool(tool.map(|s| s.to_string()))
    }

    #[test]
    fn test_extract_tool_sequence() {
        let events = vec![
            make_event(Role::ToolCall, Some("Read")),
            make_event(Role::ToolCall, Some("Bash")),
            make_event(Role::ToolCall, Some("Edit")),
        ];
        let seq = extract_tool_sequence(&events, 3).unwrap();
        assert_eq!(seq, "Read → Bash → Edit");
    }

    #[test]
    fn test_collect_error_fingerprints() {
        let mut event1 = make_event(Role::Assistant, None);
        event1.error_fingerprints = vec!["error:E123".to_string(), "error:compile".to_string()];
        let mut event2 = make_event(Role::ToolResult, None);
        event2.error_fingerprints = vec!["error:E123".to_string()];

        let fps = collect_error_fingerprints(&[event1, event2]);
        assert_eq!(fps.len(), 2);
        assert!(fps.contains(&"error:E123".to_string()));
        assert!(fps.contains(&"error:compile".to_string()));
    }

    #[test]
    fn test_reflect_session_serialization() {
        let session = ReflectSession {
            session_id: "test/123".to_string(),
            agent: "claude".to_string(),
            project: Some("/project".to_string()),
            started: Utc::now(),
            ended: None,
            duration_secs: 60,
            outcome: "success".to_string(),
            summary: Some("Test summary".to_string()),
            tags: vec!["debug".to_string()],
            model: Some("claude-sonnet-5".to_string()),
            tool_call_counts: HashMap::new(),
            re_read_count: 0,
            bash_failure_count: 0,
            read_config_files: Vec::new(),
            modified_config_files: Vec::new(),
            error_fingerprints: Vec::new(),
            anti_patterns: Vec::new(),
            files_touched: Vec::new(),
            config_changes_after: None,
            parent_session_id: None,
        };

        let json = serde_json::to_string(&session).unwrap();
        let _deser: ReflectSession = serde_json::from_str(&json).unwrap();
    }
}
