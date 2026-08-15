// Quick test to debug the exclude pattern issue
use std::fs;
use tempfile::tempdir;

fn main() {
    let temp = tempdir().unwrap();
    let logs_dir = temp.path().join("logs");

    // Create the same structure as the test
    fs::create_dir_all(logs_dir.join("project-a/subagents")).unwrap();
    fs::create_dir_all(logs_dir.join("project-b/subagents/nested")).unwrap();
    fs::create_dir_all(logs_dir.join("vendor/node_modules")).unwrap();
    fs::create_dir_all(logs_dir.join("vendor/otherlib")).unwrap();

    // Create test files
    fs::write(logs_dir.join("root.jsonl"), "root session").unwrap();
    fs::write(logs_dir.join("project-a/session.jsonl"), "project a").unwrap();
    fs::write(logs_dir.join("project-a/subagents/agent-1.jsonl"), "subagent 1").unwrap();
    fs::write(logs_dir.join("project-b/session.jsonl"), "project b").unwrap();
    fs::write(logs_dir.join("project-b/subagents/nested/agent-2.jsonl"), "subagent 2").unwrap();
    fs::write(logs_dir.join("vendor/node_modules/package.json"), "node package").unwrap();
    fs::write(logs_dir.join("vendor/otherlib/lib.json"), "library").unwrap();

    // Test glob pattern matching
    let abs_path = logs_dir.join("vendor/node_modules/package.json");
    let pattern_str = "*/subagents/*";

    // Test with glob::Pattern
    let pattern = glob::Pattern::new(pattern_str).unwrap();
    let matches = pattern.matches_path(&abs_path);

    println!("Absolute path: {:?}", abs_path);
    println!("Pattern: {}", pattern_str);
    println!("Direct matches_path: {}", matches);

    // Test with normalized pattern
    let normalized = format!("**/{}", pattern_str);
    let norm_pattern = glob::Pattern::new(&normalized).unwrap();
    let norm_matches = norm_pattern.matches_path(&abs_path);

    println!("Normalized pattern: {}", normalized);
    println!("Normalized matches_path: {}", norm_matches);

    // Now test what the code actually produces
    let exclude_expanded = pattern_str.to_string();
    let normalized_pattern = if !exclude_expanded.starts_with('/') && !exclude_expanded.starts_with("**") {
        let stripped = if exclude_expanded.starts_with("./") {
            &exclude_expanded[2..]
        } else {
            &exclude_expanded
        };
        format!("**/{}", stripped)
    } else {
        exclude_expanded
    };

    println!("Code's normalized pattern: {}", normalized_pattern);
    let code_pattern = glob::Pattern::new(&normalized_pattern).unwrap();
    println!("Code's pattern matches_path: {}", code_pattern.matches_path(&abs_path));

    // Test against other files
    let subagent_path = logs_dir.join("project-a/subagents/agent-1.jsonl");
    println!("\nSubagent path: {:?}", subagent_path);
    println!("Pattern matches subagent: {}", code_pattern.matches_path(&subagent_path));
}
