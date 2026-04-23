//! Quarterly Pulse Report — State of AI Coding
//!
//! Generates a comprehensive quarterly deep-dive report from the AgentScribe index.
//! Covers monthly breakdown, agent comparison, error pattern trends, model usage,
//! ASCII data visualizations, HTML output for web, and key insights for PR/media.

use crate::analytics::{self, AgentMetrics, AnalyticsOptions, AnalyticsOutput};
use crate::config::Config;
use crate::error::{AgentScribeError, Result};
use crate::recurring::{self, RecurringOptions};
use chrono::{DateTime, Datelike, TimeZone, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

// ── Report output format ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    Markdown,
    Html,
    Json,
}

impl ReportFormat {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "markdown" | "md" => Some(ReportFormat::Markdown),
            "html" => Some(ReportFormat::Html),
            "json" => Some(ReportFormat::Json),
            _ => None,
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            ReportFormat::Markdown => "md",
            ReportFormat::Html => "html",
            ReportFormat::Json => "json",
        }
    }
}

// ── Quarter parsing ───────────────────────────────────────────────────────────

/// A calendar quarter with its date bounds.
#[derive(Debug, Clone)]
pub struct Quarter {
    /// Human label e.g. "Q1 2026"
    pub label: String,
    /// Machine key e.g. "2026-Q1"
    pub key: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

/// Parse a quarter string like "2026-Q1", "2026-q2", or "current".
pub fn parse_quarter(s: &str) -> Result<Quarter> {
    let s_lower = s.to_lowercase();
    if s_lower == "current" {
        return current_quarter();
    }

    // Expect "YYYY-Q[1-4]"
    let parts: Vec<&str> = s.splitn(2, '-').collect();
    if parts.len() != 2 {
        return Err(AgentScribeError::Config(format!(
            "Invalid quarter '{}'. Use 'YYYY-Q1' through 'YYYY-Q4' or 'current'",
            s
        )));
    }

    let year: i32 = parts[0]
        .parse()
        .map_err(|_| AgentScribeError::Config(format!("Invalid year in quarter string '{}'", s)))?;

    let q_str = parts[1].to_uppercase();
    let q_num: u8 = match q_str.as_str() {
        "Q1" => 1,
        "Q2" => 2,
        "Q3" => 3,
        "Q4" => 4,
        _ => {
            return Err(AgentScribeError::Config(format!(
                "Invalid quarter '{}'. Must be Q1, Q2, Q3, or Q4",
                parts[1]
            )))
        }
    };

    make_quarter(year, q_num)
}

fn make_quarter(year: i32, q: u8) -> Result<Quarter> {
    let (start_month, end_month) = match q {
        1 => (1u32, 3u32),
        2 => (4, 6),
        3 => (7, 9),
        4 => (10, 12),
        _ => {
            return Err(AgentScribeError::Config(format!(
                "Quarter must be 1-4, got {}",
                q
            )))
        }
    };

    let start = Utc
        .with_ymd_and_hms(year, start_month, 1, 0, 0, 0)
        .single()
        .ok_or_else(|| {
            AgentScribeError::Config(format!(
                "Could not construct start date for Q{} {}",
                q, year
            ))
        })?;

    // End = first day of next quarter - 1 second
    let (end_year, next_month) = if end_month == 12 {
        (year + 1, 1u32)
    } else {
        (year, end_month + 1)
    };
    let end = Utc
        .with_ymd_and_hms(end_year, next_month, 1, 0, 0, 0)
        .single()
        .ok_or_else(|| {
            AgentScribeError::Config(format!("Could not construct end date for Q{} {}", q, year))
        })?
        - chrono::Duration::seconds(1);

    Ok(Quarter {
        label: format!("Q{} {}", q, year),
        key: format!("{}-Q{}", year, q),
        start,
        end,
    })
}

fn current_quarter() -> Result<Quarter> {
    let now = Utc::now();
    let month = now.month();
    let q = ((month - 1) / 3 + 1) as u8;
    make_quarter(now.year(), q)
}

// ── Data structures ───────────────────────────────────────────────────────────

/// Session counts / metrics for a single month within the quarter.
#[derive(Debug, Clone, Serialize)]
pub struct MonthlyStats {
    /// e.g. "January 2026"
    pub month_label: String,
    /// Short key e.g. "2026-01"
    pub month_key: String,
    pub sessions: usize,
    pub success_count: usize,
    pub failure_count: usize,
    pub abandoned_count: usize,
    pub success_rate: f64,
    pub avg_turns: f64,
    pub estimated_tokens: f64,
    pub estimated_cost: f64,
    /// Sessions by agent for this month
    pub by_agent: HashMap<String, usize>,
}

/// A model's usage within the quarter.
#[derive(Debug, Clone, Serialize)]
pub struct ModelUsageEntry {
    pub model: String,
    pub sessions: usize,
    pub estimated_tokens: f64,
    pub estimated_cost: f64,
    /// Success rate for sessions using this model (0.0 when unknown)
    pub success_rate: f64,
}

/// A key insight auto-generated from the data.
#[derive(Debug, Clone, Serialize)]
pub struct Insight {
    pub category: String,
    pub headline: String,
    pub detail: String,
}

/// A PR / media highlight with a punchy stat.
#[derive(Debug, Clone, Serialize)]
pub struct PrHighlight {
    pub headline: String,
    pub stat: String,
    pub context: String,
}

/// Top error pattern entry.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorPatternEntry {
    pub fingerprint: String,
    pub occurrences: usize,
    pub resolution_rate: f64,
    pub agents_affected: Vec<String>,
}

/// Full pulse report output.
#[derive(Debug, Serialize)]
pub struct PulseReportOutput {
    pub quarter: String,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub total_sessions: usize,
    pub overall_success_rate: f64,
    pub overall_avg_turns: f64,
    pub estimated_total_tokens: f64,
    pub estimated_total_cost: f64,
    pub monthly_breakdown: Vec<MonthlyStats>,
    pub agent_metrics: Vec<AgentMetrics>,
    pub model_usage: Vec<ModelUsageEntry>,
    pub top_error_patterns: Vec<ErrorPatternEntry>,
    pub problem_type_distribution: Vec<(String, usize, f64)>,
    pub weekly_trend: Vec<(String, usize, f64)>, // (week, sessions, success_rate)
    pub key_insights: Vec<Insight>,
    pub pr_highlights: Vec<PrHighlight>,
    pub computed_at: DateTime<Utc>,
}

/// Options for pulse report generation.
pub struct PulseReportOptions {
    /// Quarter string, e.g. "2026-Q1" or "current"
    pub quarter: String,
    /// Write output to file instead of stdout
    pub output: Option<String>,
    pub format: ReportFormat,
}

// ── Report generation ─────────────────────────────────────────────────────────

/// Generate the quarterly pulse report.
pub fn generate_pulse_report(
    data_dir: &Path,
    opts: &PulseReportOptions,
    config: &Config,
) -> Result<PulseReportOutput> {
    let quarter = parse_quarter(&opts.quarter)?;

    // Overall analytics for the quarter
    let analytics_opts = AnalyticsOptions {
        agent: None,
        project: None,
        since: Some(quarter.start),
    };
    let analytics = analytics::compute_analytics(data_dir, &analytics_opts, config)?;

    // Recurring problems within the quarter
    let recurring_opts = RecurringOptions {
        since: quarter.start,
        threshold: 2,
    };
    let recurring = recurring::detect_recurring(data_dir, &recurring_opts)?;

    // Per-month breakdown + model usage
    let (monthly_breakdown, model_usage) =
        compute_monthly_and_model_data(data_dir, &quarter, config)?;

    // Error patterns from recurring
    let top_error_patterns: Vec<ErrorPatternEntry> = recurring
        .problems
        .iter()
        .take(15)
        .map(|p| ErrorPatternEntry {
            fingerprint: p.fingerprint.clone(),
            occurrences: p.session_count,
            resolution_rate: if p.session_count > 0 && !p.fix_agents.is_empty() {
                1.0
            } else {
                0.0
            },
            agents_affected: p.agents.clone(),
        })
        .collect();

    // Weekly trend from analytics
    let weekly_trend: Vec<(String, usize, f64)> = analytics
        .trends
        .iter()
        .map(|t| (t.week.clone(), t.sessions, t.success_rate))
        .collect();

    // Problem type distribution
    let problem_type_distribution: Vec<(String, usize, f64)> = analytics
        .problem_types
        .iter()
        .map(|p| (p.problem_type.clone(), p.count, p.percentage))
        .collect();

    let total_sessions = analytics.total_sessions;
    let overall_success_rate = analytics.overall_success_rate;
    let overall_avg_turns = analytics.overall_avg_turns;
    let estimated_total_cost = analytics.estimated_total_cost;
    let estimated_total_tokens = analytics.overall_avg_tokens * total_sessions as f64;

    let key_insights = generate_insights(
        &analytics,
        &monthly_breakdown,
        &model_usage,
        &top_error_patterns,
    );

    let pr_highlights = generate_pr_highlights(
        &quarter,
        total_sessions,
        overall_success_rate,
        estimated_total_cost,
        &analytics.agents,
        &problem_type_distribution,
    );

    Ok(PulseReportOutput {
        quarter: quarter.label.clone(),
        period_start: quarter.start,
        period_end: quarter.end,
        total_sessions,
        overall_success_rate,
        overall_avg_turns,
        estimated_total_tokens,
        estimated_total_cost,
        monthly_breakdown,
        agent_metrics: analytics.agents,
        model_usage,
        top_error_patterns,
        problem_type_distribution,
        weekly_trend,
        key_insights,
        pr_highlights,
        computed_at: Utc::now(),
    })
}

// ── Monthly + model data extraction ──────────────────────────────────────────

fn compute_monthly_and_model_data(
    data_dir: &Path,
    quarter: &Quarter,
    config: &Config,
) -> Result<(Vec<MonthlyStats>, Vec<ModelUsageEntry>)> {
    use crate::analytics::{estimate_cost, estimate_tokens, extract_session_data};
    use crate::index::build_schema;
    use crate::search::open_index;
    use tantivy::collector::TopDocs;
    use tantivy::query::AllQuery;

    let index = open_index(data_dir)?;
    let reader = index
        .reader()
        .map_err(|e| AgentScribeError::DataDir(format!("Failed to create index reader: {}", e)))?;
    let searcher = reader.searcher();
    let total_docs = searcher.num_docs();

    if total_docs == 0 {
        return Ok((vec![], vec![]));
    }

    let (_schema, fields) = build_schema();
    let all_docs = searcher
        .search(&AllQuery, &TopDocs::with_limit(total_docs as usize))
        .map_err(|e| AgentScribeError::DataDir(format!("Search failed: {}", e)))?;

    // Monthly buckets: key = "YYYY-MM"
    // (sessions, successes, failures, abandoned, turns, tokens, cost, agents)
    let mut month_sessions: HashMap<String, Vec<_>> = HashMap::new();
    // Model buckets: key = model string
    let mut model_sessions: HashMap<String, (usize, usize, f64, f64)> = HashMap::new(); // (total, success, tokens, cost)

    for (_score, doc_addr) in all_docs {
        let data = match extract_session_data(&searcher, doc_addr, &fields) {
            Some(d) => d,
            None => continue,
        };

        if data.timestamp < quarter.start || data.timestamp > quarter.end {
            continue;
        }

        let month_key = data.timestamp.format("%Y-%m").to_string();
        let entry = month_sessions.entry(month_key).or_default();
        entry.push((
            data.source_agent.clone(),
            data.outcome.clone(),
            data.turns,
            data.content_length,
            data.model.clone(),
        ));

        // Model tracking
        let tokens = estimate_tokens(data.content_length);
        let cost = estimate_cost(tokens, data.model.as_deref(), &config.cost);
        let is_success = data.outcome.as_deref() == Some("success");
        let model_key = data.model.unwrap_or_else(|| "unknown".to_string());
        let me = model_sessions.entry(model_key).or_insert((0, 0, 0.0, 0.0));
        me.0 += 1;
        if is_success {
            me.1 += 1;
        }
        me.2 += tokens;
        me.3 += cost;
    }

    // Build monthly stats sorted chronologically
    let mut monthly_breakdown: Vec<MonthlyStats> = month_sessions
        .into_iter()
        .map(|(month_key, sessions)| {
            let total = sessions.len();
            let success = sessions
                .iter()
                .filter(|(_, o, _, _, _)| o.as_deref() == Some("success"))
                .count();
            let failure = sessions
                .iter()
                .filter(|(_, o, _, _, _)| o.as_deref() == Some("failure"))
                .count();
            let abandoned = sessions
                .iter()
                .filter(|(_, o, _, _, _)| o.as_deref() == Some("abandoned"))
                .count();
            let success_rate = if total > 0 {
                success as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            let avg_turns = if total > 0 {
                sessions.iter().map(|(_, _, t, _, _)| *t).sum::<u64>() as f64 / total as f64
            } else {
                0.0
            };
            let estimated_tokens: f64 = sessions
                .iter()
                .map(|(_, _, _, len, _)| estimate_tokens(*len))
                .sum();
            let estimated_cost: f64 = sessions
                .iter()
                .map(|(_, _, _, len, model)| {
                    estimate_cost(estimate_tokens(*len), model.as_deref(), &config.cost)
                })
                .sum();

            let mut by_agent: HashMap<String, usize> = HashMap::new();
            for (agent, _, _, _, _) in &sessions {
                *by_agent.entry(agent.clone()).or_insert(0) += 1;
            }

            // Build label from key e.g. "2026-01" → "January 2026"
            let month_label = month_key_to_label(&month_key);

            MonthlyStats {
                month_label,
                month_key,
                sessions: total,
                success_count: success,
                failure_count: failure,
                abandoned_count: abandoned,
                success_rate,
                avg_turns,
                estimated_tokens,
                estimated_cost,
                by_agent,
            }
        })
        .collect();

    monthly_breakdown.sort_by(|a, b| a.month_key.cmp(&b.month_key));

    // Build model usage entries
    let mut model_usage: Vec<ModelUsageEntry> = model_sessions
        .into_iter()
        .map(|(model, (total, success, tokens, cost))| ModelUsageEntry {
            model,
            sessions: total,
            estimated_tokens: tokens,
            estimated_cost: cost,
            success_rate: if total > 0 {
                success as f64 / total as f64 * 100.0
            } else {
                0.0
            },
        })
        .collect();
    model_usage.sort_by_key(|b| std::cmp::Reverse(b.sessions));

    Ok((monthly_breakdown, model_usage))
}

fn month_key_to_label(key: &str) -> String {
    let parts: Vec<&str> = key.splitn(2, '-').collect();
    if parts.len() != 2 {
        return key.to_string();
    }
    let year = parts[0];
    let month_num: u32 = parts[1].parse().unwrap_or(0);
    let month_name = match month_num {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "Unknown",
    };
    format!("{} {}", month_name, year)
}

// ── Insight generation ────────────────────────────────────────────────────────

fn generate_insights(
    analytics: &AnalyticsOutput,
    monthly: &[MonthlyStats],
    models: &[ModelUsageEntry],
    errors: &[ErrorPatternEntry],
) -> Vec<Insight> {
    let mut insights = Vec::new();

    // Best-performing agent
    if let Some(best) = analytics
        .agents
        .iter()
        .filter(|a| a.total_sessions >= 3)
        .max_by(|a, b| a.success_rate.partial_cmp(&b.success_rate).unwrap())
    {
        insights.push(Insight {
            category: "Agent Performance".to_string(),
            headline: format!(
                "{} leads with {:.1}% success rate",
                best.agent, best.success_rate
            ),
            detail: format!(
                "{} completed {} sessions with {:.1}% success, averaging {:.1} turns per successful resolution.",
                best.agent, best.total_sessions, best.success_rate, best.avg_turns_success
            ),
        });
    }

    // Most active month
    if let Some(busiest) = monthly.iter().max_by_key(|m| m.sessions) {
        insights.push(Insight {
            category: "Activity Trends".to_string(),
            headline: format!(
                "{} was the most active month ({} sessions)",
                busiest.month_label, busiest.sessions
            ),
            detail: format!(
                "{} saw the highest session volume with {} sessions and a {:.1}% success rate.",
                busiest.month_label, busiest.sessions, busiest.success_rate
            ),
        });
    }

    // Month-over-month trend
    if monthly.len() >= 2 {
        let first = &monthly[0];
        let last = &monthly[monthly.len() - 1];
        if last.sessions > first.sessions {
            let growth_pct = (last.sessions as f64 - first.sessions as f64)
                / (first.sessions as f64).max(1.0)
                * 100.0;
            insights.push(Insight {
                category: "Growth".to_string(),
                headline: format!("Session volume grew {:.0}% over the quarter", growth_pct),
                detail: format!(
                    "From {} sessions in {} to {} in {} — a {:.0}% increase.",
                    first.sessions, first.month_label, last.sessions, last.month_label, growth_pct
                ),
            });
        }
    }

    // Most efficient model
    if let Some(best_model) = models
        .iter()
        .filter(|m| m.sessions >= 3 && m.estimated_cost > 0.0)
        .min_by(|a, b| {
            let ca = a.estimated_cost / a.sessions as f64;
            let cb = b.estimated_cost / b.sessions as f64;
            ca.partial_cmp(&cb).unwrap()
        })
    {
        let cost_per = best_model.estimated_cost / best_model.sessions as f64;
        insights.push(Insight {
            category: "Cost Efficiency".to_string(),
            headline: format!(
                "{} is the most cost-efficient model (${:.3}/session)",
                best_model.model, cost_per
            ),
            detail: format!(
                "{} averaged ${:.3} per session across {} sessions with {:.1}% success rate.",
                best_model.model, cost_per, best_model.sessions, best_model.success_rate
            ),
        });
    }

    // Top error pattern
    if let Some(top_err) = errors.first() {
        let fp_short = if top_err.fingerprint.len() > 60 {
            format!("{}...", &top_err.fingerprint[..57])
        } else {
            top_err.fingerprint.clone()
        };
        insights.push(Insight {
            category: "Error Patterns".to_string(),
            headline: format!(
                "Most frequent error occurred {} times this quarter",
                top_err.occurrences
            ),
            detail: format!(
                "Pattern '{}' appeared {} time(s). {}",
                fp_short,
                top_err.occurrences,
                if top_err.resolution_rate > 0.0 {
                    "Has been resolved.".to_string()
                } else {
                    "No known resolution yet.".to_string()
                }
            ),
        });
    }

    // Overall success rate insight
    let rate = analytics.overall_success_rate;
    let tier = if rate >= 80.0 {
        "excellent"
    } else if rate >= 65.0 {
        "strong"
    } else if rate >= 50.0 {
        "moderate"
    } else {
        "low"
    };
    insights.push(Insight {
        category: "Overall Health".to_string(),
        headline: format!("Overall success rate is {:.1}% ({})", rate, tier),
        detail: format!(
            "Across {} sessions, {:.1}% completed successfully. Average of {:.1} turns per session.",
            analytics.total_sessions, rate, analytics.overall_avg_turns
        ),
    });

    insights
}

// ── PR highlights ─────────────────────────────────────────────────────────────

fn generate_pr_highlights(
    quarter: &Quarter,
    total: usize,
    success_rate: f64,
    total_cost: f64,
    agents: &[AgentMetrics],
    problem_types: &[(String, usize, f64)],
) -> Vec<PrHighlight> {
    let mut highlights = Vec::new();

    highlights.push(PrHighlight {
        headline: format!(
            "State of AI Coding {} — {} Sessions Analyzed",
            quarter.label, total
        ),
        stat: format!("{} coding sessions", total),
        context: format!(
            "Comprehensive analysis of {} AI coding agent sessions across the {} quarter.",
            total, quarter.label
        ),
    });

    highlights.push(PrHighlight {
        headline: format!("{:.1}% of AI Coding Sessions Succeed", success_rate),
        stat: format!("{:.1}% success rate", success_rate),
        context: "Measured by explicit user confirmation, clean test exits, and git commits."
            .to_string(),
    });

    if let Some(best_agent) = agents
        .iter()
        .filter(|a| a.total_sessions >= 3)
        .max_by(|a, b| a.success_rate.partial_cmp(&b.success_rate).unwrap())
    {
        highlights.push(PrHighlight {
            headline: format!(
                "{} Tops Agent Rankings at {:.1}% Success",
                best_agent.agent, best_agent.success_rate
            ),
            stat: format!("{:.1}%", best_agent.success_rate),
            context: format!(
                "{} leads the field with {:.1}% success across {} sessions in {}.",
                best_agent.agent, best_agent.success_rate, best_agent.total_sessions, quarter.label
            ),
        });
    }

    if let Some((top_type, _, top_pct)) = problem_types.first() {
        highlights.push(PrHighlight {
            headline: format!(
                "{:.0}% of Sessions Are {} Tasks",
                top_pct,
                capitalize(top_type)
            ),
            stat: format!("{:.0}% {}", top_pct, top_type),
            context: format!(
                "{} is the dominant session type at {:.0}%, followed by other task categories.",
                capitalize(top_type),
                top_pct
            ),
        });
    }

    if total_cost > 0.0 {
        highlights.push(PrHighlight {
            headline: format!("${:.2} Total AI Spend This Quarter", total_cost),
            stat: format!("${:.2}", total_cost),
            context: format!(
                "Estimated across {} sessions based on token usage and model pricing.",
                total
            ),
        });
    }

    if agents.len() >= 2 {
        highlights.push(PrHighlight {
            headline: format!("{} Agents Tracked in {}", agents.len(), quarter.label),
            stat: format!("{} agents", agents.len()),
            context: agents
                .iter()
                .map(|a| a.agent.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        });
    }

    highlights
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().to_string() + c.as_str(),
    }
}

// ── ASCII chart helpers ───────────────────────────────────────────────────────

/// Render a horizontal bar chart row.
/// `value` is 0.0–100.0 (percentage) unless `max_value` is set explicitly.
fn ascii_bar(value: f64, max_value: f64, width: usize) -> String {
    let filled = if max_value > 0.0 {
        ((value / max_value) * width as f64).round() as usize
    } else {
        0
    };
    let filled = filled.min(width);
    let empty = width - filled;
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

// ── Markdown formatter ────────────────────────────────────────────────────────

/// Render the pulse report as Markdown.
pub fn format_markdown(r: &PulseReportOutput) -> String {
    let mut md = String::new();

    // Title
    md.push_str(&format!(
        "# Pulse Report: State of AI Coding — {}\n\n",
        r.quarter
    ));
    md.push_str(&format!(
        "> **Period:** {} to {}  \n",
        r.period_start.format("%B %-d, %Y"),
        r.period_end.format("%B %-d, %Y")
    ));
    md.push_str(&format!(
        "> **Generated:** {}  \n\n",
        r.computed_at.format("%Y-%m-%d %H:%M UTC")
    ));

    if r.total_sessions == 0 {
        md.push_str("No sessions found for this quarter.\n");
        return md;
    }

    // Executive summary
    md.push_str("## Executive Summary\n\n");
    md.push_str("| Metric | Value |\n");
    md.push_str("|--------|-------|\n");
    md.push_str(&format!("| Total Sessions | {} |\n", r.total_sessions));
    md.push_str(&format!(
        "| Overall Success Rate | {:.1}% |\n",
        r.overall_success_rate
    ));
    md.push_str(&format!(
        "| Avg Turns / Session | {:.1} |\n",
        r.overall_avg_turns
    ));
    md.push_str(&format!(
        "| Est. Total Tokens | {:.0} |\n",
        r.estimated_total_tokens
    ));
    if r.estimated_total_cost > 0.0 {
        md.push_str(&format!(
            "| Est. Total Cost | ${:.2} |\n",
            r.estimated_total_cost
        ));
    }
    md.push_str(&format!("| Agents Tracked | {} |\n", r.agent_metrics.len()));
    md.push('\n');

    // Monthly breakdown
    md.push_str("## Monthly Breakdown\n\n");
    if r.monthly_breakdown.is_empty() {
        md.push_str("_(no session data)_\n\n");
    } else {
        md.push_str(
            "| Month | Sessions | Success | Fail | Abandoned | Success Rate | Avg Turns |\n",
        );
        md.push_str("|-------|----------|---------|------|-----------|-------------|----------|\n");
        for m in &r.monthly_breakdown {
            md.push_str(&format!(
                "| {} | {} | {} | {} | {} | {:.1}% | {:.1} |\n",
                m.month_label,
                m.sessions,
                m.success_count,
                m.failure_count,
                m.abandoned_count,
                m.success_rate,
                m.avg_turns,
            ));
        }
        md.push('\n');

        // Session volume ASCII chart
        md.push_str("### Session Volume by Month\n\n```\n");
        let max_sessions = r
            .monthly_breakdown
            .iter()
            .map(|m| m.sessions)
            .max()
            .unwrap_or(1);
        for m in &r.monthly_breakdown {
            let bar = ascii_bar(m.sessions as f64, max_sessions as f64, 30);
            md.push_str(&format!(
                "{:<14}  {}  {}\n",
                m.month_label
                    .split_whitespace()
                    .next()
                    .unwrap_or(&m.month_label),
                bar,
                m.sessions,
            ));
        }
        md.push_str("```\n\n");

        // Success rate ASCII chart
        md.push_str("### Success Rate by Month\n\n```\n");
        for m in &r.monthly_breakdown {
            let bar = ascii_bar(m.success_rate, 100.0, 30);
            md.push_str(&format!(
                "{:<14}  {}  {:.1}%\n",
                m.month_label
                    .split_whitespace()
                    .next()
                    .unwrap_or(&m.month_label),
                bar,
                m.success_rate,
            ));
        }
        md.push_str("```\n\n");
    }

    // Agent comparison
    md.push_str("## Agent Comparison\n\n");
    if r.agent_metrics.is_empty() {
        md.push_str("_(no agent data)_\n\n");
    } else {
        md.push_str("| Agent | Sessions | Success Rate | Avg Turns | Avg Tokens | Est. Cost |\n");
        md.push_str("|-------|----------|-------------|-----------|------------|----------|\n");
        for a in &r.agent_metrics {
            let cost_str = if a.estimated_cost > 0.0 {
                format!("${:.2}", a.estimated_cost)
            } else {
                "-".to_string()
            };
            md.push_str(&format!(
                "| {} | {} | {:.1}% | {:.1} | {:.0} | {} |\n",
                a.agent,
                a.total_sessions,
                a.success_rate,
                a.avg_turns_all,
                a.avg_tokens_success,
                cost_str,
            ));
        }
        md.push('\n');

        // Success rate comparison chart
        md.push_str("### Success Rate Comparison\n\n```\n");
        for a in &r.agent_metrics {
            let bar = ascii_bar(a.success_rate, 100.0, 30);
            md.push_str(&format!(
                "{:<18}  {}  {:.1}%\n",
                a.agent, bar, a.success_rate
            ));
        }
        md.push_str("```\n\n");
    }

    // Problem type distribution
    md.push_str("## Problem Type Distribution\n\n");
    if r.problem_type_distribution.is_empty() {
        md.push_str("_(no data)_\n\n");
    } else {
        md.push_str("```\n");
        for (ptype, count, pct) in &r.problem_type_distribution {
            let bar = ascii_bar(*pct, 100.0, 25);
            md.push_str(&format!(
                "{:<16}  {}  {} ({:.1}%)\n",
                ptype, bar, count, pct
            ));
        }
        md.push_str("```\n\n");
    }

    // Model usage
    md.push_str("## Model Usage\n\n");
    if r.model_usage.is_empty() {
        md.push_str("_(no model data)_\n\n");
    } else {
        md.push_str("| Model | Sessions | Est. Tokens | Est. Cost | Success Rate |\n");
        md.push_str("|-------|----------|-------------|-----------|-------------|\n");
        for m in &r.model_usage {
            let cost_str = if m.estimated_cost > 0.0 {
                format!("${:.2}", m.estimated_cost)
            } else {
                "-".to_string()
            };
            let rate_str = if m.success_rate > 0.0 {
                format!("{:.1}%", m.success_rate)
            } else {
                "-".to_string()
            };
            md.push_str(&format!(
                "| {} | {} | {:.0} | {} | {} |\n",
                m.model, m.sessions, m.estimated_tokens, cost_str, rate_str,
            ));
        }
        md.push('\n');
    }

    // Top error patterns
    md.push_str("## Top Error Patterns\n\n");
    if r.top_error_patterns.is_empty() {
        md.push_str("No recurring error patterns detected.\n\n");
    } else {
        for (i, ep) in r.top_error_patterns.iter().enumerate().take(10) {
            let fp_display = if ep.fingerprint.len() > 70 {
                format!("{}...", &ep.fingerprint[..67])
            } else {
                ep.fingerprint.clone()
            };
            md.push_str(&format!(
                "{}. **{}** — {} occurrence(s)",
                i + 1,
                fp_display,
                ep.occurrences
            ));
            if !ep.agents_affected.is_empty() {
                md.push_str(&format!("  _(affects: {})_", ep.agents_affected.join(", ")));
            }
            md.push('\n');
        }
        md.push('\n');
    }

    // Key insights
    md.push_str("## Key Insights\n\n");
    for ins in &r.key_insights {
        md.push_str(&format!("### {}\n\n", ins.headline));
        md.push_str(&format!("**Category:** {}  \n", ins.category));
        md.push_str(&format!("{}\n\n", ins.detail));
    }

    // PR / media highlights
    md.push_str("## PR & Media Highlights\n\n");
    md.push_str("_Key statistics for announcements, blog posts, and media outreach:_\n\n");
    for (i, h) in r.pr_highlights.iter().enumerate() {
        md.push_str(&format!("{}. **{}**\n", i + 1, h.headline));
        md.push_str(&format!("   - Stat: `{}`\n", h.stat));
        md.push_str(&format!("   - Context: {}\n\n", h.context));
    }

    // Methodology
    md.push_str("## Methodology\n\n");
    md.push_str(
        "This report was generated by AgentScribe from indexed agent conversation logs.\n\n",
    );
    md.push_str("- **Data source:** Tantivy full-text index of normalized JSONL session files\n");
    md.push_str("- **Outcome classification:** Signal-scoring system (user confirmation, exit codes, git commits)\n");
    md.push_str("- **Token estimation:** `content_length / 4` characters per token\n");
    md.push_str("- **Cost estimation:** Per-model pricing from `config.toml [cost.models]`; sessions with unknown model are excluded from cost totals\n");
    md.push_str("- **Error patterns:** Normalized fingerprints (paths, hosts, ports, PIDs stripped); grouped by structural pattern\n");
    md.push_str("- **PDF version:** Convert this Markdown to PDF with `pandoc pulse-report.md -o pulse-report.pdf --pdf-engine=wkhtmltopdf`\n");
    md.push_str("- **Web version:** Use `agentscribe pulse-report --format html` to generate a standalone HTML report\n\n");

    md
}

// ── HTML formatter ─────────────────────────────────────────────────────────────

/// Render the pulse report as a self-contained HTML document.
pub fn format_html(r: &PulseReportOutput) -> String {
    let mut html = String::new();

    html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
    html.push_str("<meta charset=\"UTF-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n");
    html.push_str(&format!(
        "<title>Pulse Report: State of AI Coding — {}</title>\n",
        r.quarter
    ));
    html.push_str("<style>\n");
    html.push_str(HTML_CSS);
    html.push_str("</style>\n</head>\n<body>\n");

    // Header
    html.push_str("<div class=\"report-header\">\n");
    html.push_str(&format!(
        "<h1>Pulse Report: State of AI Coding</h1>\n<h2>{}</h2>\n",
        r.quarter
    ));
    html.push_str(&format!(
        "<p class=\"meta\">Period: {} — {} &nbsp;|&nbsp; Generated: {}</p>\n",
        r.period_start.format("%B %-d, %Y"),
        r.period_end.format("%B %-d, %Y"),
        r.computed_at.format("%Y-%m-%d %H:%M UTC"),
    ));
    html.push_str("</div>\n");

    if r.total_sessions == 0 {
        html.push_str("<p>No sessions found for this quarter.</p>\n</body>\n</html>\n");
        return html;
    }

    // Summary cards
    html.push_str("<div class=\"summary-cards\">\n");
    html_card(
        &mut html,
        "Total Sessions",
        &r.total_sessions.to_string(),
        "sessions analyzed",
    );
    html_card(
        &mut html,
        "Success Rate",
        &format!("{:.1}%", r.overall_success_rate),
        "of sessions succeeded",
    );
    html_card(
        &mut html,
        "Avg Turns",
        &format!("{:.1}", r.overall_avg_turns),
        "turns per session",
    );
    if r.estimated_total_cost > 0.0 {
        html_card(
            &mut html,
            "Est. Cost",
            &format!("${:.2}", r.estimated_total_cost),
            "total AI spend",
        );
    }
    html_card(
        &mut html,
        "Agents",
        &r.agent_metrics.len().to_string(),
        "agent types",
    );
    html.push_str("</div>\n");

    // Monthly breakdown
    html.push_str("<section>\n<h2>Monthly Breakdown</h2>\n");
    if !r.monthly_breakdown.is_empty() {
        html.push_str("<table>\n<thead><tr>");
        html.push_str("<th>Month</th><th>Sessions</th><th>Success</th><th>Fail</th><th>Abandoned</th><th>Success Rate</th><th>Avg Turns</th>");
        html.push_str("</tr></thead>\n<tbody>\n");
        for m in &r.monthly_breakdown {
            html.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{:.1}%</td><td>{:.1}</td></tr>\n",
                m.month_label, m.sessions, m.success_count, m.failure_count, m.abandoned_count,
                m.success_rate, m.avg_turns,
            ));
        }
        html.push_str("</tbody></table>\n");

        // Session volume bar chart
        html.push_str("<h3>Session Volume</h3>\n<div class=\"chart\">\n");
        let max_s = r
            .monthly_breakdown
            .iter()
            .map(|m| m.sessions)
            .max()
            .unwrap_or(1);
        for m in &r.monthly_breakdown {
            let pct = m.sessions as f64 / max_s as f64 * 100.0;
            let label = m
                .month_label
                .split_whitespace()
                .next()
                .unwrap_or(&m.month_label);
            html_bar_row(&mut html, label, pct, &m.sessions.to_string());
        }
        html.push_str("</div>\n");
    } else {
        html.push_str("<p><em>No session data for this quarter.</em></p>\n");
    }
    html.push_str("</section>\n");

    // Agent comparison
    html.push_str("<section>\n<h2>Agent Comparison</h2>\n");
    if !r.agent_metrics.is_empty() {
        html.push_str("<table>\n<thead><tr><th>Agent</th><th>Sessions</th><th>Success Rate</th><th>Avg Turns</th><th>Est. Cost</th></tr></thead>\n<tbody>\n");
        for a in &r.agent_metrics {
            let cost_str = if a.estimated_cost > 0.0 {
                format!("${:.2}", a.estimated_cost)
            } else {
                "—".to_string()
            };
            html.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{:.1}%</td><td>{:.1}</td><td>{}</td></tr>\n",
                a.agent, a.total_sessions, a.success_rate, a.avg_turns_all, cost_str,
            ));
        }
        html.push_str("</tbody></table>\n");

        html.push_str("<h3>Success Rate Comparison</h3>\n<div class=\"chart\">\n");
        for a in &r.agent_metrics {
            html_bar_row(
                &mut html,
                &a.agent,
                a.success_rate,
                &format!("{:.1}%", a.success_rate),
            );
        }
        html.push_str("</div>\n");
    }
    html.push_str("</section>\n");

    // Problem type distribution
    html.push_str("<section>\n<h2>Problem Type Distribution</h2>\n");
    if !r.problem_type_distribution.is_empty() {
        html.push_str("<div class=\"chart\">\n");
        for (ptype, count, pct) in &r.problem_type_distribution {
            html_bar_row(&mut html, ptype, *pct, &format!("{} ({:.1}%)", count, pct));
        }
        html.push_str("</div>\n");
    }
    html.push_str("</section>\n");

    // Model usage
    html.push_str("<section>\n<h2>Model Usage</h2>\n");
    if !r.model_usage.is_empty() {
        html.push_str("<table>\n<thead><tr><th>Model</th><th>Sessions</th><th>Est. Tokens</th><th>Est. Cost</th><th>Success Rate</th></tr></thead>\n<tbody>\n");
        for m in &r.model_usage {
            let cost_str = if m.estimated_cost > 0.0 {
                format!("${:.2}", m.estimated_cost)
            } else {
                "—".to_string()
            };
            let rate_str = if m.success_rate > 0.0 {
                format!("{:.1}%", m.success_rate)
            } else {
                "—".to_string()
            };
            html.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{:.0}</td><td>{}</td><td>{}</td></tr>\n",
                m.model, m.sessions, m.estimated_tokens, cost_str, rate_str,
            ));
        }
        html.push_str("</tbody></table>\n");
    }
    html.push_str("</section>\n");

    // Top error patterns
    html.push_str("<section>\n<h2>Top Error Patterns</h2>\n");
    if r.top_error_patterns.is_empty() {
        html.push_str("<p>No recurring error patterns detected.</p>\n");
    } else {
        html.push_str("<ol>\n");
        for ep in r.top_error_patterns.iter().take(10) {
            let fp_display = html_escape(&if ep.fingerprint.len() > 80 {
                format!("{}...", &ep.fingerprint[..77])
            } else {
                ep.fingerprint.clone()
            });
            html.push_str(&format!(
                "<li><code>{}</code> — {} occurrence(s)",
                fp_display, ep.occurrences
            ));
            if !ep.agents_affected.is_empty() {
                html.push_str(&format!(
                    " <em>(affects: {})</em>",
                    ep.agents_affected.join(", ")
                ));
            }
            html.push_str("</li>\n");
        }
        html.push_str("</ol>\n");
    }
    html.push_str("</section>\n");

    // Key insights
    html.push_str("<section>\n<h2>Key Insights</h2>\n");
    for ins in &r.key_insights {
        html.push_str("<div class=\"insight\">\n");
        html.push_str(&format!(
            "<span class=\"badge\">{}</span>\n",
            html_escape(&ins.category)
        ));
        html.push_str(&format!("<h3>{}</h3>\n", html_escape(&ins.headline)));
        html.push_str(&format!("<p>{}</p>\n", html_escape(&ins.detail)));
        html.push_str("</div>\n");
    }
    html.push_str("</section>\n");

    // PR highlights
    html.push_str("<section>\n<h2>PR &amp; Media Highlights</h2>\n");
    html.push_str(
        "<p>Key statistics for announcements, blog posts, and media outreach:</p>\n<ul>\n",
    );
    for h in &r.pr_highlights {
        html.push_str(&format!(
            "<li><strong>{}</strong><br><code>{}</code> — {}</li>\n",
            html_escape(&h.headline),
            html_escape(&h.stat),
            html_escape(&h.context),
        ));
    }
    html.push_str("</ul>\n</section>\n");

    // Methodology
    html.push_str("<section>\n<h2>Methodology</h2>\n");
    html.push_str(
        "<p>Generated by <strong>AgentScribe</strong> from indexed agent conversation logs.</p>\n",
    );
    html.push_str("<ul>\n");
    html.push_str("<li><strong>Data source:</strong> Tantivy full-text index of normalized JSONL session files</li>\n");
    html.push_str("<li><strong>Outcome classification:</strong> Signal-scoring (user confirmation, exit codes, git commits)</li>\n");
    html.push_str(
        "<li><strong>Token estimation:</strong> content_length / 4 characters per token</li>\n",
    );
    html.push_str("<li><strong>Cost estimation:</strong> Per-model pricing from config.toml; sessions with unknown model excluded from cost totals</li>\n");
    html.push_str("</ul>\n</section>\n");

    html.push_str("<footer><p>Generated by AgentScribe &mdash; <em>Archive, search, and learn from coding agent conversations.</em></p></footer>\n");
    html.push_str("</body>\n</html>\n");

    html
}

fn html_card(html: &mut String, label: &str, value: &str, sub: &str) {
    html.push_str("<div class=\"card\">\n");
    html.push_str(&format!("<div class=\"card-value\">{}</div>\n", value));
    html.push_str(&format!("<div class=\"card-label\">{}</div>\n", label));
    html.push_str(&format!("<div class=\"card-sub\">{}</div>\n", sub));
    html.push_str("</div>\n");
}

fn html_bar_row(html: &mut String, label: &str, pct: f64, value_label: &str) {
    let pct_clamped = pct.clamp(0.0, 100.0);
    html.push_str("<div class=\"bar-row\">\n");
    html.push_str(&format!(
        "<div class=\"bar-label\">{}</div>\n",
        html_escape(label)
    ));
    html.push_str(&format!(
        "<div class=\"bar-track\"><div class=\"bar-fill\" style=\"width:{:.1}%\"></div></div>\n",
        pct_clamped
    ));
    html.push_str(&format!(
        "<div class=\"bar-value\">{}</div>\n",
        html_escape(value_label)
    ));
    html.push_str("</div>\n");
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const HTML_CSS: &str = r#"
  * { box-sizing: border-box; margin: 0; padding: 0; }
  body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
         background: #f8f9fa; color: #212529; line-height: 1.6; padding: 2rem; }
  .report-header { background: #1a1a2e; color: white; padding: 2.5rem; border-radius: 12px;
                   margin-bottom: 2rem; text-align: center; }
  .report-header h1 { font-size: 2rem; margin-bottom: 0.5rem; }
  .report-header h2 { font-size: 1.4rem; font-weight: 400; color: #a0c4ff; margin-bottom: 0.75rem; }
  .meta { color: #8a9bb8; font-size: 0.9rem; }
  .summary-cards { display: flex; flex-wrap: wrap; gap: 1rem; margin-bottom: 2rem; }
  .card { background: white; border: 1px solid #dee2e6; border-radius: 8px; padding: 1.5rem;
          flex: 1 1 150px; text-align: center; box-shadow: 0 1px 3px rgba(0,0,0,.07); }
  .card-value { font-size: 2rem; font-weight: 700; color: #0d6efd; }
  .card-label { font-weight: 600; color: #495057; margin: 0.3rem 0 0.2rem; }
  .card-sub { font-size: 0.8rem; color: #868e96; }
  section { background: white; border: 1px solid #dee2e6; border-radius: 8px; padding: 1.75rem;
            margin-bottom: 1.5rem; box-shadow: 0 1px 3px rgba(0,0,0,.07); }
  section h2 { font-size: 1.3rem; color: #1a1a2e; border-bottom: 2px solid #e9ecef;
               padding-bottom: 0.5rem; margin-bottom: 1rem; }
  section h3 { font-size: 1rem; color: #495057; margin: 1.25rem 0 0.75rem; }
  table { width: 100%; border-collapse: collapse; font-size: 0.9rem; }
  th { background: #f1f3f5; text-align: left; padding: 0.65rem 0.75rem;
       border-bottom: 2px solid #dee2e6; font-weight: 600; color: #495057; }
  td { padding: 0.55rem 0.75rem; border-bottom: 1px solid #f1f3f5; }
  tr:last-child td { border-bottom: none; }
  tr:hover td { background: #f8f9fa; }
  .chart { margin-top: 0.75rem; }
  .bar-row { display: flex; align-items: center; gap: 0.75rem; margin-bottom: 0.5rem; }
  .bar-label { width: 140px; font-size: 0.85rem; color: #495057; text-align: right;
               flex-shrink: 0; }
  .bar-track { flex: 1; background: #e9ecef; border-radius: 4px; height: 18px; overflow: hidden; }
  .bar-fill { height: 100%; background: linear-gradient(90deg, #0d6efd, #6ea8fe);
              border-radius: 4px; transition: width 0.3s; }
  .bar-value { width: 80px; font-size: 0.85rem; color: #495057; flex-shrink: 0; }
  .insight { background: #f8f9fa; border-left: 4px solid #0d6efd; padding: 1rem 1.25rem;
             border-radius: 0 8px 8px 0; margin-bottom: 1rem; }
  .badge { display: inline-block; background: #0d6efd; color: white; font-size: 0.75rem;
           padding: 0.15rem 0.6rem; border-radius: 12px; margin-bottom: 0.4rem; }
  .insight h3 { font-size: 1rem; margin: 0.3rem 0; color: #212529; }
  .insight p { font-size: 0.9rem; color: #495057; }
  ol, ul { padding-left: 1.5rem; }
  li { margin-bottom: 0.5rem; line-height: 1.5; }
  code { background: #f1f3f5; padding: 0.1rem 0.4rem; border-radius: 4px;
         font-size: 0.85rem; color: #c7254e; }
  footer { text-align: center; color: #868e96; font-size: 0.85rem; padding: 1.5rem 0 0; }
  @media (max-width: 600px) { .bar-label { width: 80px; } .summary-cards { flex-direction: column; } }
"#;

// ── JSON formatter ─────────────────────────────────────────────────────────────

/// Render the pulse report as JSON.
pub fn format_json(r: &PulseReportOutput) -> String {
    serde_json::to_string_pretty(r).unwrap_or_else(|_| "{}".to_string())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_quarter_q1() {
        let q = parse_quarter("2026-Q1").unwrap();
        assert_eq!(q.key, "2026-Q1");
        assert_eq!(q.label, "Q1 2026");
        assert_eq!(q.start.month(), 1);
        assert_eq!(q.start.day(), 1);
        assert_eq!(q.end.month(), 3);
        assert_eq!(q.end.day(), 31);
    }

    #[test]
    fn test_parse_quarter_q2() {
        let q = parse_quarter("2025-Q2").unwrap();
        assert_eq!(q.key, "2025-Q2");
        assert_eq!(q.start.month(), 4);
        assert_eq!(q.end.month(), 6);
    }

    #[test]
    fn test_parse_quarter_q4() {
        let q = parse_quarter("2025-Q4").unwrap();
        assert_eq!(q.start.month(), 10);
        assert_eq!(q.end.month(), 12);
        assert_eq!(q.end.day(), 31);
    }

    #[test]
    fn test_parse_quarter_lowercase() {
        let q = parse_quarter("2026-q1").unwrap();
        assert_eq!(q.key, "2026-Q1");
    }

    #[test]
    fn test_parse_quarter_current() {
        let q = parse_quarter("current").unwrap();
        let now = Utc::now();
        assert!(q.start <= now);
        assert!(q.end >= now);
    }

    #[test]
    fn test_parse_quarter_invalid() {
        assert!(parse_quarter("2026-Q5").is_err());
        assert!(parse_quarter("bad-input").is_err());
        assert!(parse_quarter("2026").is_err());
    }

    #[test]
    fn test_report_format_from_str() {
        assert_eq!(
            ReportFormat::parse("markdown"),
            Some(ReportFormat::Markdown)
        );
        assert_eq!(ReportFormat::parse("md"), Some(ReportFormat::Markdown));
        assert_eq!(ReportFormat::parse("html"), Some(ReportFormat::Html));
        assert_eq!(ReportFormat::parse("json"), Some(ReportFormat::Json));
        assert_eq!(ReportFormat::parse("PDF"), None);
    }

    #[test]
    fn test_ascii_bar() {
        let bar = ascii_bar(50.0, 100.0, 10);
        assert_eq!(bar.chars().count(), 10);
        let bar_full = ascii_bar(100.0, 100.0, 10);
        assert!(!bar_full.contains('░'));
        let bar_empty = ascii_bar(0.0, 100.0, 10);
        assert!(!bar_empty.contains('█'));
    }

    #[test]
    fn test_month_key_to_label() {
        assert_eq!(month_key_to_label("2026-01"), "January 2026");
        assert_eq!(month_key_to_label("2026-03"), "March 2026");
        assert_eq!(month_key_to_label("2025-12"), "December 2025");
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(
            html_escape("<b>test & \"hi\"</b>"),
            "&lt;b&gt;test &amp; &quot;hi&quot;&lt;/b&gt;"
        );
    }

    #[test]
    fn test_format_markdown_empty_report() {
        let r = PulseReportOutput {
            quarter: "Q1 2026".to_string(),
            period_start: Utc::now(),
            period_end: Utc::now(),
            total_sessions: 0,
            overall_success_rate: 0.0,
            overall_avg_turns: 0.0,
            estimated_total_tokens: 0.0,
            estimated_total_cost: 0.0,
            monthly_breakdown: vec![],
            agent_metrics: vec![],
            model_usage: vec![],
            top_error_patterns: vec![],
            problem_type_distribution: vec![],
            weekly_trend: vec![],
            key_insights: vec![],
            pr_highlights: vec![],
            computed_at: Utc::now(),
        };
        let md = format_markdown(&r);
        assert!(md.contains("Pulse Report"));
        assert!(md.contains("Q1 2026"));
        assert!(md.contains("No sessions found"));
    }

    #[test]
    fn test_format_html_empty_report() {
        let r = PulseReportOutput {
            quarter: "Q1 2026".to_string(),
            period_start: Utc::now(),
            period_end: Utc::now(),
            total_sessions: 0,
            overall_success_rate: 0.0,
            overall_avg_turns: 0.0,
            estimated_total_tokens: 0.0,
            estimated_total_cost: 0.0,
            monthly_breakdown: vec![],
            agent_metrics: vec![],
            model_usage: vec![],
            top_error_patterns: vec![],
            problem_type_distribution: vec![],
            weekly_trend: vec![],
            key_insights: vec![],
            pr_highlights: vec![],
            computed_at: Utc::now(),
        };
        let html = format_html(&r);
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("Pulse Report"));
        assert!(html.contains("No sessions found"));
    }

    #[test]
    fn test_format_markdown_with_data() {
        use std::collections::HashMap;
        let r = make_test_report();
        let md = format_markdown(&r);
        assert!(md.contains("Q1 2026"));
        assert!(md.contains("Executive Summary"));
        assert!(md.contains("Monthly Breakdown"));
        assert!(md.contains("Agent Comparison"));
        assert!(md.contains("Problem Type Distribution"));
        assert!(md.contains("Key Insights"));
        assert!(md.contains("PR & Media Highlights"));
        assert!(md.contains("Methodology"));
        // Verify ASCII chart rows are present
        assert!(md.contains('█'));
        let _ = HashMap::<String, usize>::new(); // suppress unused import warning
    }

    #[test]
    fn test_format_html_with_data() {
        let r = make_test_report();
        let html = format_html(&r);
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("Monthly Breakdown"));
        assert!(html.contains("Agent Comparison"));
        assert!(html.contains("bar-fill"));
        assert!(html.contains("card-value"));
    }

    #[test]
    fn test_format_json_valid() {
        let r = make_test_report();
        let json_str = format_json(&r);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed["quarter"], "Q1 2026");
        assert_eq!(parsed["total_sessions"], 42);
    }

    #[test]
    fn test_capitalize() {
        assert_eq!(capitalize("debug"), "Debug");
        assert_eq!(capitalize(""), "");
        assert_eq!(capitalize("feature"), "Feature");
    }

    fn make_test_report() -> PulseReportOutput {
        use crate::analytics::AgentMetrics;
        use std::collections::HashMap;

        let now = Utc::now();
        PulseReportOutput {
            quarter: "Q1 2026".to_string(),
            period_start: now - chrono::Duration::days(90),
            period_end: now,
            total_sessions: 42,
            overall_success_rate: 71.4,
            overall_avg_turns: 9.2,
            estimated_total_tokens: 210_000.0,
            estimated_total_cost: 3.15,
            monthly_breakdown: vec![
                MonthlyStats {
                    month_label: "January 2026".to_string(),
                    month_key: "2026-01".to_string(),
                    sessions: 12,
                    success_count: 9,
                    failure_count: 2,
                    abandoned_count: 1,
                    success_rate: 75.0,
                    avg_turns: 8.5,
                    estimated_tokens: 60_000.0,
                    estimated_cost: 0.90,
                    by_agent: HashMap::from([("claude-code".to_string(), 12)]),
                },
                MonthlyStats {
                    month_label: "February 2026".to_string(),
                    month_key: "2026-02".to_string(),
                    sessions: 15,
                    success_count: 11,
                    failure_count: 3,
                    abandoned_count: 1,
                    success_rate: 73.3,
                    avg_turns: 9.1,
                    estimated_tokens: 75_000.0,
                    estimated_cost: 1.12,
                    by_agent: HashMap::from([
                        ("claude-code".to_string(), 10),
                        ("aider".to_string(), 5),
                    ]),
                },
                MonthlyStats {
                    month_label: "March 2026".to_string(),
                    month_key: "2026-03".to_string(),
                    sessions: 15,
                    success_count: 10,
                    failure_count: 3,
                    abandoned_count: 2,
                    success_rate: 66.7,
                    avg_turns: 9.8,
                    estimated_tokens: 75_000.0,
                    estimated_cost: 1.13,
                    by_agent: HashMap::from([
                        ("claude-code".to_string(), 9),
                        ("aider".to_string(), 6),
                    ]),
                },
            ],
            agent_metrics: vec![
                AgentMetrics {
                    agent: "claude-code".to_string(),
                    total_sessions: 31,
                    success_count: 23,
                    failure_count: 6,
                    abandoned_count: 2,
                    unknown_count: 0,
                    success_rate: 74.2,
                    avg_turns_success: 8.9,
                    avg_turns_all: 9.5,
                    avg_tokens_success: 4800.0,
                    specialization: HashMap::from([
                        ("debug".to_string(), 14),
                        ("feature".to_string(), 10),
                        ("refactor".to_string(), 7),
                    ]),
                    estimated_cost: 2.30,
                    cost_per_success: 0.10,
                },
                AgentMetrics {
                    agent: "aider".to_string(),
                    total_sessions: 11,
                    success_count: 7,
                    failure_count: 2,
                    abandoned_count: 2,
                    unknown_count: 0,
                    success_rate: 63.6,
                    avg_turns_success: 6.2,
                    avg_turns_all: 8.1,
                    avg_tokens_success: 3200.0,
                    specialization: HashMap::from([
                        ("debug".to_string(), 5),
                        ("feature".to_string(), 4),
                        ("refactor".to_string(), 2),
                    ]),
                    estimated_cost: 0.85,
                    cost_per_success: 0.12,
                },
            ],
            model_usage: vec![ModelUsageEntry {
                model: "claude-sonnet-4".to_string(),
                sessions: 31,
                estimated_tokens: 148_800.0,
                estimated_cost: 2.30,
                success_rate: 74.2,
            }],
            top_error_patterns: vec![ErrorPatternEntry {
                fingerprint: "ConnectionRefusedError:Connection refused to {host}:{port}"
                    .to_string(),
                occurrences: 5,
                resolution_rate: 1.0,
                agents_affected: vec!["claude-code".to_string()],
            }],
            problem_type_distribution: vec![
                ("debug".to_string(), 19, 45.2),
                ("feature".to_string(), 14, 33.3),
                ("refactor".to_string(), 9, 21.4),
            ],
            weekly_trend: vec![
                ("2026-01-05".to_string(), 3, 66.7),
                ("2026-01-12".to_string(), 4, 75.0),
            ],
            key_insights: vec![Insight {
                category: "Agent Performance".to_string(),
                headline: "claude-code leads with 74.2% success rate".to_string(),
                detail: "claude-code completed 31 sessions with strong success.".to_string(),
            }],
            pr_highlights: vec![PrHighlight {
                headline: "State of AI Coding Q1 2026 — 42 Sessions Analyzed".to_string(),
                stat: "42 coding sessions".to_string(),
                context: "Comprehensive quarterly analysis.".to_string(),
            }],
            computed_at: Utc::now(),
        }
    }
}
