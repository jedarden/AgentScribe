//! Enrichment pipeline: adds intelligence to scraped sessions.
//!
//! This module implements Phase 3 enrichment:
//! - Summary generation from first user prompt + outcome + files touched
//! - Outcome detection with signal-scoring system
//! - Solution extraction from resolution windows
//! - Error fingerprinting with normalization
//! - Anti-pattern detection for failed sessions
//! - Code artifact extraction from assistant responses
//! - Git commit correlation

pub mod antipatterns;
pub mod code_artifacts;
pub mod errors;
pub mod git;
pub mod outcome;
pub mod solution;
pub mod summary;

pub use antipatterns::{AntiPattern, detect_antipatterns, write_antipatterns_sidecar};
pub use code_artifacts::{CodeArtifact, extract_code_artifacts};
pub use errors::{extract_error_fingerprints, normalize_error, enrich_events};
pub use git::{GitCommit, correlate_commits, blame_file_line, BlameResult, build_commit_index, file_git_log};
pub use outcome::{detect_outcome, Outcome, OutcomeConfig, OutcomeSignal};
pub use solution::extract_solution;
pub use summary::generate_summary;

use crate::event::{Event, SessionManifest};
use crate::scraper::Scraper;
use std::path::Path;

/// Result of enriching a session.
pub struct EnrichmentResult {
    /// Detected outcome
    pub outcome: Outcome,
    /// Generated summary
    pub summary: String,
    /// Extracted solution text
    pub solution_summary: Option<String>,
    /// Code artifacts extracted from assistant responses
    pub code_artifacts: Vec<CodeArtifact>,
    /// Anti-patterns detected
    pub anti_patterns: Vec<AntiPattern>,
    /// Git commits correlated with this session
    pub git_commits: Vec<GitCommit>,
}

/// Run the full enrichment pipeline on a session.
pub fn enrich_session(
    events: &[Event],
    manifest: &SessionManifest,
    outcome_config: &OutcomeConfig,
    data_dir: &Path,
    scraper: &Scraper,
) -> EnrichmentResult {
    // Enrich events with error fingerprints
    let events_with_fps = enrich_events(events);

    // Detect outcome
    let outcome = detect_outcome(&events_with_fps, manifest, outcome_config);

    // Generate summary
    let summary = generate_summary(&events_with_fps, manifest);

    // Extract solution
    let solution_summary = extract_solution(&events_with_fps);

    // Extract code artifacts
    let code_artifacts = extract_code_artifacts(&events_with_fps);

    // Detect anti-patterns
    let outcome_str = outcome.as_str();
    let anti_patterns = detect_antipatterns(&events_with_fps, manifest, Some(outcome_str), scraper);

    // Correlate git commits
    let git_commits = if let Some(ref project) = manifest.project {
        correlate_commits(project, manifest.started, manifest.ended)
    } else {
        Vec::new()
    };

    EnrichmentResult {
        outcome,
        summary,
        solution_summary,
        code_artifacts,
        anti_patterns,
        git_commits,
    }
}
