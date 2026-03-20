//! Tag extraction: explicit and structural tiers
//!
//! Tier 1 (explicit): tool names from tool_call events, language names from code fences.
//! Tier 2 (structural): file extension -> language mappings, bash command names, error fingerprints.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use crate::event::{Event, Role};

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
    }
}
