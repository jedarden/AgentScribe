//! File path extraction from events
//!
//! Extracts file paths from both structured tool_call fields and content strings.

use crate::event::Event;
use crate::plugin::Plugin;
use std::sync::LazyLock;

use regex::Regex;

/// Known file extensions that suggest a file path
static FILE_EXTENSIONS: &[&str] = &[
    "rs", "py", "js", "ts", "tsx", "jsx", "go", "java", "c", "cpp", "h", "hpp", "cs", "php",
    "rb", "swift", "kt", "scala", "sh", "bash", "zsh", "fish", "toml", "yaml", "yml", "json",
    "xml", "html", "css", "scss", "sass", "md", "txt", "rst", "adoc", "sql", "db", "sqlite",
    "db3", "lock", "sum", "mod", "gitignore", "dockerignore", "env", "dockerfile", "makefile",
    "cmakelists", "gradle", "pom",
];

// Compiled-once regex patterns
static QUOTED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"["']([^"']+\.[a-z]{1,6})["']"#).unwrap());

static COMMAND_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:edit|cat|less|vim|nano|open|read|write|create|delete|modify)\s+([^\s]+\.[a-z]{1,6})",
    )
    .unwrap()
});

static BACKTICK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"`([^`]+\.[a-z]{1,6})`").unwrap());

static PATH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\s)([~/][^\s,\)]+|/\S+)").unwrap());

// Relative path: e.g. "src/db/pool.rs", "tests/fixtures/foo.json"
static REL_PATH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\s)((?:\w[\w\-]*/)+[\w\-]+\.[a-z]{1,8})(?:\s|$|,|\))").unwrap());

/// File path extractor
pub struct FilePathExtractor;

impl FilePathExtractor {
    /// Extract file paths from an event
    pub fn extract_from_event(event: &Event, plugin: &Plugin) -> Vec<String> {
        let mut paths = Vec::new();

        // 1. For ToolCall events, check if content itself is a file path
        if event.role == crate::event::Role::ToolCall {
            if let Some(ref config) = plugin.parser.file_paths {
                if config.tool_call_field.is_some() {
                    let trimmed = event.content.trim();
                    if Self::looks_like_file_path(trimmed) {
                        paths.push(trimmed.to_string());
                    }
                }
            }
        }

        // 2. Regex extraction from content
        if plugin
            .parser
            .file_paths
            .as_ref()
            .and_then(|c| c.content_regex)
            .unwrap_or(false)
        {
            paths.extend(Self::extract_from_content(&event.content));
        }

        // 3. Deduplicate and filter
        paths.sort();
        paths.dedup();
        paths.retain(|p| Self::looks_like_file_path(p));

        paths
    }

    /// Extract file paths from content string using regex
    pub fn extract_from_content(content: &str) -> Vec<String> {
        let mut paths = Vec::new();

        for captures in QUOTED_RE.captures_iter(content) {
            if let Some(path) = captures.get(1) {
                paths.push(path.as_str().to_string());
            }
        }

        for captures in COMMAND_RE.captures_iter(content) {
            if let Some(path) = captures.get(1) {
                paths.push(path.as_str().to_string());
            }
        }

        for captures in BACKTICK_RE.captures_iter(content) {
            if let Some(path) = captures.get(1) {
                paths.push(path.as_str().to_string());
            }
        }

        for captures in PATH_RE.captures_iter(content) {
            if let Some(path) = captures.get(1) {
                paths.push(path.as_str().to_string());
            }
        }

        for captures in REL_PATH_RE.captures_iter(content) {
            if let Some(path) = captures.get(1) {
                paths.push(path.as_str().to_string());
            }
        }

        paths
    }

    /// Extract file paths from structured tool_call JSON
    #[allow(dead_code)]
    pub fn extract_from_tool_call(
        tool_call: &serde_json::Value,
        field_path: &str,
    ) -> Vec<String> {
        use crate::parser::extract_field;
        let mut paths = Vec::new();

        if let Some(field) = extract_field(tool_call, field_path) {
            if let Some(path) = field.as_str() {
                paths.push(path.to_string());
            } else if let Some(arr) = field.as_array() {
                for item in arr {
                    if let Some(path) = item.as_str() {
                        paths.push(path.to_string());
                    }
                }
            }
        }

        paths
    }

    /// Check if a string looks like a file path (no regex compilation).
    pub fn looks_like_file_path(s: &str) -> bool {
        if s.is_empty() {
            return false;
        }

        // Exclusions — check with simple string ops (no regex)
        if s.starts_with("http://")
            || s.starts_with("https://")
            || s.starts_with("ftp://")
            || s.contains('\x1b')
        {
            return false;
        }

        // Path-prefix indicators
        if s.starts_with("~/")
            || s.starts_with("./")
            || s.starts_with("../")
            || (s.starts_with('/') && s.len() > 1 && s.chars().nth(1).is_some_and(|c| c.is_alphanumeric()))
        {
            return true;
        }

        // Known file extensions
        for ext in FILE_EXTENSIONS {
            if s.ends_with(&format!(".{}", ext)) {
                return true;
            }
        }

        false
    }

    /// Resolve a relative path against a project directory
    #[allow(dead_code)]
    pub fn resolve_path(path: &str, project_dir: Option<&str>) -> String {
        use std::path::{Path, PathBuf};

        let path_buf = PathBuf::from(path);

        if path_buf.is_absolute() {
            return path.to_string();
        }

        // Expand ~
        if path.starts_with("~/") {
            if let Some(home) = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf()) {
                return home
                    .join(path.strip_prefix("~/").unwrap_or(path))
                    .to_string_lossy()
                    .to_string();
            }
        }

        // Resolve against project directory
        if let Some(project) = project_dir {
            let base = Path::new(project);
            return base.join(path).to_string_lossy().to_string();
        }

        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_from_content() {
        let content = r#"The file "src/main.rs" needs to be updated.
Also check /home/user/project/config.toml and edit `tests/test.rs`.
Use ./scripts/setup.sh to run."#;

        let paths = FilePathExtractor::extract_from_content(content);

        assert!(paths.iter().any(|p| p.contains("main.rs")));
        assert!(paths.iter().any(|p| p.contains("config.toml")));
        assert!(paths.iter().any(|p| p.contains("test.rs")));
    }

    #[test]
    fn test_looks_like_file_path() {
        assert!(FilePathExtractor::looks_like_file_path("src/main.rs"));
        assert!(FilePathExtractor::looks_like_file_path("/home/user/file.py"));
        assert!(FilePathExtractor::looks_like_file_path("~/project/config.toml"));
        assert!(FilePathExtractor::looks_like_file_path("./script.sh"));

        assert!(!FilePathExtractor::looks_like_file_path("https://example.com"));
        assert!(!FilePathExtractor::looks_like_file_path("not a path"));
    }

    #[test]
    fn test_resolve_path() {
        let resolved = FilePathExtractor::resolve_path("src/main.rs", Some("/home/user/project"));
        assert_eq!(resolved, "/home/user/project/src/main.rs");

        // Absolute path should stay absolute
        let resolved = FilePathExtractor::resolve_path("/etc/config", Some("/home/user/project"));
        assert_eq!(resolved, "/etc/config");
    }

    #[test]
    fn test_bare_relative_path_in_content() {
        // Bare paths like "src/db/pool.rs" should be extracted
        let paths = FilePathExtractor::extract_from_content("src/db/pool.rs");
        let has_pool = paths.iter().any(|p| p.contains("pool.rs"));
        assert!(has_pool, "expected pool.rs in extracted paths, got: {:?}", paths);
    }

    #[test]
    fn test_tool_call_content_is_file_path() {
        use crate::event::{Event, Role};
        use crate::plugin::{
            FilePathExtraction, LogFormat, ModelDetection, Parser, Plugin, PluginMeta,
            ProjectDetection, SessionDetection, SessionIdSource, Source,
        };
        use chrono::Utc;

        let plugin = Plugin {
            plugin: PluginMeta {
                name: "test".to_string(),
                version: "1.0".to_string(),
            },
            source: Source {
                paths: vec![],
                exclude: vec![],
                format: LogFormat::Jsonl,
                session_detection: SessionDetection::OneFilePerSession {
                    session_id_from: SessionIdSource::Filename,
                },
                tree: None,
                truncation_limit: None,
            },
            parser: Parser {
                file_paths: Some(FilePathExtraction {
                    tool_call_field: Some("input.file_path".to_string()),
                    content_regex: Some(true),
                }),
                project: Some(ProjectDetection::ParentDir),
                model: Some(ModelDetection::None),
                ..Default::default()
            },
            metadata: None,
        };

        let mut event = Event::new(
            Utc::now(),
            "test/1".into(),
            "test".into(),
            Role::ToolCall,
            "src/db/pool.rs".into(),
        );
        event.tool = Some("Read".to_string());

        let paths = FilePathExtractor::extract_from_event(&event, &plugin);
        assert!(
            paths.iter().any(|p| p.contains("pool.rs")),
            "expected pool.rs in file_paths from ToolCall content, got: {:?}",
            paths
        );
    }
}
