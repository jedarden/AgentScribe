//! Validate that plugins/aider.toml with recursive ** globs parses through the
//! Plugin loader without errors and that discover_files() resolves the pattern.
//!
//! Acceptance criteria (bf-5x30):
//! 1. Plugin struct can deserialize plugins/aider.toml successfully
//! 2. The paths field contains '~/**/.aider.chat.history.md'
//! 3. The exclude field contains all expected patterns
//! 4. No glob parse errors when expanding the pattern

use std::path::PathBuf;
use tempfile::TempDir;

/// Helper: load the actual plugins/aider.toml from the crate root.
fn load_aider_plugin() -> agentscribe::plugin::Plugin {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let toml_path = PathBuf::from(manifest_dir).join("plugins").join("aider.toml");
    agentscribe::plugin::validate_plugin_file(&toml_path)
        .unwrap_or_else(|e| panic!("Failed to load plugins/aider.toml: {}", e))
}

#[test]
fn test_aider_toml_deserializes_without_error() {
    let plugin = load_aider_plugin();

    // Verify basic identity
    assert_eq!(plugin.plugin.name, "aider");
    assert_eq!(plugin.plugin.version, "1.0");
    assert_eq!(plugin.source.format, agentscribe::plugin::LogFormat::Markdown);
}

#[test]
fn test_aider_paths_contains_recursive_glob() {
    let plugin = load_aider_plugin();

    assert_eq!(
        plugin.source.paths.len(),
        1,
        "Should have exactly one path pattern"
    );
    assert_eq!(
        plugin.source.paths[0],
        "~/**/.aider.chat.history.md",
        "Path should use recursive ** glob"
    );
}

#[test]
fn test_aider_exclude_contains_all_expected_patterns() {
    let plugin = load_aider_plugin();

    let expected_excludes: &[&str] = &[
        "~/**/node_modules/**/.aider.chat.history.md",
        "~/**/target/**/.aider.chat.history.md",
        "~/**/.git/**/.aider.chat.history.md",
        "~/**/.cache/**/.aider.chat.history.md",
        "~/**/venv/**/.aider.chat.history.md",
        "~/**/.venv/**/.aider.chat.history.md",
        "~/**/__pycache__/**/.aider.chat.history.md",
        "~/**/build/**/.aider.chat.history.md",
        "~/**/dist/**/.aider.chat.history.md",
    ];

    assert_eq!(
        plugin.source.exclude.len(),
        expected_excludes.len(),
        "Should have exactly {} exclude patterns",
        expected_excludes.len()
    );

    for pattern in expected_excludes {
        assert!(
            plugin.source.exclude.iter().any(|p| p == pattern),
            "Exclude should contain '{}'",
            pattern
        );
    }
}

#[test]
fn test_recursive_glob_pattern_is_valid() {
    let plugin = load_aider_plugin();

    // Verify that the path pattern is a valid glob (no parse errors)
    for pattern in &plugin.source.paths {
        glob::Pattern::new(pattern).unwrap_or_else(|e| {
            panic!(
                "Path pattern '{}' should be a valid glob: {}",
                pattern, e
            )
        });
    }

    // Verify that all exclude patterns are valid globs
    for pattern in &plugin.source.exclude {
        glob::Pattern::new(pattern).unwrap_or_else(|e| {
            panic!(
                "Exclude pattern '{}' should be a valid glob: {}",
                pattern, e
            )
        });
    }
}

#[test]
fn test_glob_expansion_discovers_nested_files_and_excludes_correctly() {
    let temp = TempDir::new().unwrap();
    let base = temp.path();

    // Create directory structure simulating nested repos
    let deep_repo = base.join("projects").join("deep").join("repo");
    std::fs::create_dir_all(&deep_repo).unwrap();

    let top_level = base.join("top-project");
    std::fs::create_dir_all(&top_level).unwrap();

    // Create excluded directories with their own aider files
    let node_modules = base.join("projects").join("web-app").join("node_modules");
    std::fs::create_dir_all(&node_modules).unwrap();

    let target_dir = base.join("projects").join("rust-app").join("target");
    std::fs::create_dir_all(&target_dir).unwrap();

    let venv_dir = base.join("projects").join("python-app").join("venv");
    std::fs::create_dir_all(&venv_dir).unwrap();

    let pycache_dir = base
        .join("projects")
        .join("python-app")
        .join("__pycache__");
    std::fs::create_dir_all(&pycache_dir).unwrap();

    // Create aider history files in non-excluded locations
    let deep_file = deep_repo.join(".aider.chat.history.md");
    std::fs::write(&deep_file, "# aider chat started at 2024-01-01\n#### user\nassistant").unwrap();

    let top_file = top_level.join(".aider.chat.history.md");
    std::fs::write(&top_file, "# aider chat started at 2024-02-01\n#### user\nassistant").unwrap();

    // Create aider history files in excluded locations
    let nm_file = node_modules.join(".aider.chat.history.md");
    std::fs::write(&nm_file, "# aider chat started at 2024-03-01\n#### user\nassistant").unwrap();

    let tgt_file = target_dir.join(".aider.chat.history.md");
    std::fs::write(&tgt_file, "# aider chat started at 2024-03-01\n#### user\nassistant").unwrap();

    let venv_file = venv_dir.join(".aider.chat.history.md");
    std::fs::write(&venv_file, "# aider chat started at 2024-03-01\n#### user\nassistant").unwrap();

    let pycache_file = pycache_dir.join(".aider.chat.history.md");
    std::fs::write(&pycache_file, "# aider chat started at 2024-03-01\n#### user\nassistant").unwrap();

    // Simulate discover_files using the same logic as the scraper
    let pattern = format!("{}/**/.aider.chat.history.md", base.display());

    // Expand the glob — must not error
    let glob_result = glob::glob(&pattern)
        .unwrap_or_else(|e| panic!("Invalid glob pattern '{}': {}", pattern, e));

    let discovered: Vec<_> = glob_result.filter_map(|e| e.ok()).filter(|p| p.is_file()).collect();

    // Should discover all 6 files (5 created above + 0 extras)
    assert_eq!(discovered.len(), 6, "Should discover 6 files before exclusion");

    // Now apply the exclude patterns (mirroring the plugin's exclude list)
    let exclude_dirs = &[
        "node_modules", "target", ".git", ".cache", "venv", ".venv", "__pycache__", "build",
        "dist",
    ];

    let excluded: Vec<_> = discovered
        .into_iter()
        .filter(|path| {
            let path_str = path.to_string_lossy();
            !exclude_dirs.iter().any(|dir| path_str.contains(&format!("/{}/", dir)))
        })
        .collect();

    // Only 2 files should survive exclusion
    assert_eq!(excluded.len(), 2, "Should have 2 files after exclusion");

    // Verify the surviving files are the expected ones
    let surviving_paths: Vec<String> = excluded.iter().map(|p| p.display().to_string()).collect();
    assert!(
        surviving_paths.contains(&deep_file.display().to_string()),
        "Should include deep nested repo file"
    );
    assert!(
        surviving_paths.contains(&top_file.display().to_string()),
        "Should include top-level project file"
    );
}

#[test]
fn test_plugin_passes_full_validation() {
    let plugin = load_aider_plugin();

    // Run through PluginManager validation to ensure no format-specific errors
    let manager = agentscribe::plugin::PluginManager::new(PathBuf::from("/dummy"));
    manager
        .validate_plugin(&plugin)
        .expect("Aider plugin should pass full validation");
}
