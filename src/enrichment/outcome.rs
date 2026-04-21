//! Outcome detection with signal-scoring system.
//!
//! Classifies sessions as success/failure/abandoned/unknown based on
//! configurable signal weights.

use crate::event::{Event, Role, SessionManifest};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Outcome classification for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Success,
    Failure,
    Abandoned,
    Unknown,
}

impl Outcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Outcome::Success => "success",
            Outcome::Failure => "failure",
            Outcome::Abandoned => "abandoned",
            Outcome::Unknown => "unknown",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "success" => Some(Outcome::Success),
            "failure" => Some(Outcome::Failure),
            "abandoned" => Some(Outcome::Abandoned),
            "unknown" => Some(Outcome::Unknown),
            _ => None,
        }
    }
}

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
    /// Final tool call was an error
    FinalError,
    /// Session ended with an error message visible
    HasUnresolvedError,
    /// Tool results indicate success
    ToolSuccess,
    /// Tool results indicate failure
    ToolFailure,
    /// Session was very short (likely abandoned)
    VeryShortSession,
    /// User asked for help but no resolution
    HelpWithoutResolution,
    /// Task completion phrases detected
    TaskCompletion,
}

/// Configuration for outcome detection weights.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeConfig {
    /// Weight for user satisfaction signals
    #[serde(default = "default_user_satisfaction")]
    pub user_satisfaction: i32,

    /// Weight for user frustration signals
    #[serde(default = "default_user_frustration")]
    pub user_frustration: i32,

    /// Weight for final edit/write signals
    #[serde(default = "default_final_edit_write")]
    pub final_edit_write: i32,

    /// Weight for final error signals
    #[serde(default = "default_final_error")]
    pub final_error: i32,

    /// Weight for unresolved error signals
    #[serde(default = "default_unresolved_error")]
    pub unresolved_error: i32,

    /// Weight for tool success signals
    #[serde(default = "default_tool_success")]
    pub tool_success: i32,

    /// Weight for tool failure signals
    #[serde(default = "default_tool_failure")]
    pub tool_failure: i32,

    /// Weight for very short session signals
    #[serde(default = "default_very_short_session")]
    pub very_short_session: i32,

    /// Weight for help without resolution
    #[serde(default = "default_help_without_resolution")]
    pub help_without_resolution: i32,

    /// Weight for task completion signals
    #[serde(default = "default_task_completion")]
    pub task_completion: i32,

    /// Threshold for success classification
    #[serde(default = "default_success_threshold")]
    pub success_threshold: i32,

    /// Threshold for failure classification
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: i32,

    /// Minimum turns to not be considered "very short"
    #[serde(default = "default_min_turns")]
    pub min_turns: u32,
}

fn default_user_satisfaction() -> i32 {
    30
}
fn default_user_frustration() -> i32 {
    -25
}
fn default_final_edit_write() -> i32 {
    15
}
fn default_final_error() -> i32 {
    -20
}
fn default_unresolved_error() -> i32 {
    -15
}
fn default_tool_success() -> i32 {
    10
}
fn default_tool_failure() -> i32 {
    -10
}
fn default_very_short_session() -> i32 {
    -5
}
fn default_help_without_resolution() -> i32 {
    -10
}
fn default_task_completion() -> i32 {
    25
}
fn default_success_threshold() -> i32 {
    20
}
fn default_failure_threshold() -> i32 {
    -20
}
fn default_min_turns() -> u32 {
    3
}

impl Default for OutcomeConfig {
    fn default() -> Self {
        OutcomeConfig {
            user_satisfaction: default_user_satisfaction(),
            user_frustration: default_user_frustration(),
            final_edit_write: default_final_edit_write(),
            final_error: default_final_error(),
            unresolved_error: default_unresolved_error(),
            tool_success: default_tool_success(),
            tool_failure: default_tool_failure(),
            very_short_session: default_very_short_session(),
            help_without_resolution: default_help_without_resolution(),
            task_completion: default_task_completion(),
            success_threshold: default_success_threshold(),
            failure_threshold: default_failure_threshold(),
            min_turns: default_min_turns(),
        }
    }
}

// Regex patterns for signal detection
static SATISFACTION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(thanks|thank you|works? now|fixed|perfect|great|awesome|that worked|exactly what|helped|solved)\b").unwrap()
});

static FRUSTRATION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(still not working|doesn'?t work|still broken|gave up|never mind|forget it|too complicated|this is stupid|waste of time|frustrating)\b").unwrap()
});

static COMPLETION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(done|completed|finished|implemented|added|created|updated|removed|deleted|merged|deployed|pushed)\b").unwrap()
});

static ERROR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(error|failed|exception|panic|fatal|timeout|refused|denied|invalid|not found|unexpected)\b").unwrap()
});

static SUCCESS_TOOL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(success\w*|ok|completed|done|passed)\b").unwrap());

/// Detect the outcome of a session based on events and configuration.
pub fn detect_outcome(
    events: &[Event],
    manifest: &SessionManifest,
    config: &OutcomeConfig,
) -> Outcome {
    // If manifest already has an outcome, respect it
    if let Some(ref outcome) = manifest.outcome {
        if let Some(parsed) = Outcome::from_str(outcome) {
            if parsed != Outcome::Unknown {
                return parsed;
            }
        }
    }

    let mut score: i32 = 0;
    let mut signals: Vec<OutcomeSignal> = Vec::new();

    // Check for very short session
    if manifest.turns < config.min_turns {
        signals.push(OutcomeSignal::VeryShortSession);
        score += config.very_short_session;
    }

    // Analyze events for signals
    let mut has_user_message = false;
    let mut last_user_satisfied = false;
    let mut has_unresolved_error = false;
    let mut last_tool_result_success = None;

    for event in events {
        match event.role {
            Role::User => {
                has_user_message = true;

                if SATISFACTION_RE.is_match(&event.content) {
                    signals.push(OutcomeSignal::UserSatisfaction);
                    score += config.user_satisfaction;
                    last_user_satisfied = true;
                }

                if FRUSTRATION_RE.is_match(&event.content) {
                    signals.push(OutcomeSignal::UserFrustration);
                    score += config.user_frustration;
                    last_user_satisfied = false;
                }

                if COMPLETION_RE.is_match(&event.content) {
                    signals.push(OutcomeSignal::TaskCompletion);
                    score += config.task_completion;
                }
            }
            Role::ToolCall => {
                if let Some(ref tool) = event.tool {
                    // Check if final tool call was an edit/write operation
                    if matches!(tool.as_str(), "Edit" | "Write" | "apply_edit") {
                        // Will be checked later for final event
                    }
                }
            }
            Role::ToolResult => {
                if ERROR_RE.is_match(&event.content) {
                    has_unresolved_error = true;
                    last_tool_result_success = Some(false);
                } else if SUCCESS_TOOL_RE.is_match(&event.content) {
                    last_tool_result_success = Some(true);
                }
            }
            _ => {}
        }
    }

    // Check final events for resolution signals
    if let Some(last_event) = events.last() {
        if last_event.role == Role::ToolCall {
            if let Some(ref tool) = last_event.tool {
                if matches!(tool.as_str(), "Edit" | "Write" | "apply_edit") {
                    signals.push(OutcomeSignal::FinalEditWrite);
                    score += config.final_edit_write;
                }
            }
        } else if last_event.role == Role::ToolResult {
            // Check if the preceding event was an edit/write tool call
            if events.len() >= 2 {
                if let Some(prev_event) = events.get(events.len() - 2) {
                    if prev_event.role == Role::ToolCall {
                        if let Some(ref tool) = prev_event.tool {
                            if matches!(tool.as_str(), "Edit" | "Write" | "apply_edit") {
                                signals.push(OutcomeSignal::FinalEditWrite);
                                score += config.final_edit_write;
                            }
                        }
                    }
                }
            }

            if ERROR_RE.is_match(&last_event.content) {
                signals.push(OutcomeSignal::FinalError);
                score += config.final_error;
            }
        }
    }

    // Apply unresolved error penalty if user didn't express satisfaction
    if has_unresolved_error && !last_user_satisfied {
        signals.push(OutcomeSignal::HasUnresolvedError);
        score += config.unresolved_error;
    }

    // Apply tool result signals
    if let Some(success) = last_tool_result_success {
        if success {
            signals.push(OutcomeSignal::ToolSuccess);
            score += config.tool_success;
        } else {
            signals.push(OutcomeSignal::ToolFailure);
            score += config.tool_failure;
        }
    }

    // Classify based on score
    if score >= config.success_threshold {
        Outcome::Success
    } else if score <= config.failure_threshold {
        Outcome::Failure
    } else if manifest.turns < config.min_turns && has_user_message {
        // Short session with user message but no clear outcome
        Outcome::Abandoned
    } else {
        Outcome::Unknown
    }
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

    fn make_tool_call(tool: &str) -> Event {
        Event::new(
            Utc::now(),
            "test/1".into(),
            "test".into(),
            Role::ToolCall,
            "".into(),
        )
        .with_tool(Some(tool.into()))
    }

    fn make_manifest(turns: u32) -> SessionManifest {
        let mut m = SessionManifest::new("test/1".into(), "test".into());
        m.turns = turns;
        m
    }

    #[test]
    fn test_user_satisfaction_detection() {
        let events = vec![
            make_event(Role::User, "fix this bug"),
            make_event(Role::Assistant, "I fixed it"),
            make_event(Role::User, "thanks, that worked!"),
        ];
        let manifest = make_manifest(3);
        let config = OutcomeConfig::default();

        let outcome = detect_outcome(&events, &manifest, &config);
        assert_eq!(outcome, Outcome::Success);
    }

    #[test]
    fn test_user_frustration_detection() {
        let events = vec![
            make_event(Role::User, "fix this bug"),
            make_event(Role::Assistant, "I tried"),
            make_event(Role::ToolResult, "error: something failed"),
            make_event(Role::User, "still not working, giving up"),
        ];
        let manifest = make_manifest(4);
        let config = OutcomeConfig::default();

        let outcome = detect_outcome(&events, &manifest, &config);
        assert_eq!(outcome, Outcome::Failure);
    }

    #[test]
    fn test_final_edit_write_signal() {
        let events = vec![
            make_event(Role::User, "add a feature"),
            make_event(Role::Assistant, "I'll add it"),
            make_tool_call("Edit"),
            make_event(Role::ToolResult, "File updated successfully"),
        ];
        let manifest = make_manifest(4);
        let config = OutcomeConfig::default();

        let outcome = detect_outcome(&events, &manifest, &config);
        assert_eq!(outcome, Outcome::Success);
    }

    #[test]
    fn test_very_short_session_abandoned() {
        let events = vec![make_event(Role::User, "can you help with")];
        let manifest = make_manifest(1);
        let config = OutcomeConfig::default();

        let outcome = detect_outcome(&events, &manifest, &config);
        assert_eq!(outcome, Outcome::Abandoned);
    }

    #[test]
    fn test_respects_manifest_outcome() {
        let events = vec![make_event(Role::User, "do something")];
        let mut manifest = make_manifest(1);
        manifest.outcome = Some("success".into());
        let config = OutcomeConfig::default();

        let outcome = detect_outcome(&events, &manifest, &config);
        assert_eq!(outcome, Outcome::Success);
    }

    #[test]
    fn test_unresolved_error_penalty() {
        let events = vec![
            make_event(Role::User, "fix this"),
            make_event(Role::ToolResult, "error: connection refused"),
            make_event(Role::User, "try again"),
            make_event(Role::ToolResult, "error: timeout"),
        ];
        let manifest = make_manifest(4);
        let config = OutcomeConfig::default();

        let outcome = detect_outcome(&events, &manifest, &config);
        assert_eq!(outcome, Outcome::Failure);
    }

    #[test]
    fn test_error_then_satisfaction() {
        let events = vec![
            make_event(Role::User, "fix this"),
            make_event(Role::ToolResult, "error: something failed"),
            make_event(Role::Assistant, "Let me try a different approach"),
            make_event(Role::ToolResult, "Success!"),
            make_event(Role::User, "thanks, works now!"),
        ];
        let manifest = make_manifest(5);
        let config = OutcomeConfig::default();

        let outcome = detect_outcome(&events, &manifest, &config);
        // Satisfaction should outweigh the earlier error
        assert_eq!(outcome, Outcome::Success);
    }

    #[test]
    fn test_outcome_as_str() {
        assert_eq!(Outcome::Success.as_str(), "success");
        assert_eq!(Outcome::Failure.as_str(), "failure");
        assert_eq!(Outcome::Abandoned.as_str(), "abandoned");
        assert_eq!(Outcome::Unknown.as_str(), "unknown");
    }

    #[test]
    fn test_outcome_from_str() {
        assert_eq!(Outcome::from_str("success"), Some(Outcome::Success));
        assert_eq!(Outcome::from_str("FAILURE"), Some(Outcome::Failure));
        assert_eq!(Outcome::from_str("invalid"), None);
    }

    #[test]
    fn test_config_defaults() {
        let config = OutcomeConfig::default();
        assert!(config.user_satisfaction > 0);
        assert!(config.user_frustration < 0);
        assert!(config.success_threshold > 0);
        assert!(config.failure_threshold < 0);
    }
}
