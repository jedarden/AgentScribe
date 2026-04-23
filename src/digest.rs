//! Weekly digest command
//!
//! Generates an automated activity summary over a configurable period.
//! Combines session counts, recurring problems, agent comparison, most-touched
//! files, error patterns, and token/cost estimation into a skimmable markdown report.

use crate::analytics::{self, AnalyticsOptions, AnalyticsOutput};
use crate::config::Config;
use crate::error::Result;
use crate::recurring::{self, RecurringOptions};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

/// Digest generation options
pub struct DigestOptions {
    /// Only include sessions after this timestamp
    pub since: DateTime<Utc>,
    /// Path to write output (stdout if None)
    #[allow(dead_code)]
    pub output: Option<String>,
    /// JSON output mode
    #[allow(dead_code)]
    pub json: bool,
}

/// A file ranked by how many sessions touched it
#[derive(Debug, Clone, Serialize)]
pub struct TouchedFile {
    pub path: String,
    pub session_count: usize,
}

/// Per-model token/cost summary
#[derive(Debug, Clone, Serialize)]
pub struct ModelCostEntry {
    pub model: String,
    pub sessions: usize,
    pub estimated_tokens: f64,
    pub estimated_cost: f64,
}

/// New error pattern discovered in the period
#[derive(Debug, Clone, Serialize)]
pub struct NewErrorPattern {
    pub fingerprint: String,
    pub first_seen: DateTime<Utc>,
    pub occurrences: usize,
}

/// Full digest output (structured)
#[derive(Debug, Serialize)]
pub struct DigestOutput {
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub total_sessions: usize,
    pub sessions_by_agent: HashMap<String, usize>,
    pub sessions_by_project: HashMap<String, usize>,
    pub analytics: AnalyticsOutput,
    pub recurring_problems: Vec<recurring::RecurringProblem>,
    pub most_touched_files: Vec<TouchedFile>,
    pub new_error_patterns: Vec<NewErrorPattern>,
    pub model_costs: Vec<ModelCostEntry>,
    pub estimated_total_cost: f64,
    pub estimated_total_tokens: f64,
    pub computed_at: DateTime<Utc>,
}

/// Generate the digest report
pub fn generate_digest(
    data_dir: &Path,
    opts: &DigestOptions,
    config: &Config,
) -> Result<DigestOutput> {
    // Compute analytics for the period
    let analytics_opts = AnalyticsOptions {
        agent: None,
        project: None,
        since: Some(opts.since),
    };
    let analytics = analytics::compute_analytics(data_dir, &analytics_opts, config)?;

    // Detect recurring problems
    let recurring_opts = RecurringOptions {
        since: opts.since,
        threshold: 2,
    };
    let recurring_output = recurring::detect_recurring(data_dir, &recurring_opts)?;

    // Extract additional digest data from the analytics session data
    let (
        sessions_by_agent,
        sessions_by_project,
        most_touched_files,
        new_error_patterns,
        model_costs,
        total_tokens,
        total_cost,
    ) = extract_digest_data(data_dir, &opts.since, config)?;

    let period_start = opts.since;
    let period_end = Utc::now();

    Ok(DigestOutput {
        period_start,
        period_end,
        total_sessions: analytics.total_sessions,
        sessions_by_agent,
        sessions_by_project,
        analytics,
        recurring_problems: recurring_output.problems,
        most_touched_files,
        new_error_patterns,
        model_costs,
        estimated_total_cost: total_cost,
        estimated_total_tokens: total_tokens,
        computed_at: Utc::now(),
    })
}

type DigestDataTuple = (
    HashMap<String, usize>,
    HashMap<String, usize>,
    Vec<TouchedFile>,
    Vec<NewErrorPattern>,
    Vec<ModelCostEntry>,
    f64,
    f64,
);

/// Extract digest-specific data by scanning the Tantivy index
fn extract_digest_data(
    data_dir: &Path,
    since: &DateTime<Utc>,
    config: &Config,
) -> Result<DigestDataTuple> {
    use crate::analytics::{estimate_cost, estimate_tokens};
    use crate::index::build_schema;
    use crate::search::open_index;
    use tantivy::collector::TopDocs;
    use tantivy::query::AllQuery;

    let index = open_index(data_dir)?;
    let reader = index.reader().map_err(|e| {
        crate::error::AgentScribeError::DataDir(format!("Failed to create index reader: {}", e))
    })?;
    let searcher = reader.searcher();
    let total_docs = searcher.num_docs();

    if total_docs == 0 {
        return Ok((
            HashMap::new(),
            HashMap::new(),
            vec![],
            vec![],
            vec![],
            0.0,
            0.0,
        ));
    }

    let (_schema, fields) = build_schema();

    let all_docs: Vec<_> = searcher
        .search(&AllQuery, &TopDocs::with_limit(total_docs as usize))
        .map_err(|e| crate::error::AgentScribeError::DataDir(format!("Search failed: {}", e)))?;

    let mut sessions_by_agent: HashMap<String, usize> = HashMap::new();
    let mut sessions_by_project: HashMap<String, usize> = HashMap::new();
    let mut file_counts: HashMap<String, usize> = HashMap::new();
    let mut error_first_seen: HashMap<String, DateTime<Utc>> = HashMap::new();
    let mut error_counts: HashMap<String, usize> = HashMap::new();
    let mut model_tokens: HashMap<String, (usize, f64, f64)> = HashMap::new(); // (sessions, tokens, cost)
    let mut total_tokens = 0.0;
    let mut total_cost = 0.0;

    for (_score, doc_addr) in all_docs {
        let data = crate::analytics::extract_session_data(&searcher, doc_addr, &fields);
        let data = match data {
            Some(d) => d,
            None => continue,
        };

        if data.timestamp < *since {
            continue;
        }

        // Sessions by agent
        *sessions_by_agent
            .entry(data.source_agent.clone())
            .or_insert(0) += 1;

        // Sessions by project
        if let Some(ref project) = data.project {
            *sessions_by_project.entry(project.clone()).or_insert(0) += 1;
        }

        // Most-touched files
        for fp in &data.file_paths {
            *file_counts.entry(fp.clone()).or_insert(0) += 1;
        }

        // New error patterns (first seen in this period)
        for err in &data.error_fingerprints {
            *error_counts.entry(err.clone()).or_insert(0) += 1;
            error_first_seen
                .entry(err.clone())
                .and_modify(|ts| {
                    if data.timestamp < *ts {
                        *ts = data.timestamp;
                    }
                })
                .or_insert(data.timestamp);
        }

        // Token/cost by model
        let tokens = estimate_tokens(data.content_length);
        let cost = estimate_cost(tokens, data.model.as_deref(), &config.cost);
        total_tokens += tokens;
        total_cost += cost;

        let model_key = data.model.clone().unwrap_or_else(|| "unknown".to_string());
        let entry = model_tokens.entry(model_key).or_insert((0, 0.0, 0.0));
        entry.0 += 1;
        entry.1 += tokens;
        entry.2 += cost;
    }

    // Build most-touched files (top 20)
    let mut touched_files: Vec<TouchedFile> = file_counts
        .into_iter()
        .map(|(path, session_count)| TouchedFile {
            path,
            session_count,
        })
        .collect();
    touched_files.sort_by_key(|b| std::cmp::Reverse(b.session_count));
    touched_files.truncate(20);

    // Build new error patterns (errors first seen in this period)
    let mut new_errors: Vec<NewErrorPattern> = error_counts
        .into_iter()
        .filter_map(|(fp, count)| {
            let first = error_first_seen.get(&fp)?;
            if *first >= *since {
                Some(NewErrorPattern {
                    fingerprint: fp,
                    first_seen: *first,
                    occurrences: count,
                })
            } else {
                None
            }
        })
        .collect();
    new_errors.sort_by_key(|b| std::cmp::Reverse(b.occurrences));

    // Build model cost entries
    let mut model_costs: Vec<ModelCostEntry> = model_tokens
        .into_iter()
        .map(|(model, (sessions, tokens, cost))| ModelCostEntry {
            model,
            sessions,
            estimated_tokens: tokens,
            estimated_cost: cost,
        })
        .collect();
    model_costs.sort_by(|a, b| b.estimated_cost.partial_cmp(&a.estimated_cost).unwrap());

    Ok((
        sessions_by_agent,
        sessions_by_project,
        touched_files,
        new_errors,
        model_costs,
        total_tokens,
        total_cost,
    ))
}

/// Format digest as skimmable markdown
pub fn format_markdown(output: &DigestOutput) -> String {
    let mut md = String::new();

    if output.total_sessions == 0 {
        md.push_str("# AgentScribe Digest\n\n");
        md.push_str(&format!(
            "**Period:** {} to {}\n\n",
            output.period_start.format("%Y-%m-%d"),
            output.period_end.format("%Y-%m-%d"),
        ));
        md.push_str("No sessions found in this period.\n");
        return md;
    }

    // Header
    md.push_str("# AgentScribe Digest\n\n");
    md.push_str(&format!(
        "**Period:** {} to {}  |  **{} sessions**\n\n",
        output.period_start.format("%Y-%m-%d"),
        output.period_end.format("%Y-%m-%d"),
        output.total_sessions,
    ));

    // Sessions by agent
    md.push_str("## Sessions by Agent\n\n");
    let mut agents: Vec<_> = output.sessions_by_agent.iter().collect();
    agents.sort_by(|a, b| b.1.cmp(a.1));
    for (agent, count) in &agents {
        let pct = **count as f64 / output.total_sessions as f64 * 100.0;
        md.push_str(&format!("- **{}**: {} ({:.0}%)\n", agent, count, pct));
    }
    md.push('\n');

    // Sessions by project
    if !output.sessions_by_project.is_empty() {
        md.push_str("## Sessions by Project\n\n");
        let mut projects: Vec<_> = output.sessions_by_project.iter().collect();
        projects.sort_by(|a, b| b.1.cmp(a.1));
        for (project, count) in &projects {
            let name = std::path::Path::new(project.as_str())
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(project.as_str());
            md.push_str(&format!("- **{}**: {}\n", name, count));
        }
        md.push('\n');
    }

    // Agent comparison table
    md.push_str("## Agent Comparison\n\n");
    if output.analytics.agents.is_empty() {
        md.push_str("(no agent data)\n\n");
    } else {
        md.push_str("| Agent | Sessions | Success | Fail | Abandoned | Avg Turns | Est. Cost |\n");
        md.push_str("|-------|----------|---------|------|-----------|-----------|----------|\n");
        for agent in &output.analytics.agents {
            let cost_str = if agent.estimated_cost > 0.0 {
                format!("${:.2}", agent.estimated_cost)
            } else {
                "-".to_string()
            };
            md.push_str(&format!(
                "| {} | {} | {:.1}% | {} | {} | {:.1} | {} |\n",
                agent.agent,
                agent.total_sessions,
                agent.success_rate,
                agent.failure_count,
                agent.abandoned_count,
                agent.avg_turns_all,
                cost_str,
            ));
        }
        md.push('\n');

        // Specialization notes
        for agent in &output.analytics.agents {
            let mut spec: Vec<_> = agent.specialization.iter().collect();
            spec.sort_by(|a, b| b.1.cmp(a.1));
            if !spec.is_empty() {
                let top: Vec<String> = spec
                    .iter()
                    .take(3)
                    .map(|(t, c)| format!("{}:{}", t, c))
                    .collect();
                md.push_str(&format!(
                    "- **{}** specializes in: {}\n",
                    agent.agent,
                    top.join(", "),
                ));
            }
        }
        md.push('\n');
    }

    // Recurring problems
    md.push_str("## Recurring Problems\n\n");
    if output.recurring_problems.is_empty() {
        md.push_str("No recurring problems detected.\n\n");
    } else {
        for (i, prob) in output.recurring_problems.iter().enumerate().take(10) {
            let fp_display = truncate_fingerprint(&prob.fingerprint, 60);
            md.push_str(&format!(
                "{}. **{}** ({} sessions)\n",
                i + 1,
                fp_display,
                prob.session_count,
            ));
            if !prob.projects.is_empty() {
                let proj_names: Vec<String> = prob
                    .projects
                    .iter()
                    .map(|p| {
                        std::path::Path::new(p)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(p)
                            .to_string()
                    })
                    .collect();
                md.push_str(&format!("   - Projects: {}\n", proj_names.join(", ")));
            }
            if !prob.fix_agents.is_empty() {
                md.push_str(&format!("   - Fixed by: {}\n", prob.fix_agents.join(", ")));
            } else {
                md.push_str("   - Status: unresolved\n");
            }
            md.push('\n');
        }
    }

    // Most-touched files
    md.push_str("## Most-Touched Files\n\n");
    if output.most_touched_files.is_empty() {
        md.push_str("(no file data)\n\n");
    } else {
        md.push_str("| File | Sessions |\n");
        md.push_str("|------|----------|\n");
        for file in &output.most_touched_files {
            md.push_str(&format!("| `{}` | {} |\n", file.path, file.session_count));
        }
        md.push('\n');
    }

    // New error patterns
    if !output.new_error_patterns.is_empty() {
        md.push_str("## New Error Patterns\n\n");
        for (i, err) in output.new_error_patterns.iter().enumerate().take(10) {
            let fp_display = truncate_fingerprint(&err.fingerprint, 60);
            md.push_str(&format!(
                "{}. **{}** — {} occurrence(s), first seen {}\n",
                i + 1,
                fp_display,
                err.occurrences,
                err.first_seen.format("%Y-%m-%d"),
            ));
        }
        md.push('\n');
    }

    // Token usage & costs
    md.push_str("## Token Usage & Costs\n\n");
    if output.model_costs.is_empty() {
        md.push_str("(no model/cost data)\n\n");
    } else {
        md.push_str("| Model | Sessions | Est. Tokens | Est. Cost |\n");
        md.push_str("|-------|----------|-------------|----------|\n");
        for mc in &output.model_costs {
            let cost_str = if mc.estimated_cost > 0.0 {
                format!("${:.2}", mc.estimated_cost)
            } else {
                "-".to_string()
            };
            md.push_str(&format!(
                "| {} | {} | {:.0} | {} |\n",
                mc.model, mc.sessions, mc.estimated_tokens, cost_str,
            ));
        }
        md.push('\n');
        md.push_str(&format!(
            "**Total estimated tokens:** {:.0}\n\n",
            output.estimated_total_tokens,
        ));
        if output.estimated_total_cost > 0.0 {
            md.push_str(&format!(
                "**Total estimated cost:** ${:.2}\n\n",
                output.estimated_total_cost,
            ));
        }
    }

    md
}

/// Format digest as JSON
pub fn format_json(output: &DigestOutput) -> String {
    serde_json::to_string_pretty(output).unwrap_or_else(|_| "{}".to_string())
}

/// Truncate a fingerprint for display
fn truncate_fingerprint(fp: &str, max_len: usize) -> String {
    if fp.len() <= max_len {
        return fp.to_string();
    }
    if let Some(colon_pos) = fp.find(':') {
        let prefix = &fp[..=colon_pos];
        let remaining = max_len.saturating_sub(prefix.len() + 4);
        if remaining > 10 {
            return format!(
                "{}{}...",
                prefix,
                &fp[colon_pos + 1..colon_pos + 1 + remaining]
            );
        }
    }
    format!("{}...", &fp[..max_len.saturating_sub(3)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_fingerprint() {
        assert_eq!(
            truncate_fingerprint("ErrorType:short", 60),
            "ErrorType:short"
        );
        assert_eq!(
            truncate_fingerprint("SomeVeryLongErrorType:this is a very long message", 40),
            "SomeVeryLongErrorType:this is a very..."
        );
    }

    #[test]
    fn test_format_markdown_empty() {
        let output = DigestOutput {
            period_start: Utc::now(),
            period_end: Utc::now(),
            total_sessions: 0,
            sessions_by_agent: HashMap::new(),
            sessions_by_project: HashMap::new(),
            analytics: AnalyticsOutput {
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
            },
            recurring_problems: vec![],
            most_touched_files: vec![],
            new_error_patterns: vec![],
            model_costs: vec![],
            estimated_total_cost: 0.0,
            estimated_total_tokens: 0.0,
            computed_at: Utc::now(),
        };

        let md = format_markdown(&output);
        assert!(md.contains("# AgentScribe Digest"));
        assert!(md.contains("No sessions found"));
    }

    #[test]
    fn test_format_markdown_with_data() {
        let mut sessions_by_agent = HashMap::new();
        sessions_by_agent.insert("claude-code".to_string(), 10);

        let mut sessions_by_project = HashMap::new();
        sessions_by_project.insert("/home/user/myproject".to_string(), 8);

        let output = DigestOutput {
            period_start: Utc::now() - chrono::Duration::days(7),
            period_end: Utc::now(),
            total_sessions: 10,
            sessions_by_agent,
            sessions_by_project,
            analytics: AnalyticsOutput {
                period_start: Utc::now() - chrono::Duration::days(7),
                period_end: Utc::now(),
                total_sessions: 10,
                overall_success_rate: 80.0,
                overall_avg_turns: 12.0,
                overall_avg_tokens: 5000.0,
                agents: vec![analytics::AgentMetrics {
                    agent: "claude-code".to_string(),
                    total_sessions: 10,
                    success_count: 8,
                    failure_count: 1,
                    abandoned_count: 1,
                    unknown_count: 0,
                    success_rate: 80.0,
                    avg_turns_success: 10.0,
                    avg_turns_all: 12.0,
                    avg_tokens_success: 4500.0,
                    specialization: HashMap::from([
                        ("debug".to_string(), 5),
                        ("feature".to_string(), 3),
                    ]),
                    estimated_cost: 2.50,
                    cost_per_success: 0.31,
                }],
                problem_types: vec![],
                trends: vec![],
                estimated_total_cost: 2.50,
                computed_at: Utc::now(),
            },
            recurring_problems: vec![],
            most_touched_files: vec![TouchedFile {
                path: "src/main.rs".to_string(),
                session_count: 5,
            }],
            new_error_patterns: vec![],
            model_costs: vec![ModelCostEntry {
                model: "claude-sonnet-4".to_string(),
                sessions: 10,
                estimated_tokens: 50000.0,
                estimated_cost: 2.50,
            }],
            estimated_total_cost: 2.50,
            estimated_total_tokens: 50000.0,
            computed_at: Utc::now(),
        };

        let md = format_markdown(&output);
        assert!(md.contains("# AgentScribe Digest"));
        assert!(md.contains("## Sessions by Agent"));
        assert!(md.contains("claude-code"));
        assert!(md.contains("## Agent Comparison"));
        assert!(md.contains("## Most-Touched Files"));
        assert!(md.contains("src/main.rs"));
        assert!(md.contains("## Token Usage & Costs"));
        assert!(md.contains("$2.50"));
        assert!(md.contains("specializes in"));
    }
}
