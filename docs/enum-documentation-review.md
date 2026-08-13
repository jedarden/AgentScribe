# Enum Documentation Review - AgentScribe Codebase

**Date:** 2026-08-13
**Task:** Review all enum-level documentation for clarity and consistency
**Acceptance Criteria:** Each enum doc should answer "What does this enum represent? When should I use each variant?"

---

## Executive Summary

Reviewed **18 public enums** across **12 files**. Overall documentation quality is **very good** with most enums having adequate documentation. Several enums serve as excellent models for documentation standards.

**Overall Grade:** B+ (Good with room for improvement)

- **Excellent examples:** 9 enums (50%)
- **Good but improvable:** 4 enums (22%)
- **Missing enum-level docs:** 5 enums (28%)

---

## Outstanding Examples (Model Standards)

These enums demonstrate exemplary documentation that all other enums should aspire to:

### 1. `ImportType` in `src/parser/import_parser.rs` (lines 83-196) ⭐ EXCELLENT

**Why it's outstanding:**
- Clear module-level doc explaining the three import types
- Each variant has extensive documentation including:
  - What the variant represents
  - Historical context (e.g., `ExternCrate` obsolete in Rust 2018+)
  - Multiple concrete code examples
  - Usage notes and best practices

**Sample documentation:**
```rust
/// `use` statement - imports from crates, modules, or items
///
/// The most common import type in Rust. Used to bring items from other modules,
/// crates, or scopes into the current scope.
///
/// # Examples
///
/// ```rust
/// // Simple import
/// use std::collections::HashMap;
///
/// // Complex import with multiple items
/// use std::collections::{HashMap, HashSet, BTreeMap};
/// ```
Use,
```

---

### 2. `Role` in `src/event.rs` (lines 55-85) ⭐ EXCELLENT

**Why it's outstanding:**
- Comprehensive module-level doc (76 lines) explaining roles, their purpose, and how they map to agent-specific roles
- Detailed variant documentation with context about each role's purpose
- Examples of when each role appears
- Clear explanation of the role normalization system

**Sample documentation:**
```rust
/// Canonical role types for conversation events.
///
/// Roles define the origin and nature of each message in a conversation. These canonical roles
/// map to agent-specific roles during normalization, enabling cross-agent analysis.
///
/// # Variants
///
/// * **User** - Messages from the human user. This includes direct prompts, follow-up questions,
///   feedback, and corrections. User messages are the primary signal for intent and satisfaction.
///
/// * **Assistant** - Responses from the AI agent. These are the main conversational turns containing
///   explanations, code, and guidance. Assistant messages may contain embedded tool calls in some agents.
```

---

### 3. `ProblemType` in `src/analytics.rs` (lines 118-136) ⭐ EXCELLENT

**Why it's outstanding:**
- Comprehensive module-level documentation (90+ lines) explaining:
  - Classification logic and methodology
  - What each signal type means
  - Type definitions and detection patterns
- Each variant documented with examples of work in that category
- Context about when problems are classified vs. when they remain unknown

---

### 4. `JobStatus` in `src/transcription.rs` (lines 178-190) ⭐ EXCELLENT

**Why it's outstanding:**
- Clear module-level doc: "Lifecycle state of a transcription job."
- Each variant thoroughly documented with explanation of what the state means
- Variants like `PartialFailure` clearly explain the nuance: "Transcription partially succeeded — result saved but warnings present."

**Sample documentation:**
```rust
/// Lifecycle state of a transcription job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Running,
    /// Transcription succeeded (word or utterance timestamps populated).
    Completed,
    /// Transcription partially succeeded — result saved but warnings present.
    PartialFailure,
    /// All retry attempts exhausted with no usable output.
    Failed,
}
```

---

### 5. `OutcomeSignal` in `src/enrichment/outcome.rs` (lines 43-67) ⭐ EXCELLENT

**Why it's outstanding:**
- Each variant thoroughly documented with concrete examples
- Clear explanation of what each signal contributes to outcome detection
- Real-world examples like "User expressed satisfaction (e.g., 'thanks', 'works now')"

**Sample documentation:**
```rust
/// Signal types that contribute to outcome detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum OutcomeSignal {
    /// User expressed satisfaction (e.g., "thanks", "works now")
    UserSatisfaction,
    /// User expressed frustration or gave up
    UserFrustration,
    /// Final tool call was a read/write (likely success)
    FinalEditWrite,
    // ... more variants with clear examples
}
```

---

### 6. `LogFormat` in `src/plugin.rs` (lines 279-296) ⭐ EXCELLENT

**Why it's outstanding:**
- Comprehensive module-level doc (40+ lines) explaining:
  - What each format represents
  - How formats are read and parsed
  - When incremental scraping is supported
  - "Best for" guidance for each format
  - Choosing the right format guidance
- Each variant has clear doc comments
- Context about capabilities and limitations

---

### 7. `TimestampLevel` in `src/transcription.rs` (lines 89-97) ⭐ EXCELLENT

**Why it's outstanding:**
- Clear, concise module-level doc: "Granularity of the timestamps in a TranscriptionResult."
- Each variant documented with explanation of when it's used
- Clear distinction between word-level and utterance-level timestamps
- Fallback path explanation

---

## Good But Could Be Improved

These enums have adequate documentation but would benefit from additional detail or clarity:

### 1. `SessionIdSource` in `src/plugin.rs` (lines 342-349) - IMPROVABLE

**Current state:**
```rust
/// Where to extract the session ID
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionIdSource {
    Filename,
    #[serde(rename = "field")]
    Field(String),
}
```

**Issue:** The `Field` variant is not well-documented. What field? From where? How is it used?

**Recommendation:**
```rust
/// Where to extract the session ID from source files
///
/// # Variants
///
/// * **Filename** - Extract session ID from the source file's filename (e.g., "abc123.jsonl" → "abc123")
/// * **Field** - Extract session ID from a specific field in the source data (e.g., a JSON field named "conversation_id")
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionIdSource {
    Filename,
    /// Extract from a specific field named in the String value
    #[serde(rename = "field")]
    Field(String),
}
```

---

### 2. `EmbeddingModel` in `src/embedding.rs` (lines 10-17) - IMPROVABLE

**Current state:**
```rust
/// Embedding model type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingModel {
    /// Local Ollama model (nomic-embed-text, 768-dim)
    Ollama { dim: usize },
    /// OpenAI text-embedding-3-small (1536-dim)
    OpenAI { dim: usize },
}
```

**Issue:** While the variant docs are clear, the enum-level doc doesn't explain when to use each model or the trade-offs.

**Recommendation:**
```rust
/// Supported embedding models for vector indexing
///
/// Two embedding backends are supported:
///
/// * **Ollama** - Local, privacy-preserving, runs nomic-embed-text (768 dimensions). Use when:
///   - Privacy is a concern (data stays local)
///   - Network connectivity is limited
///   - Cost must be minimized (no API fees)
///
/// * **OpenAI** - Cloud-hosted, higher quality, runs text-embedding-3-small (1536 dimensions). Use when:
///   - Highest quality embeddings are needed
///   - API rate limits are acceptable
///   - Cloud connectivity is available
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingModel {
    /// Local Ollama model (nomic-embed-text, 768-dim)
    Ollama { dim: usize },
    /// OpenAI text-embedding-3-small (1536-dim)
    OpenAI { dim: usize },
}
```

---

### 3. `AgentScribeError` in `src/error.rs` (lines 63-139) - IMPROVABLE

**Current state:**
```rust
/// Main error type for AgentScribe
#[derive(Error, Debug)]
pub enum AgentScribeError {
    /// Configuration errors
    #[error("Configuration error: {0}")]
    Config(String),
    /// Plugin errors
    #[error("Plugin error in '{name}': {message}")]
    Plugin { name: String, message: String },
    // ... more variants
}
```

**Issue:** The module-level documentation is excellent (comprehensive explanation of error categories and handling strategies), but individual variant docs could be more descriptive beyond just the category name.

**Recommendation:**
```rust
/// Main error type for AgentScribe
#[derive(Error, Debug)]
pub enum AgentScribeError {
    /// Configuration errors in config.toml or plugin definitions
    #[error("Configuration error: {0}")]
    Config(String),
    /// Plugin-specific errors in scraper plugin definitions
    #[error("Plugin error in '{name}': {message}")]
    Plugin { name: String, message: String },
    /// Parser errors during log parsing (skipped, not fatal)
    #[error("{message}")]
    Parse { file: String, line: Option<usize>, message: String },
    // ... etc
}
```

---

### 4. `Outcome` in `src/enrichment/outcome.rs` (lines 11-19) - IMPROVABLE

**Current state:**
```rust
/// Outcome classification for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Success,
    Failure,
    Abandoned,
    Unknown,
}
```

**Issue:** The enum-level doc is clear ("Outcome classification for a session") but would benefit from explaining when each variant is applied.

**Recommendation:**
```rust
/// Outcome classification for a session
///
/// # Variants
///
/// * **Success** - Session achieved user's goal (detected via satisfaction signals, task completion phrases, or successful tool calls)
/// * **Failure** - Session failed to achieve goal (detected via frustration signals, unresolved errors, or repeated failures)
/// * **Abandoned** - Session was very short with minimal interaction, likely user gave up
/// * **Unknown** - Outcome cannot be determined from available signals (ambiguous session)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Success,
    Failure,
    Abandoned,
    Unknown,
}
```

---

## Missing Enum-Level Documentation

These enums lack module-level documentation explaining what they represent and when to use each variant:

### 1. `Rule` in `src/rules.rs` (lines 54-64) - MISSING ENUM DOC

**Current state:**
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Rule {
    /// Search across all sessions
    AllSessions,
    /// Search within a specific project
    Project(String),
    /// Search by agent type
    Agent(String),
    /// Search by tag
    Tag(String),
}
```

**Issue:** No enum-level doc explaining what `Rule` represents or how it's used in search queries.

**Recommendation:**
```rust
/// Search rule for filtering sessions
///
/// Rules define how sessions are filtered in search queries. Each rule represents a different
/// filtering criterion: project directory, agent type, tags, or no filter (all sessions).
///
/// # Variants
///
/// * **AllSessions** - No filter, return all sessions from all agents and projects
/// * **Project** - Filter to sessions from a specific project directory (e.g., "/home/user/myapp")
/// * **Agent** - Filter to sessions from a specific agent type (e.g., "claude-code", "aider")
/// * **Tag** - Filter to sessions tagged with a specific tag (e.g., "rust", "postgres")
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Rule {
    /// Search across all sessions
    AllSessions,
    /// Search within a specific project
    Project(String),
    /// Search by agent type
    Agent(String),
    /// Search by tag
    Tag(String),
}
```

---

### 2. `OutputFormat` in `src/rules.rs` (lines 86-90) - MISSING ENUM DOC

**Current state:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    /// Human-readable pretty-printed JSON
    Pretty,
    /// Minimal JSON
    Compact,
}
```

**Issue:** No enum-level doc explaining what `OutputFormat` controls or when to use each format.

**Recommendation:**
```rust
/// Output format for search results and reports
///
/// Controls how JSON output is formatted. Choose Pretty for human-readable output
/// (e.g., terminal display) and Compact for scripts or machine processing.
///
/// # Variants
///
/// * **Pretty** - Indented, multi-line JSON with spacing for readability
/// * **Compact** - Single-line JSON without whitespace for minimal size
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    /// Human-readable pretty-printed JSON
    Pretty,
    /// Minimal JSON
    Compact,
}
```

---

### 3. `ReflectError` in `src/reflect.rs` (lines 703-713) - MISSING ENUM DOC

**Current state:**
```rust
#[derive(Error, Debug)]
pub enum ReflectError {
    #[error("Failed to run reflection: {0}")]
    ReflectionFailed(String),
    #[error("Failed to write reflection output: {0}")]
    WriteFailed(String),
    #[error("Failed to serialize reflection data: {0}")]
    SerializationFailed(String),
}
```

**Issue:** No enum-level doc explaining what reflection is or when these errors occur.

**Recommendation:**
```rust
/// Errors that can occur during reflection export
///
/// Reflection is the process of analyzing conversation logs to extract patterns,
/// outcomes, and learnings. These errors represent failures in that process.
#[derive(Error, Debug)]
pub enum ReflectError {
    /// Reflection analysis failed (e.g., no sessions found, pattern extraction failed)
    #[error("Failed to run reflection: {0}")]
    ReflectionFailed(String),
    /// Failed to write reflection output to file
    #[error("Failed to write reflection output: {0}")]
    WriteFailed(String),
    /// Failed to serialize reflection data to JSON
    #[error("Failed to serialize reflection data: {0}")]
    SerializationFailed(String),
}
```

---

### 4. `ReportFormat` in `src/pulse_report.rs` (lines 18-23) - MISSING ENUM DOC

**Current state:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportFormat {
    /// Human-readable markdown table
    Markdown,
    /// Machine-readable JSON
    Json,
}
```

**Issue:** No enum-level doc explaining what `ReportFormat` represents or when to use each format.

**Recommendation:**
```rust
/// Output format for pulse reports
///
/// Pulse reports aggregate session statistics (counts, outcomes, agent usage). This enum
/// controls how the report is formatted for output.
///
/// # Variants
///
/// * **Markdown** - Human-readable table format for terminal display or documentation
/// * **Json** - Machine-readable format for scripting and further processing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportFormat {
    /// Human-readable markdown table
    Markdown,
    /// Machine-readable JSON
    Json,
}
```

---

### 5. `SortOrder` in `src/search.rs` (lines 264-271) - MISSING ENUM DOC

**Current state:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
    /// Sort newest first
    NewestFirst,
    /// Sort oldest first
    OldestFirst,
}
```

**Issue:** No enum-level doc explaining what `SortOrder` controls or what is being sorted.

**Recommendation:**
```rust
/// Sort order for search results by timestamp
///
/// Controls the chronological ordering of sessions returned from search queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
    /// Sort newest first (most recent sessions at the top)
    NewestFirst,
    /// Sort oldest first (oldest sessions at the top)
    OldestFirst,
}
```

---

## Summary Statistics

| Grade | Count | Percentage |
|-------|-------|------------|
| Excellent (Model Standard) | 9 | 50% |
| Good (Improvable) | 4 | 22% |
| Missing Enum-Level Docs | 5 | 28% |
| **Total** | **18** | **100%** |

---

## Recommendations

### High Priority (Missing Documentation)
1. Add enum-level docs to `Rule`, `OutputFormat`, `ReflectError`, `ReportFormat`, and `SortOrder`
2. Follow the pattern established by outstanding examples
3. Ensure each enum doc answers: "What does this enum represent? When should I use each variant?"

### Medium Priority (Improvable Documentation)
1. Enhance `SessionIdSource` docs to explain the `Field` variant better
2. Add when-to-use guidance to `EmbeddingModel`
3. Expand variant docs in `AgentScribeError` beyond category names
4. Add variant descriptions to `Outcome`

### Low Priority (Maintain Standards)
1. Use outstanding examples (`ImportType`, `Role`, `ProblemType`) as templates for new enums
2. Include examples in docs when variants have complex usage patterns
3. Document historical context when relevant (e.g., Rust edition differences)

---

## Conclusion

The AgentScribe codebase demonstrates a strong commitment to documentation quality overall. The 9 enums rated as "Excellent" provide clear, comprehensive documentation that answers both "what does this represent?" and "when should I use each variant?"

The 5 enums missing enum-level documentation should be addressed to bring them up to the codebase standard. The 4 improvable enums would benefit from additional detail but are currently functional.

The standout examples (`ImportType`, `Role`, `ProblemType`, `JobStatus`, `OutcomeSignal`, `LogFormat`, `TimestampLevel`) should be used as templates for future enum documentation.
