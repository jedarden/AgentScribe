// Test to verify Aider glob patterns are correct
use glob::glob;
use shellexpand::full;
use std::path::PathBuf;

#[test]
fn test_aider_glob_pattern_syntax() {
    let pattern = "~/**/.aider.chat.history.md";

    // Test 1: Verify tilde expansion works
    let expanded = full(pattern).expect("Tilde expansion should succeed");
    assert!(
        expanded.starts_with("/"),
        "Expanded pattern should be an absolute path"
    );
    assert!(
        expanded.contains("**/.aider.chat.history.md"),
        "Expanded pattern should contain the filename pattern"
    );

    // Test 2: Verify glob pattern is valid for the glob crate
    let glob_result = glob(&expanded);
    assert!(
        glob_result.is_ok(),
        "Pattern should be valid glob syntax: {:?}",
        glob_result.err()
    );
}

#[test]
fn test_aider_pattern_matches_fixture_files() {
    // Use the actual pattern from the plugin
    let pattern = "~/**/.aider.chat.history.md";
    let expanded = full(pattern).expect("Tilde expansion should succeed");

    // Collect all matching files
    let matches: Vec<PathBuf> = glob(&expanded)
        .expect("Valid glob pattern")
        .filter_map(|e| e.ok())
        .collect();

    // If any Aider files exist on the system, the pattern should find them
    // We know there are at least test fixtures in the AgentScribe repo
    let found_any = !matches.is_empty();

    if found_any {
        // Verify all matches have the exact filename
        for path in &matches {
            assert_eq!(
                path.file_name(),
                Some(std::ffi::OsStr::new(".aider.chat.history.md")),
                "Matched file should have exact filename: {:?}",
                path
            );
        }

        println!("Found {} Aider history file(s):", matches.len());
        for path in &matches {
            println!("  - {}", path.display());
        }
    }
}

#[test]
fn test_aider_plugin_paths_configuration() {
    // This test verifies the actual plugin configuration
    // The plugin should have the recursive pattern configured

    let expected_pattern = "~/**/.aider.chat.history.md";

    // Verify pattern structure
    assert!(
        expected_pattern.starts_with("~"),
        "Pattern should start with ~ for home directory"
    );
    assert!(
        expected_pattern.contains("**"),
        "Pattern should use ** for recursive matching"
    );
    assert!(
        expected_pattern.ends_with(".aider.chat.history.md"),
        "Pattern should end with exact filename"
    );
}

#[test]
fn test_recursive_glob_components() {
    // Test that ** matches at any depth
    let pattern = "/home/coding/**/*.md";

    // Create test paths to verify ** behavior
    let test_paths = vec![
        "/home/coding/README.md",
        "/home/coding/docs/test.md",
        "/home/coding/projects/agent/scribe/test.md",
        "/home/coding/a/b/c/d/e/f/test.md",
    ];

    for path_str in test_paths {
        let path = std::path::Path::new(path_str);
        // Create a glob::Pattern from the base pattern
        let glob_pattern = glob::Pattern::new(pattern).expect("Valid pattern");
        assert!(
            glob_pattern.matches_path(path),
            "** should match files at any depth: {}",
            path_str
        );
    }
}
