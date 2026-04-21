//! Solution extraction.
//!
//! Identifies the resolution window (last error -> user confirmation)
//! and extracts Edit/Write/Bash tool calls as a solution summary.

#![allow(dead_code)]

use crate::event::{Event, Role};

/// Maximum solution summary length in characters.
const SOLUTION_MAX_CHARS: usize = 2000;

/// Extract the solution from a session's events.
///
/// Strategy:
/// 1. Find the last error/failure signal in the conversation
/// 2. Identify the resolution window from that point forward
/// 3. Extract tool calls (Edit, Write, Bash) as the solution summary
/// 4. If no error found, use all tool calls from the last third of the session
pub fn extract_solution(events: &[Event]) -> Option<String> {
    if events.is_empty() {
        return None;
    }

    // Find the last error signal
    let last_error_idx = find_last_error(events);

    // Determine the resolution window start
    let window_start = if let Some(idx) = last_error_idx {
        idx
    } else {
        // No error found: use the last third of the session
        events.len() / 3
    };

    // Extract tool calls from the resolution window
    let solution_parts: Vec<String> = events[window_start..]
        .iter()
        .filter(|e| e.role == Role::ToolCall)
        .filter_map(|e| {
            let tool = e.tool.as_deref()?;
            match tool {
                "Edit" | "Write" => {
                    // For Edit/Write, extract a short description
                    let desc = summarize_file_op(&e.content);
                    Some(format!("[{}] {}", tool, desc))
                }
                "Bash" => {
                    // For Bash, extract the command
                    let cmd = e.content.lines().next().unwrap_or("");
                    let truncated = if cmd.len() > 120 { &cmd[..120] } else { cmd };
                    Some(format!("[Bash] {}", truncated.trim()))
                }
                _ => None,
            }
        })
        .collect();

    if solution_parts.is_empty() {
        return None;
    }

    let solution = solution_parts.join("\n");

    if solution.len() > SOLUTION_MAX_CHARS {
        let mut end = SOLUTION_MAX_CHARS;
        while end > 0 && !solution.is_char_boundary(end) {
            end -= 1;
        }
        Some(format!("{}...\n(truncated)", &solution[..end]))
    } else {
        Some(solution)
    }
}

/// Find the index of the last error signal in events.
fn find_last_error(events: &[Event]) -> Option<usize> {
    let error_patterns = [
        "error",
        "failed",
        "failure",
        "panic",
        "exception",
        "traceback",
        "fatal",
        "exit code",
        "segfault",
        "permission denied",
    ];

    for (i, event) in events.iter().enumerate().rev() {
        let lower = event.content.to_lowercase();
        let is_error = match event.role {
            Role::ToolResult => error_patterns.iter().any(|p| lower.contains(p)),
            Role::User => {
                // User reporting an error
                lower.contains("error")
                    || lower.contains("doesn't work")
                    || lower.contains("still broken")
                    || lower.contains("failing")
            }
            _ => false,
        };

        if is_error {
            return Some(i);
        }
    }

    None
}

/// Summarize a file operation (Edit/Write) content into a short description.
fn summarize_file_op(content: &str) -> String {
    // Try to extract file path from the content
    let lines: Vec<&str> = content.lines().collect();

    // Look for file path patterns
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with("File: ") || trimmed.starts_with("Path: ") {
            return trimmed.to_string();
        }
    }

    // Use first non-empty line as description
    if let Some(first) = lines.iter().find(|l| !l.trim().is_empty()) {
        let desc = first.trim();
        if desc.len() > 100 {
            format!("{}...", &desc[..100])
        } else {
            desc.to_string()
        }
    } else {
        "(file operation)".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use chrono::Utc;

    fn make_event(role: Role, content: &str, tool: Option<&str>) -> Event {
        let mut e = Event::new(
            Utc::now(),
            "test/1".into(),
            "test".into(),
            role,
            content.into(),
        );
        e.tool = tool.map(|s| s.to_string());
        e
    }

    #[test]
    fn test_solution_from_error_recovery() {
        let events = vec![
            make_event(Role::User, "fix the auth bug", None),
            make_event(Role::Assistant, "let me look at it", None),
            make_event(
                Role::ToolResult,
                "error: compile failed\nmissing import",
                None,
            ),
            make_event(Role::Assistant, "I need to add the import", None),
            make_event(
                Role::ToolCall,
                "Edit src/auth.rs\n+use std::collections::HashMap;",
                Some("Edit"),
            ),
            make_event(Role::ToolCall, "Bash cargo test", Some("Bash")),
            make_event(
                Role::ToolResult,
                "running 5 tests...\ntest result: ok",
                None,
            ),
            make_event(Role::User, "thanks, works now", None),
        ];
        let solution = extract_solution(&events).unwrap();
        assert!(solution.contains("[Edit]"));
        assert!(solution.contains("[Bash]"));
    }

    #[test]
    fn test_solution_no_error_uses_last_third() {
        let events = vec![
            make_event(Role::User, "add a feature", None),
            make_event(Role::Assistant, "sure", None),
            make_event(Role::ToolCall, "Write src/feature.rs", Some("Write")),
            make_event(Role::ToolCall, "Bash cargo test", Some("Bash")),
            make_event(Role::User, "looks good", None),
        ];
        let solution = extract_solution(&events).unwrap();
        assert!(solution.contains("[Write]"));
    }

    #[test]
    fn test_no_tool_calls_no_solution() {
        let events = vec![
            make_event(Role::User, "explain something", None),
            make_event(Role::Assistant, "explanation", None),
        ];
        assert!(extract_solution(&events).is_none());
    }

    #[test]
    fn test_empty_events() {
        assert!(extract_solution(&[]).is_none());
    }

    #[test]
    fn test_non_solution_tools_ignored() {
        let events = vec![
            make_event(Role::User, "find files", None),
            make_event(Role::ToolCall, "Glob **/*.rs", Some("Glob")),
            make_event(Role::ToolResult, "src/main.rs\nsrc/lib.rs", None),
            make_event(Role::Assistant, "found 2 files", None),
        ];
        // Glob is not a solution tool, so no solution should be extracted
        assert!(extract_solution(&events).is_none());
    }
}
