#!/usr/bin/env rust-script
//! Extract import statements from all test files in the AgentScribe project

use std::fs;
use std::path::Path;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
struct ImportInfo {
    use_statements: Vec<String>,
    extern_crates: Vec<String>,
}

fn extract_imports_from_file(file_path: &Path) -> ImportInfo {
    let content = match fs::read_to_string(file_path) {
        Ok(content) => content,
        Err(_) => return ImportInfo {
            use_statements: vec![],
            extern_crates: vec![],
        },
    };

    let mut use_statements = Vec::new();
    let mut extern_crates = Vec::new();
    let mut in_test_module = false;
    let mut brace_depth = 0;

    for line in content.lines() {
        let trimmed = line.trim();

        // Track if we're inside a test module
        if trimmed.contains("#[cfg(test)]") {
            in_test_module = true;
            continue;
        }

        // Track brace depth to know when we exit the test module
        if in_test_module {
            brace_depth += trimmed.matches('{').count() as i32;
            brace_depth -= trimmed.matches('}').count() as i32;

            if brace_depth <= 0 {
                in_test_module = false;
                brace_depth = 0;
            }
        }

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }

        // Extract extern crate statements
        if trimmed.starts_with("extern crate") {
            extern_crates.push(trimmed.to_string());
        }

        // Extract use statements (both in test modules and at the top level)
        if trimmed.starts_with("use ") {
            // Clean up the use statement
            let clean_use = if let Some(semi_pos) = trimmed.find(';') {
                trimmed[..semi_pos].trim().to_string()
            } else {
                trimmed.trim().to_string()
            };

            // Remove "use " prefix for consistency
            let import_path = if clean_use.starts_with("use ") {
                clean_use[4..].trim().to_string()
            } else {
                clean_use
            };

            if !import_path.is_empty() && !use_statements.contains(&import_path) {
                use_statements.push(import_path);
            }
        }
    }

    ImportInfo {
        use_statements,
        extern_crates,
    }
}

fn main() {
    // All test files from the catalog
    let test_files = vec![
        // Integration test files
        "/home/coding/AgentScribe/tests/aider_glob_discovery_test.rs",
        "/home/coding/AgentScribe/tests/aider_input_scrape_test.rs",
        "/home/coding/AgentScribe/tests/aider_toml_glob_validation_test.rs",
        "/home/coding/AgentScribe/tests/context_tests.rs",
        "/home/coding/AgentScribe/tests/daemon_mcp.rs",
        "/home/coding/AgentScribe/tests/integration_tests.rs",
        "/home/coding/AgentScribe/tests/main_session_parent_tests.rs",
        "/home/coding/AgentScribe/tests/parent_session_tests.rs",
        "/home/coding/AgentScribe/tests/phase6_tests.rs",
        "/home/coding/AgentScribe/tests/pulse_report_tests.rs",
        "/home/coding/AgentScribe/tests/render_tests.rs",
        "/home/coding/AgentScribe/tests/subagent_integration_test.rs",
        "/home/coding/AgentScribe/tests/subagent_parent_session_unit_tests.rs",
        "/home/coding/AgentScribe/tests/subagent_spawning_integration_tests.rs",
        "/home/coding/AgentScribe/tests/test_helpers.rs",
        "/home/coding/AgentScribe/tests/transcription_tests.rs",
        "/home/coding/AgentScribe/tests/zero_write_tests.rs",

        // Dedicated unit test files
        "/home/coding/AgentScribe/src/parser/jsonl/jsonl_subagent_test.rs",
        "/home/coding/AgentScribe/test_timestamps.rs",

        // Source files with embedded tests
        "/home/coding/AgentScribe/src/analytics.rs",
        "/home/coding/AgentScribe/src/annotations.rs",
        "/home/coding/AgentScribe/src/capacity.rs",
        "/home/coding/AgentScribe/src/config.rs",
        "/home/coding/AgentScribe/src/daemon.rs",
        "/home/coding/AgentScribe/src/digest.rs",
        "/home/coding/AgentScribe/src/embedding.rs",
        "/home/coding/AgentScribe/src/event.rs",
        "/home/coding/AgentScribe/src/file_knowledge.rs",
        "/home/coding/AgentScribe/src/gc.rs",
        "/home/coding/AgentScribe/src/index.rs",
        "/home/coding/AgentScribe/src/mcp.rs",
        "/home/coding/AgentScribe/src/plugin.rs",
        "/home/coding/AgentScribe/src/projects.rs",
        "/home/coding/AgentScribe/src/pulse_report.rs",
        "/home/coding/AgentScribe/src/recurring.rs",
        "/home/coding/AgentScribe/src/redaction.rs",
        "/home/coding/AgentScribe/src/reflect.rs",
        "/home/coding/AgentScribe/src/render.rs",
        "/home/coding/AgentScribe/src/rules.rs",
        "/home/coding/AgentScribe/src/search.rs",
        "/home/coding/AgentScribe/src/shell_hook.rs",
        "/home/coding/AgentScribe/src/tags.rs",
        "/home/coding/AgentScribe/src/transcription.rs",
        "/home/coding/AgentScribe/src/vector.rs",
        "/home/coding/AgentScribe/src/write_guard.rs",
        "/home/coding/AgentScribe/src/enrichment/antipatterns.rs",
        "/home/coding/AgentScribe/src/enrichment/behavioral_signals.rs",
        "/home/coding/AgentScribe/src/enrichment/code_artifacts.rs",
        "/home/coding/AgentScribe/src/enrichment/config_change_tracker.rs",
        "/home/coding/AgentScribe/src/enrichment/errors.rs",
        "/home/coding/AgentScribe/src/enrichment/git.rs",
        "/home/coding/AgentScribe/src/enrichment/outcome.rs",
        "/home/coding/AgentScribe/src/enrichment/solution.rs",
        "/home/coding/AgentScribe/src/enrichment/summary.rs",
        "/home/coding/AgentScribe/src/parser/aider_input.rs",
        "/home/coding/AgentScribe/src/parser/json_array.rs",
        "/home/coding/AgentScribe/src/parser/jsonl.rs",
        "/home/coding/AgentScribe/src/parser/json_tree.rs",
        "/home/coding/AgentScribe/src/parser/markdown.rs",
        "/home/coding/AgentScribe/src/parser/mod.rs",
        "/home/coding/AgentScribe/src/parser/sqlite.rs",
        "/home/coding/AgentScribe/src/scraper/companion.rs",
        "/home/coding/AgentScribe/src/scraper/file_path_extractor.rs",
        "/home/coding/AgentScribe/src/scraper/mod.rs",
        "/home/coding/AgentScribe/src/scraper/state.rs",
    ];

    let mut all_imports: BTreeMap<String, ImportInfo> = BTreeMap::new();
    let mut total_use_count = 0;
    let mut total_extern_crate_count = 0;
    let mut files_with_imports = 0;

    for file_path in test_files {
        let path = Path::new(file_path);
        if !path.exists() {
            eprintln!("Warning: File not found: {}", file_path);
            continue;
        }

        let import_info = extract_imports_from_file(path);

        if !import_info.use_statements.is_empty() || !import_info.extern_crates.is_empty() {
            files_with_imports += 1;
            total_use_count += import_info.use_statements.len();
            total_extern_crate_count += import_info.extern_crates.len();

            // Use relative path for cleaner output
            let display_path = if let Ok(rel) = path.strip_prefix("/home/coding/AgentScribe/") {
                format!("/home/coding/AgentScribe/{}", rel.display())
            } else {
                file_path.to_string()
            };

            all_imports.insert(display_path, import_info);
        }
    }

    // Generate markdown output
    println!("# AgentScribe Test File Import Analysis");
    println!();
    println!("Generated: 2026-08-12");
    println!("Total test files analyzed: {}", all_imports.len());
    println!();

    for (file_path, import_info) in &all_imports {
        println!("## {}", file_path);
        println!();

        if !import_info.extern_crates.is_empty() {
            println!("### Extern Crate Statements ({})", import_info.extern_crates.len());
            println!("```rust");
            for extern_crate in &import_info.extern_crates {
                println!("{}", extern_crate);
            }
            println!("```");
            println!();
        }

        if !import_info.use_statements.is_empty() {
            println!("### Use Statements ({})", import_info.use_statements.len());
            println!("```rust");
            for use_stmt in &import_info.use_statements {
                println!("use {};", use_stmt);
            }
            println!("```");
            println!();
        }
    }

    println!("## Summary Statistics");
    println!("- Total files with imports: {}", files_with_imports);
    println!("- Total use statements: {}", total_use_count);
    println!("- Total extern crate statements: {}", total_extern_crate_count);
}
