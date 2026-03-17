//! File path extraction from events
//!
//! Extracts file paths from both structured tool_call fields and content strings.

use crate::event::Event;
use crate::parser::extract_field;
use crate::plugin::Plugin;
use regex::Regex;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Known file extensions that suggest a file path
static FILE_EXTENSIONS: &[&str] = &[
    "rs", "py", "js", "ts", "tsx", "jsx", "go", "java", "c", "cpp", "h", "hpp",
    "cs", "php", "rb", "swift", "kt", "scala", "sh", "bash", "zsh", "fish",
    "toml", "yaml", "yml", "json", "xml", "html", "css", "scss", "sass",
    "md", "txt", "rst", "adoc", "sql", "db", "sqlite", "db3",
    "lock", "sum", "mod", "gitignore", "dockerignore", "env",
    "dockerfile", "makefile", "cmakelists", "gradle", "pom",
];

/// Patterns that suggest a path component
static PATH_PATTERNS: &[&str] = &[
    r"^~/",                  // Home directory
    r"^/\w",                 // Absolute path
    r"^\./",                 // Relative current dir
    r"^\.\./",               // Relative parent dir
    r"\.(?:[a-z]{1,6})$",    // File extension
];

/// Patterns that are NOT file paths (false positives)
static EXCLUSION_PATTERNS: &[&str] = &[
    r"https?://",            // URLs
    r"ftp://",               // FTP URLs
    r"\x1b\[[0-9;]*m",       // ANSI escape sequences
];

/// File path extractor
pub struct FilePathExtractor;

impl FilePathExtractor {
    /// Extract file paths from an event
    pub fn extract_from_event(event: &Event, plugin: &Plugin) -> Vec<String> {
        let mut paths = Vec::new();

        // 1. Structured extraction from tool_call fields (if available in source)
        if event.role == crate::event::Role::ToolCall {
            if let Some(ref config) = plugin.parser.file_paths {
                if config.tool_call_field.is_some() {
                    // This would need access to the original JSON source
                    // For now, we'll rely on content regex extraction
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

        // Pattern for quoted paths: "path/to/file.ext" or 'path/to/file.ext'
        let quoted_re = Regex::new(r#"["']([^"']+\.[a-z]{1,6})["']"#).unwrap();

        // Pattern for unquoted paths after common commands
        // e.g., "edit file.rs", "cat /path/to/file"
        let command_re = Regex::new(r"(?:edit|cat|less|vim|nano|open|read|write|create|delete|modify)\s+([^\s]+\.[a-z]{1,6})").unwrap();

        // Pattern for paths in backticks
        let backtick_re = Regex::new(r"`([^`]+\.[a-z]{1,6})`").unwrap();

        // Pattern for bare paths starting with ./, ~/ or /
        let path_re = Regex::new(r"(?:^|\s)([~/][^\s,\)]+|/\S+)").unwrap();

        for captures in quoted_re.captures_iter(content) {
            if let Some(path) = captures.get(1) {
                paths.push(path.as_str().to_string());
            }
        }

        for captures in command_re.captures_iter(content) {
            if let Some(path) = captures.get(1) {
                paths.push(path.as_str().to_string());
            }
        }

        for captures in backtick_re.captures_iter(content) {
            if let Some(path) = captures.get(1) {
                paths.push(path.as_str().to_string());
            }
        }

        for captures in path_re.captures_iter(content) {
            if let Some(path) = captures.get(1) {
                paths.push(path.as_str().to_string());
            }
        }

        paths
    }

    /// Extract file paths from structured tool_call JSON
    pub fn extract_from_tool_call(tool_call: &Value, field_path: &str) -> Vec<String> {
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

    /// Check if a string looks like a file path
    pub fn looks_like_file_path(s: &str) -> bool {
        // Check exclusions first
        for pattern in EXCLUSION_PATTERNS {
            if let Ok(re) = Regex::new(pattern) {
                if re.is_match(s) {
                    return false;
                }
            }
        }

        // Check if it looks like a path
        for pattern in PATH_PATTERNS {
            if let Ok(re) = Regex::new(pattern) {
                if re.is_match(s) {
                    return true;
                }
            }
        }

        // Check for known file extensions
        for ext in FILE_EXTENSIONS {
            if s.ends_with(&format!(".{}", ext)) {
                return true;
            }
        }

        false
    }

    /// Resolve a relative path against a project directory
    pub fn resolve_path(path: &str, project_dir: Option<&str>) -> String {
        let path_buf = PathBuf::from(path);

        if path_buf.is_absolute() {
            return path.to_string();
        }

        // Expand ~
        if path.starts_with("~/") {
            if let Some(home) = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf()) {
                return home.join(path.strip_prefix("~/").unwrap_or(path))
                    .to_string_lossy().to_string();
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
}
