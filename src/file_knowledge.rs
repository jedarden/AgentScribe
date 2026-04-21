//! File knowledge map: aggregates known gotchas, error patterns, and statistics
//! for a specific file path across all sessions.
//!
//! Combines Tantivy index data with anti-pattern sidecar files to produce
//! a comprehensive view of what's known about a file.

use crate::analytics;
use crate::config::Config;
use crate::error::Result;
use crate::index::build_schema;
use crate::search::open_index;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

/// A known gotcha extracted from anti-pattern data for a specific file.
#[derive(Debug, Clone, Serialize)]
pub struct KnownGotcha {
    /// Description of the anti-pattern or issue
    pub pattern: String,
    /// Error fingerprints associated with this gotcha
    pub error_fingerprints: Vec<String>,
    /// Sessions where this gotcha was observed
    pub affected_sessions: Vec<String>,
    /// Sessions that successfully resolved this issue (if any)
    pub resolution_sessions: Vec<String>,
}

/// Error pattern summary for a file.
#[derive(Debug, Clone, Serialize)]
pub struct FileErrorPattern {
    /// Normalized error fingerprint
    pub fingerprint: String,
    /// Number of sessions where this error appeared for this file
    pub session_count: usize,
    /// Last time this error was seen
    pub last_seen: DateTime<Utc>,
}

/// File knowledge map output.
#[derive(Debug, Serialize)]
pub struct FileKnowledge {
    /// The file path being analyzed
    pub file_path: String,
    /// Number of sessions that touched this file
    pub session_count: usize,
    /// Success rate across sessions touching this file
    pub success_rate: f64,
    /// Breakdown of problem types
    pub problem_types: HashMap<String, usize>,
    /// Known gotchas from anti-pattern data
    pub gotchas: Vec<KnownGotcha>,
    /// Common error patterns
    pub error_patterns: Vec<FileErrorPattern>,
    /// Agents that have worked on this file
    pub agents: Vec<String>,
    /// Projects where this file appears
    pub projects: Vec<String>,
    /// Session IDs for reference
    pub sample_sessions: Vec<String>,
}

/// Build a file knowledge map for a specific file path.
pub fn build_file_knowledge(
    data_dir: &Path,
    file_path: &str,
    _config: &Config,
) -> Result<FileKnowledge> {
    let sessions = find_sessions_for_file(data_dir, file_path)?;

    if sessions.is_empty() {
        return Ok(FileKnowledge {
            file_path: file_path.to_string(),
            session_count: 0,
            success_rate: 0.0,
            problem_types: HashMap::new(),
            gotchas: vec![],
            error_patterns: vec![],
            agents: vec![],
            projects: vec![],
            sample_sessions: vec![],
        });
    }

    // Step 2: Compute analytics for these sessions
    let session_ids: Vec<String> = sessions.iter().map(|s| s.session_id.clone()).collect();

    // Step 3: Aggregate statistics
    let total = sessions.len();
    let successes = sessions
        .iter()
        .filter(|s| s.outcome.as_deref() == Some("success"))
        .count();
    let success_rate = if total > 0 {
        successes as f64 / total as f64 * 100.0
    } else {
        0.0
    };

    let mut problem_types: HashMap<String, usize> = HashMap::new();
    let mut agents: Vec<String> = sessions
        .iter()
        .map(|s| s.source_agent.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    agents.sort();

    let mut projects: Vec<String> = sessions
        .iter()
        .filter_map(|s| s.project.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    projects.sort();

    for session in &sessions {
        *problem_types
            .entry(session.primary_type.as_str().to_string())
            .or_insert(0) += 1;
    }

    // Step 4: Collect error patterns
    let mut error_map: HashMap<String, (usize, DateTime<Utc>)> = HashMap::new();
    for session in &sessions {
        for fp in &session.error_fingerprints {
            let entry = error_map.entry(fp.clone()).or_insert((0, session.timestamp));
            entry.0 += 1;
            if session.timestamp > entry.1 {
                entry.1 = session.timestamp;
            }
        }
    }

    let mut error_patterns: Vec<FileErrorPattern> = error_map
        .into_iter()
        .map(|(fingerprint, (count, last_seen))| FileErrorPattern {
            fingerprint,
            session_count: count,
            last_seen,
        })
        .collect();
    error_patterns.sort_by(|a, b| b.session_count.cmp(&a.session_count));

    // Step 5: Load anti-pattern gotchas from sidecar files
    let gotchas = load_gotchas_for_sessions(data_dir, &session_ids)?;

    // Step 6: Sample sessions (last 5 by timestamp)
    let mut sorted_sessions = sessions;
    sorted_sessions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    let sample_sessions: Vec<String> = sorted_sessions
        .iter()
        .take(5)
        .map(|s| s.session_id.clone())
        .collect();

    Ok(FileKnowledge {
        file_path: file_path.to_string(),
        session_count: total,
        success_rate,
        problem_types,
        gotchas,
        error_patterns,
        agents,
        projects,
        sample_sessions,
    })
}

/// Session data for file knowledge extraction.
struct FileSessionData {
    session_id: String,
    source_agent: String,
    project: Option<String>,
    timestamp: DateTime<Utc>,
    outcome: Option<String>,
    error_fingerprints: Vec<String>,
    primary_type: analytics::ProblemType,
}

/// Find all sessions that touch a specific file path.
fn find_sessions_for_file(
    data_dir: &Path,
    file_path: &str,
) -> Result<Vec<FileSessionData>> {
    let index = open_index(data_dir)?;
    let reader = index.reader().map_err(|e| {
        crate::error::AgentScribeError::DataDir(format!("Failed to create index reader: {}", e))
    })?;
    let searcher = reader.searcher();
    let total_docs = searcher.num_docs();

    if total_docs == 0 {
        return Ok(vec![]);
    }

    let (_schema, fields) = build_schema();

    let mut sessions = Vec::new();

    use tantivy::collector::TopDocs;
    use tantivy::query::BooleanQuery;
    use tantivy::query::TermQuery;
    use tantivy::schema::IndexRecordOption;

    let fp_field = fields.file_paths;

    // Build a term query for the file path
    let term = tantivy::schema::Term::from_field_text(fp_field, file_path);
    let fp_query: Box<dyn tantivy::query::Query> =
        Box::new(TermQuery::new(term, IndexRecordOption::Basic));

    // Combine with doc_type=session filter
    let dt_field = fields.doc_type;
    let dt_term = tantivy::schema::Term::from_field_text(dt_field, "session");
    let dt_query: Box<dyn tantivy::query::Query> =
        Box::new(TermQuery::new(dt_term, IndexRecordOption::Basic));

    let combined = BooleanQuery::intersection(vec![fp_query, dt_query]);

    let all_docs: Vec<_> = searcher
        .search(&combined, &TopDocs::with_limit(total_docs as usize))
        .map_err(|e| {
            crate::error::AgentScribeError::DataDir(format!("Search failed: {}", e))
        })?;

    for (_score, doc_addr) in all_docs {
        if let Some(data) =
            analytics::extract_session_data(&searcher, doc_addr, &fields)
        {
            sessions.push(FileSessionData {
                session_id: data.session_id,
                source_agent: data.source_agent,
                project: data.project,
                timestamp: data.timestamp,
                outcome: data.outcome,
                error_fingerprints: data.error_fingerprints,
                primary_type: data.primary_type,
            });
        }
    }

    Ok(sessions)
}

/// Load gotchas from anti-pattern sidecar files for the given sessions.
fn load_gotchas_for_sessions(
    data_dir: &Path,
    session_ids: &[String],
) -> Result<Vec<KnownGotcha>> {
    let sessions_dir = data_dir.join("sessions");
    let mut gotchas = Vec::new();

    for session_id in session_ids {
        let parts: Vec<&str> = session_id.splitn(2, '/').collect();
        if parts.len() != 2 {
            continue;
        }

        let sidecar_path = sessions_dir
            .join(parts[0])
            .join(format!("{}.anti-patterns.jsonl", parts[1]));

        if !sidecar_path.exists() {
            continue;
        }

        let content = match std::fs::read_to_string(&sidecar_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }

            let pattern: crate::enrichment::antipatterns::AntiPattern = match serde_json::from_str(line) {
                Ok(p) => p,
                Err(_) => continue,
            };

            gotchas.push(KnownGotcha {
                pattern: pattern.pattern,
                error_fingerprints: pattern.error_fingerprints,
                affected_sessions: pattern.session_ids,
                resolution_sessions: pattern.alternative_session_ids,
            });
        }
    }

    // Deduplicate by pattern description
    let mut seen = std::collections::HashSet::new();
    gotchas.retain(|g| seen.insert(g.pattern.clone()));

    Ok(gotchas)
}

/// Format file knowledge as human-readable output.
pub fn format_human(knowledge: &FileKnowledge) -> String {
    let mut lines = Vec::new();

    lines.push(format!(
        "File Knowledge: {}\n",
        knowledge.file_path
    ));

    if knowledge.session_count == 0 {
        lines.push("No sessions found touching this file.".to_string());
        return lines.join("\n");
    }

    // Overview
    lines.push(format!(
        "  {} session(s) | {:.0}% success rate",
        knowledge.session_count, knowledge.success_rate,
    ));

    if !knowledge.agents.is_empty() {
        lines.push(format!("  Agents: {}", knowledge.agents.join(", ")));
    }

    if !knowledge.problem_types.is_empty() {
        let mut types: Vec<_> = knowledge.problem_types.iter().collect();
        types.sort_by(|a, b| b.1.cmp(a.1));
        let type_str: Vec<String> = types
            .iter()
            .map(|(t, c)| format!("{}:{}", t, c))
            .collect();
        lines.push(format!("  Problem types: {}", type_str.join(", ")));
    }

    lines.push(String::new());

    // Known gotchas
    if !knowledge.gotchas.is_empty() {
        lines.push("Known Gotchas".to_string());
        lines.push("-------------".to_string());
        for (i, gotcha) in knowledge.gotchas.iter().enumerate() {
            lines.push(format!(
                "{}. {}",
                i + 1,
                gotcha.pattern
            ));
            if !gotcha.error_fingerprints.is_empty() {
                let fps: Vec<&str> = gotcha.error_fingerprints.iter().map(|s| s.as_str()).take(3).collect();
                lines.push(format!("   Errors: {}", fps.join(", ")));
            }
            if !gotcha.resolution_sessions.is_empty() {
                lines.push(format!(
                    "   Resolved in: {} session(s)",
                    gotcha.resolution_sessions.len()
                ));
            }
        }
        lines.push(String::new());
    }

    // Common error patterns
    if !knowledge.error_patterns.is_empty() {
        lines.push("Common Error Patterns".to_string());
        lines.push("---------------------".to_string());
        for ep in knowledge.error_patterns.iter().take(10) {
            let fp_display = truncate_fingerprint(&ep.fingerprint, 80);
            lines.push(format!(
                "  {} ({} session(s), last: {})",
                fp_display,
                ep.session_count,
                ep.last_seen.format("%Y-%m-%d"),
            ));
        }
        lines.push(String::new());
    }

    // Sample sessions
    if !knowledge.sample_sessions.is_empty() {
        lines.push("Recent Sessions".to_string());
        lines.push("---------------".to_string());
        for sid in &knowledge.sample_sessions {
            lines.push(format!("  - {}", sid));
        }
    }

    lines.join("\n")
}

/// Truncate a fingerprint for display.
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
        assert_eq!(truncate_fingerprint("ErrorType:short", 80), "ErrorType:short");
        assert!(truncate_fingerprint(
            "SomeVeryLongErrorType:this is a very long message that should be truncated",
            40
        )
        .ends_with("..."));
    }

    #[test]
    fn test_format_human_empty() {
        let knowledge = FileKnowledge {
            file_path: "src/main.rs".to_string(),
            session_count: 0,
            success_rate: 0.0,
            problem_types: HashMap::new(),
            gotchas: vec![],
            error_patterns: vec![],
            agents: vec![],
            projects: vec![],
            sample_sessions: vec![],
        };

        let output = format_human(&knowledge);
        assert!(output.contains("src/main.rs"));
        assert!(output.contains("No sessions found"));
    }

    #[test]
    fn test_format_human_with_data() {
        let knowledge = FileKnowledge {
            file_path: "src/main.rs".to_string(),
            session_count: 10,
            success_rate: 80.0,
            problem_types: HashMap::from([
                ("debug".to_string(), 5),
                ("feature".to_string(), 3),
            ]),
            gotchas: vec![KnownGotcha {
                pattern: "Rejection window: 3 attempts without resolution".to_string(),
                error_fingerprints: vec!["CompileError:missing import".to_string()],
                affected_sessions: vec!["claude/1".to_string()],
                resolution_sessions: vec!["claude/2".to_string()],
            }],
            error_patterns: vec![FileErrorPattern {
                fingerprint: "CompileError:missing import".to_string(),
                session_count: 4,
                last_seen: Utc::now(),
            }],
            agents: vec!["claude-code".to_string(), "aider".to_string()],
            projects: vec!["/home/user/proj".to_string()],
            sample_sessions: vec!["claude/10".to_string()],
        };

        let output = format_human(&knowledge);
        assert!(output.contains("10 session(s)"));
        assert!(output.contains("80%"));
        assert!(output.contains("Known Gotchas"));
        assert!(output.contains("Rejection window"));
        assert!(output.contains("Common Error Patterns"));
        assert!(output.contains("CompileError"));
        assert!(output.contains("claude-code"));
        assert!(output.contains("debug:5"));
    }

    #[test]
    fn test_file_knowledge_empty_index() {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path();

        // Create minimal structure
        std::fs::create_dir_all(data_dir.join("index")).unwrap();
        std::fs::create_dir_all(data_dir.join("sessions")).unwrap();
        std::fs::create_dir_all(data_dir.join("plugins")).unwrap();

        // Create empty index
        let (schema, _) = build_schema();
        let index_path = data_dir.join("index").join("tantivy");
        std::fs::create_dir_all(&index_path).unwrap();
        tantivy::Index::create_in_dir(&index_path, schema).unwrap();

        let config = Config::default();
        let knowledge = build_file_knowledge(data_dir, "src/main.rs", &config).unwrap();

        assert_eq!(knowledge.session_count, 0);
        assert!(knowledge.gotchas.is_empty());
        assert!(knowledge.error_patterns.is_empty());
    }

    #[test]
    fn test_file_knowledge_with_indexed_sessions() {
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::IndexManager;

        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path();

        // Create directory structure
        std::fs::create_dir_all(data_dir.join("sessions/claude-code")).unwrap();
        std::fs::create_dir_all(data_dir.join("plugins")).unwrap();
        std::fs::create_dir_all(data_dir.join("state")).unwrap();

        let now = Utc::now();

        // Session 1: debug session touching src/auth.rs
        let mut manifest1 = SessionManifest::new("claude-code/s1".to_string(), "claude-code".to_string());
        manifest1.project = Some("/proj".to_string());
        manifest1.started = now;
        manifest1.turns = 10;
        manifest1.outcome = Some("success".to_string());
        manifest1.model = Some("claude-sonnet-4".to_string());

        let events1 = vec![
            Event::new(now, "claude-code/s1".into(), "claude-code".into(), Role::User, "fix the auth bug".into())
                .with_file_paths(vec!["src/auth.rs".to_string()])
                .with_error_fingerprints(vec!["AuthError:invalid token".to_string()]),
            Event::new(now, "claude-code/s1".into(), "claude-code".into(), Role::Assistant, "fixed it".into())
                .with_file_paths(vec!["src/auth.rs".to_string()]),
        ];

        // Session 2: another debug session touching src/auth.rs
        let mut manifest2 = SessionManifest::new("claude-code/s2".to_string(), "claude-code".to_string());
        manifest2.project = Some("/proj".to_string());
        manifest2.started = now;
        manifest2.turns = 5;
        manifest2.outcome = Some("failure".to_string());

        let events2 = vec![
            Event::new(now, "claude-code/s2".into(), "claude-code".into(), Role::User, "fix auth again".into())
                .with_file_paths(vec!["src/auth.rs".to_string()])
                .with_error_fingerprints(vec!["AuthError:invalid token".to_string()]),
        ];

        // Session 3: session touching a different file (should not appear)
        let mut manifest3 = SessionManifest::new("claude-code/s3".to_string(), "claude-code".to_string());
        manifest3.project = Some("/proj".to_string());
        manifest3.started = now;
        manifest3.turns = 3;
        manifest3.outcome = Some("success".to_string());

        let events3 = vec![
            Event::new(now, "claude-code/s3".into(), "claude-code".into(), Role::User, "add feature".into())
                .with_file_paths(vec!["src/utils.rs".to_string()]),
        ];

        // Build index with all sessions
        let mut manager = IndexManager::open(data_dir).unwrap();
        manager.begin_write().unwrap();
        manager.index_session(&events1, &manifest1).unwrap();
        manager.index_session(&events2, &manifest2).unwrap();
        manager.index_session(&events3, &manifest3).unwrap();
        manager.finish().unwrap();

        // Query file knowledge for src/auth.rs
        let config = Config::default();
        let knowledge = build_file_knowledge(data_dir, "src/auth.rs", &config).unwrap();

        assert_eq!(knowledge.session_count, 2); // s1 and s2, not s3
        assert_eq!(knowledge.agents, vec!["claude-code"]);
        assert_eq!(knowledge.error_patterns.len(), 1);
        assert_eq!(knowledge.error_patterns[0].fingerprint, "AuthError:invalid token");
        assert_eq!(knowledge.error_patterns[0].session_count, 2);

        // Success rate: 1 success out of 2 = 50%
        assert!((knowledge.success_rate - 50.0).abs() < 0.1);

        let output = format_human(&knowledge);
        assert!(output.contains("2 session(s)"));
        assert!(output.contains("50%"));
        assert!(output.contains("Common Error Patterns"));
    }

    #[test]
    fn test_file_knowledge_with_gotchas() {
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::IndexManager;

        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path();

        std::fs::create_dir_all(data_dir.join("sessions/claude-code")).unwrap();
        std::fs::create_dir_all(data_dir.join("plugins")).unwrap();
        std::fs::create_dir_all(data_dir.join("state")).unwrap();

        let now = Utc::now();

        // Create a session
        let mut manifest = SessionManifest::new("claude-code/s1".to_string(), "claude-code".to_string());
        manifest.project = Some("/proj".to_string());
        manifest.started = now;
        manifest.turns = 5;
        manifest.outcome = Some("failure".to_string());

        let events = vec![
            Event::new(now, "claude-code/s1".into(), "claude-code".into(), Role::User, "fix bug".into())
                .with_file_paths(vec!["src/db.rs".to_string()])
                .with_error_fingerprints(vec!["DBError:connection pool exhausted".to_string()]),
        ];

        // Build index
        let mut manager = IndexManager::open(data_dir).unwrap();
        manager.begin_write().unwrap();
        manager.index_session(&events, &manifest).unwrap();
        manager.finish().unwrap();

        // Write anti-pattern sidecar
        crate::enrichment::antipatterns::write_antipatterns_sidecar(
            data_dir,
            "claude-code/s1",
            &[crate::enrichment::antipatterns::AntiPattern {
                pattern: "Rejection window: 5 attempts without resolution".to_string(),
                error_fingerprints: vec!["DBError:connection pool exhausted".to_string()],
                session_ids: vec!["claude-code/s1".to_string()],
                alternative_session_ids: vec![],
            }],
        )
        .unwrap();

        // Query file knowledge
        let config = Config::default();
        let knowledge = build_file_knowledge(data_dir, "src/db.rs", &config).unwrap();

        assert_eq!(knowledge.session_count, 1);
        assert_eq!(knowledge.gotchas.len(), 1);
        assert!(knowledge.gotchas[0].pattern.contains("Rejection window"));
        assert!(knowledge.gotchas[0].error_fingerprints.contains(&"DBError:connection pool exhausted".to_string()));
    }

    #[test]
    fn test_file_knowledge_no_match() {
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::IndexManager;

        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path();

        std::fs::create_dir_all(data_dir.join("sessions/test")).unwrap();
        std::fs::create_dir_all(data_dir.join("plugins")).unwrap();
        std::fs::create_dir_all(data_dir.join("state")).unwrap();

        let now = Utc::now();
        let mut manifest = SessionManifest::new("test/s1".to_string(), "test".to_string());
        manifest.project = Some("/proj".to_string());
        manifest.started = now;
        manifest.turns = 3;

        let events = vec![
            Event::new(now, "test/s1".into(), "test".into(), Role::User, "fix bug".into())
                .with_file_paths(vec!["src/main.rs".to_string()]),
        ];

        let mut manager = IndexManager::open(data_dir).unwrap();
        manager.begin_write().unwrap();
        manager.index_session(&events, &manifest).unwrap();
        manager.finish().unwrap();

        let config = Config::default();
        let knowledge = build_file_knowledge(data_dir, "src/nonexistent.rs", &config).unwrap();

        assert_eq!(knowledge.session_count, 0);
        assert!(knowledge.gotchas.is_empty());
    }
}
