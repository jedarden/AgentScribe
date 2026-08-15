use glob::glob;
use shellexpand;

fn test_aider_glob_pattern() {
    let pattern = "~/**/.aider.chat.history.md";

    // Expand ~ to home directory
    let expanded = shellexpand::full(pattern).unwrap();
    println!("Expanded pattern: {}", expanded);

    // Test the glob pattern
    let glob_result = glob(&expanded);

    match glob_result {
        Ok(paths) => {
            let mut count = 0;
            for entry in paths.filter_map(|e| e.ok()) {
                println!("Found: {}", entry.display());
                count += 1;
            }
            println!("Total matches: {}", count);

            if count == 0 {
                println!("WARNING: No files found - this may be expected if no aider history files exist");
            }
        }
        Err(e) => {
            eprintln!("ERROR: Invalid glob pattern: {}", e);
            std::process::exit(1);
        }
    }
}

fn main() {
    println!("Testing Aider glob pattern: ~/**/.aider.chat.history.md");
    println!("===\n");

    test_aider_glob_pattern();

    println!("\n===\nTest completed successfully");
    println!("\nPattern analysis:");
    println!("  ~              → Expands to user's home directory");
    println!("  /**/           → Recursive directory match (zero or more directories)");
    println!("  .aider.chat.history.md → Exact filename match");
    println!("\nThis pattern will match .aider.chat.history.md files at any depth under the home directory.");
}
