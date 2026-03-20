//! Tag extraction: three-tier pipeline
//!
//! Tier 1 (explicit): tool names from tool_call events, language names from code fences.
//! Tier 2 (structural): file extension -> language mappings, bash command names, error fingerprints.
//! Tier 3 (keyword): word-boundary matching against a bundled technology dictionary.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use crate::event::{Event, Role};

/// Bundled keyword dictionary loaded at startup.
static KEYWORDS: LazyLock<Vec<String>> = LazyLock::new(|| {
    include_str!("../data/tech_keywords.txt")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_lowercase())
        .collect()
});

static CODE_FENCE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"```([a-zA-Z0-9_+#-]+)").unwrap());

static EXTENSION_MAP: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    [
        ("rs", "rust"),
        ("py", "python"),
        ("ts", "typescript"),
        ("tsx", "react"),
        ("jsx", "react"),
        ("js", "javascript"),
        ("go", "go"),
        ("mod", "go"),
        ("rb", "ruby"),
        ("java", "java"),
        ("kt", "kotlin"),
        ("swift", "swift"),
        ("c", "c"),
        ("cpp", "cpp"),
        ("h", "c"),
        ("hpp", "cpp"),
        ("cs", "csharp"),
        ("php", "php"),
        ("sh", "shell"),
        ("bash", "shell"),
        ("sql", "sql"),
        ("html", "html"),
        ("css", "css"),
        ("scss", "scss"),
        ("toml", "toml"),
        ("yaml", "yaml"),
        ("yml", "yaml"),
        ("json", "json"),
        ("md", "markdown"),
        ("lua", "lua"),
        ("r", "r"),
        ("dart", "dart"),
        ("zig", "zig"),
        ("ex", "elixir"),
        ("exs", "elixir"),
        ("hs", "haskell"),
        ("scala", "scala"),
    ]
    .into_iter()
    .collect()
});

static BASH_CMD_MAP: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    [
        ("docker", "docker"),
        ("docker-compose", "docker"),
        ("git", "git"),
        ("npm", "npm"),
        ("cargo", "cargo"),
        ("kubectl", "kubectl"),
        ("make", "make"),
        ("pip", "pip"),
        ("pip3", "pip"),
        ("python", "python"),
        ("python3", "python"),
        ("node", "node"),
        ("yarn", "yarn"),
        ("pnpm", "pnpm"),
    ]
    .into_iter()
    .collect()
});

/// Extract explicit tags from events.
///
/// Pulls tool names from `ToolCall` events and language identifiers from markdown code fences.
pub fn extract_explicit_tags(events: &[Event]) -> Vec<String> {
    let mut tags = HashSet::new();

    for event in events {
        if event.role == Role::ToolCall {
            if let Some(ref tool) = event.tool {
                tags.insert(tool.to_lowercase());
            }
        }

        for cap in CODE_FENCE_RE.captures_iter(&event.content) {
            if let Some(lang) = cap.get(1) {
                let lang = lang.as_str().to_lowercase();
                if !lang.is_empty() {
                    tags.insert(lang);
                }
            }
        }
    }

    let mut result: Vec<String> = tags.into_iter().collect();
    result.sort();
    result
}

/// Extract structural tags from events.
///
/// Maps file extensions to languages, extracts command names from Bash content,
/// and collects error fingerprints.
pub fn extract_structural_tags(events: &[Event]) -> Vec<String> {
    let mut tags = HashSet::new();

    for event in events {
        for path in &event.file_paths {
            if let Some(ext) = path.rsplit('.').next() {
                if let Some(&lang) = EXTENSION_MAP.get(ext) {
                    tags.insert(lang.to_string());
                }
            }
        }

        if event.role == Role::ToolCall && event.tool.as_deref() == Some("Bash") {
            let mut tokens = event.content.split_whitespace();
            let cmd = tokens.next().unwrap_or("");
            // Skip shell prompt prefixes like `$` or `#`
            if cmd == "$" || cmd == "#" {
                if let Some(actual_cmd) = tokens.next() {
                    if let Some(&tag) = BASH_CMD_MAP.get(actual_cmd) {
                        tags.insert(tag.to_string());
                    }
                }
            } else if let Some(&tag) = BASH_CMD_MAP.get(cmd) {
                tags.insert(tag.to_string());
            }
        }

        for fp in &event.error_fingerprints {
            tags.insert(fp.to_lowercase());
        }
    }

    let mut result: Vec<String> = tags.into_iter().collect();
    result.sort();
    result
}

/// Extract keyword tags from content using a technology dictionary.
///
/// Uses word-boundary matching (case-insensitive) so "react" matches "using React"
/// but not "reactive" or "rediscover".
pub fn extract_keyword_tags(content: &str, dictionary: &[String]) -> Vec<String> {
    let mut tags = HashSet::new();
    let lower = content.to_lowercase();

    for keyword in dictionary {
        // Build word-boundary regex: \bkeyword\b
        let pattern = format!(r"\b{}\b", regex::escape(keyword));
        if let Ok(re) = Regex::new(&pattern) {
            if re.is_match(&lower) {
                tags.insert(keyword.clone());
            }
        }
    }

    let mut result: Vec<String> = tags.into_iter().collect();
    result.sort();
    result
}

/// Extract all tags from a list of events, combining all three tiers.
///
/// Returns deduplicated, sorted tags.
pub fn extract_tags(events: &[Event]) -> Vec<String> {
    let mut all_tags = HashSet::new();

    // Tier 1: explicit tags
    for tag in extract_explicit_tags(events) {
        all_tags.insert(tag);
    }

    // Tier 2: structural tags
    for tag in extract_structural_tags(events) {
        all_tags.insert(tag);
    }

    // Tier 3: keyword tags (match against all event content)
    let content: String = events.iter().map(|e| e.content.as_str()).collect::<Vec<_>>().join(" ");
    for tag in extract_keyword_tags(&content, &KEYWORDS) {
        all_tags.insert(tag);
    }

    let mut result: Vec<String> = all_tags.into_iter().collect();
    result.sort();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use chrono::Utc;

    fn make_tool_call_event(tool: &str, content: &str) -> Event {
        Event::new(
            Utc::now(),
            "test/1".into(),
            "test".into(),
            Role::ToolCall,
            content.into(),
        )
        .with_tool(Some(tool.into()))
    }

    fn make_event(role: Role, content: &str) -> Event {
        Event::new(
            Utc::now(),
            "test/1".into(),
            "test".into(),
            role,
            content.into(),
        )
    }

    #[test]
    fn test_tool_call_produces_tag() {
        let events = vec![make_tool_call_event("Edit", "some content")];
        let tags = extract_explicit_tags(&events);
        assert!(tags.contains(&"edit".to_string()));
    }

    #[test]
    fn test_code_fence_python() {
        let events =
            vec![make_event(Role::Assistant, "Here is code:\n```python\nprint('hi')\n```")];
        let tags = extract_explicit_tags(&events);
        assert!(tags.contains(&"python".to_string()));
    }

    #[test]
    fn test_file_path_rust() {
        let events = vec![make_event(Role::ToolCall, "read file")
            .with_tool(Some("Read".into()))
            .with_file_paths(vec!["src/main.rs".into()])];
        let tags = extract_structural_tags(&events);
        assert!(tags.contains(&"rust".to_string()));
    }

    #[test]
    fn test_bash_docker() {
        let events = vec![make_tool_call_event("Bash", "docker build .")];
        let tags = extract_structural_tags(&events);
        assert!(tags.contains(&"docker".to_string()));
    }

    #[test]
    fn test_duplicate_tags_deduplicated() {
        let events = vec![
            make_tool_call_event("Edit", ""),
            make_tool_call_event("Edit", ""),
            make_tool_call_event("edit", ""),
        ];
        let tags = extract_explicit_tags(&events);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0], "edit");
    }

    #[test]
    fn test_tags_are_lowercase() {
        let events = vec![make_tool_call_event("Bash", "")];
        let tags = extract_explicit_tags(&events);
        assert!(tags.iter().all(|t| *t == t.to_lowercase()));
    }

    #[test]
    fn test_at_least_15_extension_mappings() {
        assert!(EXTENSION_MAP.len() >= 15);
    }

    #[test]
    fn test_multiple_extensions() {
        let events = vec![make_event(Role::ToolCall, "")
            .with_tool(Some("Read".into()))
            .with_file_paths(vec![
                "src/main.rs".into(),
                "src/lib.py".into(),
                "go.mod".into(),
            ])];
        let tags = extract_structural_tags(&events);
        assert!(tags.contains(&"rust".to_string()));
        assert!(tags.contains(&"python".to_string()));
        assert!(tags.contains(&"go".to_string()));
    }

    #[test]
    fn test_multiple_code_fences() {
        let events = vec![make_event(
            Role::Assistant,
            "```rust\nfn main() {}\n```\nand\n```typescript\nconst x = 1;\n```",
        )];
        let tags = extract_explicit_tags(&events);
        assert!(tags.contains(&"rust".to_string()));
        assert!(tags.contains(&"typescript".to_string()));
    }

    #[test]
    fn test_error_fingerprints() {
        let events = vec![make_event(Role::Assistant, "something failed")
            .with_error_fingerprints(vec!["OutOfMemory".into(), "TimeoutError".into()])];
        let tags = extract_structural_tags(&events);
        assert!(tags.contains(&"outofmemory".to_string()));
        assert!(tags.contains(&"timeouterror".to_string()));
    }

    #[test]
    fn test_bash_command_strip_prompt() {
        let events = vec![make_tool_call_event("Bash", "$ docker ps")];
        let tags = extract_structural_tags(&events);
        assert!(tags.contains(&"docker".to_string()));
    }

    #[test]
    fn test_bash_unknown_command_ignored() {
        let events = vec![make_tool_call_event("Bash", "foobar --help")];
        let tags = extract_structural_tags(&events);
        assert!(!tags.contains(&"foobar".to_string()));
    }

    #[test]
    fn test_non_bash_tool_ignored_for_commands() {
        let events = vec![make_tool_call_event("Edit", "docker build .")];
        let tags = extract_structural_tags(&events);
        assert!(!tags.contains(&"docker".to_string()));
    }

    #[test]
    fn test_empty_events() {
        let events: Vec<Event> = vec![];
        assert!(extract_explicit_tags(&events).is_empty());
        assert!(extract_structural_tags(&events).is_empty());
        assert!(extract_tags(&events).is_empty());
    }

    // --- Tier 3: keyword matching tests ---

    #[test]
    fn test_keyword_match_react() {
        let dict = vec!["react".to_string(), "postgres".to_string()];
        let tags = extract_keyword_tags("I am using react and postgres", &dict);
        assert!(tags.contains(&"react".to_string()));
        assert!(tags.contains(&"postgres".to_string()));
    }

    #[test]
    fn test_keyword_no_substring_match() {
        // "reactive" should NOT match "react"
        let dict = vec!["react".to_string()];
        let tags = extract_keyword_tags("reactive programming", &dict);
        assert!(!tags.contains(&"react".to_string()));
    }

    #[test]
    fn test_keyword_no_redis_in_rediscover() {
        let dict = vec!["redis".to_string()];
        let tags = extract_keyword_tags("rediscover the lost art", &dict);
        assert!(!tags.contains(&"redis".to_string()));
    }

    #[test]
    fn test_keyword_case_insensitive() {
        let dict = vec!["docker".to_string(), "kubernetes".to_string()];
        let tags = extract_keyword_tags("Deploy with Docker and Kubernetes", &dict);
        assert!(tags.contains(&"docker".to_string()));
        assert!(tags.contains(&"kubernetes".to_string()));
    }

    #[test]
    fn test_keyword_no_match() {
        let dict = vec!["react".to_string()];
        let tags = extract_keyword_tags("no relevant tech here", &dict);
        assert!(tags.is_empty());
    }

    #[test]
    fn test_keyword_deduplication() {
        let dict = vec!["react".to_string()];
        let tags = extract_keyword_tags("react react react", &dict);
        assert_eq!(tags.len(), 1);
    }

    #[test]
    fn test_keyword_with_special_chars_escaped() {
        // Keywords with dots like "next.js" should be escaped in regex
        let dict = vec!["next.js".to_string()];
        let tags = extract_keyword_tags("Built with next.js framework", &dict);
        assert!(tags.contains(&"next.js".to_string()));
        // Should NOT match "nextXjs" or "next js"
        let tags2 = extract_keyword_tags("using next js separately", &dict);
        assert!(!tags2.contains(&"next.js".to_string()));
    }

    // --- Combined extract_tags tests ---

    #[test]
    fn test_extract_tags_combines_all_tiers() {
        let events = vec![
            // Tier 1: tool call + code fence
            make_tool_call_event("Bash", "docker compose up"),
            // Tier 1: code fence language
            make_event(Role::Assistant, "```python\nprint('hi')\n```"),
            // Tier 2: file extension
            make_event(Role::ToolCall, "read")
                .with_tool(Some("Read".into()))
                .with_file_paths(vec!["src/main.rs".into()]),
            // Tier 3: keyword
            make_event(Role::User, "deploy this to kubernetes"),
        ];
        let tags = extract_tags(&events);
        assert!(tags.contains(&"bash".to_string()));
        assert!(tags.contains(&"python".to_string()));
        assert!(tags.contains(&"rust".to_string()));
        assert!(tags.contains(&"docker".to_string()));
        assert!(tags.contains(&"kubernetes".to_string()));
    }

    #[test]
    fn test_extract_tags_no_duplicates_across_tiers() {
        // "docker" appears in Tier 2 (bash cmd) and could appear in Tier 3 (keyword)
        let events = vec![
            make_tool_call_event("Bash", "docker build ."),
            make_event(Role::User, "use docker for containerization"),
        ];
        let tags = extract_tags(&events);
        let docker_count = tags.iter().filter(|t| *t == "docker").count();
        assert_eq!(docker_count, 1);
    }

    #[test]
    fn test_extract_tags_sorted() {
        let events = vec![make_tool_call_event("Edit", "content")];
        let tags = extract_tags(&events);
        let mut sorted = tags.clone();
        sorted.sort();
        assert_eq!(tags, sorted);
    }

    #[test]
    fn test_bundled_dictionary_has_terms() {
        assert!(KEYWORDS.len() >= 150);
    }
}
