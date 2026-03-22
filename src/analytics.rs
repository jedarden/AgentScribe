//! Agent effectiveness analytics
//!
//! Cross-agent performance comparison and analytics. Computes success rates,
//! turn/token efficiency, problem type classification, specialization, trends,
//! and cost efficiency.

use crate::config::Config;
use crate::error::{AgentScribeError, Result};
use crate::index::build_schema;
use crate::search::open_index;
use chrono::{DateTime, Datelike, Duration, Utc};
use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;
use tantivy::collector::TopDocs;
use tantivy::query::AllQuery;
use tantivy::schema::Value;
use tantivy::{DocAddress, Searcher, TantivyDocument};

/// Approximate chars per token for estimation
const CHARS_PER_TOKEN: f64 = 4.0;

/// Problem types for session classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProblemType {
    Debug,
    Feature,
    Refactor,
    Investigation,
    Configuration,
    Documentation,
}

impl ProblemType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProblemType::Debug => "debug",
            ProblemType::Feature => "feature",
            ProblemType::Refactor => "refactor",
            ProblemType::Investigation => "investigation",
            ProblemType::Configuration => "configuration",
            ProblemType::Documentation => "documentation",
        }
    }

    pub fn all() -> &'static [ProblemType] {
        &[
            ProblemType::Debug,
            ProblemType::Feature,
            ProblemType::Refactor,
            ProblemType::Investigation,
            ProblemType::Configuration,
            ProblemType::Documentation,
        ]
    }
}

// Classification regex patterns
static DEBUG_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(fix|bug|error|crash|debug|broken|fault|regression)\b").unwrap());

static FEATURE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(add|implement|create|build|new|support|integrate|enable)\b").unwrap());

static REFACTOR_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(refactor|rename|move|extract|clean\s*up|restructure|simplify|reorganize)\b").unwrap());

static INVESTIGATION_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(explain|how does|what is|why|understand|investigate|explore|look into|figure out)\b").unwrap());

static DOCUMENTATION_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(document|readme|changelog|docstring|comment|docs)\b").unwrap());

/// Config file extensions for configuration problem type
const CONFIG_EXTENSIONS: &[&str] = &[
    ".toml", ".yaml", ".yml", ".json", ".env", ".ini", ".cfg", ".conf",
];

const CONFIG_FILENAMES: &[&str] = &["Dockerfile", "Makefile", "docker-compose.yml", "docker-compose.yaml"];

/// Doc file extensions for documentation problem type
const DOC_EXTENSIONS: &[&str] = &[".md", ".rst", ".txt", ".adoc"];

/// Data extracted from a single indexed session for analytics
pub(crate) struct SessionData {
    pub(crate) session_id: String,
    pub(crate) source_agent: String,
    pub(crate) project: Option<String>,
    pub(crate) timestamp: DateTime<Utc>,
    pub(crate) turns: u64,
    pub(crate) outcome: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) error_fingerprints: Vec<String>,
    pub(crate) file_paths: Vec<String>,
    pub(crate) content_length: usize,
    pub(crate) primary_type: ProblemType,
    pub(crate) secondary_type: Option<ProblemType>,
}

/// Analytics options
pub struct AnalyticsOptions {
    /// Filter by agent name
    pub agent: Option<String>,
    /// Filter by project path
    pub project: Option<String>,
    /// Only include sessions after this timestamp
    pub since: Option<DateTime<Utc>>,
}

/// Per-agent analytics summary
#[derive(Debug, Clone, Serialize)]
pub struct AgentMetrics {
    pub agent: String,
    pub total_sessions: usize,
    pub success_count: usize,
    pub failure_count: usize,
    pub abandoned_count: usize,
    pub unknown_count: usize,
    pub success_rate: f64,
    pub avg_turns_success: f64,
    pub avg_turns_all: f64,
    pub avg_tokens_success: f64,
    pub specialization: HashMap<String, usize>,
    pub estimated_cost: f64,
    pub cost_per_success: f64,
}

/// Problem type distribution entry
#[derive(Debug, Clone, Serialize)]
pub struct ProblemTypeEntry {
    pub problem_type: String,
    pub count: usize,
    pub percentage: f64,
}

/// Trend data point (per-week aggregation)
#[derive(Debug, Clone, Serialize)]
pub struct TrendPoint {
    pub week: String,
    pub sessions: usize,
    pub success_rate: f64,
    pub avg_turns: f64,
}

/// Full analytics output
#[derive(Debug, Serialize)]
pub struct AnalyticsOutput {
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub total_sessions: usize,
    pub overall_success_rate: f64,
    pub overall_avg_turns: f64,
    pub overall_avg_tokens: f64,
    pub agents: Vec<AgentMetrics>,
    pub problem_types: Vec<ProblemTypeEntry>,
    pub trends: Vec<TrendPoint>,
    pub estimated_total_cost: f64,
    pub computed_at: DateTime<Utc>,
}

/// Classify a session's problem type based on content, fingerprints, and file paths.
///
/// Returns (primary_type, secondary_type).
fn classify_problem_type(
    content: &str,
    error_fingerprints: &[String],
    file_paths: &[String],
) -> (ProblemType, Option<ProblemType>) {
    let has_errors = !error_fingerprints.is_empty();
    let has_config_files = file_paths.iter().any(|f| is_config_file(f));
    let has_doc_files = file_paths.iter().any(|f| is_doc_file(f));

    let content_lower = content.to_lowercase();

    // Score each type
    let mut scores: HashMap<ProblemType, i32> = HashMap::new();

    // Debug: error fingerprints present, or matches debug patterns
    if has_errors {
        *scores.entry(ProblemType::Debug).or_insert(0) += 3;
    }
    *scores.entry(ProblemType::Debug).or_insert(0) +=
        DEBUG_PATTERN.find_iter(&content_lower).count() as i32;

    // Feature: matches feature patterns
    *scores.entry(ProblemType::Feature).or_insert(0) +=
        FEATURE_PATTERN.find_iter(&content_lower).count() as i32;

    // Refactor: matches refactor patterns (and no config/doc files as primary signal)
    if REFACTOR_PATTERN.is_match(&content_lower) {
        *scores.entry(ProblemType::Refactor).or_insert(0) += 2;
    }

    // Investigation: matches investigation patterns
    if INVESTIGATION_PATTERN.is_match(&content_lower) {
        *scores.entry(ProblemType::Investigation).or_insert(0) += 2;
    }

    // Configuration: config files touched
    if has_config_files {
        *scores.entry(ProblemType::Configuration).or_insert(0) += 3;
    }

    // Documentation: doc files touched or matches doc patterns
    if has_doc_files {
        *scores.entry(ProblemType::Documentation).or_insert(0) += 3;
    }
    *scores.entry(ProblemType::Documentation).or_insert(0) +=
        DOCUMENTATION_PATTERN.find_iter(&content_lower).count() as i32;

    // Find primary and secondary by score
    let mut ranked: Vec<(ProblemType, i32)> = scores.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1));

    let primary = ranked.first().map(|(t, _)| *t).unwrap_or(ProblemType::Feature);
    let secondary = ranked.get(1).filter(|(_, s)| *s > 0).map(|(t, _)| *t);

    (primary, secondary)
}

fn is_config_file(path: &str) -> bool {
    if CONFIG_FILENAMES.iter().any(|n| path.ends_with(n) || path.contains(n)) {
        return true;
    }
    if let Some(ext) = std::path::Path::new(path).extension().and_then(|e| e.to_str()) {
        CONFIG_EXTENSIONS.contains(&ext)
    } else {
        false
    }
}

fn is_doc_file(path: &str) -> bool {
    if let Some(ext) = std::path::Path::new(path).extension().and_then(|e| e.to_str()) {
        DOC_EXTENSIONS.contains(&ext)
    } else {
        false
    }
}

/// Extract session data from a Tantivy document
pub(crate) fn extract_session_data(
    searcher: &Searcher,
    doc_addr: DocAddress,
    fields: &crate::index::IndexFields,
) -> Option<SessionData> {
    let doc: TantivyDocument = searcher.doc(doc_addr).ok()?;

    // Only process session documents, not code_artifacts
    let doc_type = doc
        .get_first(fields.doc_type)
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if doc_type != "session" {
        return None;
    }

    let session_id = doc
        .get_first(fields.session_id)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let source_agent = doc
        .get_first(fields.source_agent)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let project = doc
        .get_first(fields.project)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let timestamp = doc
        .get_first(fields.timestamp)
        .and_then(|v| v.as_datetime())
        .map(|dt| {
            DateTime::from_timestamp(dt.into_timestamp_secs(), 0).unwrap_or_default()
        })?;

    let turns = doc.get_first(fields.turn_count).and_then(|v| v.as_u64()).unwrap_or(0);

    let outcome = doc
        .get_first(fields.outcome)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let model = doc
        .get_first(fields.model)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let error_fingerprints: Vec<String> = doc
        .get_all(fields.error_fingerprint)
        .filter_map(|v| v.as_str())
        .map(|s| s.to_string())
        .collect();

    let file_paths: Vec<String> = doc
        .get_all(fields.file_paths)
        .filter_map(|v| v.as_str())
        .map(|s| s.to_string())
        .collect();

    let content_length = doc
        .get_first(fields.content)
        .and_then(|v| v.as_str())
        .map(|s| s.len())
        .unwrap_or(0);

    let content_text = doc
        .get_first(fields.content)
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let (primary_type, secondary_type) =
        classify_problem_type(content_text, &error_fingerprints, &file_paths);

    Some(SessionData {
        session_id,
        source_agent,
        project,
        timestamp,
        turns,
        outcome,
        model,
        error_fingerprints,
        file_paths,
        content_length,
        primary_type,
        secondary_type,
    })
}

/// Estimate tokens from content length
pub fn estimate_tokens(content_length: usize) -> f64 {
    content_length as f64 / CHARS_PER_TOKEN
}

/// Estimate cost for a session based on model pricing config
pub fn estimate_cost(
    estimated_tokens: f64,
    model: Option<&str>,
    cost_config: &crate::config::CostConfig,
) -> f64 {
    let model_name = match model {
        Some(m) if !m.is_empty() => m,
        _ => return 0.0,
    };

    // Try exact match first
    if let Some(pricing) = cost_config.models.get(model_name) {
        // Assume 50/50 input/output split
        let half_tokens = estimated_tokens / 2.0;
        let input_cost = (half_tokens / 1_000_000.0) * pricing.input_per_1m;
        let output_cost = (half_tokens / 1_000_000.0) * pricing.output_per_1m;
        return input_cost + output_cost;
    }

    // Try prefix match (e.g., "claude-sonnet-4" matching "claude-sonnet-4-20250514")
    for (key, pricing) in &cost_config.models {
        if model_name.starts_with(key.as_str()) || key.starts_with(model_name) {
            let half_tokens = estimated_tokens / 2.0;
            let input_cost = (half_tokens / 1_000_000.0) * pricing.input_per_1m;
            let output_cost = (half_tokens / 1_000_000.0) * pricing.output_per_1m;
            return input_cost + output_cost;
        }
    }

    0.0
}

/// Compute analytics from the Tantivy index
pub fn compute_analytics(
    data_dir: &Path,
    opts: &AnalyticsOptions,
    config: &Config,
) -> Result<AnalyticsOutput> {
    let index = open_index(data_dir)?;
    let reader = index.reader().map_err(|e| {
        AgentScribeError::DataDir(format!("Failed to create index reader: {}", e))
    })?;
    let searcher = reader.searcher();
    let total_docs = searcher.num_docs();

    if total_docs == 0 {
        return Ok(AnalyticsOutput {
            period_start: Utc::now(),
            period_end: Utc::now(),
            total_sessions: 0,
            overall_success_rate: 0.0,
            overall_avg_turns: 0.0,
            overall_avg_tokens: 0.0,
            agents: vec![],
            problem_types: vec![],
            trends: vec![],
            estimated_total_cost: 0.0,
            computed_at: Utc::now(),
        });
    }

    let (_schema, fields) = build_schema();

    // Fetch all session documents using MatchAllQuery
    let all_docs = searcher
        .search(&AllQuery, &TopDocs::with_limit(total_docs as usize))
        .map_err(|e| AgentScribeError::DataDir(format!("Search failed: {}", e)))?;

    // Extract session data with filtering
    let mut sessions: Vec<SessionData> = Vec::new();
    for (_score, doc_addr) in all_docs {
        if let Some(data) = extract_session_data(&searcher, doc_addr, &fields) {
            // Apply filters
            if let Some(ref agent_filter) = opts.agent {
                if &data.source_agent != agent_filter {
                    continue;
                }
            }
            if let Some(ref project_filter) = opts.project {
                if data.project.as_deref() != Some(project_filter.as_str()) {
                    continue;
                }
            }
            if let Some(since) = opts.since {
                if data.timestamp < since {
                    continue;
                }
            }
            sessions.push(data);
        }
    }

    if sessions.is_empty() {
        return Ok(AnalyticsOutput {
            period_start: opts.since.unwrap_or_else(Utc::now),
            period_end: Utc::now(),
            total_sessions: 0,
            overall_success_rate: 0.0,
            overall_avg_turns: 0.0,
            overall_avg_tokens: 0.0,
            agents: vec![],
            problem_types: vec![],
            trends: vec![],
            estimated_total_cost: 0.0,
            computed_at: Utc::now(),
        });
    }

    // Determine period
    let period_start = sessions
        .iter()
        .map(|s| s.timestamp)
        .min()
        .unwrap_or_else(Utc::now);
    let period_end = sessions
        .iter()
        .map(|s| s.timestamp)
        .max()
        .unwrap_or_else(Utc::now);

    // Overall metrics
    let total_sessions = sessions.len();
    let success_count = sessions
        .iter()
        .filter(|s| s.outcome.as_deref() == Some("success"))
        .count();
    let _failure_count = sessions
        .iter()
        .filter(|s| s.outcome.as_deref() == Some("failure"))
        .count();
    let _abandoned_count = sessions
        .iter()
        .filter(|s| s.outcome.as_deref() == Some("abandoned"))
        .count();

    let overall_success_rate = if total_sessions > 0 {
        success_count as f64 / total_sessions as f64 * 100.0
    } else {
        0.0
    };

    let overall_avg_turns = if total_sessions > 0 {
        sessions.iter().map(|s| s.turns).sum::<u64>() as f64 / total_sessions as f64
    } else {
        0.0
    };

    let overall_avg_tokens = if total_sessions > 0 {
        sessions
            .iter()
            .map(|s| estimate_tokens(s.content_length))
            .sum::<f64>()
            / total_sessions as f64
    } else {
        0.0
    };

    // Per-agent metrics
    let mut agent_map: HashMap<String, Vec<&SessionData>> = HashMap::new();
    for session in &sessions {
        agent_map
            .entry(session.source_agent.clone())
            .or_default()
            .push(session);
    }

    let cost_config = &config.cost;

    let mut agents: Vec<AgentMetrics> = agent_map
        .into_iter()
        .map(|(agent_name, agent_sessions)| {
            let total = agent_sessions.len();
            let successes = agent_sessions
                .iter()
                .filter(|s| s.outcome.as_deref() == Some("success"))
                .count();
            let failures = agent_sessions
                .iter()
                .filter(|s| s.outcome.as_deref() == Some("failure"))
                .count();
            let abandoned = agent_sessions
                .iter()
                .filter(|s| s.outcome.as_deref() == Some("abandoned"))
                .count();
            let unknown = total - successes - failures - abandoned;

            let success_rate = if total > 0 {
                successes as f64 / total as f64 * 100.0
            } else {
                0.0
            };

            let avg_turns_all = if total > 0 {
                agent_sessions.iter().map(|s| s.turns).sum::<u64>() as f64 / total as f64
            } else {
                0.0
            };

            let avg_turns_success = if successes > 0 {
                agent_sessions
                    .iter()
                    .filter(|s| s.outcome.as_deref() == Some("success"))
                    .map(|s| s.turns)
                    .sum::<u64>() as f64
                    / successes as f64
            } else {
                0.0
            };

            let avg_tokens_success = if successes > 0 {
                agent_sessions
                    .iter()
                    .filter(|s| s.outcome.as_deref() == Some("success"))
                    .map(|s| estimate_tokens(s.content_length))
                    .sum::<f64>()
                    / successes as f64
            } else {
                0.0
            };

            // Specialization by problem type
            let mut specialization: HashMap<String, usize> = HashMap::new();
            for session in &agent_sessions {
                *specialization
                    .entry(session.primary_type.as_str().to_string())
                    .or_insert(0) += 1;
            }

            // Cost estimation
            let estimated_cost: f64 = agent_sessions
                .iter()
                .map(|s| {
                    estimate_cost(
                        estimate_tokens(s.content_length),
                        s.model.as_deref(),
                        cost_config,
                    )
                })
                .sum();

            let cost_per_success = if successes > 0 {
                estimated_cost / successes as f64
            } else {
                0.0
            };

            AgentMetrics {
                agent: agent_name,
                total_sessions: total,
                success_count: successes,
                failure_count: failures,
                abandoned_count: abandoned,
                unknown_count: unknown,
                success_rate,
                avg_turns_success,
                avg_turns_all,
                avg_tokens_success,
                specialization,
                estimated_cost,
                cost_per_success,
            }
        })
        .collect();

    agents.sort_by(|a, b| b.total_sessions.cmp(&a.total_sessions));

    // Problem type distribution
    let mut type_counts: HashMap<ProblemType, usize> = HashMap::new();
    for session in &sessions {
        *type_counts.entry(session.primary_type).or_insert(0) += 1;
    }

    let mut problem_types: Vec<ProblemTypeEntry> = type_counts
        .into_iter()
        .map(|(pt, count)| ProblemTypeEntry {
            problem_type: pt.as_str().to_string(),
            count,
            percentage: count as f64 / total_sessions as f64 * 100.0,
        })
        .collect();
    problem_types.sort_by(|a, b| b.count.cmp(&a.count));

    // Trends (weekly buckets)
    let trends = compute_weekly_trends(&sessions);

    // Total cost
    let estimated_total_cost: f64 = sessions
        .iter()
        .map(|s| {
            estimate_cost(
                estimate_tokens(s.content_length),
                s.model.as_deref(),
                cost_config,
            )
        })
        .sum();

    Ok(AnalyticsOutput {
        period_start,
        period_end,
        total_sessions,
        overall_success_rate,
        overall_avg_turns,
        overall_avg_tokens,
        agents,
        problem_types,
        trends,
        estimated_total_cost,
        computed_at: Utc::now(),
    })
}

/// Compute weekly trend data
fn compute_weekly_trends(sessions: &[SessionData]) -> Vec<TrendPoint> {
    if sessions.is_empty() {
        return vec![];
    }

    // Find the Monday of the earliest session's week
    let earliest = sessions.iter().map(|s| s.timestamp).min().unwrap();
    let latest = sessions.iter().map(|s| s.timestamp).max().unwrap();

    // Align to Monday
    // Get the weekday (Mon=1, Tue=2, ..., Sun=7) using format
    let weekday_str = earliest.format("%u").to_string();
    let weekday: u32 = weekday_str.parse().unwrap_or(1);
    let days_from_monday = if weekday >= 1 { (weekday - 1) as i64 } else { 0 };
    let mut current_monday = earliest - Duration::days(days_from_monday);
    // Set to midnight using naive date
    let naive_monday = current_monday.date_naive().and_hms_opt(0, 0, 0).unwrap();
    current_monday = naive_monday.and_utc();

    let mut trends = Vec::new();

    while current_monday <= latest {
        let next_monday = current_monday + Duration::weeks(1);

        let week_sessions: Vec<&SessionData> = sessions
            .iter()
            .filter(|s| s.timestamp >= current_monday && s.timestamp < next_monday)
            .collect();

        if !week_sessions.is_empty() {
            let week_total = week_sessions.len();
            let week_successes = week_sessions
                .iter()
                .filter(|s| s.outcome.as_deref() == Some("success"))
                .count();
            let week_avg_turns = week_sessions.iter().map(|s| s.turns).sum::<u64>() as f64
                / week_total as f64;
            let week_success_rate = week_successes as f64 / week_total as f64 * 100.0;

            trends.push(TrendPoint {
                week: current_monday.format("%Y-%m-%d").to_string(),
                sessions: week_total,
                success_rate: week_success_rate,
                avg_turns: week_avg_turns,
            });
        }

        current_monday = next_monday;
    }

    trends
}

/// Format analytics as human-readable output
pub fn format_human(output: &AnalyticsOutput) -> String {
    let mut lines = Vec::new();

    if output.total_sessions == 0 {
        lines.push("No sessions found for analytics.".to_string());
        return lines.join("\n");
    }

    // Header
    lines.push(format!(
        "Agent Analytics ({} sessions, {} to {})\n",
        output.total_sessions,
        output.period_start.format("%Y-%m-%d"),
        output.period_end.format("%Y-%m-%d"),
    ));

    // Overall summary
    lines.push("Overall Summary".to_string());
    lines.push("---------------".to_string());
    lines.push(format!(
        "  Success rate:    {:.1}%",
        output.overall_success_rate,
    ));
    lines.push(format!(
        "  Avg turns:       {:.1}",
        output.overall_avg_turns,
    ));
    lines.push(format!(
        "  Avg tokens:      {:.0} (estimated)",
        output.overall_avg_tokens,
    ));
    if output.estimated_total_cost > 0.0 {
        lines.push(format!(
            "  Est. total cost: ${:.2}",
            output.estimated_total_cost,
        ));
    }
    lines.push(String::new());

    // Per-agent table
    lines.push("Agent Performance".to_string());
    lines.push("-----------------".to_string());
    if output.agents.is_empty() {
        lines.push("  (no data)".to_string());
    } else {
        for agent in &output.agents {
            lines.push(format!(
                "  {:<16} {:>4} sessions  success: {:>5.1}%  avg turns (success): {:>5.1}  avg tokens (success): {:>7.0}",
                agent.agent,
                agent.total_sessions,
                agent.success_rate,
                agent.avg_turns_success,
                agent.avg_tokens_success,
            ));
            if agent.estimated_cost > 0.0 {
                lines.push(format!(
                "  {:<16} failures: {:>3}  abandoned: {:>3}  cost: ${:>8.2}  cost/success: ${:>7.2}",
                "", agent.failure_count, agent.abandoned_count,
                agent.estimated_cost, agent.cost_per_success,
            ));
            } else {
                lines.push(format!(
                    "  {:<16} failures: {:>3}  abandoned: {:>3}",
                    "",
                    agent.failure_count,
                    agent.abandoned_count,
                ));
            }
            // Top specialization
            let mut spec: Vec<_> = agent.specialization.iter().collect();
            spec.sort_by(|a, b| b.1.cmp(a.1));
            if !spec.is_empty() {
                let top_spec: Vec<String> = spec
                    .iter()
                    .take(3)
                    .map(|(t, c)| format!("{}:{}", t, c))
                    .collect();
                lines.push(format!("  {:<16} top types: {}", "", top_spec.join(", ")));
            }
            lines.push(String::new());
        }
    }

    // Problem type distribution
    lines.push("Problem Type Distribution".to_string());
    lines.push("------------------------".to_string());
    if output.problem_types.is_empty() {
        lines.push("  (no data)".to_string());
    } else {
        for pt in &output.problem_types {
            let bar_len = (pt.percentage / 5.0).round() as usize;
            let bar: String = "#".repeat(bar_len.max(1));
            lines.push(format!(
                "  {:<16} {:>4} ({:>5.1}%)  {}",
                pt.problem_type, pt.count, pt.percentage, bar,
            ));
        }
    }
    lines.push(String::new());

    // Trends
    if !output.trends.is_empty() {
        lines.push("Weekly Trends".to_string());
        lines.push("-------------".to_string());
        for trend in &output.trends {
            lines.push(format!(
                "  {}  {:>3} sessions  success: {:>5.1}%  avg turns: {:>5.1}",
                trend.week, trend.sessions, trend.success_rate, trend.avg_turns,
            ));
        }
        lines.push(String::new());
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Event, Role, SessionManifest};
    use crate::index::build_session_document;
    use tempfile::TempDir;

    fn make_test_session(
        session_id: &str,
        agent: &str,
        project: &str,
        outcome: &str,
        turns: u32,
        content: &str,
        error_fps: Vec<&str>,
        file_paths: Vec<&str>,
        model: Option<&str>,
    ) -> (SessionManifest, Vec<Event>) {
        let now = Utc::now();
        let mut manifest = SessionManifest::new(session_id.to_string(), agent.to_string());
        manifest.project = Some(project.to_string());
        manifest.started = now;
        manifest.turns = turns;
        manifest.outcome = Some(outcome.to_string());
        manifest.model = model.map(|s| s.to_string());
        manifest.files_touched = file_paths.iter().map(|s| s.to_string()).collect();

        let events = vec![Event::new(now, session_id.to_string(), agent.to_string(), Role::User, content.to_string())
            .with_error_fingerprints(error_fps.iter().map(|s| s.to_string()).collect())];

        (manifest, events)
    }

    fn build_test_index(
        sessions: Vec<(SessionManifest, Vec<Event>)>,
    ) -> (TempDir, crate::index::IndexFields) {
        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");

        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        for (manifest, events) in &sessions {
            let doc = build_session_document(&fields, events, manifest);
            writer.add_document(doc).unwrap();
        }

        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        (temp_dir, fields)
    }

    #[test]
    fn test_classify_debug() {
        let (primary, secondary) = classify_problem_type(
            "fix the bug error crash",
            &["ErrorType:connection refused".to_string()],
            &[],
        );
        assert_eq!(primary, ProblemType::Debug);
    }

    #[test]
    fn test_classify_feature() {
        let (primary, _) = classify_problem_type(
            "implement a new feature to add support for websockets",
            &[],
            &[],
        );
        assert_eq!(primary, ProblemType::Feature);
    }

    #[test]
    fn test_classify_refactor() {
        let (primary, _) = classify_problem_type(
            "refactor the code to extract the helper method and clean up",
            &[],
            &[],
        );
        assert_eq!(primary, ProblemType::Refactor);
    }

    #[test]
    fn test_classify_investigation() {
        let (primary, _) = classify_problem_type(
            "explain how does the authentication work",
            &[],
            &[],
        );
        assert_eq!(primary, ProblemType::Investigation);
    }

    #[test]
    fn test_classify_configuration() {
        let (primary, _) = classify_problem_type(
            "update the settings",
            &[],
            &["config.toml".to_string(), "docker-compose.yml".to_string()],
        );
        assert_eq!(primary, ProblemType::Configuration);
    }

    #[test]
    fn test_classify_documentation() {
        let (primary, _) = classify_problem_type(
            "update the readme",
            &[],
            &["docs/README.md".to_string()],
        );
        assert_eq!(primary, ProblemType::Documentation);
    }

    #[test]
    fn test_classify_secondary_type() {
        let (primary, secondary) = classify_problem_type(
            "fix the bug in the config.toml file",
            &["ErrorType:something".to_string()],
            &["config.toml".to_string()],
        );
        // Debug has error fingerprints (weight 3) + pattern match
        // Configuration has config file (weight 3)
        // Debug may win due to both error fps and pattern match
        assert!(matches!(primary, ProblemType::Debug | ProblemType::Configuration));
        assert!(secondary.is_some());
    }

    #[test]
    fn test_is_config_file() {
        assert!(is_config_file("config.toml"));
        assert!(is_config_file("settings.yaml"));
        assert!(is_config_file(".env"));
        assert!(is_config_file("Dockerfile"));
        assert!(is_config_file("docker-compose.yml"));
        assert!(!is_config_file("src/main.rs"));
    }

    #[test]
    fn test_is_doc_file() {
        assert!(is_doc_file("README.md"));
        assert!(is_doc_file("docs/CHANGELOG.md"));
        assert!(is_doc_file("guide.rst"));
        assert!(!is_doc_file("src/main.rs"));
    }

    #[test]
    fn test_analytics_empty_index() {
        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");
        let (schema, _) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        tantivy::Index::create_in_dir(&index_path, schema).unwrap();

        let config = Config::default();
        let opts = AnalyticsOptions {
            agent: None,
            project: None,
            since: None,
        };

        let output = compute_analytics(temp_dir.path(), &opts, &config).unwrap();
        assert_eq!(output.total_sessions, 0);
        assert!(output.agents.is_empty());
    }

    #[test]
    fn test_analytics_single_agent() {
        let sessions = vec![
            make_test_session("claude/1", "claude-code", "/proj", "success", 10, "fix the bug", vec!["Err:X"], vec!["src/main.rs"], Some("claude-sonnet-4")),
            make_test_session("claude/2", "claude-code", "/proj", "failure", 5, "tried to fix", vec!["Err:Y"], vec!["src/main.rs"], Some("claude-sonnet-4")),
            make_test_session("claude/3", "claude-code", "/proj", "success", 8, "added feature", vec![], vec!["src/new.rs"], Some("claude-sonnet-4")),
        ];

        let (temp_dir, _) = build_test_index(sessions);
        let config = Config::default();
        let opts = AnalyticsOptions {
            agent: None,
            project: None,
            since: None,
        };

        let output = compute_analytics(temp_dir.path(), &opts, &config).unwrap();
        assert_eq!(output.total_sessions, 3);
        assert_eq!(output.agents.len(), 1);
        assert_eq!(output.agents[0].agent, "claude-code");
        assert_eq!(output.agents[0].total_sessions, 3);
        assert_eq!(output.agents[0].success_count, 2);
        assert_eq!(output.agents[0].failure_count, 1);
        assert!((output.agents[0].success_rate - 66.7).abs() < 0.1);
    }

    #[test]
    fn test_analytics_multi_agent() {
        let sessions = vec![
            make_test_session("claude/1", "claude-code", "/proj", "success", 10, "fixed bug", vec!["Err:X"], vec![], None),
            make_test_session("aider/1", "aider", "/proj", "success", 5, "added feature", vec![], vec![], None),
            make_test_session("aider/2", "aider", "/proj", "failure", 20, "failed to fix", vec!["Err:Y"], vec![], None),
        ];

        let (temp_dir, _) = build_test_index(sessions);
        let config = Config::default();
        let opts = AnalyticsOptions {
            agent: None,
            project: None,
            since: None,
        };

        let output = compute_analytics(temp_dir.path(), &opts, &config).unwrap();
        assert_eq!(output.total_sessions, 3);
        assert_eq!(output.agents.len(), 2);
    }

    #[test]
    fn test_analytics_agent_filter() {
        let sessions = vec![
            make_test_session("claude/1", "claude-code", "/proj", "success", 10, "fixed", vec![], vec![], None),
            make_test_session("aider/1", "aider", "/proj", "success", 5, "added", vec![], vec![], None),
        ];

        let (temp_dir, _) = build_test_index(sessions);
        let config = Config::default();
        let opts = AnalyticsOptions {
            agent: Some("aider".to_string()),
            project: None,
            since: None,
        };

        let output = compute_analytics(temp_dir.path(), &opts, &config).unwrap();
        assert_eq!(output.total_sessions, 1);
        assert_eq!(output.agents[0].agent, "aider");
    }

    #[test]
    fn test_analytics_problem_types() {
        let sessions = vec![
            make_test_session("s/1", "test", "/p", "success", 5, "fix the bug", vec!["Err:A"], vec![], None),
            make_test_session("s/2", "test", "/p", "success", 5, "fix the bug", vec!["Err:B"], vec![], None),
            make_test_session("s/3", "test", "/p", "success", 5, "implement new feature", vec![], vec![], None),
            make_test_session("s/4", "test", "/p", "success", 5, "update the readme", vec![], vec!["README.md"], None),
        ];

        let (temp_dir, _) = build_test_index(sessions);
        let config = Config::default();
        let opts = AnalyticsOptions {
            agent: None,
            project: None,
            since: None,
        };

        let output = compute_analytics(temp_dir.path(), &opts, &config).unwrap();
        assert_eq!(output.problem_types.len(), 3);

        // Debug should have count 2
        let debug_entry = output.problem_types.iter().find(|p| p.problem_type == "debug");
        assert!(debug_entry.is_some());
        assert_eq!(debug_entry.unwrap().count, 2);
    }

    #[test]
    fn test_analytics_cost_with_pricing() {
        let sessions = vec![
            make_test_session("s/1", "test", "/p", "success", 5, &"x".repeat(4000), vec![], vec![], Some("claude-sonnet-4")),
        ];

        let (temp_dir, _) = build_test_index(sessions);

        let mut config = Config::default();
        let mut models = std::collections::HashMap::new();
        models.insert(
            "claude-sonnet-4".to_string(),
            crate::config::ModelPricing {
                input_per_1m: 3.0,
                output_per_1m: 15.0,
            },
        );
        config.cost.models = models;

        let opts = AnalyticsOptions {
            agent: None,
            project: None,
            since: None,
        };

        let output = compute_analytics(temp_dir.path(), &opts, &config).unwrap();
        assert!(output.estimated_total_cost > 0.0);
        assert!(output.agents[0].estimated_cost > 0.0);
    }

    #[test]
    fn test_analytics_cost_null_model_excluded() {
        let sessions = vec![
            make_test_session("s/1", "test", "/p", "success", 5, "content", vec![], vec![], None),
        ];

        let (temp_dir, _) = build_test_index(sessions);

        let mut config = Config::default();
        let mut models = std::collections::HashMap::new();
        models.insert(
            "claude-sonnet-4".to_string(),
            crate::config::ModelPricing {
                input_per_1m: 3.0,
                output_per_1m: 15.0,
            },
        );
        config.cost.models = models;

        let opts = AnalyticsOptions {
            agent: None,
            project: None,
            since: None,
        };

        let output = compute_analytics(temp_dir.path(), &opts, &config).unwrap();
        assert_eq!(output.estimated_total_cost, 0.0);
    }

    #[test]
    fn test_format_human() {
        let output = AnalyticsOutput {
            period_start: Utc::now() - Duration::days(30),
            period_end: Utc::now(),
            total_sessions: 10,
            overall_success_rate: 70.0,
            overall_avg_turns: 8.5,
            overall_avg_tokens: 2000.0,
            agents: vec![AgentMetrics {
                agent: "claude-code".to_string(),
                total_sessions: 10,
                success_count: 7,
                failure_count: 2,
                abandoned_count: 1,
                unknown_count: 0,
                success_rate: 70.0,
                avg_turns_success: 7.0,
                avg_turns_all: 8.5,
                avg_tokens_success: 1800.0,
                specialization: HashMap::from([
                    ("debug".to_string(), 4),
                    ("feature".to_string(), 3),
                ]),
                estimated_cost: 1.50,
                cost_per_success: 0.21,
            }],
            problem_types: vec![
                ProblemTypeEntry {
                    problem_type: "debug".to_string(),
                    count: 5,
                    percentage: 50.0,
                },
                ProblemTypeEntry {
                    problem_type: "feature".to_string(),
                    count: 3,
                    percentage: 30.0,
                },
                ProblemTypeEntry {
                    problem_type: "investigation".to_string(),
                    count: 2,
                    percentage: 20.0,
                },
            ],
            trends: vec![
                TrendPoint {
                    week: "2026-02-23".to_string(),
                    sessions: 4,
                    success_rate: 75.0,
                    avg_turns: 8.0,
                },
                TrendPoint {
                    week: "2026-03-16".to_string(),
                    sessions: 6,
                    success_rate: 66.7,
                    avg_turns: 9.0,
                },
            ],
            estimated_total_cost: 1.50,
            computed_at: Utc::now(),
        };

        let formatted = format_human(&output);
        assert!(formatted.contains("Agent Analytics"));
        assert!(formatted.contains("Overall Summary"));
        assert!(formatted.contains("70.0%"));
        assert!(formatted.contains("Agent Performance"));
        assert!(formatted.contains("claude-code"));
        assert!(formatted.contains("Problem Type Distribution"));
        assert!(formatted.contains("debug"));
        assert!(formatted.contains("Weekly Trends"));
    }

    #[test]
    fn test_format_human_empty() {
        let output = AnalyticsOutput {
            period_start: Utc::now(),
            period_end: Utc::now(),
            total_sessions: 0,
            overall_success_rate: 0.0,
            overall_avg_turns: 0.0,
            overall_avg_tokens: 0.0,
            agents: vec![],
            problem_types: vec![],
            trends: vec![],
            estimated_total_cost: 0.0,
            computed_at: Utc::now(),
        };

        let formatted = format_human(&output);
        assert!(formatted.contains("No sessions found"));
    }
}
