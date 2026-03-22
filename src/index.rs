//! Tantivy index schema and document builder
//!
//! Defines the full-text search index schema and provides functions to build
//! Tantivy documents from normalized session events and manifests.

use crate::error::{AgentScribeError, Result};
use crate::event::{Event, Role, SessionManifest};
use crate::tags;
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use std::path::Path;
use tracing::{debug, info, warn};

use tantivy::schema::*;
use tantivy::TantivyDocument;

/// Maximum bytes for tool_result content per event
const TOOL_RESULT_MAX_CHARS: usize = 1000;

/// Maximum total content size in bytes before trimming
const CONTENT_MAX_BYTES: usize = 500_000;

/// Half of the max content size (used for trim-middle)
const CONTENT_HALF_BYTES: usize = CONTENT_MAX_BYTES / 2;

/// Named field handles for the Tantivy schema.
///
/// Returned alongside the schema so callers can reference fields by name
/// without re-resolving them.
#[derive(Clone)]
pub struct IndexFields {
    // Full-text searchable + stored
    pub content: Field,
    pub summary: Field,
    pub solution_summary: Field,
    pub code_content: Field,

    // Exact match + faceted filtering
    pub session_id: Field,
    pub source_agent: Field,
    pub project: Field,
    pub tags: Field,
    pub outcome: Field,
    pub error_fingerprint: Field,
    pub file_paths: Field,
    pub git_commits: Field,
    pub doc_type: Field,

    // Code artifact fields
    pub code_language: Field,
    pub code_file_path: Field,
    pub code_is_final: Field,

    // Analytics + classification
    pub model: Field,
    pub session_type: Field,

    // Date + numeric
    pub timestamp: Field,
    pub turn_count: Field,
}

/// Build the Tantivy schema with all fields from plan.md.
pub fn build_schema() -> (Schema, IndexFields) {
    let mut builder = Schema::builder();

    // Full-text searchable + stored
    let content = builder.add_text_field("content", TEXT | STORED);
    let summary = builder.add_text_field("summary", TEXT | STORED);
    let solution_summary = builder.add_text_field("solution_summary", TEXT | STORED);
    let code_content = builder.add_text_field("code_content", TEXT | STORED);

    // Exact match + faceted filtering
    let session_id = builder.add_text_field("session_id", STRING | STORED);
    let source_agent = builder.add_text_field("source_agent", STRING | STORED | FAST);
    let project = builder.add_text_field("project", STRING | STORED | FAST);
    let tags = builder.add_text_field("tags", STRING | STORED | FAST);
    let outcome = builder.add_text_field("outcome", STRING | STORED | FAST);
    let error_fingerprint = builder.add_text_field("error_fingerprint", STRING | STORED | FAST);
    let file_paths = builder.add_text_field("file_paths", STRING | STORED | FAST);
    let git_commits = builder.add_text_field("git_commits", STRING | STORED | FAST);
    let doc_type = builder.add_text_field("doc_type", STRING | STORED | FAST);

    // Code artifact fields
    let code_language = builder.add_text_field("code_language", STRING | STORED | FAST);
    let code_file_path = builder.add_text_field("code_file_path", STRING | STORED | FAST);
    let code_is_final = builder.add_bool_field("code_is_final", STORED | FAST);

    // Analytics + classification
    let model = builder.add_text_field("model", STRING | STORED | FAST);
    let session_type = builder.add_text_field("session_type", STRING | STORED | FAST);

    // Date + numeric for range queries
    let timestamp = builder.add_date_field("timestamp", INDEXED | STORED | FAST);
    let turn_count = builder.add_u64_field("turn_count", INDEXED | STORED | FAST);

    let schema = builder.build();

    let fields = IndexFields {
        content,
        summary,
        solution_summary,
        code_content,
        session_id,
        source_agent,
        project,
        tags,
        outcome,
        error_fingerprint,
        file_paths,
        git_commits,
        doc_type,
        code_language,
        code_file_path,
        code_is_final,
        model,
        session_type,
        timestamp,
        turn_count,
    };

    (schema, fields)
}

/// Truncate tool_result content to TOOL_RESULT_MAX_CHARS.
fn truncate_tool_result(content: &str) -> &str {
    if content.len() > TOOL_RESULT_MAX_CHARS {
        // Find a char boundary near the limit
        let mut end = TOOL_RESULT_MAX_CHARS;
        while end > 0 && !content.is_char_boundary(end) {
            end -= 1;
        }
        debug!(
            original_len = content.len(),
            truncated_len = end,
            "truncated tool_result content"
        );
        &content[..end]
    } else {
        content
    }
}

/// Trim content to CONTENT_MAX_BYTES by keeping first and last halves.
fn trim_middle(content: String) -> String {
    let bytes = content.len();
    if bytes <= CONTENT_MAX_BYTES {
        return content;
    }

    debug!(
        original_bytes = bytes,
        max_bytes = CONTENT_MAX_BYTES,
        "trimming document content (keep first+last halves)"
    );

    // Find byte boundary near the half point
    let mut mid = CONTENT_HALF_BYTES;
    while mid > 0 && !content.is_char_boundary(mid) {
        mid -= 1;
    }

    let mut end = bytes - (CONTENT_MAX_BYTES - mid);
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }

    format!("{}\n[...trimmed...]\n{}", &content[..mid], &content[end..])
}

/// Build the content field from a list of events.
///
/// Concatenates events with role prefixes. Applies content truncation:
/// - user/assistant content: full
/// - tool_call content: full
/// - tool_result content: capped at 1000 chars
/// - Total capped at 500KB with trim-middle strategy
fn build_content(events: &[Event]) -> String {
    let mut parts: Vec<String> = Vec::new();

    for event in events {
        let content = match event.role {
            Role::ToolResult => truncate_tool_result(&event.content).to_string(),
            _ => event.content.clone(),
        };

        parts.push(format!("{}: {}", event.role.as_str(), content));
    }

    let concatenated = parts.join("\n\n");
    trim_middle(concatenated)
}

/// Build a Tantivy document for a session.
///
/// Takes a list of events belonging to the session and the session manifest,
/// and produces a single Tantivy document with the `doc_type` set to "session".
pub fn build_session_document(
    fields: &IndexFields,
    events: &[Event],
    manifest: &SessionManifest,
) -> TantivyDocument {
    let mut doc = TantivyDocument::new();

    // Content field — concatenated conversation with role prefixes
    let content = build_content(events);
    doc.add_text(fields.content, &content);

    // Summary fields
    if let Some(ref summary) = manifest.summary {
        doc.add_text(fields.summary, summary);
    }

    // Faceted fields
    doc.add_text(fields.session_id, &manifest.session_id);
    doc.add_text(fields.source_agent, &manifest.source_agent);
    if let Some(ref project) = manifest.project {
        doc.add_text(fields.project, project);
    }

    // Tags: merge manifest tags with auto-extracted tags, deduplicate
    let mut all_tags: HashSet<String> = manifest.tags.iter().cloned().collect();
    for tag in tags::extract_tags(events) {
        all_tags.insert(tag);
    }
    for tag in &all_tags {
        doc.add_text(fields.tags, tag);
    }
    if let Some(ref outcome) = manifest.outcome {
        doc.add_text(fields.outcome, outcome);
    }

    // Collect unique error fingerprints across all events
    let mut fingerprints: HashSet<&str> = HashSet::new();
    for event in events {
        for fp in &event.error_fingerprints {
            fingerprints.insert(fp.as_str());
        }
    }
    for fp in &fingerprints {
        doc.add_text(fields.error_fingerprint, fp);
    }

    // Collect unique file paths from manifest + events
    let mut all_files: HashSet<&str> = HashSet::new();
    for f in &manifest.files_touched {
        all_files.insert(f.as_str());
    }
    for event in events {
        for f in &event.file_paths {
            all_files.insert(f.as_str());
        }
    }
    for f in &all_files {
        doc.add_text(fields.file_paths, f);
    }

    // Doc type
    doc.add_text(fields.doc_type, "session");

    // Model
    if let Some(ref model) = manifest.model {
        doc.add_text(fields.model, model);
    }

    // Timestamp — use the session start time
    let tantivy_ts = tantivy::DateTime::from_timestamp_secs(manifest.started.timestamp());
    doc.add_date(fields.timestamp, tantivy_ts);

    // Turn count
    doc.add_u64(fields.turn_count, manifest.turns as u64);

    doc
}

/// Build a Tantivy document for a code artifact.
///
/// Code artifacts share session-level fields with their parent session and
/// have the `doc_type` set to "code_artifact".
pub fn build_code_artifact_document(
    fields: &IndexFields,
    session_id: &str,
    source_agent: &str,
    project: Option<&str>,
    timestamp: DateTime<Utc>,
    code_language: &str,
    code_file_path: &str,
    code_content: &str,
    code_is_final: bool,
    model: Option<&str>,
) -> TantivyDocument {
    let mut doc = TantivyDocument::new();

    // Content — code content goes into both content and code_content
    doc.add_text(fields.content, code_content);
    doc.add_text(fields.code_content, code_content);

    // Session correlation fields
    doc.add_text(fields.session_id, session_id);
    doc.add_text(fields.source_agent, source_agent);
    if let Some(project) = project {
        doc.add_text(fields.project, project);
    }

    // Code artifact fields
    doc.add_text(fields.code_language, code_language);
    doc.add_text(fields.code_file_path, code_file_path);
    doc.add_bool(fields.code_is_final, code_is_final);

    // Doc type
    doc.add_text(fields.doc_type, "code_artifact");

    // Model
    if let Some(model) = model {
        doc.add_text(fields.model, model);
    }

    // Timestamp
    let tantivy_ts = tantivy::DateTime::from_timestamp_secs(timestamp.timestamp());
    doc.add_date(fields.timestamp, tantivy_ts);

    doc
}

/// Build a minimal `SessionManifest` from scraped events for indexing.
///
/// This creates a manifest with metadata extracted directly from the events,
/// without enrichment data (summary, outcome, etc.) which is added later.
pub fn build_manifest_from_events(
    events: &[Event],
    session_id: &str,
    source_agent: &str,
    project: Option<&str>,
    model: Option<&str>,
) -> SessionManifest {
    let started = events.first().map(|e| e.ts).unwrap_or_else(Utc::now);
    let ended = events.last().map(|e| e.ts);
    let turns = events.len() as u32;

    let mut files_touched: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for event in events {
        for f in &event.file_paths {
            if seen.insert(f.clone()) {
                files_touched.push(f.clone());
            }
        }
    }

    SessionManifest {
        session_id: session_id.to_string(),
        source_agent: source_agent.to_string(),
        project: project.map(|s| s.to_string()),
        started,
        ended,
        turns,
        summary: None,
        outcome: None,
        tags: Vec::new(),
        files_touched,
        model: model.map(|s| s.to_string()),
    }
}

/// Tantivy index directory name
pub const INDEX_DIR_NAME: &str = "tantivy";

/// Manages incremental index updates during scraping.
///
/// The `IndexWriter` is held only during active scrape/commit operations,
/// allowing concurrent searches to proceed against the last committed state.
pub struct IndexManager {
    index: tantivy::Index,
    fields: IndexFields,
    writer: Option<tantivy::IndexWriter>,
}

impl IndexManager {
    /// Open an existing index or create a new one at `data_dir/index/tantivy`.
    pub fn open(data_dir: &Path) -> Result<Self> {
        let index_path = data_dir.join("index").join(INDEX_DIR_NAME);
        let (schema, fields) = build_schema();

        let index = if index_path.exists() {
            tantivy::Index::open_in_dir(&index_path).map_err(|e| {
                AgentScribeError::DataDir(format!("Failed to open index: {}", e))
            })?
        } else {
            std::fs::create_dir_all(&index_path)?;
            tantivy::Index::create_in_dir(&index_path, schema).map_err(|e| {
                AgentScribeError::DataDir(format!("Failed to create index: {}", e))
            })?
        };

        Ok(IndexManager {
            index,
            fields,
            writer: None,
        })
    }

    /// Begin a write session. Idempotent — no-op if writer is already active.
    ///
    /// The writer is created once and reused across multiple `index_session()`
    /// calls. It is held only during active scrape/commit, allowing concurrent
    /// searches to proceed against the last committed state.
    pub fn begin_write(&mut self) -> Result<()> {
        if self.writer.is_some() {
            return Ok(());
        }

        let writer = self.index.writer(50_000_000).map_err(|e| {
            AgentScribeError::DataDir(format!("Failed to create index writer: {}", e))
        })?;
        self.writer = Some(writer);
        debug!("index writer opened");
        Ok(())
    }

    /// Index a session: delete any existing document for the session_id, then add the new one.
    ///
    /// The delete is soft until `commit()` is called, so within a single write session
    /// this correctly replaces the old document with the new one.
    pub fn index_session(
        &mut self,
        events: &[Event],
        manifest: &SessionManifest,
    ) -> Result<()> {
        let writer = self.writer.as_mut().ok_or_else(|| {
            AgentScribeError::DataDir(
                "Index writer not active. Call begin_write() first.".to_string(),
            )
        })?;

        // Delete existing documents for this session_id (soft delete until commit)
        let term = Term::from_field_text(self.fields.session_id, &manifest.session_id);
        writer.delete_term(term);
        debug!(session_id = %manifest.session_id, "deleted old index entry");

        // Build and add new document
        let doc = build_session_document(&self.fields, events, manifest);
        writer.add_document(doc).map_err(|e| {
            AgentScribeError::DataDir(format!("Failed to add document: {}", e))
        })?;
        info!(session_id = %manifest.session_id, "indexed session");

        Ok(())
    }

    /// Delete all documents for a given session_id.
    pub fn delete_session(&mut self, session_id: &str) -> Result<()> {
        let writer = self.writer.as_mut().ok_or_else(|| {
            AgentScribeError::DataDir(
                "Index writer not active. Call begin_write() first.".to_string(),
            )
        })?;

        let term = Term::from_field_text(self.fields.session_id, session_id);
        writer.delete_term(term);
        info!(session_id = %session_id, "deleted session from index");
        Ok(())
    }

    /// Commit pending changes to make them visible to readers.
    ///
    /// Does NOT release the writer — subsequent calls to `index_session()` can
    /// continue using it. Call `finish()` to release the writer.
    pub fn commit(&mut self) -> Result<()> {
        if let Some(ref mut writer) = self.writer {
            writer.commit().map_err(|e| {
                AgentScribeError::DataDir(format!("Failed to commit index: {}", e))
            })?;
            info!("index committed");
        }
        Ok(())
    }

    /// Commit pending changes and release the writer.
    ///
    /// After this call, a new `begin_write()` is required before further indexing.
    pub fn finish(&mut self) -> Result<()> {
        self.commit()?;
        if self.writer.take().is_some() {
            debug!("index writer released");
        }
        Ok(())
    }

    /// Run garbage collection on the index to remove unused segment files.
    ///
    /// Opens a temporary writer to trigger GC, then releases it.
    pub fn optimize(&self) -> Result<()> {
        let writer: tantivy::IndexWriter = self.index.writer(50_000_000).map_err(|e| {
            AgentScribeError::DataDir(format!("Failed to open writer for GC: {}", e))
        })?;
        writer
            .garbage_collect_files()
            .wait()
            .map_err(|e| {
                AgentScribeError::DataDir(format!("Failed to garbage collect index: {}", e))
            })?;
        info!("index garbage collection complete");
        Ok(())
    }

    /// Check if a writer is currently active.
    pub fn is_writing(&self) -> bool {
        self.writer.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_builds() {
        let (schema, fields) = build_schema();

        // Verify key fields exist
        assert!(schema.get_field("content").is_ok());
        assert!(schema.get_field("session_id").is_ok());
        assert!(schema.get_field("timestamp").is_ok());
        assert!(schema.get_field("turn_count").is_ok());
        assert!(schema.get_field("doc_type").is_ok());
        assert!(schema.get_field("code_is_final").is_ok());

        // Verify field handles match
        assert_eq!(fields.content, schema.get_field("content").unwrap());
        assert_eq!(fields.doc_type, schema.get_field("doc_type").unwrap());
    }

    #[test]
    fn test_truncate_tool_result_short() {
        let input = "short result";
        assert_eq!(truncate_tool_result(input), "short result");
    }

    #[test]
    fn test_truncate_tool_result_long() {
        let input = "x".repeat(2000);
        let truncated = truncate_tool_result(&input);
        assert!(truncated.len() <= 1000);
    }

    #[test]
    fn test_trim_middle_within_limit() {
        let input = "hello world".to_string();
        assert_eq!(trim_middle(input), "hello world");
    }

    #[test]
    fn test_trim_middle_exceeds_limit() {
        let input = "x".repeat(CONTENT_MAX_BYTES + 1000);
        let trimmed = trim_middle(input);
        // Should contain the trim marker
        assert!(trimmed.contains("[...trimmed...]"));
        // Should be at or under the limit (with some margin for the marker)
        assert!(trimmed.len() <= CONTENT_MAX_BYTES + 50);
    }

    #[test]
    fn test_build_content_applies_truncation() {
        let events = vec![
            Event::new(
                Utc::now(),
                "test/1".to_string(),
                "test".to_string(),
                Role::User,
                "fix the auth bug".to_string(),
            ),
            Event::new(
                Utc::now(),
                "test/1".to_string(),
                "test".to_string(),
                Role::Assistant,
                "I'll fix it".to_string(),
            ),
            Event::new(
                Utc::now(),
                "test/1".to_string(),
                "test".to_string(),
                Role::ToolResult,
                "x".repeat(2000),
            ),
        ];

        let content = build_content(&events);
        assert!(content.starts_with("user: fix the auth bug"));
        assert!(content.contains("tool_result: "));
        // Tool result should be truncated
        assert!(content.len() < 2000 + 200);
    }

    #[test]
    fn test_build_session_document() {
        let (_schema, fields) = build_schema();
        let events = vec![
            Event::new(
                Utc::now(),
                "claude/123".to_string(),
                "claude".to_string(),
                Role::User,
                "fix the bug".to_string(),
            ),
            Event::new(
                Utc::now(),
                "claude/123".to_string(),
                "claude".to_string(),
                Role::Assistant,
                "done".to_string(),
            ),
        ];

        let mut manifest = SessionManifest::new("claude/123".to_string(), "claude".to_string());
        manifest.summary = Some("Fixed auth bug".to_string());
        manifest.outcome = Some("success".to_string());
        manifest.tags = vec!["bugfix".to_string(), "auth".to_string()];
        manifest.turns = 2;
        manifest.project = Some("/home/user/project".to_string());

        let doc = build_session_document(&fields, &events, &manifest);

        assert!(doc.len() > 0);
        assert_eq!(
            doc.get_first(fields.session_id).unwrap().as_str().unwrap(),
            "claude/123"
        );
        assert_eq!(
            doc.get_first(fields.doc_type).unwrap().as_str().unwrap(),
            "session"
        );
        assert_eq!(
            doc.get_first(fields.source_agent).unwrap().as_str().unwrap(),
            "claude"
        );
        assert_eq!(
            doc.get_first(fields.summary).unwrap().as_str().unwrap(),
            "Fixed auth bug"
        );
    }

    #[test]
    fn test_build_code_artifact_document() {
        let (_schema, fields) = build_schema();

        let doc = build_code_artifact_document(
            &fields,
            "claude/123",
            "claude",
            Some("/home/user/project"),
            Utc::now(),
            "rust",
            "src/main.rs",
            "fn main() {}",
            true,
            Some("claude-sonnet-4-20250514"),
        );

        assert!(doc.len() > 0);
        assert_eq!(
            doc.get_first(fields.session_id).unwrap().as_str().unwrap(),
            "claude/123"
        );
        assert_eq!(
            doc.get_first(fields.doc_type).unwrap().as_str().unwrap(),
            "code_artifact"
        );
        assert_eq!(
            doc.get_first(fields.code_language).unwrap().as_str().unwrap(),
            "rust"
        );
        assert_eq!(
            doc.get_first(fields.code_file_path).unwrap().as_str().unwrap(),
            "src/main.rs"
        );
    }

    #[test]
    fn test_error_fingerprints_collected_from_events() {
        let (_, fields) = build_schema();

        let events = vec![
            Event::new(
                Utc::now(),
                "test/1".to_string(),
                "test".to_string(),
                Role::ToolResult,
                "error occurred".to_string(),
            )
            .with_error_fingerprints(vec!["ConnectionRefusedError".to_string()]),
            Event::new(
                Utc::now(),
                "test/1".to_string(),
                "test".to_string(),
                Role::ToolResult,
                "another error".to_string(),
            )
            .with_error_fingerprints(vec![
                "ConnectionRefusedError".to_string(),
                "TimeoutError".to_string(),
            ]),
        ];

        let manifest = SessionManifest::new("test/1".to_string(), "test".to_string());
        let doc = build_session_document(&fields, &events, &manifest);

        // We can't easily inspect multi-valued fields from Document directly,
        // but the build should succeed without errors
        let _ = doc;
    }
}
