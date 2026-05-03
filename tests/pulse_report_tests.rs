//! Integration tests for pulse-report command.
//!
//! Tests:
//!   - Quarter parsing (current, YYYY-Q1..Q4)
//!   - Markdown output format correctness
//!   - HTML self-contained output
//!   - JSON output structure
//!   - Empty index edge case (no sessions)

use std::fs;

use agentscribe::config::Config;
use agentscribe::pulse_report::{
    format_html, format_json, format_markdown, parse_quarter, PulseReportOptions,
    ReportFormat,
};
use agentscribe::scraper::Scraper;
use chrono::{Datelike, Timelike, Utc};

// ─── Test helpers ─────────────────────────────────────────────────────────────

/// Create a temp data directory with the required sub-structure.
fn make_data_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    fs::create_dir_all(dir.path().join("plugins")).unwrap();
    fs::create_dir_all(dir.path().join("sessions")).unwrap();
    fs::create_dir_all(dir.path().join("state")).unwrap();
    dir
}

/// Build a JSONL-format plugin pointing at the given glob pattern.
fn jsonl_plugin(name: &str, glob: &str) -> agentscribe::plugin::Plugin {
    agentscribe::plugin::Plugin {
        plugin: agentscribe::plugin::PluginMeta {
            name: name.to_string(),
            version: "1.0".to_string(),
        },
        source: agentscribe::plugin::Source {
            paths: vec![glob.to_string()],
            exclude: vec![],
            format: agentscribe::plugin::LogFormat::Jsonl,
            session_detection: agentscribe::plugin::SessionDetection::OneFilePerSession {
                session_id_from: agentscribe::plugin::SessionIdSource::Filename,
            },
            tree: None,
            truncation_limit: None,
        },
        parser: agentscribe::plugin::Parser {
            timestamp: Some("timestamp".to_string()),
            role: Some("message.role".to_string()),
            content: Some("message.content".to_string()),
            type_field: Some("type".to_string()),
            tool_name: Some("message.tool".to_string()),
            project: Some(agentscribe::plugin::ProjectDetection::Field {
                field: "cwd".to_string(),
            }),
            model: Some(agentscribe::plugin::ModelDetection::None),
            file_paths: Some(agentscribe::plugin::FilePathExtraction {
                tool_call_field: Some("input.file_path".to_string()),
                content_regex: Some(true),
            }),
            ..Default::default()
        },
        metadata: None,
    }
}

/// Generate test session JSONL files for specific quarters.
fn generate_quarter_sessions(dir: &std::path::Path, year: i32, quarter: u8, count: usize) {
    let (start_month, end_month) = match quarter {
        1 => (1, 3),
        2 => (4, 6),
        3 => (7, 9),
        4 => (10, 12),
        _ => return,
    };

    for i in 0..count {
        let session_id = format!("q{}-session-{:04}", quarter, i);
        // Distribute sessions across the quarter's months
        let month = start_month + ((i % ((end_month - start_month + 1) as usize)) as u32);
        let day = 1 + ((i % 28) as u32);

        let timestamp = format!("{}-{:02}-{:02}T10:00:00Z", year, month, day);

        let content = format!(
            "{}\n{}\n",
            serde_json::json!({
                "type": "user",
                "uuid": format!("u{}-1", i),
                "sessionId": session_id,
                "timestamp": timestamp,
                "cwd": format!("/home/user/project-{}", i % 3),
                "version": "1.0.0",
                "message": {"role": "user", "content": format!("Fix the bug in module {}", i)}
            }),
            serde_json::json!({
                "type": "assistant",
                "uuid": format!("u{}-2", i),
                "sessionId": session_id,
                "timestamp": timestamp,
                "cwd": format!("/home/user/project-{}", i % 3),
                "message": {"role": "assistant", "content": format!("I'll fix the bug in module {}. The issue is a missing null check.", i)}
            })
        );
        let file_path = dir.join(format!("{}.jsonl", session_id));
        fs::write(&file_path, content).expect("failed to write session");
    }
}

// ─── Quarter parsing tests ──────────────────────────────────────────────────────

/// Parsing "current" returns the current calendar quarter.
#[test]
fn test_quarter_parsing_current() {
    let q = parse_quarter("current").unwrap();
    let now = Utc::now();
    assert!(q.start <= now, "quarter start should be in the past or now");
    assert!(q.end >= now, "quarter end should be in the future or now");

    // Verify the quarter label is valid
    assert!(q.label.starts_with('Q'), "label should start with Q");
    assert!(q.label.contains(&now.year().to_string()), "label should contain year");

    // Verify the key format
    assert!(q.key.contains('-'), "key should contain hyphen");
    let parts: Vec<&str> = q.key.split('-').collect();
    assert_eq!(parts.len(), 2, "key should have two parts separated by hyphen");
    assert!(parts[0].parse::<i32>().is_ok(), "first part should be a valid year");
    assert!(parts[1].starts_with('Q'), "second part should start with Q");
}

/// Parsing "YYYY-Q1" through "YYYY-Q4" returns correct quarters.
#[test]
fn test_quarter_parsing_explicit_quarters() {
    // Test Q1 2025
    let q1 = parse_quarter("2025-Q1").unwrap();
    assert_eq!(q1.key, "2025-Q1");
    assert_eq!(q1.label, "Q1 2025");
    assert_eq!(q1.start.year(), 2025);
    assert_eq!(q1.start.month(), 1);
    assert_eq!(q1.start.day(), 1);
    assert_eq!(q1.end.month(), 3);
    assert_eq!(q1.end.day(), 31);
    assert_eq!(q1.end.hour(), 23);
    assert_eq!(q1.end.minute(), 59);
    assert_eq!(q1.end.second(), 59);

    // Test Q2 2025
    let q2 = parse_quarter("2025-Q2").unwrap();
    assert_eq!(q2.key, "2025-Q2");
    assert_eq!(q2.start.month(), 4);
    assert_eq!(q2.end.month(), 6);
    assert_eq!(q2.end.day(), 30);

    // Test Q3 2025
    let q3 = parse_quarter("2025-Q3").unwrap();
    assert_eq!(q3.key, "2025-Q3");
    assert_eq!(q3.start.month(), 7);
    assert_eq!(q3.end.month(), 9);
    assert_eq!(q3.end.day(), 30);

    // Test Q4 2025
    let q4 = parse_quarter("2025-Q4").unwrap();
    assert_eq!(q4.key, "2025-Q4");
    assert_eq!(q4.start.month(), 10);
    assert_eq!(q4.end.month(), 12);
    assert_eq!(q4.end.day(), 31);

    // Test lowercase
    let q_lower = parse_quarter("2025-q1").unwrap();
    assert_eq!(q_lower.key, "2025-Q1");
}

/// Invalid quarter strings return errors.
#[test]
fn test_quarter_parsing_invalid() {
    // Invalid quarter number
    assert!(parse_quarter("2025-Q5").is_err());
    assert!(parse_quarter("2025-Q0").is_err());

    // Missing quarter part
    assert!(parse_quarter("2025").is_err());

    // Invalid format
    assert!(parse_quarter("bad-input").is_err());
    assert!(parse_quarter("2025/01").is_err());

    // Invalid year
    assert!(parse_quarter("abc-Q1").is_err());
}

// ─── Empty index edge case ───────────────────────────────────────────────────────

/// Empty index (no sessions) returns a valid report with zero counts.
#[test]
fn test_empty_index_returns_valid_report() {
    let data_dir = make_data_dir();

    // Create a proper empty Tantivy index
    use agentscribe::index::build_schema;
    use tantivy::Index;

    let index_path = data_dir.path().join("index").join("tantivy");
    fs::create_dir_all(&index_path).unwrap();

    let schema = build_schema().0;
    Index::create_in_dir(&index_path, schema).expect("failed to create index");

    let config = Config::default();
    let opts = PulseReportOptions {
        quarter: "2025-Q1".to_string(),
        output: None,
        format: ReportFormat::Markdown,
    };

    let report =
        agentscribe::pulse_report::generate_pulse_report(data_dir.path(), &opts, &config);

    assert!(report.is_ok(), "empty index should return Ok report: {:?}", report.err());

    let r = report.unwrap();
    assert_eq!(r.total_sessions, 0);
    assert_eq!(r.overall_success_rate, 0.0);
    assert_eq!(r.monthly_breakdown.len(), 0);
    assert_eq!(r.agent_metrics.len(), 0);
    assert_eq!(r.model_usage.len(), 0);
}

// ─── Markdown output format ───────────────────────────────────────────────────────

/// Markdown output contains all required sections and proper formatting.
#[test]
fn test_markdown_output_format() {
    let data_dir = make_data_dir();
    let sessions_src = tempfile::tempdir().expect("tempdir for sessions");

    // Generate sessions for Q1 2025
    generate_quarter_sessions(sessions_src.path(), 2025, 1, 10);

    let glob = format!("{}/*.jsonl", sessions_src.path().display());
    let plugin = jsonl_plugin("claude-code", &glob);

    let mut scraper = Scraper::new(data_dir.path().to_path_buf()).expect("scraper init");
    scraper.plugin_manager_mut().add_plugin(plugin.clone());
    scraper.scrape_plugin(&plugin).expect("scrape failed");

    let config = Config::default();
    let opts = PulseReportOptions {
        quarter: "2025-Q1".to_string(),
        output: None,
        format: ReportFormat::Markdown,
    };

    let report =
        agentscribe::pulse_report::generate_pulse_report(data_dir.path(), &opts, &config)
            .expect("generate failed");

    let md = format_markdown(&report);

    // Verify title and metadata
    assert!(md.contains("# Pulse Report: State of AI Coding"));
    assert!(md.contains("Q1 2025"));
    assert!(md.contains("> **Period:**"));
    assert!(md.contains("> **Generated:**"));

    // Verify all sections are present
    assert!(md.contains("## Executive Summary"));
    assert!(md.contains("## Monthly Breakdown"));
    assert!(md.contains("## Agent Comparison"));
    assert!(md.contains("## Problem Type Distribution"));
    assert!(md.contains("## Model Usage"));
    assert!(md.contains("## Top Error Patterns"));
    assert!(md.contains("## Key Insights"));
    assert!(md.contains("## PR & Media Highlights"));
    assert!(md.contains("## Methodology"));

    // Verify ASCII charts are present
    assert!(md.contains('█'), "Markdown should contain ASCII bar characters");

    // Verify table formatting
    assert!(md.contains("|"), "Markdown should contain table separators");
    assert!(md.contains("---"), "Markdown should contain table header separators");

    // Verify data is populated
    assert!(md.contains("Total Sessions"));
    assert!(report.total_sessions > 0, "should have sessions in report");
}

// ─── HTML output format ──────────────────────────────────────────────────────────

/// HTML output is self-contained with embedded CSS and proper structure.
#[test]
fn test_html_output_self_contained() {
    let data_dir = make_data_dir();
    let sessions_src = tempfile::tempdir().expect("tempdir for sessions");

    // Generate sessions for Q1 2025
    generate_quarter_sessions(sessions_src.path(), 2025, 1, 10);

    let glob = format!("{}/*.jsonl", sessions_src.path().display());
    let plugin = jsonl_plugin("claude-code", &glob);

    let mut scraper = Scraper::new(data_dir.path().to_path_buf()).expect("scraper init");
    scraper.plugin_manager_mut().add_plugin(plugin.clone());
    scraper.scrape_plugin(&plugin).expect("scrape failed");

    let config = Config::default();
    let opts = PulseReportOptions {
        quarter: "2025-Q1".to_string(),
        output: None,
        format: ReportFormat::Html,
    };

    let report =
        agentscribe::pulse_report::generate_pulse_report(data_dir.path(), &opts, &config)
            .expect("generate failed");

    let html = format_html(&report);

    // Verify document structure
    assert!(html.contains("<!DOCTYPE html>"));
    assert!(html.contains("<html lang=\"en\">"));
    assert!(html.contains("<head>"));
    assert!(html.contains("<body>"));
    assert!(html.contains("</html>"));

    // Verify CSS is embedded (no external links for styles)
    assert!(html.contains("<style>"));
    assert!(html.contains("</style>"));
    assert!(!html.contains("<link rel=\"stylesheet\""), "should not have external CSS links");

    // Verify required CSS classes are present
    assert!(html.contains("class=\"report-header\""));
    assert!(html.contains("class=\"summary-cards\""));
    assert!(html.contains("class=\"card\""));
    assert!(html.contains("class=\"chart\""));
    assert!(html.contains("class=\"bar-row\""));
    assert!(html.contains("class=\"insight\""));

    // Verify key sections
    assert!(html.contains("<h2>Monthly Breakdown</h2>"));
    assert!(html.contains("<h2>Agent Comparison</h2>"));
    assert!(html.contains("<h2>Problem Type Distribution</h2>"));
    assert!(html.contains("<h2>Model Usage</h2>"));
    assert!(html.contains("<h2>Top Error Patterns</h2>"));
    assert!(html.contains("<h2>Key Insights</h2>"));
    assert!(html.contains("<h2>PR &amp; Media Highlights</h2>"));
    assert!(html.contains("<h2>Methodology</h2>"));

    // Verify tables are present
    assert!(html.contains("<table>"));
    assert!(html.contains("<thead>"));
    assert!(html.contains("<tbody>"));
    assert!(html.contains("<th>"));
    assert!(html.contains("<td>"));

    // Verify footer
    assert!(html.contains("<footer>"));
    assert!(html.contains("AgentScribe"));

    // Verify HTML escaping for & character (the title "PR & Media Highlights" gets escaped)
    assert!(html.contains("&amp;"));
}

// ─── JSON output structure ───────────────────────────────────────────────────────

/// JSON output has correct structure and serializes properly.
#[test]
fn test_json_output_structure() {
    let data_dir = make_data_dir();
    let sessions_src = tempfile::tempdir().expect("tempdir for sessions");

    // Generate sessions for Q1 2025
    generate_quarter_sessions(sessions_src.path(), 2025, 1, 10);

    let glob = format!("{}/*.jsonl", sessions_src.path().display());
    let plugin = jsonl_plugin("claude-code", &glob);

    let mut scraper = Scraper::new(data_dir.path().to_path_buf()).expect("scraper init");
    scraper.plugin_manager_mut().add_plugin(plugin.clone());
    scraper.scrape_plugin(&plugin).expect("scrape failed");

    let config = Config::default();
    let opts = PulseReportOptions {
        quarter: "2025-Q1".to_string(),
        output: None,
        format: ReportFormat::Json,
    };

    let report =
        agentscribe::pulse_report::generate_pulse_report(data_dir.path(), &opts, &config)
            .expect("generate failed");

    let json_str = format_json(&report);

    // Verify valid JSON
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("should be valid JSON");

    // Verify top-level fields exist
    assert!(parsed.get("quarter").is_some());
    assert!(parsed.get("period_start").is_some());
    assert!(parsed.get("period_end").is_some());
    assert!(parsed.get("total_sessions").is_some());
    assert!(parsed.get("overall_success_rate").is_some());
    assert!(parsed.get("overall_avg_turns").is_some());
    assert!(parsed.get("estimated_total_tokens").is_some());
    assert!(parsed.get("estimated_total_cost").is_some());
    assert!(parsed.get("monthly_breakdown").is_some());
    assert!(parsed.get("agent_metrics").is_some());
    assert!(parsed.get("model_usage").is_some());
    assert!(parsed.get("top_error_patterns").is_some());
    assert!(parsed.get("problem_type_distribution").is_some());
    assert!(parsed.get("weekly_trend").is_some());
    assert!(parsed.get("key_insights").is_some());
    assert!(parsed.get("pr_highlights").is_some());
    assert!(parsed.get("computed_at").is_some());

    // Verify types
    assert!(parsed["quarter"].is_string());
    assert!(parsed["total_sessions"].is_number());
    assert!(parsed["overall_success_rate"].is_number());
    assert!(parsed["monthly_breakdown"].is_array());
    assert!(parsed["agent_metrics"].is_array());

    // Verify values match the report struct
    assert_eq!(
        parsed["quarter"].as_str().unwrap(),
        report.quarter,
        "quarter should match"
    );
    assert_eq!(
        parsed["total_sessions"].as_u64().unwrap() as usize,
        report.total_sessions,
        "total_sessions should match"
    );
}

// ─── ReportFormat parsing ────────────────────────────────────────────────────────

/// ReportFormat parses valid format strings correctly.
#[test]
fn test_report_format_parsing() {
    assert_eq!(
        ReportFormat::parse("markdown"),
        Some(ReportFormat::Markdown)
    );
    assert_eq!(ReportFormat::parse("md"), Some(ReportFormat::Markdown));
    assert_eq!(ReportFormat::parse("Markdown"), Some(ReportFormat::Markdown));
    assert_eq!(ReportFormat::parse("MARKDOWN"), Some(ReportFormat::Markdown));

    assert_eq!(ReportFormat::parse("html"), Some(ReportFormat::Html));
    assert_eq!(ReportFormat::parse("HTML"), Some(ReportFormat::Html));
    assert_eq!(ReportFormat::parse("Html"), Some(ReportFormat::Html));

    assert_eq!(ReportFormat::parse("json"), Some(ReportFormat::Json));
    assert_eq!(ReportFormat::parse("JSON"), Some(ReportFormat::Json));
    assert_eq!(ReportFormat::parse("Json"), Some(ReportFormat::Json));

    // Invalid formats
    assert_eq!(ReportFormat::parse("pdf"), None);
    assert_eq!(ReportFormat::parse("txt"), None);
    assert_eq!(ReportFormat::parse("xml"), None);
    assert_eq!(ReportFormat::parse(""), None);
}

/// ReportFormat::extension returns correct file extensions.
#[test]
fn test_report_format_extensions() {
    assert_eq!(ReportFormat::Markdown.extension(), "md");
    assert_eq!(ReportFormat::Html.extension(), "html");
    assert_eq!(ReportFormat::Json.extension(), "json");
}

// ─── Full integration with scraped data ───────────────────────────────────────────

/// Full integration: scrape sessions, generate report, verify all sections.
#[test]
fn test_full_pulse_report_integration() {
    let data_dir = make_data_dir();
    let sessions_src = tempfile::tempdir().expect("tempdir for sessions");

    // Generate sessions for Q1 2025 only (to avoid quarter filtering complexity)
    generate_quarter_sessions(sessions_src.path(), 2025, 1, 10);

    let glob = format!("{}/*.jsonl", sessions_src.path().display());
    let plugin = jsonl_plugin("claude-code", &glob);

    let mut scraper = Scraper::new(data_dir.path().to_path_buf()).expect("scraper init");
    scraper.plugin_manager_mut().add_plugin(plugin.clone());
    let scrape_result = scraper.scrape_plugin(&plugin).expect("scrape failed");

    assert_eq!(scrape_result.sessions_scraped, 10);
    assert_eq!(scrape_result.sessions_indexed, 10);

    // Test Q1 report (should have 10 sessions)
    let config = Config::default();
    let q1_opts = PulseReportOptions {
        quarter: "2025-Q1".to_string(),
        output: None,
        format: ReportFormat::Markdown,
    };

    let q1_report =
        agentscribe::pulse_report::generate_pulse_report(data_dir.path(), &q1_opts, &config)
            .expect("Q1 generate failed");

    assert_eq!(q1_report.quarter, "Q1 2025");
    assert_eq!(q1_report.total_sessions, 10, "Q1 should have 10 sessions");

    // Verify monthly breakdown for Q1
    assert_eq!(q1_report.monthly_breakdown.len(), 3, "Q1 should have 3 months");
    let month_keys: Vec<&str> = q1_report
        .monthly_breakdown
        .iter()
        .map(|m| m.month_key.as_str())
        .collect();
    assert!(month_keys.contains(&"2025-01"));
    assert!(month_keys.contains(&"2025-02"));
    assert!(month_keys.contains(&"2025-03"));

    // Verify agent metrics
    assert_eq!(q1_report.agent_metrics.len(), 1, "should have 1 agent");
    assert_eq!(q1_report.agent_metrics[0].agent, "claude-code");
    assert_eq!(q1_report.agent_metrics[0].total_sessions, 10);
}

// ─── Edge cases ──────────────────────────────────────────────────────────────────

/// Report with sessions but no errors still generates valid output.
#[test]
fn test_report_without_error_patterns() {
    let data_dir = make_data_dir();
    let sessions_src = tempfile::tempdir().expect("tempdir for sessions");

    // Generate a single session with no error content
    let session_id = "clean-session";
    let content = format!(
        "{}\n{}\n",
        serde_json::json!({
            "type": "user",
            "uuid": "u1",
            "sessionId": session_id,
            "timestamp": "2025-01-15T10:00:00Z",
            "cwd": "/project",
            "version": "1.0.0",
            "message": {"role": "user", "content": "Add a new feature"}
        }),
        serde_json::json!({
            "type": "assistant",
            "uuid": "u2",
            "sessionId": session_id,
            "timestamp": "2025-01-15T10:00:30Z",
            "cwd": "/project",
            "message": {"role": "assistant", "content": "Feature added successfully"}
        })
    );
    let file_path = sessions_src.path().join(format!("{}.jsonl", session_id));
    fs::write(&file_path, content).unwrap();

    let glob = format!("{}/*.jsonl", sessions_src.path().display());
    let plugin = jsonl_plugin("claude-code", &glob);

    let mut scraper = Scraper::new(data_dir.path().to_path_buf()).expect("scraper init");
    scraper.plugin_manager_mut().add_plugin(plugin.clone());
    scraper.scrape_plugin(&plugin).expect("scrape failed");

    let config = Config::default();
    let opts = PulseReportOptions {
        quarter: "2025-Q1".to_string(),
        output: None,
        format: ReportFormat::Markdown,
    };

    let report =
        agentscribe::pulse_report::generate_pulse_report(data_dir.path(), &opts, &config)
            .expect("generate failed");

    assert_eq!(report.total_sessions, 1);
    assert_eq!(report.top_error_patterns.len(), 0, "should have no error patterns");

    // Verify formats handle empty error patterns gracefully
    let md = format_markdown(&report);
    assert!(md.contains("No recurring error patterns detected"));

    let html = format_html(&report);
    assert!(html.contains("No recurring error patterns detected"));

    let json = format_json(&report);
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        parsed["top_error_patterns"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0),
        0
    );
}
