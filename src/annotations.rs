//! Session annotation sidecar storage
//!
//! Annotations are stored as JSON sidecar files alongside normalized session JSONL files.
//! Each session can have multiple annotations (tags with optional notes).

use crate::error::{AgentScribeError, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// A single annotation on a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annotation {
    /// Tag/label for the annotation
    pub tag: String,
    /// Optional note describing the annotation
    pub note: Option<String>,
    /// ISO 8601 timestamp when the annotation was created
    pub created_at: String,
    /// Creator of the annotation: "human", "reflection-tool", or "agentscribe"
    pub created_by: String,
}

/// Sidecar file structure for session annotations
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AnnotationSidecar {
    /// Session ID in format "<agent>/<id>"
    session_id: String,
    /// List of annotations on this session
    annotations: Vec<Annotation>,
}

impl AnnotationSidecar {
    /// Create a new empty sidecar for a session
    fn new(session_id: String) -> Self {
        AnnotationSidecar {
            session_id,
            annotations: Vec::new(),
        }
    }
}

/// Create a new annotation with the current timestamp
///
/// # Arguments
/// * `tag` - Tag/label for the annotation
/// * `note` - Optional note describing the annotation
/// * `created_by` - Creator of the annotation (optional, defaults to "human")
///
/// # Returns
/// A new Annotation struct with created_at set to the current UTC time in ISO 8601 format
pub fn new_annotation(tag: String, note: Option<String>, created_by: Option<String>) -> Annotation {
    Annotation {
        tag,
        note,
        created_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        created_by: created_by.unwrap_or_else(|| "human".to_string()),
    }
}

/// Get the annotation sidecar file path for a session
///
/// # Arguments
/// * `sessions_dir` - Path to the sessions directory (e.g., ~/.agentscribe/sessions)
/// * `session_id` - Session ID in format "\<agent\>/\<id\>"
///
/// # Returns
/// Path to the annotation sidecar file
fn annotation_file_path(sessions_dir: &Path, session_id: &str) -> Result<PathBuf> {
    let parts: Vec<&str> = session_id.split('/').collect();
    if parts.len() != 2 {
        return Err(AgentScribeError::State(format!(
            "Invalid session_id format: '{}'. Expected '<agent>/<id>'",
            session_id
        )));
    }

    let agent = parts[0];
    let id = parts[1];

    let agent_dir = sessions_dir.join(agent);
    Ok(agent_dir.join(format!("{}.annotations.json", id)))
}

use std::path::PathBuf;

/// Load annotations from a session's sidecar file
///
/// # Arguments
/// * `sessions_dir` - Path to the sessions directory (e.g., ~/.agentscribe/sessions)
/// * `session_id` - Session ID in format "\<agent\>/\<id\>"
///
/// # Returns
/// Vector of annotations for the session, or an empty vector if the sidecar doesn't exist
pub fn load_annotations(sessions_dir: &Path, session_id: &str) -> Result<Vec<Annotation>> {
    let file_path = annotation_file_path(sessions_dir, session_id)?;

    // If the sidecar file doesn't exist, return an empty list
    if !file_path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&file_path).map_err(|e| {
        AgentScribeError::Io(std::io::Error::other(format!(
            "Failed to read annotation file {}: {}",
            file_path.display(),
            e
        )))
    })?;

    let sidecar: AnnotationSidecar = serde_json::from_str(&content).map_err(|e| {
        AgentScribeError::State(format!(
            "Failed to parse annotation file {}: {}",
            file_path.display(),
            e
        ))
    })?;

    Ok(sidecar.annotations)
}

/// Add an annotation to a session's sidecar file
///
/// This function loads existing annotations (if any), appends the new annotation,
/// and writes the updated list back to the sidecar file. The operation is append-only
/// - it never removes or modifies existing annotations.
///
/// # Arguments
/// * `sessions_dir` - Path to the sessions directory (e.g., ~/.agentscribe/sessions)
/// * `session_id` - Session ID in format "\<agent\>/\<id\>"
/// * `annotation` - The annotation to add
///
/// # Returns
/// Ok(()) on success, or an error if the operation fails
pub fn add_annotation(sessions_dir: &Path, session_id: &str, annotation: Annotation) -> Result<()> {
    let file_path = annotation_file_path(sessions_dir, session_id)?;

    // Load existing annotations or create a new sidecar
    let mut sidecar = if file_path.exists() {
        let content = fs::read_to_string(&file_path).map_err(|e| {
            AgentScribeError::Io(std::io::Error::other(format!(
                "Failed to read annotation file {}: {}",
                file_path.display(),
                e
            )))
        })?;

        serde_json::from_str::<AnnotationSidecar>(&content).map_err(|e| {
            AgentScribeError::State(format!(
                "Failed to parse annotation file {}: {}",
                file_path.display(),
                e
            ))
        })?
    } else {
        // Ensure the agent directory exists
        if let Some(agent_dir) = file_path.parent() {
            fs::create_dir_all(agent_dir).map_err(|e| {
                AgentScribeError::Io(std::io::Error::other(format!(
                    "Failed to create directory {}: {}",
                    agent_dir.display(),
                    e
                )))
            })?;
        }
        AnnotationSidecar::new(session_id.to_string())
    };

    // Append the new annotation
    sidecar.annotations.push(annotation);

    // Write back to the sidecar file
    let content = serde_json::to_string_pretty(&sidecar).map_err(|e| {
        AgentScribeError::State(format!(
            "Failed to serialize annotations for session {}: {}",
            session_id, e
        ))
    })?;

    fs::write(&file_path, content).map_err(|e| {
        AgentScribeError::Io(std::io::Error::other(format!(
            "Failed to write annotation file {}: {}",
            file_path.display(),
            e
        )))
    })?;

    Ok(())
}

/// Merge annotation tags with enrichment tags for a session
///
/// # Arguments
/// * `sessions_dir` - Path to the sessions directory (e.g., ~/.agentscribe/sessions)
/// * `session_id` - Session ID in format "\<agent\>/\<id\>"
/// * `enrichment_tags` - Existing tags from enrichment (indexed in Tantivy)
///
/// # Returns
/// A deduplicated vector of tags containing both enrichment and annotation tags
pub fn merge_annotation_tags(
    sessions_dir: &Path,
    session_id: &str,
    enrichment_tags: Vec<String>,
) -> Vec<String> {
    // Load annotations from sidecar
    let annotations = match load_annotations(sessions_dir, session_id) {
        Ok(ann) => ann,
        Err(_) => return enrichment_tags, // Return enrichment tags on error
    };

    // Extract annotation tags
    let mut annotation_tags: std::collections::HashSet<String> =
        annotations.into_iter().map(|a| a.tag).collect();

    // Add enrichment tags
    for tag in enrichment_tags {
        annotation_tags.insert(tag);
    }

    // Convert to sorted vector for consistent output
    let mut merged: Vec<String> = annotation_tags.into_iter().collect();
    merged.sort();
    merged
}

/// Remove an annotation by tag from a session's sidecar file
///
/// # Arguments
/// * `sessions_dir` - Path to the sessions directory (e.g., ~/.agentscribe/sessions)
/// * `session_id` - Session ID in format "\<agent\>/\<id\>"
/// * `tag` - The tag of the annotation to remove
///
/// # Returns
/// * `Ok(true)` - if the annotation was found and removed
/// * `Ok(false)` - if the annotation was not found
/// * `Err(...)` - if an error occurred
pub fn remove_annotation(sessions_dir: &Path, session_id: &str, tag: &str) -> Result<bool> {
    let file_path = annotation_file_path(sessions_dir, session_id)?;

    // If the sidecar file doesn't exist, the annotation doesn't exist
    if !file_path.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(&file_path).map_err(|e| {
        AgentScribeError::Io(std::io::Error::other(format!(
            "Failed to read annotation file {}: {}",
            file_path.display(),
            e
        )))
    })?;

    let mut sidecar: AnnotationSidecar = serde_json::from_str(&content).map_err(|e| {
        AgentScribeError::State(format!(
            "Failed to parse annotation file {}: {}",
            file_path.display(),
            e
        ))
    })?;

    // Find and remove the annotation with the matching tag
    let original_len = sidecar.annotations.len();
    sidecar.annotations.retain(|a| a.tag != tag);

    if sidecar.annotations.len() == original_len {
        // No annotation was removed
        return Ok(false);
    }

    // If this was the last annotation, delete the sidecar file entirely
    if sidecar.annotations.is_empty() {
        fs::remove_file(&file_path).map_err(|e| {
            AgentScribeError::Io(std::io::Error::other(format!(
                "Failed to remove annotation file {}: {}",
                file_path.display(),
                e
            )))
        })?;
        return Ok(true);
    }

    // Write the updated annotations back to the file
    let updated_content = serde_json::to_string_pretty(&sidecar).map_err(|e| {
        AgentScribeError::State(format!(
            "Failed to serialize annotations for session {}: {}",
            session_id, e
        ))
    })?;

    fs::write(&file_path, updated_content).map_err(|e| {
        AgentScribeError::Io(std::io::Error::other(format!(
            "Failed to write annotation file {}: {}",
            file_path.display(),
            e
        )))
    })?;

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_dir() -> TempDir {
        TempDir::new().unwrap()
    }

    #[test]
    fn test_new_annotation() {
        let annotation = new_annotation(
            "bug-fix".to_string(),
            Some("Fixed critical parsing error".to_string()),
            Some("human".to_string()),
        );

        assert_eq!(annotation.tag, "bug-fix");
        assert_eq!(
            annotation.note,
            Some("Fixed critical parsing error".to_string())
        );
        assert_eq!(annotation.created_by, "human");
        assert!(!annotation.created_at.is_empty());
    }

    #[test]
    fn test_annotation_file_path() {
        let temp_dir = setup_test_dir();
        let sessions_dir = temp_dir.path().join("sessions");

        let path = annotation_file_path(&sessions_dir, "claude-code/abc123").unwrap();
        assert!(path
            .to_str()
            .unwrap()
            .ends_with("sessions/claude-code/abc123.annotations.json"));
    }

    #[test]
    fn test_add_and_load_annotations() {
        let temp_dir = setup_test_dir();
        let sessions_dir = temp_dir.path().join("sessions");

        let annotation1 = new_annotation(
            "bug-fix".to_string(),
            Some("Fixed parsing error".to_string()),
            Some("human".to_string()),
        );

        let annotation2 = new_annotation(
            "feature".to_string(),
            None,
            Some("reflection-tool".to_string()),
        );

        // Add first annotation
        add_annotation(&sessions_dir, "test-agent/session-1", annotation1.clone()).unwrap();

        // Add second annotation (should append, not overwrite)
        add_annotation(&sessions_dir, "test-agent/session-1", annotation2.clone()).unwrap();

        // Load and verify both annotations are present
        let loaded = load_annotations(&sessions_dir, "test-agent/session-1").unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].tag, "bug-fix");
        assert_eq!(loaded[1].tag, "feature");
    }

    #[test]
    fn test_load_annotations_empty() {
        let temp_dir = setup_test_dir();
        let sessions_dir = temp_dir.path().join("sessions");

        // Loading annotations for a non-existent session should return an empty vector
        let loaded = load_annotations(&sessions_dir, "test-agent/nonexistent").unwrap();
        assert_eq!(loaded.len(), 0);
    }

    #[test]
    fn test_remove_annotation() {
        let temp_dir = setup_test_dir();
        let sessions_dir = temp_dir.path().join("sessions");

        let annotation1 = new_annotation(
            "bug-fix".to_string(),
            Some("Fixed parsing error".to_string()),
            Some("human".to_string()),
        );

        let annotation2 = new_annotation(
            "feature".to_string(),
            None,
            Some("reflection-tool".to_string()),
        );

        add_annotation(&sessions_dir, "test-agent/session-1", annotation1).unwrap();
        add_annotation(&sessions_dir, "test-agent/session-1", annotation2).unwrap();

        // Remove the first annotation
        let removed = remove_annotation(&sessions_dir, "test-agent/session-1", "bug-fix").unwrap();
        assert!(removed);

        // Verify only one annotation remains
        let loaded = load_annotations(&sessions_dir, "test-agent/session-1").unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].tag, "feature");
    }

    #[test]
    fn test_remove_annotation_deletes_file_when_last() {
        let temp_dir = setup_test_dir();
        let sessions_dir = temp_dir.path().join("sessions");

        let annotation = new_annotation(
            "bug-fix".to_string(),
            Some("Fixed parsing error".to_string()),
            Some("human".to_string()),
        );

        add_annotation(&sessions_dir, "test-agent/session-1", annotation).unwrap();

        // Remove the only annotation
        let removed = remove_annotation(&sessions_dir, "test-agent/session-1", "bug-fix").unwrap();
        assert!(removed);

        // Verify the sidecar file was deleted
        let file_path = annotation_file_path(&sessions_dir, "test-agent/session-1").unwrap();
        assert!(!file_path.exists());
    }

    #[test]
    fn test_remove_nonexistent_annotation() {
        let temp_dir = setup_test_dir();
        let sessions_dir = temp_dir.path().join("sessions");

        let annotation = new_annotation(
            "bug-fix".to_string(),
            Some("Fixed parsing error".to_string()),
            Some("human".to_string()),
        );

        add_annotation(&sessions_dir, "test-agent/session-1", annotation).unwrap();

        // Try to remove an annotation that doesn't exist
        let removed = remove_annotation(&sessions_dir, "test-agent/session-1", "feature").unwrap();
        assert!(!removed);

        // Verify the original annotation is still there
        let loaded = load_annotations(&sessions_dir, "test-agent/session-1").unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].tag, "bug-fix");
    }

    #[test]
    fn test_invalid_session_id_format() {
        let temp_dir = setup_test_dir();
        let sessions_dir = temp_dir.path().join("sessions");

        // Missing slash separator
        let result = annotation_file_path(&sessions_dir, "invalid-session-id");
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.to_string().contains("Invalid session_id format"));
    }
}
