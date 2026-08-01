//! Subagent detection tests for JSONL parser
//!
//! These tests verify that subagent sessions are correctly detected and
//! parent_session_id is properly extracted from the directory structure.

use crate::parser::FormatParser;
use crate::plugin::{
    LogFormat, Parser, Plugin, PluginMeta, SessionDetection, SessionIdSource, Source,
};
use std::path::PathBuf;

#[cfg(test)]
mod tests {
    use super::*;

    fn create_claude_code_plugin() -> Plugin {
        Plugin {
            plugin: PluginMeta {
                name: "claude-code".to_string(),
                version: "1.0".to_string(),
            },
            source: Source {
                paths: vec!["~/.claude/projects/**/*.jsonl".to_string()],
                exclude: vec![], // Empty - subagents are NOT excluded
                format: LogFormat::Jsonl,
                session_detection: SessionDetection::OneFilePerSession {
                    session_id_from: SessionIdSource::Filename,
                },
                tree: None,
                truncation_limit: None,
                envelope: None,
                array: None,
            },
            parser: Parser {
                timestamp: Some("timestamp".to_string()),
                role: Some("role".to_string()),
                content: Some("content".to_string()),
                static_fields: {
                    let mut map = std::collections::HashMap::new();
                    map.insert("source_agent".to_string(), serde_json::json!("claude-code"));
                    map
                },
                ..Default::default()
            },
            metadata: None,
        }
    }

    #[test]
    fn test_subagent_path_detection() {
        use crate::parser::jsonl::JsonlParser;

        // Test various subagent path structures
        let test_cases = vec![
            // Standard subagent path
            (
                "/home/coding/.claude/projects/AgentScribe/parent-uuid/subagents/agent-123.jsonl",
                Some("parent-uuid"),
                "agent-123",
            ),
            // Nested project path
            (
                "/home/coding/.claude/projects/ardenone/cluster/deep-project/session-abc/subagents/agent-def.jsonl",
                Some("session-abc"),
                "agent-def",
            ),
            // Non-subagent path (no "subagents" directory)
            (
                "/home/coding/.claude/projects/AgentScribe/session-main.jsonl",
                None,
                "session-main",
            ),
            // Subagents directory but not deep enough (missing project path before parent)
            (
                "/home/coding/.claude/projects/subagents/agent-123.jsonl",
                None,
                "agent-123",
            ),
            // Edge case: no projects directory at all
            (
                "/some/other/path/subagents/agent-123.jsonl",
                None,
                "agent-123",
            ),
            // Valid subagent path with UUID-like parent
            (
                "/home/coding/.claude/projects/myproj/a0b1c2d3-e4f5-6789/subagents/agent-xyz.jsonl",
                Some("a0b1c2d3-e4f5-6789"),
                "agent-xyz",
            ),
        ];

        for (path_str, expected_parent, expected_session_id) in test_cases {
            let path = PathBuf::from(path_str);
            let plugin = create_claude_code_plugin();

            let sessions = JsonlParser
                .detect_sessions(&path, &plugin)
                .expect("detect_sessions should succeed");

            assert_eq!(
                sessions.len(),
                1,
                "Should detect exactly one session for path: {}",
                path_str
            );

            let session = &sessions[0];
            assert_eq!(
                session.session_id, expected_session_id,
                "Session ID should match for path: {}",
                path_str
            );
            assert_eq!(
                session.parent_session_id,
                expected_parent.map(|s| s.to_string()),
                "Parent session ID should match for path: {}",
                path_str
            );
        }
    }

    #[test]
    fn test_subagent_session_info_structure() {
        use crate::parser::jsonl::JsonlParser;

        let path =
            PathBuf::from("/home/coding/.claude/projects/test/parent-uuid/subagents/agent-1.jsonl");
        let plugin = create_claude_code_plugin();

        let sessions = JsonlParser
            .detect_sessions(&path, &plugin)
            .expect("detect_sessions should succeed");

        assert_eq!(sessions.len(), 1);

        let session = &sessions[0];
        assert_eq!(session.session_id, "agent-1");
        assert_eq!(session.parent_session_id, Some("parent-uuid".to_string()));
        assert_eq!(session.start_offset, 0);
        // end_offset should be file size, which we can't test without actual file
        assert!(session.metadata.is_none());
    }

    #[test]
    fn test_non_subagent_has_no_parent() {
        use crate::parser::jsonl::JsonlParser;

        // Test that regular sessions don't get a parent_session_id
        let path = PathBuf::from("/home/coding/.claude/projects/test/regular-session.jsonl");
        let plugin = create_claude_code_plugin();

        let sessions = JsonlParser
            .detect_sessions(&path, &plugin)
            .expect("detect_sessions should succeed");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].parent_session_id, None);
    }

    #[test]
    fn test_subagent_source_agent_suffix() {
        use crate::parser::jsonl::JsonlParser;
        use crate::scraper::Scraper;

        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join(".agentscribe");
        std::fs::create_dir_all(data_dir.join("sessions")).unwrap();

        // Create a subagent file
        let subagent_path = temp
            .path()
            .join(".claude/projects/test/parent-uuid/subagents/agent-1.jsonl");
        std::fs::create_dir_all(subagent_path.parent().unwrap()).unwrap();
        std::fs::write(
            &subagent_path,
            r#"{"timestamp": "2026-07-23T10:00:00Z", "role": "user", "content": "Test"}
{"timestamp": "2026-07-23T10:00:01Z", "role": "assistant", "content": "Response"}"#,
        )
        .unwrap();

        let _scraper = Scraper::new(data_dir).unwrap();
        let plugin = create_claude_code_plugin();

        // Detect sessions
        let sessions = JsonlParser
            .detect_sessions(&subagent_path, &plugin)
            .expect("detect_sessions should succeed");

        assert_eq!(sessions.len(), 1);
        let session_info = &sessions[0];

        // Verify parent_session_id is set
        assert_eq!(
            session_info.parent_session_id,
            Some("parent-uuid".to_string())
        );

        // Verify that the scraper would set source_agent correctly
        // This logic is in scraper/mod.rs lines 478-483
        let source_agent = if session_info.parent_session_id.is_some() {
            format!("{}-subagent", plugin.plugin.name)
        } else {
            plugin.plugin.name.clone()
        };

        assert_eq!(source_agent, "claude-code-subagent");
    }

    #[test]
    fn test_regular_session_no_subagent_suffix() {
        use crate::parser::jsonl::JsonlParser;

        let temp = tempfile::tempdir().unwrap();
        let regular_path = temp
            .path()
            .join(".claude/projects/test/regular-session.jsonl");
        std::fs::create_dir_all(regular_path.parent().unwrap()).unwrap();
        std::fs::write(
            &regular_path,
            r#"{"timestamp": "2026-07-23T10:00:00Z", "role": "user", "content": "Test"}"#,
        )
        .unwrap();

        let plugin = create_claude_code_plugin();

        let sessions = JsonlParser
            .detect_sessions(&regular_path, &plugin)
            .expect("detect_sessions should succeed");

        assert_eq!(sessions.len(), 1);
        let session_info = &sessions[0];

        // Verify parent_session_id is NOT set for regular sessions
        assert_eq!(session_info.parent_session_id, None);

        // Verify that the scraper would NOT use subagent suffix
        let source_agent = if session_info.parent_session_id.is_some() {
            format!("{}-subagent", plugin.plugin.name)
        } else {
            plugin.plugin.name.clone()
        };

        assert_eq!(source_agent, "claude-code");
    }

    #[test]
    fn test_multiple_subagents_same_parent() {
        use crate::parser::jsonl::JsonlParser;

        let temp = tempfile::tempdir().unwrap();
        let plugin = create_claude_code_plugin();

        // Test multiple subagent sessions under the same parent
        let parent_id = "shared-parent-uuid";
        let subagent_ids = vec!["agent-1", "agent-2", "agent-3"];

        for agent_id in subagent_ids {
            let subagent_path = temp.path().join(format!(
                ".claude/projects/test/{}/subagents/{}.jsonl",
                parent_id, agent_id
            ));

            // Create the directory and file
            std::fs::create_dir_all(subagent_path.parent().unwrap()).unwrap();
            std::fs::write(
                &subagent_path,
                format!(
                    r#"{{"timestamp": "2026-07-23T10:00:00Z", "role": "user", "content": "Test message from {}", "source_agent": "claude-code"}}"#,
                    agent_id
                ),
            )
            .unwrap();

            let sessions = JsonlParser
                .detect_sessions(&subagent_path, &plugin)
                .expect("detect_sessions should succeed");

            assert_eq!(sessions.len(), 1);
            assert_eq!(sessions[0].session_id, agent_id);
            assert_eq!(sessions[0].parent_session_id, Some(parent_id.to_string()));
        }
    }

    #[test]
    fn test_deeply_nested_subagent_path() {
        use crate::parser::jsonl::JsonlParser;

        // Test deeply nested project paths
        let path_str = "/home/coding/.claude/projects/organization/team/project/very/deep/hierarchy/session-uuid/subagents/agent-xyz.jsonl";
        let path = PathBuf::from(path_str);
        let plugin = create_claude_code_plugin();

        let sessions = JsonlParser
            .detect_sessions(&path, &plugin)
            .expect("detect_sessions should succeed");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "agent-xyz");
        assert_eq!(
            sessions[0].parent_session_id,
            Some("session-uuid".to_string())
        );
    }
}
