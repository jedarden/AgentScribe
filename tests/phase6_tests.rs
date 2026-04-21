//! Phase 6 integration tests: Analytics, Knowledge Synthesis & Shell Integration
//!
//! Tests:
//!   - Recurring problem detection with synthetic fingerprint data
//!   - Analytics accuracy with known session outcomes
//!   - Shell hook generation for bash, zsh, fish
//!   - GC dry-run shows deletions without acting
//!   - Rules generation produces valid output in all formats
//!   - Digest produces readable Markdown summary
//!   - File knowledge map with gotchas

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use agentscribe::analytics::{self, AnalyticsOptions, AgentMetrics};
use agentscribe::config::Config;
use agentscribe::digest::{self, DigestOptions};
use agentscribe::enrichment::antipatterns;
use agentscribe::event::{Event, Role, SessionManifest};
use agentscribe::gc;
use agentscribe::index::IndexManager;
use agentscribe::recurring::{self, RecurringOptions};
use agentscribe::rules::{self, OutputFormat, Rule};
use agentscribe::search::{execute_search, SearchOptions};
use agentscribe::shell_hook;
use chrono::{Duration, Utc};

// ─── Test helpers ─────────────────────────────────────────────────────────────

fn make_data_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    fs::create_dir_all(dir.path().join("plugins")).unwrap();
    fs::create_dir_all(dir.path().join("sessions/test-agent")).unwrap();
    fs::create_dir_all(dir.path().join("sessions/claude-code")).unwrap();
    fs::create_dir_all(dir.path().join("sessions/aider")).unwrap();
    fs::create_dir_all(dir.path().join("state")).unwrap();
    dir
}

/// Write a minimal plugin TOML so the Scraper can discover sessions.
fn write_test_plugin(data_dir: &std::path::Path, plugin_name: &str) {
    let plugin_dir = data_dir.join("plugins");
    fs::create_dir_all(&plugin_dir).unwrap();
    fs::write(
        plugin_dir.join(format!("{}.toml", plugin_name)),
        format!(
            r#"
[plugin]
name = "{name}"
version = "0.1.0"

[source]
format = "jsonl"
paths = ["/tmp/{name}-logs"]

[parser]
timestamp = "ts"
role = "role"
content = "content"
"#,
            name = plugin_name
        ),
    )
    .unwrap();
}

fn make_event(
    ts: chrono::DateTime<Utc>,
    session_id: &str,
    role: Role,
    content: &str,
) -> Event {
    Event::new(
        ts,
        session_id.to_string(),
        "test-agent".to_string(),
        role,
        content.to_string(),
    )
}

#[allow(clippy::too_many_arguments)]
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
    ts: chrono::DateTime<Utc>,
) -> (SessionManifest, Vec<Event>) {
    let mut manifest = SessionManifest::new(session_id.to_string(), agent.to_string());
    manifest.project = Some(project.to_string());
    manifest.started = ts;
    manifest.turns = turns;
    manifest.outcome = Some(outcome.to_string());
    manifest.model = model.map(|s| s.to_string());
    manifest.files_touched = file_paths.iter().map(|s| s.to_string()).collect();

    let events = vec![Event::new(
        ts,
        session_id.to_string(),
        agent.to_string(),
        Role::User,
        content.to_string(),
    )
    .with_error_fingerprints(error_fps.iter().map(|s| s.to_string()).collect())
    .with_file_paths(file_paths.iter().map(|s| s.to_string()).collect())
    .with_project(Some(project.to_string()))];

    (manifest, events)
}

/// Build a Tantivy index with the given sessions in the data directory.
fn build_index(
    data_dir: &std::path::Path,
    sessions: Vec<(SessionManifest, Vec<Event>)>,
) {
    let mut manager = IndexManager::open(data_dir).unwrap();
    manager.begin_write().unwrap();
    for (manifest, events) in &sessions {
        manager.index_session(events, manifest).unwrap();
    }
    manager.finish().unwrap();
}

/// Build an empty Tantivy index (no documents) in the data directory.
fn build_empty_index(data_dir: &std::path::Path) {
    let mut manager = IndexManager::open(data_dir).unwrap();
    manager.begin_write().unwrap();
    manager.finish().unwrap();
}

/// Write session files to disk so the scraper can find them.
fn write_session_files(
    data_dir: &std::path::Path,
    sessions: &[(SessionManifest, Vec<Event>)],
) {
    for (manifest, events) in sessions {
        let parts: Vec<&str> = manifest.session_id.splitn(2, '/').collect();
        if parts.len() != 2 {
            continue;
        }
        let dir = data_dir
            .join("sessions")
            .join(parts[0]);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{}.jsonl", parts[1]));
        let content: Vec<String> = events.iter().map(|e| e.to_jsonl().unwrap()).collect();
        fs::write(path, content.join("\n")).unwrap();
    }
}

// ─── Recurring Problem Detection ─────────────────────────────────────────────

/// Recurring detection surfaces problems that appear in 3+ sessions.
#[test]
fn test_recurring_surfaces_frequent_problems() {
    let data_dir = make_data_dir();
    let now = Utc::now();

    let sessions: Vec<(SessionManifest, Vec<Event>)> = (0..5)
        .map(|i| {
            make_test_session(
                &format!("test-agent/s{}", i),
                "test-agent",
                "/proj",
                if i < 3 { "success" } else { "failure" },
                5,
                "fix the error",
                vec!["ConnectionError:connection refused"],
                vec!["src/db.rs"],
                None,
                now - Duration::days(10 - i as i64),
            )
        })
        .collect();

    write_session_files(data_dir.path(), &sessions);
    write_test_plugin(data_dir.path(), "test-agent");

    let opts = RecurringOptions {
        since: now - Duration::days(30),
        threshold: 3,
    };

    let output = recurring::detect_recurring(data_dir.path(), &opts).unwrap();

    assert_eq!(output.problems.len(), 1);
    assert_eq!(
        output.problems[0].fingerprint,
        "ConnectionError:connection refused"
    );
    assert!(output.problems[0].session_count >= 3);
    assert!(output
        .problems[0]
        .projects
        .contains(&"/proj".to_string()));
}

/// Recurring detection respects the threshold parameter.
#[test]
fn test_recurring_threshold_filters_correctly() {
    let data_dir = make_data_dir();
    let now = Utc::now();

    // Create a fingerprint that appears exactly 2 times
    let sessions: Vec<(SessionManifest, Vec<Event>)> = (0..2)
        .map(|i| {
            make_test_session(
                &format!("test-agent/s{}", i),
                "test-agent",
                "/proj",
                "success",
                3,
                "fix error",
                vec!["RareError:only twice"],
                vec![],
                None,
                now - Duration::days(i),
            )
        })
        .collect();

    write_session_files(data_dir.path(), &sessions);
    write_test_plugin(data_dir.path(), "test-agent");

    let opts = RecurringOptions {
        since: now - Duration::days(30),
        threshold: 3,
    };

    let output = recurring::detect_recurring(data_dir.path(), &opts).unwrap();
    assert!(output.problems.is_empty(), "should not report below threshold");
}

/// Recurring detection identifies which agents fixed the problem.
#[test]
fn test_recurring_shows_fix_agents() {
    let data_dir = make_data_dir();
    let now = Utc::now();

    let sessions = vec![
        make_test_session(
            "claude-code/s1",
            "claude-code",
            "/proj",
            "success",
            5,
            "fix the error. thanks, that worked!",
            vec!["CompileError:missing import"],
            vec!["src/main.rs"],
            None,
            now - Duration::days(5),
        ),
        make_test_session(
            "aider/s2",
            "aider",
            "/proj",
            "success",
            3,
            "fix this error, great works!",
            vec!["CompileError:missing import"],
            vec!["src/main.rs"],
            None,
            now - Duration::days(3),
        ),
        make_test_session(
            "claude-code/s3",
            "claude-code",
            "/proj",
            "failure",
            4,
            "error again",
            vec!["CompileError:missing import"],
            vec![],
            None,
            now - Duration::days(1),
        ),
    ];

    write_session_files(data_dir.path(), &sessions);
    write_test_plugin(data_dir.path(), "claude-code");
    write_test_plugin(data_dir.path(), "aider");

    let opts = RecurringOptions {
        since: now - Duration::days(30),
        threshold: 3,
    };

    let output = recurring::detect_recurring(data_dir.path(), &opts).unwrap();

    assert_eq!(output.problems.len(), 1);
    let problem = &output.problems[0];
    assert!(problem.fix_agents.contains(&"claude-code".to_string()));
    assert!(problem.fix_agents.contains(&"aider".to_string()));
    assert!(problem.last_fix_session.is_some());
}

// ─── Analytics ────────────────────────────────────────────────────────────────

/// Analytics computes per-agent success rates correctly.
#[test]
fn test_analytics_per_agent_success_rate() {
    let data_dir = make_data_dir();
    let now = Utc::now();

    let sessions = vec![
        make_test_session(
            "claude-code/s1",
            "claude-code",
            "/proj",
            "success",
            10,
            "fix the bug",
            vec!["Err:X"],
            vec!["src/main.rs"],
            Some("claude-sonnet-4"),
            now - Duration::days(5),
        ),
        make_test_session(
            "claude-code/s2",
            "claude-code",
            "/proj",
            "success",
            8,
            "add feature",
            vec![],
            vec!["src/new.rs"],
            Some("claude-sonnet-4"),
            now - Duration::days(4),
        ),
        make_test_session(
            "claude-code/s3",
            "claude-code",
            "/proj",
            "failure",
            5,
            "tried to fix",
            vec!["Err:Y"],
            vec![],
            Some("claude-sonnet-4"),
            now - Duration::days(3),
        ),
    ];

    build_index(data_dir.path(), sessions);

    let config = Config::default();
    let opts = AnalyticsOptions {
        agent: None,
        project: None,
        since: None,
    };

    let output = analytics::compute_analytics(data_dir.path(), &opts, &config).unwrap();

    assert_eq!(output.total_sessions, 3);
    assert_eq!(output.agents.len(), 1);
    let agent = &output.agents[0];
    assert_eq!(agent.agent, "claude-code");
    assert_eq!(agent.success_count, 2);
    assert_eq!(agent.failure_count, 1);
    assert!((agent.success_rate - 66.7).abs() < 0.1);
}

/// Analytics filters by agent name.
#[test]
fn test_analytics_agent_filter() {
    let data_dir = make_data_dir();
    let now = Utc::now();

    let sessions = vec![
        make_test_session(
            "claude-code/s1",
            "claude-code",
            "/proj",
            "success",
            10,
            "fixed",
            vec![],
            vec![],
            None,
            now,
        ),
        make_test_session(
            "aider/s1",
            "aider",
            "/proj",
            "failure",
            5,
            "failed",
            vec![],
            vec![],
            None,
            now,
        ),
    ];

    build_index(data_dir.path(), sessions);

    let config = Config::default();
    let opts = AnalyticsOptions {
        agent: Some("aider".to_string()),
        project: None,
        since: None,
    };

    let output = analytics::compute_analytics(data_dir.path(), &opts, &config).unwrap();
    assert_eq!(output.total_sessions, 1);
    assert_eq!(output.agents[0].agent, "aider");
}

/// Analytics computes cost efficiency when model pricing is configured.
#[test]
fn test_analytics_cost_efficiency() {
    let data_dir = make_data_dir();
    let now = Utc::now();

    let sessions = vec![
        make_test_session(
            "test/s1",
            "test",
            "/p",
            "success",
            5,
            &"x".repeat(4000), // ~1000 tokens
            vec![],
            vec![],
            Some("claude-sonnet-4"),
            now,
        ),
        make_test_session(
            "test/s2",
            "test",
            "/p",
            "failure",
            3,
            &"y".repeat(4000),
            vec![],
            vec![],
            Some("claude-sonnet-4"),
            now,
        ),
    ];

    build_index(data_dir.path(), sessions);

    let mut config = Config::default();
    let mut models = HashMap::new();
    models.insert(
        "claude-sonnet-4".to_string(),
        agentscribe::config::ModelPricing {
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

    let output = analytics::compute_analytics(data_dir.path(), &opts, &config).unwrap();

    assert!(output.estimated_total_cost > 0.0);
    assert!(output.agents[0].estimated_cost > 0.0);
    assert!(output.agents[0].cost_per_success > 0.0);
}

/// Analytics classifies problem types correctly.
#[test]
fn test_analytics_problem_type_classification() {
    let data_dir = make_data_dir();
    let now = Utc::now();

    let sessions = vec![
        make_test_session(
            "test/s1",
            "test",
            "/p",
            "success",
            5,
            "fix the bug error",
            vec!["Err:A"],
            vec![],
            None,
            now,
        ),
        make_test_session(
            "test/s2",
            "test",
            "/p",
            "success",
            5,
            "implement new feature to add support for websockets",
            vec![],
            vec![],
            None,
            now,
        ),
        make_test_session(
            "test/s3",
            "test",
            "/p",
            "success",
            5,
            "update the readme documentation",
            vec![],
            vec!["README.md"],
            None,
            now,
        ),
    ];

    build_index(data_dir.path(), sessions);

    let config = Config::default();
    let opts = AnalyticsOptions {
        agent: None,
        project: None,
        since: None,
    };

    let output = analytics::compute_analytics(data_dir.path(), &opts, &config).unwrap();

    // Should have at least debug, feature, and documentation types
    assert!(output.problem_types.len() >= 3);
    let type_names: Vec<&str> = output.problem_types.iter().map(|p| p.problem_type.as_str()).collect();
    assert!(type_names.contains(&"debug"));
    assert!(type_names.contains(&"feature"));
    assert!(type_names.contains(&"documentation"));
}

// ─── Rules Generation ────────────────────────────────────────────────────────

/// Rules generation produces valid CLAUDE.md format.
#[test]
fn test_rules_generates_claude_md() {
    let temp = tempfile::tempdir().unwrap();
    let data_dir = temp.path().join(".agentscribe");
    fs::create_dir_all(data_dir.join("sessions/test-agent")).unwrap();
    fs::create_dir_all(data_dir.join("plugins")).unwrap();
    fs::create_dir_all(data_dir.join("state")).unwrap();

    let project = temp.path().join("my-project");
    fs::create_dir_all(&project).unwrap();
    let project_path = project.to_string_lossy().to_string();
    let now = Utc::now();

    // Write sessions with tool usage patterns
    for i in 0..5 {
        let session_id = format!("test-agent/s{}", i);
        let events = [
            Event::new(now, session_id.clone(), "test-agent".into(), Role::User, "fix this".into())
                .with_project(Some(project_path.clone())),
            Event::new(now, session_id.clone(), "test-agent".into(), Role::ToolCall, "cargo build".into())
                .with_tool(Some("Bash".into()))
                .with_project(Some(project_path.clone())),
            Event::new(now, session_id.clone(), "test-agent".into(), Role::User, "don't use npm, use pnpm instead".into())
                .with_project(Some(project_path.clone())),
        ];
        let path = data_dir.join("sessions/test-agent").join(format!("s{}.jsonl", i));
        let content: Vec<String> = events.iter().map(|e| e.to_jsonl().unwrap()).collect();
        fs::write(path, content.join("\n")).unwrap();
    }

    write_test_plugin(&data_dir, "test-agent");

    let output = rules::extract_rules(&data_dir, &project).unwrap();

    // Should have extracted rules
    assert!(output.sessions_analyzed > 0);

    // Generate CLAUDE.md format
    let claude_content = rules::format_claude(&output);
    if !claude_content.is_empty() {
        assert!(claude_content.contains("Auto-generated by AgentScribe"));
        // Should contain a correction about pnpm
        assert!(
            claude_content.contains("pnpm") || claude_content.contains("cargo"),
            "CLAUDE.md should contain tool preferences"
        );
    }
}

/// Rules generation produces valid .cursorrules format.
#[test]
fn test_rules_generates_cursorrules() {
    let output = rules::RulesOutput {
        rules: vec![
            Rule::Convention("Use pnpm instead of npm".to_string()),
            Rule::Warning("Watch for error X".to_string()),
            Rule::Context("Primary language: rust".to_string()),
        ],
        project_path: PathBuf::from("/tmp/test"),
        sessions_analyzed: 5,
    };

    let content = rules::format_cursor(&output);
    assert!(content.starts_with("# Auto-generated"));
    assert!(content.contains("Use pnpm"));
    assert!(content.contains("Watch out:"));
    assert!(content.contains("rust"));
}

/// Rules generation produces valid .aider.conf.yml format.
#[test]
fn test_rules_generates_aider_conf() {
    let output = rules::RulesOutput {
        rules: vec![
            Rule::Convention("Use pnpm instead of npm".to_string()),
            Rule::Correction("don't use npm, use pnpm".to_string()),
        ],
        project_path: PathBuf::from("/tmp/test"),
        sessions_analyzed: 3,
    };

    let content = rules::format_aider(&output);
    assert!(content.starts_with("# Auto-generated"));
    assert!(content.contains("message:"));
    assert!(content.contains("pnpm"));
}

/// Rules write creates file in project directory.
#[test]
fn test_rules_write_creates_file() {
    let temp = tempfile::tempdir().unwrap();
    let output = rules::RulesOutput {
        rules: vec![Rule::Convention("Use pnpm".to_string())],
        project_path: temp.path().to_path_buf(),
        sessions_analyzed: 1,
    };

    // CLAUDE.md
    let path = rules::write_rules(&output, OutputFormat::Claude, temp.path()).unwrap();
    assert!(path.ends_with("CLAUDE.md"));
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("Use pnpm"));

    // .cursorrules
    let path = rules::write_rules(&output, OutputFormat::Cursor, temp.path()).unwrap();
    assert!(path.ends_with(".cursorrules"));
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("Use pnpm"));

    // .aider.conf.yml
    let path = rules::write_rules(&output, OutputFormat::Aider, temp.path()).unwrap();
    assert!(path.ends_with(".aider.conf.yml"));
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("pnpm"));
}

// ─── Shell Hook Generation ───────────────────────────────────────────────────

/// Shell hook generates valid bash snippet with PROMPT_COMMAND.
#[test]
fn test_shell_hook_bash() {
    let config = agentscribe::config::ShellHookConfig::default();
    let snippet = shell_hook::generate_hook("bash", &config).unwrap();

    assert!(snippet.contains("PROMPT_COMMAND"));
    assert!(snippet.contains("__agentscribe_prompt_cmd"));
    assert!(snippet.contains("agentscribe search"));
    assert!(snippet.contains("--hint"));
    // Background mode should use tempfile + disown
    assert!(snippet.contains("mktemp"));
    assert!(snippet.contains("disown"));
}

/// Shell hook generates valid zsh snippet with precmd/preexec hooks.
#[test]
fn test_shell_hook_zsh() {
    let config = agentscribe::config::ShellHookConfig::default();
    let snippet = shell_hook::generate_hook("zsh", &config).unwrap();

    assert!(snippet.contains("add-zsh-hook precmd"));
    assert!(snippet.contains("add-zsh-hook preexec"));
    assert!(snippet.contains("__agentscribe_precmd"));
    assert!(snippet.contains("__agentscribe_preexec"));
    assert!(snippet.contains("agentscribe search"));
}

/// Shell hook generates valid fish snippet with fish_postexec.
#[test]
fn test_shell_hook_fish() {
    let config = agentscribe::config::ShellHookConfig::default();
    let snippet = shell_hook::generate_hook("fish", &config).unwrap();

    assert!(snippet.contains("fish_postexec"));
    assert!(snippet.contains("agentscribe search"));
    assert!(snippet.contains("--hint"));
}

/// Shell hook rejects unsupported shells.
#[test]
fn test_shell_hook_unsupported_shell() {
    let config = agentscribe::config::ShellHookConfig::default();
    let result = shell_hook::generate_hook("powershell", &config);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("unsupported shell"));
}

/// Shell hook in blocking mode does not use background subprocess.
#[test]
fn test_shell_hook_blocking_mode() {
    let config = agentscribe::config::ShellHookConfig {
        background: false,
        stderr_capture: false,
    };
    let snippet = shell_hook::generate_hook("bash", &config).unwrap();

    assert!(snippet.contains("PROMPT_COMMAND"));
    assert!(!snippet.contains("mktemp"));
    assert!(!snippet.contains("disown"));
}

// ─── GC (Garbage Collection) ─────────────────────────────────────────────────

/// GC dry-run shows what would be deleted without actually deleting.
#[test]
fn test_gc_dry_run_no_deletion() {
    let data_dir = make_data_dir();
    let now = Utc::now();

    // Write a session file that is "old"
    let old_session_id = "test-agent/old-session";
    let old_ts = now - Duration::days(100);
    let events = vec![
        make_event(old_ts, old_session_id, Role::User, "old content"),
    ];
    let session_path = data_dir.path()
        .join("sessions/test-agent/old-session.jsonl");
    let content: Vec<String> = events.iter().map(|e| e.to_jsonl().unwrap()).collect();
    fs::write(&session_path, content.join("\n")).unwrap();

    // Write a recent session that should NOT be deleted
    let recent_session_id = "test-agent/recent-session";
    let recent_events = vec![
        make_event(now, recent_session_id, Role::User, "recent content"),
    ];
    let recent_path = data_dir.path()
        .join("sessions/test-agent/recent-session.jsonl");
    let recent_content: Vec<String> = recent_events.iter().map(|e| e.to_jsonl().unwrap()).collect();
    fs::write(&recent_path, recent_content.join("\n")).unwrap();

    // Write a plugin file for the scraper
    fs::write(
        data_dir.path().join("plugins/test-agent.toml"),
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

    let result = gc::run_gc(data_dir.path(), Duration::days(90), true).unwrap();

    assert!(result.dry_run);
    assert!(result.candidate_sessions.contains(&old_session_id.to_string()));
    assert!(!result.candidate_sessions.contains(&recent_session_id.to_string()));
    assert_eq!(result.sessions_deleted, 0);

    // Files should still exist after dry-run
    assert!(session_path.exists(), "old session file should still exist after dry-run");
    assert!(recent_path.exists(), "recent session file should still exist");
}

/// GC with act=true actually deletes old sessions.
#[test]
fn test_gc_actually_deletes_old_sessions() {
    let data_dir = make_data_dir();
    let now = Utc::now();

    // Write an old session
    let old_session_id = "test-agent/very-old";
    let old_ts = now - Duration::days(200);
    let events = vec![
        make_event(old_ts, old_session_id, Role::User, "ancient content"),
    ];
    let session_path = data_dir.path()
        .join("sessions/test-agent/very-old.jsonl");
    let content: Vec<String> = events.iter().map(|e| e.to_jsonl().unwrap()).collect();
    fs::write(&session_path, content.join("\n")).unwrap();

    // Write plugin
    fs::write(
        data_dir.path().join("plugins/test-agent.toml"),
        r#"
[plugin]
name = "test-agent"
version = "0.1.0"

[source]
format = "jsonl"
paths = ["/tmp/test"]

[parser]
timestamp = "ts"
role = "role"
content = "content"
"#,
    )
    .unwrap();

    let result = gc::run_gc(data_dir.path(), Duration::days(90), false).unwrap();

    assert!(!result.dry_run);
    assert!(result.sessions_deleted >= 1);
    assert!(!session_path.exists(), "old session file should be deleted");
}

/// GC respects the duration parameter.
#[test]
fn test_gc_duration_parsing() {
    assert_eq!(gc::parse_duration("90d").unwrap(), Duration::days(90));
    assert_eq!(gc::parse_duration("12w").unwrap(), Duration::weeks(12));
    assert_eq!(gc::parse_duration("6mo").unwrap(), Duration::days(180));
    assert_eq!(gc::parse_duration("48h").unwrap(), Duration::hours(48));
    assert!(gc::parse_duration("abc").is_err());
}

// ─── Digest ───────────────────────────────────────────────────────────────────

/// Digest produces readable Markdown with key sections.
#[test]
fn test_digest_produces_markdown() {
    let data_dir = make_data_dir();
    let now = Utc::now();

    let sessions = vec![
        make_test_session(
            "claude-code/s1",
            "claude-code",
            "/my-project",
            "success",
            10,
            "fix the connection pool bug",
            vec!["PoolError:exhausted"],
            vec!["src/db.rs"],
            Some("claude-sonnet-4"),
            now - Duration::days(2),
        ),
        make_test_session(
            "claude-code/s2",
            "claude-code",
            "/my-project",
            "failure",
            5,
            "tried to fix auth",
            vec!["AuthError:invalid token"],
            vec!["src/auth.rs"],
            Some("claude-sonnet-4"),
            now - Duration::days(1),
        ),
        make_test_session(
            "aider/s1",
            "aider",
            "/other-project",
            "success",
            3,
            "add new feature",
            vec![],
            vec!["src/feature.rs"],
            None,
            now,
        ),
    ];

    build_index(data_dir.path(), sessions);

    let config = Config::default();
    let opts = DigestOptions {
        since: now - Duration::days(7),
        output: None,
        json: false,
    };

    let output = digest::generate_digest(data_dir.path(), &opts, &config).unwrap();
    let md = digest::format_markdown(&output);

    // Key sections should be present
    assert!(md.contains("# AgentScribe Digest"));
    assert!(md.contains("## Sessions by Agent"));
    assert!(md.contains("claude-code"));
    assert!(md.contains("## Agent Comparison"));
    assert!(md.contains("## Most-Touched Files"));
    assert!(md.contains("src/db.rs"));
}

/// Digest with no sessions in the period produces a valid empty report.
#[test]
fn test_digest_empty_period() {
    let data_dir = make_data_dir();
    // Create an empty index so analytics/digest can query it
    build_empty_index(data_dir.path());

    let config = Config::default();
    let opts = DigestOptions {
        since: Utc::now() - Duration::days(7),
        output: None,
        json: false,
    };

    let output = digest::generate_digest(data_dir.path(), &opts, &config).unwrap();
    let md = digest::format_markdown(&output);

    assert!(md.contains("# AgentScribe Digest"));
    assert!(md.contains("No sessions found"));
}

/// Digest JSON output is valid JSON.
#[test]
fn test_digest_json_output() {
    let data_dir = make_data_dir();
    let now = Utc::now();

    let sessions = vec![make_test_session(
        "test/s1",
        "test",
        "/p",
        "success",
        5,
        "content",
        vec![],
        vec![],
        None,
        now,
    )];

    build_index(data_dir.path(), sessions);

    let config = Config::default();
    let opts = DigestOptions {
        since: now - Duration::days(7),
        output: None,
        json: true,
    };

    let output = digest::generate_digest(data_dir.path(), &opts, &config).unwrap();
    let json_str = digest::format_json(&output);

    // Should be valid JSON
    let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("digest JSON should be valid");
    assert!(parsed.get("total_sessions").is_some());
    assert!(parsed.get("analytics").is_some());
}

// ─── File Knowledge Map ──────────────────────────────────────────────────────

/// File knowledge shows gotchas from anti-pattern data.
#[test]
fn test_file_knowledge_with_gotchas() {
    let data_dir = make_data_dir();
    let now = Utc::now();

    let sessions = vec![
        make_test_session(
            "claude-code/s1",
            "claude-code",
            "/proj",
            "failure",
            5,
            "fix the db error",
            vec!["DBError:pool exhausted"],
            vec!["src/db.rs"],
            None,
            now,
        ),
    ];

    build_index(data_dir.path(), sessions.clone());
    write_session_files(data_dir.path(), &sessions);

    // Write anti-pattern sidecar
    antipatterns::write_antipatterns_sidecar(
        data_dir.path(),
        "claude-code/s1",
        &[antipatterns::AntiPattern {
            pattern: "Rejection window: 5 attempts without resolution".to_string(),
            error_fingerprints: vec!["DBError:pool exhausted".to_string()],
            session_ids: vec!["claude-code/s1".to_string()],
            alternative_session_ids: vec![],
        }],
    )
    .unwrap();

    let config = Config::default();
    let knowledge =
        agentscribe::file_knowledge::build_file_knowledge(data_dir.path(), "src/db.rs", &config)
            .unwrap();

    assert_eq!(knowledge.session_count, 1);
    assert_eq!(knowledge.gotchas.len(), 1);
    assert!(knowledge.gotchas[0].pattern.contains("Rejection window"));
    assert!(!knowledge.error_patterns.is_empty());

    let human = agentscribe::file_knowledge::format_human(&knowledge);
    assert!(human.contains("Known Gotchas"));
    assert!(human.contains("Common Error Patterns"));
}

/// File knowledge for unknown file returns empty results.
#[test]
fn test_file_knowledge_unknown_file() {
    let data_dir = make_data_dir();
    // Create an empty index so file_knowledge can query it
    build_empty_index(data_dir.path());
    let config = Config::default();

    let knowledge =
        agentscribe::file_knowledge::build_file_knowledge(data_dir.path(), "nonexistent.rs", &config)
            .unwrap();

    assert_eq!(knowledge.session_count, 0);
    assert!(knowledge.gotchas.is_empty());
    assert!(knowledge.error_patterns.is_empty());
}

// ─── Full Pipeline Integration ────────────────────────────────────────────────

/// Full pipeline: index sessions → analytics → verify metrics.
#[test]
fn test_pipeline_index_to_analytics() {
    let data_dir = make_data_dir();
    let now = Utc::now();

    let sessions: Vec<(SessionManifest, Vec<Event>)> = (0..10)
        .map(|i| {
            let agent = if i % 2 == 0 { "claude-code" } else { "aider" };
            let outcome = if i < 7 { "success" } else { "failure" };
            make_test_session(
                &format!("{}/s{}", agent, i),
                agent,
                "/test-project",
                outcome,
                5 + i as u32,
                if i < 5 {
                    "fix the bug error crash"
                } else {
                    "implement new feature to add websockets support"
                },
                if i < 5 {
                    vec!["CompileError:something"]
                } else {
                    vec![]
                },
                if i % 3 == 0 {
                    vec!["src/main.rs"]
                } else {
                    vec![]
                },
                if agent == "claude-code" {
                    Some("claude-sonnet-4")
                } else {
                    None
                },
                now - Duration::days(i as i64),
            )
        })
        .collect();

    build_index(data_dir.path(), sessions);

    let config = Config::default();

    // Analytics
    let opts = AnalyticsOptions {
        agent: None,
        project: None,
        since: Some(now - Duration::days(30)),
    };
    let output = analytics::compute_analytics(data_dir.path(), &opts, &config).unwrap();
    assert_eq!(output.total_sessions, 10);
    assert!(output.agents.len() >= 2);
    assert!(output.overall_success_rate > 0.0);

    // Search should work
    let search_opts = SearchOptions {
        query: Some("bug error".to_string()),
        max_results: 5,
        ..Default::default()
    };
    let search_output = execute_search(data_dir.path(), &search_opts).unwrap();
    assert!(search_output.total_matches > 0);
}

/// Full pipeline: index sessions → recurring detection → verify.
#[test]
fn test_pipeline_index_to_recurring() {
    let data_dir = make_data_dir();
    let now = Utc::now();

    // Create 4 sessions with the same error fingerprint
    let sessions: Vec<(SessionManifest, Vec<Event>)> = (0..4)
        .map(|i| {
            make_test_session(
                &format!("test-agent/s{}", i),
                "test-agent",
                "/proj",
                if i < 3 { "success" } else { "failure" },
                5,
                "fix the connection error",
                vec!["ConnectionError:timeout"],
                vec!["src/network.rs"],
                None,
                now - Duration::days(3 - i as i64),
            )
        })
        .collect();

    write_session_files(data_dir.path(), &sessions);
    write_test_plugin(data_dir.path(), "test-agent");

    let opts = RecurringOptions {
        since: now - Duration::days(30),
        threshold: 3,
    };

    let output = recurring::detect_recurring(data_dir.path(), &opts).unwrap();

    assert!(
        output.problems.iter().any(|p| p.fingerprint == "ConnectionError:timeout"),
        "should detect ConnectionError:timeout as recurring"
    );
}

/// Human-readable formatting for all Phase 6 outputs produces valid output.
#[test]
fn test_human_readable_formatting() {
    // Analytics
    let analytics_output = analytics::AnalyticsOutput {
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
            specialization: HashMap::from([("debug".to_string(), 5)]),
            estimated_cost: 1.50,
            cost_per_success: 0.21,
        }],
        problem_types: vec![],
        trends: vec![],
        estimated_total_cost: 1.50,
        computed_at: Utc::now(),
    };
    let formatted = analytics::format_human(&analytics_output);
    assert!(formatted.contains("Agent Analytics"));
    assert!(formatted.contains("70.0%"));
    assert!(formatted.contains("claude-code"));

    // Recurring
    let recurring_output = recurring::RecurringOutput {
        since: Utc::now() - Duration::days(30),
        threshold: 3,
        problems: vec![],
    };
    let formatted = recurring::format_human(&recurring_output);
    assert!(formatted.contains("No recurring problems") || formatted.contains("Recurring problems"));

    // GC
    let gc_result = gc::GcResult {
        sessions_deleted: 0,
        bytes_reclaimed: 1024 * 50, // 50 KB
        candidate_sessions: vec!["test/old".to_string()],
        dry_run: true,
    };
    let formatted = gc::format_human(&gc_result);
    assert!(formatted.contains("Dry run"));
    assert!(formatted.contains("test/old"));
}
