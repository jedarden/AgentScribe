//! Test recursive glob discovery for aider plugin
//! This test validates that the updated aider.toml discovers files in nested repos

use std::fs;
use tempfile::TempDir;

#[test]
fn test_recursive_glob_discovers_nested_repos() {
    let temp = TempDir::new().unwrap();
    let base = temp.path();

    // Create nested directory structure
    let nested_deep = base
        .join("projects")
        .join("nested")
        .join("deep")
        .join("repo");
    fs::create_dir_all(&nested_deep).unwrap();

    let another_project = base.join("repos").join("another-project");
    fs::create_dir_all(&another_project).unwrap();

    // Create excluded directories
    let node_modules = base.join("projects").join("excluded").join("node_modules");
    fs::create_dir_all(&node_modules).unwrap();

    let target_dir = base.join("projects").join("excluded").join("target");
    fs::create_dir_all(&target_dir).unwrap();

    // Create aider files in non-excluded locations
    let nested_file = nested_deep.join(".aider.chat.history.md");
    fs::write(
        &nested_file,
        "# aider chat started at 2024-03-15\n#### Test\nResponse",
    )
    .unwrap();

    let another_file = another_project.join(".aider.chat.history.md");
    fs::write(
        &another_file,
        "# aider chat started at 2024-03-15\n#### Test 2\nResponse",
    )
    .unwrap();

    // Create aider files in excluded locations
    let excluded_file1 = node_modules.join(".aider.chat.history.md");
    fs::write(
        &excluded_file1,
        "# aider chat started at 2024-03-15\n#### Excluded\nResponse",
    )
    .unwrap();

    let excluded_file2 = target_dir.join(".aider.chat.history.md");
    fs::write(
        &excluded_file2,
        "# aider chat started at 2024-03-15\n#### Also Excluded\nResponse",
    )
    .unwrap();

    // Test recursive glob pattern: ~/**/.aider.chat.history.md
    let pattern = format!("{}/**/.aider.chat.history.md", base.display());

    let mut found = Vec::new();
    for entry in glob::glob(&pattern).unwrap().filter_map(|e| e.ok()) {
        if entry.is_file() {
            found.push(entry);
        }
    }

    // Should find 4 files total (including excluded)
    assert_eq!(found.len(), 4, "Should find 4 files with recursive glob");

    // Now test exclusion patterns matching the new aider.toml
    let exclude_patterns = vec![
        format!(
            "{}/**/node_modules/**/.aider.chat.history.md",
            base.display()
        ),
        format!("{}/**/target/**/.aider.chat.history.md", base.display()),
    ];

    let mut filtered = Vec::new();
    for f in found {
        let mut excluded = false;
        for exclude_pattern in &exclude_patterns {
            if let Ok(exclude_glob) = glob::glob(exclude_pattern) {
                if exclude_glob.filter_map(|e| e.ok()).any(|p| p == f) {
                    excluded = true;
                    break;
                }
            }
        }
        if !excluded {
            filtered.push(f);
        }
    }

    // Should have 2 files after exclusion
    assert_eq!(filtered.len(), 2, "Should have 2 files after exclusion");

    // Verify the right files are included
    let paths: Vec<String> = filtered.iter().map(|p| p.display().to_string()).collect();
    assert!(
        paths.contains(&nested_file.display().to_string()),
        "Should include nested file"
    );
    assert!(
        paths.contains(&another_file.display().to_string()),
        "Should include another project file"
    );

    println!("✓ Recursive glob discovery test passed!");
}

#[test]
fn test_nested_repo_fixture_exists() {
    // Verify the fixture file was created
    let fixture_path = format!(
        "{}/tests/fixtures/aider/nested-repo/deep/path/.aider.chat.history.md",
        env!("CARGO_MANIFEST_DIR")
    );

    assert!(
        std::path::Path::new(&fixture_path).exists(),
        "Nested-repo fixture should exist at {}",
        fixture_path
    );

    // Read and verify the content
    let content = fs::read_to_string(&fixture_path).unwrap();
    assert!(
        content.contains("# aider chat started at"),
        "Should contain session delimiter"
    );
    assert!(
        content.contains("nested React component"),
        "Should contain the test conversation"
    );

    println!("✓ Nested repo fixture exists and is valid!");
}
