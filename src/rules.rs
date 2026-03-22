//! Rules extraction: distill session patterns into agent-specific rules files.
//!
//! Uses frequency-based heuristics (no LLM) to extract:
//! - Tool/command preferences (e.g. pnpm over npm)
//! - Language/framework conventions
//! - Test directory patterns
//! - Common error pitfalls
//! - User corrections ("don't use X, use Y")

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::error::Result;
use crate::event::{Event, Role};
use crate::scraper::Scraper;

use std::sync::LazyLock;

/// Regex patterns for detecting user corrections in conversation content.
static CORRECTION_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        // "don't use X, use Y" / "don't use X. Use Y"
        Regex::new(r"(?i)don'?t use\s+(\S+).*?(?:use|try|prefer)\s+(\S+)").unwrap(),
        // "use X instead of Y" / "use X instead. not Y"
        Regex::new(r"(?i)(?:use|try|prefer)\s+(\S+)\s+instead\s+of\s+(\S+)").unwrap(),
        // "always use X" / "never use Y"
        Regex::new(r"(?i)always\s+(?:use|prefer)\s+(\S+)").unwrap(),
        Regex::new(r"(?i)never\s+(?:use|prefer)\s+(\S+)").unwrap(),
        // "prefer X over Y"
        Regex::new(r"(?i)prefer\s+(\S+)\s+over\s+(\S+)").unwrap(),
        // "X is preferred" / "Y is not preferred"
        Regex::new(r"(?i)(\S+)\s+is\s+(?:not\s+)?preferred").unwrap(),
    ]
});

/// Map of competing tool pairs (if both appear, the more frequent one wins).
static COMPETING_TOOLS: LazyLock<Vec<(&'static str, &'static str)>> = LazyLock::new(|| {
    vec![
        ("pnpm", "npm"),
        ("pnpm", "yarn"),
        ("yarn", "npm"),
        ("bun", "npm"),
        ("cargo", "rustup"),
        ("pip", "conda"),
        ("pytest", "unittest"),
        ("vitest", "jest"),
        ("pnpm", "bun"),
    ]
});

/// A single extracted rule.
#[derive(Debug, Clone)]
pub enum Rule {
    /// "Use X" convention
    Convention(String),
    /// "Avoid X" warning
    Warning(String),
    /// Project context (language, framework, build tool)
    Context(String),
    /// User correction ("don't use X, use Y")
    Correction(String),
}

impl Rule {
    fn as_str(&self) -> &str {
        match self {
            Rule::Convention(s) => s,
            Rule::Warning(s) => s,
            Rule::Context(s) => s,
            Rule::Correction(s) => s,
        }
    }
}

/// Aggregated rule extraction results.
pub struct RulesOutput {
    pub rules: Vec<Rule>,
    pub project_path: PathBuf,
    pub sessions_analyzed: usize,
}

/// Output format for rules files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Claude,
    Cursor,
    Aider,
}

impl OutputFormat {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "claude" => Some(OutputFormat::Claude),
            "cursor" => Some(OutputFormat::Cursor),
            "aider" => Some(OutputFormat::Aider),
            _ => None,
        }
    }

    /// Default output filename for this format.
    pub fn filename(&self) -> &'static str {
        match self {
            OutputFormat::Claude => "CLAUDE.md",
            OutputFormat::Cursor => ".cursorrules",
            OutputFormat::Aider => ".aider.conf.yml",
        }
    }
}

/// Extract rules from all sessions matching a project path.
pub fn extract_rules(
    data_dir: &Path,
    project_path: &Path,
) -> Result<RulesOutput> {
    let mut scraper = Scraper::new(data_dir.to_path_buf())?;
    scraper.load_plugins()?;

    let project_str = project_path.to_string_lossy().to_string();
    let project_str_normalized = if project_str.ends_with('/') {
        project_str.clone()
    } else {
        format!("{}/", project_str)
    };

    let all_sessions = scraper.all_sessions()?;
    let mut rules: Vec<Rule> = Vec::new();
    let mut sessions_analyzed = 0;

    // Accumulators for frequency analysis
    let mut bash_cmd_counts: HashMap<String, usize> = HashMap::new();
    let mut code_fence_langs: HashMap<String, usize> = HashMap::new();
    let mut file_extensions: HashMap<String, usize> = HashMap::new();
    let mut tool_counts: HashMap<String, usize> = HashMap::new();
    let mut error_fingerprints: HashMap<String, usize> = HashMap::new();
    let mut test_dirs: HashMap<String, usize> = HashMap::new();
    let mut corrections: Vec<String> = Vec::new();

    for session_id in &all_sessions {
        let events = match scraper.read_session(session_id) {
            Ok(e) => e,
            Err(_) => continue,
        };

        if events.is_empty() {
            continue;
        }

        // Filter sessions by project path
        let session_project = events.iter().find_map(|e| e.project.clone());
        let matches_project = match &session_project {
            Some(p) => p.starts_with(&project_str_normalized) || p == &project_str,
            None => false,
        };

        if !matches_project {
            continue;
        }

        sessions_analyzed += 1;

        for event in &events {
            // Bash commands
            if event.role == Role::ToolCall && event.tool.as_deref() == Some("Bash") {
                let mut tokens = event.content.split_whitespace();
                let cmd = tokens.next().unwrap_or("");
                if cmd == "$" || cmd == "#" {
                    if let Some(actual) = tokens.next() {
                        *bash_cmd_counts.entry(actual.to_string()).or_insert(0) += 1;
                    }
                } else {
                    *bash_cmd_counts.entry(cmd.to_string()).or_insert(0) += 1;
                }
            }

            // Tool names
            if event.role == Role::ToolCall {
                if let Some(ref tool) = event.tool {
                    *tool_counts.entry(tool.to_lowercase()).or_insert(0) += 1;
                }
            }

            // Code fence languages
            for cap in CODE_FENCE_RE.captures_iter(&event.content) {
                if let Some(lang) = cap.get(1) {
                    let lang = lang.as_str().to_lowercase();
                    if !lang.is_empty() && lang != "text" && lang != "plaintext" {
                        *code_fence_langs.entry(lang).or_insert(0) += 1;
                    }
                }
            }

            // File extensions
            for fp in &event.file_paths {
                if let Some(ext) = fp.rsplit('.').next() {
                    *file_extensions.entry(ext.to_lowercase()).or_insert(0) += 1;
                }
                // Detect test directories
                for component in fp.split('/') {
                    if component == "tests" || component == "__tests__" || component == "spec" || component == "specs" {
                        *test_dirs.entry(component.to_string()).or_insert(0) += 1;
                    }
                    if component.starts_with("test_") || component.ends_with("_test") {
                        *test_dirs.entry(format!("{}_files", component)).or_insert(0) += 1;
                    }
                }
            }

            // Error fingerprints
            for fp in &event.error_fingerprints {
                *error_fingerprints.entry(fp.clone()).or_insert(0) += 1;
            }

            // User corrections
            if event.role == Role::User {
                extract_corrections(&event.content, &mut corrections);
            }
        }
    }

    // --- Generate rules from frequency analysis ---

    // 1. Language/framework context (top 3 code fence languages)
    let mut langs: Vec<_> = code_fence_langs.iter().collect();
    langs.sort_by(|a, b| b.1.cmp(a.1));
    for (lang, count) in langs.iter().take(3) {
        if **count >= 2 {
            rules.push(Rule::Context(format!("Primary language: {}", lang)));
        }
    }

    // 2. Tool/command preferences (competing tools)
    for (preferred, disfavored) in COMPETING_TOOLS.iter() {
        let pref_count = bash_cmd_counts.get(*preferred).copied().unwrap_or(0);
        let disf_count = bash_cmd_counts.get(*disfavored).copied().unwrap_or(0);

        if pref_count > 0 && pref_count > disf_count {
            rules.push(Rule::Convention(format!(
                "Use {} instead of {} (seen {}x vs {}x)",
                preferred, disfavored, pref_count, disf_count
            )));
        } else if pref_count > 0 && disf_count == 0 {
            rules.push(Rule::Convention(format!(
                "Use {} ({} sessions, {} never used)",
                preferred, pref_count, disfavored
            )));
        }
    }

    // 3. Build system detection
    let build_tools = ["cargo", "make", "cmake", "npm", "pnpm", "yarn", "bun", "go", "gcc", "clang"];
    for tool in &build_tools {
        if let Some(count) = bash_cmd_counts.get(*tool) {
            if *count >= 2 {
                rules.push(Rule::Context(format!("Build system: {}", tool)));
            }
        }
    }

    // 4. Test patterns
    if !test_dirs.is_empty() {
        let top_test: Vec<_> = test_dirs.iter()
            .max_by_key(|(_, c)| *c)
            .map(|(k, _)| k.clone())
            .into_iter()
            .collect();
        if let Some(dir) = top_test.first() {
            if dir.ends_with("_files") {
                rules.push(Rule::Context(format!(
                    "Test files follow {} naming convention",
                    dir.replace("_files", "")
                )));
            } else {
                rules.push(Rule::Context(format!("Tests in {} directory", dir)));
            }
        }
    }

    // 5. Common error pitfalls (top 5 recurring errors)
    let mut top_errors: Vec<_> = error_fingerprints.iter().collect();
    top_errors.sort_by(|a, b| b.1.cmp(a.1));
    for (fp, count) in top_errors.iter().take(5) {
        if **count >= 2 {
            let short_fp = truncate_fp(fp, 100);
            rules.push(Rule::Warning(format!(
                "Watch for recurring error: {} ({} occurrences)",
                short_fp, count
            )));
        }
    }

    // 6. User corrections
    for correction in &corrections {
        rules.push(Rule::Correction(correction.clone()));
    }

    // Deduplicate rules
    let mut seen = std::collections::HashSet::new();
    rules.retain(|r| seen.insert(r.as_str().to_string()));

    // Sort: corrections first, then conventions, then warnings, then context
    rules.sort_by(|a, b| match (a, b) {
        (Rule::Correction(_), _) => std::cmp::Ordering::Less,
        (_, Rule::Correction(_)) => std::cmp::Ordering::Greater,
        (Rule::Convention(_), _) => std::cmp::Ordering::Less,
        (_, Rule::Convention(_)) => std::cmp::Ordering::Greater,
        (Rule::Warning(_), _) => std::cmp::Ordering::Less,
        (_, Rule::Warning(_)) => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    });

    Ok(RulesOutput {
        rules,
        project_path: project_path.to_path_buf(),
        sessions_analyzed,
    })
}

static CODE_FENCE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"```([a-zA-Z0-9_+#-]+)").unwrap());

/// Extract correction patterns from user messages.
fn extract_corrections(content: &str, corrections: &mut Vec<String>) {
    for pattern in CORRECTION_PATTERNS.iter() {
        if let Some(caps) = pattern.captures(content) {
            // Build a readable correction string
            let full_match = caps.get(0).unwrap().as_str().trim();
            // Clean up: remove trailing punctuation, limit length
            let cleaned = full_match
                .trim_end_matches(|c: char| c == '.' || c == '!' || c == '?' || c == ',')
                .trim();

            if cleaned.len() >= 5 && cleaned.len() <= 200 {
                if !corrections.contains(&cleaned.to_string()) {
                    corrections.push(cleaned.to_string());
                }
            }
        }
    }
}

/// Truncate a fingerprint for display.
fn truncate_fp(fp: &str, max_len: usize) -> String {
    if fp.len() <= max_len {
        return fp.to_string();
    }
    if let Some(colon_pos) = fp.find(':') {
        let prefix = &fp[..=colon_pos];
        let remaining = max_len.saturating_sub(prefix.len() + 4);
        if remaining > 10 {
            return format!("{}{}...", prefix, &fp[colon_pos + 1..colon_pos + 1 + remaining]);
        }
    }
    format!("{}...", &fp[..max_len.saturating_sub(3)])
}

/// Format rules as CLAUDE.md content (additive sections).
pub fn format_claude(output: &RulesOutput) -> String {
    let mut sections = Vec::new();

    let corrections: Vec<_> = output.rules.iter()
        .filter_map(|r| match r {
            Rule::Correction(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();

    let conventions: Vec<_> = output.rules.iter()
        .filter_map(|r| match r {
            Rule::Convention(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();

    let warnings: Vec<_> = output.rules.iter()
        .filter_map(|r| match r {
            Rule::Warning(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();

    let context: Vec<_> = output.rules.iter()
        .filter_map(|r| match r {
            Rule::Context(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();

    if !corrections.is_empty() {
        sections.push(format!("## User Corrections\n\n{}", corrections.iter()
            .map(|c| format!("- {}", c))
            .collect::<Vec<_>>()
            .join("\n")));
    }

    if !conventions.is_empty() {
        sections.push(format!("## Conventions\n\n{}", conventions.iter()
            .map(|c| format!("- {}", c))
            .collect::<Vec<_>>()
            .join("\n")));
    }

    if !context.is_empty() {
        sections.push(format!("## Project Context\n\n{}", context.iter()
            .map(|c| format!("- {}", c))
            .collect::<Vec<_>>()
            .join("\n")));
    }

    if !warnings.is_empty() {
        sections.push(format!("## Known Issues\n\n{}", warnings.iter()
            .map(|w| format!("- {}", w))
            .collect::<Vec<_>>()
            .join("\n")));
    }

    if sections.is_empty() {
        return String::new();
    }

    let header = format!(
        "<!-- Auto-generated by AgentScribe rules ({} sessions analyzed) -->\n",
        output.sessions_analyzed
    );

    format!("{}\n{}\n", header, sections.join("\n\n"))
}

/// Format rules as .cursorrules content (one instruction per line).
pub fn format_cursor(output: &RulesOutput) -> String {
    if output.rules.is_empty() {
        return String::new();
    }

    let lines: Vec<String> = output.rules.iter()
        .map(|r| {
            match r {
                Rule::Convention(s) => s.clone(),
                Rule::Correction(s) => s.clone(),
                Rule::Warning(s) => format!("Watch out: {}", s),
                Rule::Context(s) => s.clone(),
            }
        })
        .collect();

    format!(
        "# Auto-generated by AgentScribe rules ({} sessions analyzed)\n{}\n",
        output.sessions_analyzed,
        lines.join("\n")
    )
}

/// Format rules as .aider.conf.yml content.
pub fn format_aider(output: &RulesOutput) -> String {
    if output.rules.is_empty() {
        return String::new();
    }

    let mut conventions: Vec<String> = Vec::new();
    let mut context: Vec<String> = Vec::new();

    for rule in &output.rules {
        match rule {
            Rule::Convention(s) | Rule::Correction(s) | Rule::Warning(s) => {
                conventions.push(s.clone());
            }
            Rule::Context(s) => {
                context.push(s.clone());
            }
        }
    }

    let mut yaml = String::new();

    if !context.is_empty() {
        // Build read-only files list from context
        yaml.push_str("read: []\n");
    }

    if !conventions.is_empty() {
        let msg = conventions.join("\\n");
        yaml.push_str(&format!("message: |\n  {}\n", msg.replace('\n', "\n  ")));
    }

    format!(
        "# Auto-generated by AgentScribe rules ({} sessions analyzed)\n{}\n",
        output.sessions_analyzed,
        yaml
    )
}

/// Write rules to a file in the specified format, additive (appends if file exists).
pub fn write_rules(
    output: &RulesOutput,
    format: OutputFormat,
    project_path: &Path,
) -> Result<PathBuf> {
    let content = match format {
        OutputFormat::Claude => format_claude(output),
        OutputFormat::Cursor => format_cursor(output),
        OutputFormat::Aider => format_aider(output),
    };

    if content.is_empty() {
        return Err(crate::error::AgentScribeError::Rules(
            "No rules extracted from session data".to_string(),
        ));
    }

    let output_path = project_path.join(format.filename());

    let final_content = if output_path.exists() {
        let existing = std::fs::read_to_string(&output_path).unwrap_or_default();
        replace_or_prepend(&existing, &content, format)
    } else {
        content
    };

    // Ensure parent directory exists
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(&output_path, &final_content)?;

    Ok(output_path)
}

/// Replace an existing generated block, or prepend if not found.
fn replace_or_prepend(existing: &str, new_content: &str, format: OutputFormat) -> String {
    let marker = match format {
        OutputFormat::Claude => "<!-- Auto-generated by AgentScribe rules",
        OutputFormat::Cursor | OutputFormat::Aider => "# Auto-generated by AgentScribe rules",
    };

    if !existing.contains(marker) {
        // No existing generated block — prepend new content
        return format!("{}\n{}", new_content.trim_end(), existing);
    }

    // Find the start of the existing generated block
    let start = match existing.find(marker) {
        Some(s) => s,
        None => return format!("{}\n{}", new_content.trim_end(), existing),
    };

    // Find the end: first non-generated line after the marker.
    // Generated lines are those that belong to our output format's sections.
    let generated_sections: Vec<&str> = match format {
        OutputFormat::Claude => vec!["User Corrections", "Conventions", "Project Context", "Known Issues"],
        OutputFormat::Cursor | OutputFormat::Aider => vec![],
    };

    let remaining = &existing[start..];
    let mut end = remaining.len();

    for (i, line) in remaining.split('\n').enumerate() {
        if i == 0 {
            continue; // skip the marker line itself
        }
        let trimmed = line.trim();

        // Empty lines within generated block are ok
        if trimmed.is_empty() {
            continue;
        }

        // For CLAUDE.md: check if this ## heading is NOT one of ours
        if format == OutputFormat::Claude && trimmed.starts_with("## ") {
            let section_name = trimmed.trim_start_matches("## ").trim();
            if !generated_sections.contains(&section_name) {
                end = start + remaining[..].find(line).unwrap();
                break;
            }
            continue;
        }

        // For cursor/aider: any non-empty, non-comment line that isn't part of our
        // generated format marks the end
        if format != OutputFormat::Claude {
            // Skip known generated prefixes
            if trimmed.starts_with('#') || trimmed.starts_with("Watch out:") {
                continue;
            }
            end = start + remaining[..].find(line).unwrap();
            break;
        }
    }

    if end >= remaining.len() {
        // Generated block extends to end of file
        new_content.trim_end().to_string()
    } else {
        format!("{}\n{}", new_content.trim_end(), &existing[end..])
    }
}

/// Format rules output for human-readable terminal display.
pub fn format_human(output: &RulesOutput) -> String {
    if output.rules.is_empty() {
        return format!(
            "No rules extracted from {} session(s) for {}\n",
            output.sessions_analyzed,
            output.project_path.display()
        );
    }

    let mut lines = Vec::new();

    lines.push(format!(
        "Rules extracted from {} session(s) for {}\n",
        output.sessions_analyzed,
        output.project_path.display()
    ));

    let mut last_type = String::new();

    for rule in &output.rules {
        let (label, text) = match rule {
            Rule::Correction(s) => ("Correction", s.as_str()),
            Rule::Convention(s) => ("Convention", s.as_str()),
            Rule::Context(s) => ("Context", s.as_str()),
            Rule::Warning(s) => ("Warning", s.as_str()),
        };

        if label != last_type {
            if !last_type.is_empty() {
                lines.push(String::new());
            }
            lines.push(format!("  [{}]", label));
            last_type = label.to_string();
        }

        lines.push(format!("    - {}", text));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use chrono::Utc;

    fn make_event(role: Role, content: &str, project: &str) -> Event {
        Event::new(
            Utc::now(),
            "test/1".into(),
            "test".into(),
            role,
            content.into(),
        )
        .with_project(Some(project.into()))
    }

    fn make_tool_event(tool: &str, content: &str, project: &str) -> Event {
        Event::new(
            Utc::now(),
            "test/1".into(),
            "test".into(),
            Role::ToolCall,
            content.into(),
        )
        .with_tool(Some(tool.into()))
        .with_project(Some(project.into()))
    }

    #[test]
    fn test_output_format_from_str() {
        assert_eq!(OutputFormat::from_str("claude"), Some(OutputFormat::Claude));
        assert_eq!(OutputFormat::from_str("cursor"), Some(OutputFormat::Cursor));
        assert_eq!(OutputFormat::from_str("aider"), Some(OutputFormat::Aider));
        assert_eq!(OutputFormat::from_str("invalid"), None);
    }

    #[test]
    fn test_output_format_filename() {
        assert_eq!(OutputFormat::Claude.filename(), "CLAUDE.md");
        assert_eq!(OutputFormat::Cursor.filename(), ".cursorrules");
        assert_eq!(OutputFormat::Aider.filename(), ".aider.conf.yml");
    }

    #[test]
    fn test_extract_corrections_basic() {
        let mut corrections = Vec::new();
        extract_corrections("don't use npm, use pnpm instead", &mut corrections);
        assert_eq!(corrections.len(), 1);
        assert!(corrections[0].contains("don't"));
    }

    #[test]
    fn test_extract_corrections_prefer() {
        let mut corrections = Vec::new();
        extract_corrections("prefer pnpm over npm", &mut corrections);
        assert_eq!(corrections.len(), 1);
    }

    #[test]
    fn test_extract_corrections_always() {
        let mut corrections = Vec::new();
        extract_corrections("always use cargo build", &mut corrections);
        assert_eq!(corrections.len(), 1);
    }

    #[test]
    fn test_extract_corrections_no_match() {
        let mut corrections = Vec::new();
        extract_corrections("please fix the build error", &mut corrections);
        assert!(corrections.is_empty());
    }

    #[test]
    fn test_extract_corrections_dedup() {
        let mut corrections = Vec::new();
        extract_corrections("don't use npm, use pnpm instead", &mut corrections);
        extract_corrections("don't use npm, use pnpm instead", &mut corrections);
        assert_eq!(corrections.len(), 1);
    }

    #[test]
    fn test_format_claude() {
        let output = RulesOutput {
            rules: vec![
                Rule::Correction("don't use npm, use pnpm".to_string()),
                Rule::Convention("Use cargo for builds".to_string()),
                Rule::Context("Primary language: rust".to_string()),
            ],
            project_path: PathBuf::from("/tmp/test"),
            sessions_analyzed: 5,
        };

        let content = format_claude(&output);
        assert!(content.contains("## User Corrections"));
        assert!(content.contains("## Conventions"));
        assert!(content.contains("## Project Context"));
        assert!(content.contains("5 sessions analyzed"));
    }

    #[test]
    fn test_format_cursor() {
        let output = RulesOutput {
            rules: vec![
                Rule::Convention("Use pnpm".to_string()),
                Rule::Warning("Some error".to_string()),
            ],
            project_path: PathBuf::from("/tmp/test"),
            sessions_analyzed: 2,
        };

        let content = format_cursor(&output);
        assert!(content.contains("# Auto-generated"));
        assert!(content.contains("Use pnpm"));
        assert!(content.contains("Watch out:"));
    }

    #[test]
    fn test_format_aider() {
        let output = RulesOutput {
            rules: vec![
                Rule::Convention("Use pnpm".to_string()),
            ],
            project_path: PathBuf::from("/tmp/test"),
            sessions_analyzed: 1,
        };

        let content = format_aider(&output);
        assert!(content.contains("# Auto-generated"));
        assert!(content.contains("message:"));
        assert!(content.contains("Use pnpm"));
    }

    #[test]
    fn test_format_empty() {
        let output = RulesOutput {
            rules: vec![],
            project_path: PathBuf::from("/tmp/test"),
            sessions_analyzed: 0,
        };

        assert!(format_claude(&output).is_empty());
        assert!(format_cursor(&output).is_empty());
        assert!(format_aider(&output).is_empty());
    }

    #[test]
    fn test_truncate_fp() {
        assert_eq!(truncate_fp("short", 100), "short");
        assert!(truncate_fp("ErrorType:this is a very long message that should be truncated beyond one hundred characters", 40).ends_with("..."));
    }

    #[test]
    fn test_format_human() {
        let output = RulesOutput {
            rules: vec![
                Rule::Correction("don't use npm".to_string()),
                Rule::Context("Primary language: rust".to_string()),
            ],
            project_path: PathBuf::from("/tmp/test"),
            sessions_analyzed: 3,
        };

        let content = format_human(&output);
        assert!(content.contains("[Correction]"));
        assert!(content.contains("[Context]"));
        assert!(content.contains("3 session(s)"));
    }

    #[test]
    fn test_write_rules_new_file() {
        let temp = tempfile::tempdir().unwrap();
        let output = RulesOutput {
            rules: vec![Rule::Convention("Use pnpm".to_string())],
            project_path: temp.path().to_path_buf(),
            sessions_analyzed: 1,
        };

        let path = write_rules(&output, OutputFormat::Claude, temp.path()).unwrap();
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("Use pnpm"));
    }

    #[test]
    fn test_write_rules_additive() {
        let temp = tempfile::tempdir().unwrap();
        let existing_path = temp.path().join("CLAUDE.md");

        // Write existing content
        std::fs::write(&existing_path, "# My Project\n\nSome existing rules.\n").unwrap();

        let output = RulesOutput {
            rules: vec![Rule::Convention("Use pnpm".to_string())],
            project_path: temp.path().to_path_buf(),
            sessions_analyzed: 1,
        };

        let path = write_rules(&output, OutputFormat::Claude, temp.path()).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();

        // Should contain both new and existing content
        assert!(content.contains("Use pnpm"));
        assert!(content.contains("# My Project"));
        assert!(content.contains("Some existing rules"));
    }
}
