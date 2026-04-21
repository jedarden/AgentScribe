//! Summary generation from session events.
//!
//! Generates a concise one-line summary from:
//! - First user prompt (truncated)
//! - Session outcome (if available)
//! - Files touched (if available)

use crate::event::{Event, Role, SessionManifest};

/// Maximum characters for the first prompt in summary
#[allow(dead_code)]
const MAX_PROMPT_CHARS: usize = 80;

/// Generate a one-line summary from session events and manifest.
///
/// The summary is constructed from:
/// 1. First user prompt (truncated to ~80 chars)
/// 2. Outcome (if detected)
/// 3. Key files touched (up to 3)
#[allow(dead_code)]
pub fn generate_summary(events: &[Event], manifest: &SessionManifest) -> String {
    // If manifest already has a good summary, use it
    if let Some(ref summary) = manifest.summary {
        if !summary.is_empty() && summary.len() > 10 {
            return summary.clone();
        }
    }

    // Extract first user prompt
    let first_prompt = events
        .iter()
        .find(|e| e.role == Role::User)
        .map(|e| e.content.as_str())
        .unwrap_or("");

    // Truncate and clean the prompt
    let prompt_part = truncate_and_clean(first_prompt, MAX_PROMPT_CHARS);

    // Get key files (up to 3, preferring files with extensions)
    let key_files = get_key_files(&manifest.files_touched, 3);

    // Build summary
    let mut parts = Vec::new();

    if !prompt_part.is_empty() {
        parts.push(prompt_part);
    }

    if !key_files.is_empty() {
        parts.push(format!("({})", key_files.join(", ")));
    }

    if parts.is_empty() {
        "Session with no user prompt".to_string()
    } else {
        parts.join(" ")
    }
}

/// Truncate text to max_chars, cleaning up whitespace and adding ellipsis if needed.
#[allow(dead_code)]
fn truncate_and_clean(text: &str, max_chars: usize) -> String {
    // Normalize whitespace
    let cleaned: String = text.split_whitespace().collect::<Vec<_>>().join(" ");

    if cleaned.len() <= max_chars {
        return cleaned;
    }

    // Find a good break point (space) near max_chars
    let truncated = &cleaned[..max_chars];
    if let Some(last_space) = truncated.rfind(' ') {
        format!("{}...", &cleaned[..last_space])
    } else {
        format!("{}...", truncated)
    }
}

/// Get key files from the list, preferring files with extensions.
#[allow(dead_code)]
fn get_key_files(files: &[String], max: usize) -> Vec<String> {
    if files.is_empty() {
        return Vec::new();
    }

    // Prefer files with extensions (likely code files)
    let mut with_ext: Vec<&String> = files
        .iter()
        .filter(|f| {
            let filename = f.rsplit('/').next().unwrap_or(f.as_str());
            filename.contains('.') && !filename.starts_with('.')
        })
        .collect();

    // Sort by path length (shorter paths often more significant)
    with_ext.sort_by_key(|f| f.len());

    // If not enough files with extensions, add others
    let mut result: Vec<String> = with_ext
        .into_iter()
        .take(max)
        .map(|s| {
            // Extract just the filename for brevity
            s.rsplit('/').next().unwrap_or(s).to_string()
        })
        .collect();

    if result.len() < max {
        for f in files {
            if result.len() >= max {
                break;
            }
            let filename = f.rsplit('/').next().unwrap_or(f).to_string();
            if !result.contains(&filename) {
                result.push(filename);
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

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
    fn test_generate_summary_from_prompt() {
        let events = vec![
            make_event(Role::User, "Fix the authentication bug in the login flow"),
            make_event(Role::Assistant, "I'll fix it"),
        ];
        let manifest = SessionManifest::new("test/1".into(), "test".into());

        let summary = generate_summary(&events, &manifest);
        assert!(summary.contains("Fix the authentication bug"));
    }

    #[test]
    fn test_generate_summary_with_files() {
        let events = vec![
            make_event(Role::User, "Update the API"),
            make_event(Role::Assistant, "Done"),
        ];
        let mut manifest = SessionManifest::new("test/1".into(), "test".into());
        manifest.files_touched = vec!["src/api/handlers.rs".into(), "src/api/mod.rs".into()];

        let summary = generate_summary(&events, &manifest);
        assert!(summary.contains("handlers.rs"));
    }

    #[test]
    fn test_generate_summary_uses_manifest_summary() {
        let events = vec![make_event(Role::User, "short")];
        let mut manifest = SessionManifest::new("test/1".into(), "test".into());
        manifest.summary = Some("A pre-existing detailed summary of the session".into());

        let summary = generate_summary(&events, &manifest);
        assert_eq!(summary, "A pre-existing detailed summary of the session");
    }

    #[test]
    fn test_generate_summary_truncates_long_prompt() {
        let long_prompt = "This is a very long prompt that should be truncated because it exceeds the maximum character limit for summaries";
        let events = vec![make_event(Role::User, long_prompt)];
        let manifest = SessionManifest::new("test/1".into(), "test".into());

        let summary = generate_summary(&events, &manifest);
        assert!(summary.len() < long_prompt.len() + 10);
        assert!(summary.ends_with("..."));
    }

    #[test]
    fn test_generate_summary_no_prompt() {
        let events = vec![make_event(Role::System, "session started")];
        let manifest = SessionManifest::new("test/1".into(), "test".into());

        let summary = generate_summary(&events, &manifest);
        assert_eq!(summary, "Session with no user prompt");
    }

    #[test]
    fn test_truncate_and_clean_whitespace() {
        let text = "  this   has   extra   whitespace  ";
        let cleaned = truncate_and_clean(text, 100);
        assert_eq!(cleaned, "this has extra whitespace");
    }

    #[test]
    fn test_get_key_files_prefers_extensions() {
        let files = vec![
            "README".into(),
            "src/main.rs".into(),
            "Cargo.toml".into(),
            "LICENSE".into(),
        ];
        let key = get_key_files(&files, 2);
        assert!(key.contains(&"main.rs".to_string()) || key.contains(&"Cargo.toml".to_string()));
    }

    #[test]
    fn test_get_key_files_extracts_filenames() {
        let files = vec!["src/components/Button.tsx".into()];
        let key = get_key_files(&files, 3);
        assert_eq!(key, vec!["Button.tsx"]);
    }
}
