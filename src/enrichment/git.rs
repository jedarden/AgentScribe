//! Git commit correlation.
//!
//! Correlates sessions with git commits using time windows.
//! Provides reverse index: commit_hash -> session_id.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A git commit correlated with a session.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommit {
    pub hash: String,
    pub short_hash: String,
    pub message: String,
    pub author: String,
    pub timestamp: DateTime<Utc>,
    pub session_id: Option<String>,
}

/// Correlate git commits with a session's time window.
///
/// Runs `git log` within the session's start/end times in the project directory.
#[allow(dead_code)]
pub fn correlate_commits(
    project: &str,
    started: DateTime<Utc>,
    ended: Option<DateTime<Utc>>,
) -> Vec<GitCommit> {
    let project_path = Path::new(project);
    if !project_path.exists() || !project_path.join(".git").exists() {
        return Vec::new();
    }

    let after = started.format("%Y-%m-%dT%H:%M:%S+00:00").to_string();
    let before = ended
        .unwrap_or_else(|| started + chrono::Duration::hours(2))
        .format("%Y-%m-%dT%H:%M:%S+00:00")
        .to_string();

    let output = Command::new("git")
        .arg("log")
        .arg(format!("--after={}", after))
        .arg(format!("--before={}", before))
        .arg("--format=%H|%h|%s|%an|%aI")
        .current_dir(project_path)
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output);
    let mut commits = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(5, '|').collect();
        if parts.len() != 5 {
            continue;
        }

        let timestamp = match DateTime::parse_from_rfc3339(parts[4]) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(_) => continue,
        };

        commits.push(GitCommit {
            hash: parts[0].to_string(),
            short_hash: parts[1].to_string(),
            message: parts[2].to_string(),
            author: parts[3].to_string(),
            timestamp,
            session_id: None,
        });
    }

    commits
}

/// Build a reverse index: commit_hash -> session_id.
///
/// Takes all sessions with git commits and builds a lookup map.
#[allow(dead_code)]
pub fn build_commit_index(
    commits_by_session: &HashMap<String, Vec<GitCommit>>,
) -> HashMap<String, String> {
    let mut index = HashMap::new();

    for (session_id, commits) in commits_by_session {
        for commit in commits {
            index.insert(commit.hash.clone(), session_id.clone());
            // Also index short hash
            index.insert(commit.short_hash.clone(), session_id.clone());
        }
    }

    index
}

/// Look up which session is associated with a specific file and line.
///
/// Uses `git blame` to find the commit hash for the given line,
/// then looks up the session via the commit index.
#[allow(dead_code)]
pub fn blame_file_line(
    project: &str,
    file: &str,
    line: u32,
    commit_index: &HashMap<String, String>,
) -> Option<BlameResult> {
    let project_path = Path::new(project);
    if !project_path.exists() || !project_path.join(".git").exists() {
        return None;
    }

    let file_path = project_path.join(file);
    if !file_path.exists() {
        return None;
    }

    let output = Command::new("git")
        .arg("blame")
        .arg("-L")
        .arg(format!("{},{}", line, line))
        .arg("--porcelain")
        .arg(file)
        .current_dir(project_path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next()?;

    // git blame porcelain format: hash rest...
    let hash = first_line.split_whitespace().next()?;

    let session_id = commit_index.get(hash).cloned();

    Some(BlameResult {
        commit_hash: hash.to_string(),
        session_id,
        file: file.to_string(),
        line,
    })
}

/// Result of a blame lookup.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlameResult {
    pub commit_hash: String,
    pub session_id: Option<String>,
    pub file: String,
    pub line: u32,
}

/// Get git log for a file, returning commits that touched it.
#[allow(dead_code)]
pub fn file_git_log(project: &str, file: &str, limit: usize) -> Vec<GitCommit> {
    let project_path = Path::new(project);
    if !project_path.exists() || !project_path.join(".git").exists() {
        return Vec::new();
    }

    let file_path = project_path.join(file);
    if !file_path.exists() {
        return Vec::new();
    }

    let output = Command::new("git")
        .arg("log")
        .arg("--format=%H|%h|%s|%an|%aI")
        .arg("-n")
        .arg(limit.to_string())
        .arg("--")
        .arg(file)
        .current_dir(project_path)
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output);
    let mut commits = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(5, '|').collect();
        if parts.len() != 5 {
            continue;
        }

        let timestamp = match DateTime::parse_from_rfc3339(parts[4]) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(_) => continue,
        };

        commits.push(GitCommit {
            hash: parts[0].to_string(),
            short_hash: parts[1].to_string(),
            message: parts[2].to_string(),
            author: parts[3].to_string(),
            timestamp,
            session_id: None,
        });
    }

    commits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_correlate_commits_nonexistent_project() {
        let commits = correlate_commits("/nonexistent/project", Utc::now(), None);
        assert!(commits.is_empty());
    }

    #[test]
    fn test_correlate_commits_no_git() {
        let temp = tempfile::tempdir().unwrap();
        let commits = correlate_commits(temp.path().to_str().unwrap(), Utc::now(), None);
        assert!(commits.is_empty());
    }

    #[test]
    fn test_correlate_commits_with_git() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path();

        // Initialize a git repo
        Command::new("git")
            .arg("init")
            .current_dir(path)
            .output()
            .unwrap();

        Command::new("git")
            .arg("config")
            .arg("user.email")
            .arg("test@test.com")
            .current_dir(path)
            .output()
            .unwrap();

        Command::new("git")
            .arg("config")
            .arg("user.name")
            .arg("Test")
            .current_dir(path)
            .output()
            .unwrap();

        // Create a file and commit
        std::fs::write(path.join("test.txt"), "hello").unwrap();
        Command::new("git")
            .arg("add")
            .arg("test.txt")
            .current_dir(path)
            .output()
            .unwrap();

        Command::new("git")
            .arg("commit")
            .arg("-m")
            .arg("initial commit")
            .current_dir(path)
            .output()
            .unwrap();

        // Correlate commits within a wide time window
        let now = Utc::now();
        let commits = correlate_commits(
            path.to_str().unwrap(),
            now - chrono::Duration::hours(1),
            Some(now + chrono::Duration::hours(1)),
        );

        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].message, "initial commit");
    }

    #[test]
    fn test_build_commit_index() {
        let mut by_session = HashMap::new();
        by_session.insert(
            "test/1".to_string(),
            vec![GitCommit {
                hash: "abc123def456".to_string(),
                short_hash: "abc123d".to_string(),
                message: "fix bug".to_string(),
                author: "test".to_string(),
                timestamp: Utc::now(),
                session_id: None,
            }],
        );

        let index = build_commit_index(&by_session);
        assert_eq!(index.get("abc123def456").unwrap(), "test/1");
        assert_eq!(index.get("abc123d").unwrap(), "test/1");
    }

    #[test]
    fn test_blame_file_line_nonexistent() {
        let index = HashMap::new();
        let result = blame_file_line("/nonexistent", "file.rs", 42, &index);
        assert!(result.is_none());
    }

    #[test]
    fn test_file_git_log_nonexistent() {
        let commits = file_git_log("/nonexistent", "file.rs", 10);
        assert!(commits.is_empty());
    }
}
