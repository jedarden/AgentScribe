//! Code artifact extraction.
//!
//! Extracts fenced code blocks from assistant responses for indexing
//! as separate Tantivy documents with doc_type "code_artifact".

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use crate::event::{Event, Role};

/// A code artifact extracted from a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeArtifact {
    /// The code content
    pub code: String,
    /// Detected language from the fence marker
    pub language: String,
    /// Associated file path (if detected from context)
    pub file_path: Option<String>,
    /// Whether this is the final version of this code in the session
    pub is_final: bool,
}

/// Regex to match fenced code blocks.
static CODE_FENCE_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?m)```([a-zA-Z0-9_+#-]*)\n([\s\S]*?)```").unwrap());

/// Regex to extract file path from content near code blocks.
static FILE_PATH_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"(?:File:|Path:|file |path |`)((?:/[\w.-]+|[\w.-]+\.\w{1,10}))`?"
    ).unwrap()
});

/// Extract code artifacts from a session's events.
///
/// Finds all fenced code blocks in assistant responses, determines
/// language and file path context, and marks the last code block of
/// each language+file combination as `is_final = true`.
pub fn extract_code_artifacts(events: &[Event]) -> Vec<CodeArtifact> {
    let mut artifacts: Vec<CodeArtifact> = Vec::new();
    // Track last index for each (language, file_path) combination
    let mut last_seen: HashMap<(String, Option<String>), usize> = HashMap::new();

    for event in events {
        if event.role != Role::Assistant {
            continue;
        }

        // Try to find file path context in the content
        let context_file = extract_context_file(&event.content);

        for cap in CODE_FENCE_RE.captures_iter(&event.content) {
            let language = cap.get(1).map_or("", |m| m.as_str()).to_lowercase();
            let code = cap.get(2).map_or("", |m| m.as_str()).to_string();

            // Skip empty code blocks
            if code.trim().is_empty() {
                continue;
            }

            let file_path = context_file.as_deref().map(|s| s.to_string());

            let key = (language.clone(), file_path.clone());
            let idx = artifacts.len();
            last_seen.insert(key, idx);

            artifacts.push(CodeArtifact {
                code,
                language,
                file_path,
                is_final: false,
            });
        }
    }

    // Mark the last artifact of each (language, file) combination as final
    for &idx in last_seen.values() {
        if let Some(artifact) = artifacts.get_mut(idx) {
            artifact.is_final = true;
        }
    }

    artifacts
}

/// Try to extract a file path from the surrounding context of a code block.
fn extract_context_file(content: &str) -> Option<String> {
    // Look for file path mentions before code blocks
    if let Some(mat) = FILE_PATH_RE.find(content) {
        let path = mat.as_str();
        // Clean up the match
        let cleaned = path
            .trim_start_matches("File: ")
            .trim_start_matches("Path: ")
            .trim_start_matches("file ")
            .trim_start_matches("path ")
            .trim_start_matches('`')
            .trim_end_matches('`')
            .trim()
            .to_string();

        if !cleaned.is_empty() && (cleaned.contains('/') || cleaned.contains('.')) {
            return Some(cleaned);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use chrono::Utc;

    fn make_assistant_event(content: &str) -> Event {
        Event::new(
            Utc::now(),
            "test/1".into(),
            "test".into(),
            Role::Assistant,
            content.into(),
        )
    }

    #[test]
    fn test_extract_basic_code_block() {
        let events = vec![make_assistant_event(
            "Here's the code:\n```rust\nfn main() {\n    println!(\"Hello\");\n}\n```",
        )];
        let artifacts = extract_code_artifacts(&events);
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].language, "rust");
        assert!(artifacts[0].code.contains("fn main"));
        assert!(artifacts[0].is_final);
    }

    #[test]
    fn test_extract_multiple_languages() {
        let events = vec![make_assistant_event(
            "Rust code:\n```rust\nfn foo() {}\n```\n\nPython code:\n```python\ndef bar():\n    pass\n```",
        )];
        let artifacts = extract_code_artifacts(&events);
        assert_eq!(artifacts.len(), 2);
        assert_eq!(artifacts[0].language, "rust");
        assert_eq!(artifacts[1].language, "python");
        assert!(artifacts[0].is_final);
        assert!(artifacts[1].is_final);
    }

    #[test]
    fn test_is_final_flag() {
        let events = vec![
            make_assistant_event("First attempt:\n```rust\nfn foo() { old }\n```"),
            make_assistant_event("Fixed version:\n```rust\nfn foo() { new }\n```"),
        ];
        let artifacts = extract_code_artifacts(&events);
        assert_eq!(artifacts.len(), 2);
        assert!(!artifacts[0].is_final); // First version is not final
        assert!(artifacts[1].is_final);  // Second version is final
    }

    #[test]
    fn test_skip_inline_code() {
        let events = vec![make_assistant_event("Use `foo` to do things.")];
        let artifacts = extract_code_artifacts(&events);
        assert!(artifacts.is_empty());
    }

    #[test]
    fn test_skip_empty_code_block() {
        let events = vec![make_assistant_event("Empty:\n```\n```")];
        let artifacts = extract_code_artifacts(&events);
        assert!(artifacts.is_empty());
    }

    #[test]
    fn test_extract_file_path_context() {
        let events = vec![make_assistant_event(
            "Here's the fix for File: `src/main.rs`:\n```rust\nfn main() {}\n```",
        )];
        let artifacts = extract_code_artifacts(&events);
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].file_path.as_deref(), Some("src/main.rs"));
    }

    #[test]
    fn test_non_assistant_events_ignored() {
        let events = vec![
            Event::new(
                Utc::now(),
                "test/1".into(),
                "test".into(),
                Role::User,
                "```rust\nfn main() {}\n```".into(),
            ),
        ];
        let artifacts = extract_code_artifacts(&events);
        assert!(artifacts.is_empty());
    }

    #[test]
    fn test_no_language_marker() {
        let events = vec![make_assistant_event(
            "Here's some code:\n```\necho hello\n```",
        )];
        let artifacts = extract_code_artifacts(&events);
        assert_eq!(artifacts.len(), 1);
        assert!(artifacts[0].language.is_empty());
    }

    #[test]
    fn test_different_file_same_language_both_final() {
        let events = vec![
            make_assistant_event(
                "File: `src/a.rs`:\n```rust\nfn a() {}\n```",
            ),
            make_assistant_event(
                "File: `src/b.rs`:\n```rust\nfn b() {}\n```",
            ),
        ];
        let artifacts = extract_code_artifacts(&events);
        assert_eq!(artifacts.len(), 2);
        assert!(artifacts[0].is_final); // Different file paths
        assert!(artifacts[1].is_final);
    }
}
