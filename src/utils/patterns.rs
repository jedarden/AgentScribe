//! Pattern matching utilities for file path detection
//!
//! This module provides helper functions for matching file paths against
//! configurable patterns, with support for glob patterns and common
//! configuration file extensions.

use crate::error::Result;
use glob::Pattern;
use std::path::Path;

/// Common configuration file extensions
const CONFIG_EXTENSIONS: &[&str] = &[
    ".toml",   // TOML configuration
    ".yaml",   // YAML configuration
    ".yml",    // YAML alternative extension
    ".json",   // JSON configuration
    ".ini",    // INI configuration
    ".conf",   // Generic configuration files
    ".cfg",    // Configuration files
    ".config", // Configuration files
];

/// Detects if a file path matches configuration file patterns.
///
/// This function checks if a given path matches any of the provided patterns
/// or has a common configuration file extension. Patterns support glob syntax
/// (e.g., "**/*.toml", "config/**", "*.yaml").
///
/// # Arguments
///
/// * `path` - The file path to check
/// * `patterns` - Optional list of glob patterns to match against
///
/// # Returns
///
/// * `true` if the path matches a pattern or has a config extension
/// * `false` otherwise
///
/// # Examples
///
/// ```ignore
/// use agentscribe::utils::patterns::is_config_path;
///
/// // Match by extension
/// assert!(is_config_path("config.toml", &[]));
/// assert!(is_config_path("settings.yaml", &[]));
///
/// // Match by glob pattern
/// assert!(is_config_path("src/config/app.json", &["**/*.json"]));
/// assert!(is_config_path("config/db.toml", &["config/**"]));
///
/// // Non-config paths
/// assert!(!is_config_path("src/main.rs", &[]));
/// assert!(!is_config_path("README.md", &[]));
/// ```
pub fn is_config_path(path: &str, patterns: &[String]) -> bool {
    let path_obj = Path::new(path);

    // If no patterns provided, check by extension only
    if patterns.is_empty() {
        return check_config_extension(path_obj);
    }

    // Check against provided glob patterns only
    for pattern_str in patterns {
        match Pattern::new(pattern_str) {
            Ok(pattern) => {
                // Try matching against the full path first
                if pattern.matches(path) {
                    return true;
                }

                // Also try matching against just the filename
                if let Some(file_name) = path_obj.file_name() {
                    if let Some(name_str) = file_name.to_str() {
                        if pattern.matches(name_str) {
                            return true;
                        }
                    }
                }

                // Try matching against relative path (without leading ./)
                let normalized_path = path.strip_prefix("./").unwrap_or(path);
                if pattern.matches(normalized_path) {
                    return true;
                }
            }
            Err(_) => {
                // Invalid glob pattern - log warning but continue checking
                // In production, you might want to log this
                continue;
            }
        }
    }

    // No pattern matched
    false
}

/// Checks if a path has a common configuration file extension.
///
/// # Arguments
///
/// * `path` - The path to check
///
/// # Returns
///
/// * `true` if the path has a config extension
/// * `false` otherwise
fn check_config_extension(path: &Path) -> bool {
    // Get the file extension
    if let Some(extension) = path.extension() {
        if let Some(ext_str) = extension.to_str() {
            // Check against known config extensions
            let ext_with_dot = format!(".{}", ext_str.to_lowercase());
            return CONFIG_EXTENSIONS.contains(&ext_with_dot.as_str());
        }
    }

    false
}

/// Validates if a given pattern string is a valid glob pattern.
///
/// # Arguments
///
/// * `pattern` - The pattern string to validate
///
/// # Returns
///
/// * `Ok(())` if the pattern is valid
/// * `Err(AgentScribeError)` if the pattern is invalid
pub fn validate_pattern(pattern: &str) -> Result<()> {
    Pattern::new(pattern).map(|_| ()).map_err(|e| {
        crate::error::AgentScribeError::Glob(format!("Invalid glob pattern '{}': {}", pattern, e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_extensions() {
        // Test common config extensions
        assert!(is_config_path("config.toml", &[]));
        assert!(is_config_path("settings.yaml", &[]));
        assert!(is_config_path("app.yml", &[]));
        assert!(is_config_path("config.json", &[]));
        assert!(is_config_path("app.ini", &[]));
        assert!(is_config_path("nginx.conf", &[]));
        assert!(is_config_path("myapp.cfg", &[]));
        assert!(is_config_path("app.config", &[]));

        // Test non-config extensions
        assert!(!is_config_path("main.rs", &[]));
        assert!(!is_config_path("README.md", &[]));
        assert!(!is_config_path("script.py", &[]));
        assert!(!is_config_path("logo.png", &[]));
    }

    #[test]
    fn test_nested_paths_with_extensions() {
        // Test nested paths with config extensions
        assert!(is_config_path("src/config/app.toml", &[]));
        assert!(is_config_path("project/settings/dev.yaml", &[]));
        assert!(is_config_path("/absolute/path/config.json", &[]));
        assert!(is_config_path("./relative/config.ini", &[]));
    }

    #[test]
    fn test_glob_patterns() {
        // Test glob patterns
        assert!(is_config_path("config.toml", &["*.toml".to_string()]));
        assert!(is_config_path(
            "config/app.toml",
            &["**/*.toml".to_string()]
        ));
        assert!(is_config_path(
            "src/config/db.json",
            &["**/*.json".to_string()]
        ));

        // Test directory patterns
        assert!(is_config_path(
            "config/app.toml",
            &["config/**".to_string()]
        ));
        assert!(is_config_path(
            "config/subdir/file.yaml",
            &["config/**".to_string()]
        ));

        // Test pattern with multiple wildcards
        assert!(is_config_path(
            "src/config/dev/production.json",
            &["**/config/**/*json".to_string()]
        ));

        // Test non-matching patterns
        assert!(!is_config_path("config.toml", &["*.json".to_string()]));
        assert!(!is_config_path("src/main.rs", &["**/*.toml".to_string()]));
    }

    #[test]
    fn test_empty_patterns() {
        // Test with empty pattern list - should still match by extension
        assert!(is_config_path("config.toml", &[]));
        assert!(is_config_path("settings.yaml", &[]));
        assert!(!is_config_path("main.rs", &[]));
    }

    #[test]
    fn test_multiple_patterns() {
        // Test with multiple patterns - should match any
        let patterns = vec![
            "**/*.toml".to_string(),
            "**/*.yaml".to_string(),
            "config/**".to_string(),
        ];

        assert!(is_config_path("app.toml", &patterns));
        assert!(is_config_path("settings.yaml", &patterns));
        assert!(is_config_path("config/anything", &patterns));
        assert!(!is_config_path("main.rs", &patterns));
    }

    #[test]
    fn test_path_normalization() {
        // Test that ./ prefix is handled
        let patterns = vec!["**/*.toml".to_string()];
        assert!(is_config_path("./config.toml", &patterns));
        assert!(is_config_path("config.toml", &patterns));
    }

    #[test]
    fn test_invalid_patterns() {
        // Test that invalid patterns are skipped without panicking
        let patterns = vec![
            "**/*.toml".to_string(),
            "[invalid".to_string(), // Invalid glob pattern
            "**/*.json".to_string(),
        ];

        // Should still match valid patterns
        assert!(is_config_path("app.toml", &patterns));
        assert!(is_config_path("config.json", &patterns));
        assert!(!is_config_path("main.rs", &patterns));
    }

    #[test]
    fn test_filename_only_matching() {
        // Test that patterns can match just the filename
        let patterns = vec!["*.toml".to_string()];
        assert!(is_config_path("config/app.toml", &patterns));
        assert!(is_config_path("deep/nested/path/config.toml", &patterns));
    }

    #[test]
    fn test_config_extensions_case_insensitive() {
        // Test that extension matching is case-insensitive
        assert!(is_config_path("config.TOML", &[]));
        assert!(is_config_path("settings.YAML", &[]));
        assert!(is_config_path("app.JSON", &[]));
    }

    #[test]
    fn test_paths_without_extensions() {
        // Test paths without extensions
        assert!(!is_config_path("Dockerfile", &[]));
        assert!(!is_config_path("Makefile", &[]));
        assert!(!is_config_path("script", &[]));
    }

    #[test]
    fn test_special_files() {
        // Test special configuration files without standard extensions
        assert!(!is_config_path(".env", &[]));
        assert!(!is_config_path(".gitignore", &[]));
        assert!(!is_config_path("CMakeLists.txt", &[]));

        // But they should match with explicit patterns
        assert!(is_config_path(".env", &[".env*".to_string()]));
        assert!(is_config_path(".gitignore", &[".git*".to_string()]));
    }

    #[test]
    fn test_validate_pattern() {
        // Test pattern validation
        assert!(validate_pattern("*.toml").is_ok());
        assert!(validate_pattern("**/*.json").is_ok());
        assert!(validate_pattern("config/**").is_ok());

        // Test invalid patterns
        assert!(validate_pattern("[invalid").is_err());
        assert!(validate_pattern("**[[").is_err());
    }

    #[test]
    fn test_pattern_with_brackets() {
        // Test patterns with character classes (valid glob syntax)
        let patterns = vec!["config.[jt]son".to_string()];
        assert!(is_config_path("config.json", &patterns));
        assert!(is_config_path("config.tson", &patterns));
        assert!(!is_config_path("config.yaml", &patterns));
    }
}
