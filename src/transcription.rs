//! Local Whisper transcription service.
//!
//! Provides an async job queue for audio transcription using a local Whisper
//! model (whisper.cpp or OpenAI Whisper CLI). Supports wav/mp3/m4a inputs
//! with word-level timestamps.
//!
//! Failure modes
//! - Word-level timestamps unavailable → falls back to utterance timestamps
//!   and sets `has_warnings = true`.
//! - Whisper subprocess exits non-zero but writes partial output → partial
//!   transcript is saved with warnings.
//! - All retries exhausted → `JobStatus::Failed` with the last error message.
//!
//! Privacy: every transcript passes through [`RedactionScanner`] before the
//! result is stored or returned (§18).

use crate::config::{RedactionConfig, WhisperConfig};
use crate::error::{AgentScribeError, Result};
use crate::redaction::RedactionScanner;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

// ─── Unique job ID counter ────────────────────────────────────────────────────

static JOB_COUNTER: AtomicU64 = AtomicU64::new(0);

fn new_job_id() -> String {
    let seq = JOB_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("txjob-{}-{:06x}", Utc::now().timestamp_millis(), seq)
}

// ─── Audio format ─────────────────────────────────────────────────────────────

/// Supported audio input formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioFormat {
    Wav,
    Mp3,
    M4a,
}

impl AudioFormat {
    /// Detect format from the file extension. Returns `None` for unsupported types.
    pub fn from_path(path: &Path) -> Option<Self> {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .as_deref()
        {
            Some("wav") => Some(AudioFormat::Wav),
            Some("mp3") => Some(AudioFormat::Mp3),
            Some("m4a") => Some(AudioFormat::M4a),
            _ => None,
        }
    }
}

// ─── Timestamp types ──────────────────────────────────────────────────────────

/// A single word with timing data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordTimestamp {
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    /// Whisper token probability [0, 1]. `None` if not reported by backend.
    pub probability: Option<f32>,
}

/// A spoken utterance (segment) with timing data.
/// Used as the primary result when word-level timestamps are unavailable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtteranceTimestamp {
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

/// Granularity of the timestamps in a [`TranscriptionResult`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimestampLevel {
    /// Word-level timestamps are populated.
    Word,
    /// Only utterance-level timestamps are available (fallback path).
    Utterance,
}

// ─── TranscriptionResult ─────────────────────────────────────────────────────

/// Output of a completed or partial transcription.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    /// Full transcript text (redacted).
    pub full_text: String,
    /// Granularity of the timestamps in this result.
    pub timestamp_level: TimestampLevel,
    /// Word-level timestamps (empty when `timestamp_level == Utterance`).
    pub word_timestamps: Vec<WordTimestamp>,
    /// Utterance-level timestamps (always populated when utterances exist).
    pub utterance_timestamps: Vec<UtteranceTimestamp>,
    /// Language detected or configured.
    pub language: Option<String>,
    /// `true` when the result includes degraded data (fallback timestamps,
    /// partial transcript after failures).
    pub has_warnings: bool,
    /// Human-readable warning messages for display.
    pub warnings: Vec<String>,
    /// When the result was produced.
    pub transcribed_at: DateTime<Utc>,
}

impl TranscriptionResult {
    fn word_level(
        full_text: String,
        words: Vec<WordTimestamp>,
        utterances: Vec<UtteranceTimestamp>,
        language: Option<String>,
    ) -> Self {
        TranscriptionResult {
            full_text,
            timestamp_level: TimestampLevel::Word,
            word_timestamps: words,
            utterance_timestamps: utterances,
            language,
            has_warnings: false,
            warnings: Vec::new(),
            transcribed_at: Utc::now(),
        }
    }

    fn utterance_level(
        full_text: String,
        utterances: Vec<UtteranceTimestamp>,
        language: Option<String>,
        warnings: Vec<String>,
    ) -> Self {
        let has_warnings = !warnings.is_empty();
        TranscriptionResult {
            full_text,
            timestamp_level: TimestampLevel::Utterance,
            word_timestamps: Vec::new(),
            utterance_timestamps: utterances,
            language,
            has_warnings,
            warnings,
            transcribed_at: Utc::now(),
        }
    }

    /// `true` when no transcribed text is present.
    pub fn is_empty(&self) -> bool {
        self.full_text.trim().is_empty()
    }

    /// Count of transcribed words (word-level) or utterance count (fallback).
    pub fn segment_count(&self) -> usize {
        if self.timestamp_level == TimestampLevel::Word {
            self.word_timestamps.len()
        } else {
            self.utterance_timestamps.len()
        }
    }
}

// ─── Job types ────────────────────────────────────────────────────────────────

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

/// A transcription job tracked by the queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionJob {
    pub id: String,
    pub input_path: PathBuf,
    pub status: JobStatus,
    pub attempts: u32,
    pub max_retries: u32,
    pub result: Option<TranscriptionResult>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TranscriptionJob {
    pub fn new(input_path: PathBuf, max_retries: u32) -> Self {
        let now = Utc::now();
        TranscriptionJob {
            id: new_job_id(),
            input_path,
            status: JobStatus::Pending,
            attempts: 0,
            max_retries,
            result: None,
            error: None,
            created_at: now,
            updated_at: now,
        }
    }
}

// ─── Whisper output parsers ───────────────────────────────────────────────────

/// whisper.cpp JSON schema (--output-json-full).
mod whisper_cpp_schema {
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    pub struct Output {
        pub transcription: Vec<Segment>,
    }

    #[derive(Debug, Deserialize)]
    pub struct Segment {
        pub offsets: Offsets,
        pub text: String,
        #[serde(default)]
        pub tokens: Vec<Token>,
    }

    #[derive(Debug, Deserialize)]
    pub struct Offsets {
        pub from: u64,
        pub to: u64,
    }

    #[derive(Debug, Deserialize)]
    pub struct Token {
        pub text: String,
        pub offsets: Offsets,
        /// Token probability from whisper.cpp (field name "p").
        #[serde(default)]
        pub p: f32,
    }
}

/// OpenAI Whisper / faster-whisper JSON schema.
mod openai_whisper_schema {
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    pub struct Output {
        pub text: String,
        #[serde(default)]
        pub segments: Vec<Segment>,
        pub language: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    pub struct Segment {
        pub start: f64,
        pub end: f64,
        pub text: String,
        #[serde(default)]
        pub words: Vec<Word>,
    }

    #[derive(Debug, Deserialize)]
    pub struct Word {
        pub word: String,
        pub start: f64,
        pub end: f64,
        #[serde(default)]
        pub probability: f32,
    }
}

fn parse_whisper_cpp(json: &serde_json::Value) -> Result<TranscriptionResult> {
    let output: whisper_cpp_schema::Output = serde_json::from_value(json.clone()).map_err(|e| {
        AgentScribeError::Transcription(format!("whisper.cpp output parse error: {}", e))
    })?;

    let mut full_text = String::new();
    let mut utterances = Vec::new();
    let mut words = Vec::new();
    let mut has_word_data = false;

    for seg in &output.transcription {
        let seg_text = seg.text.trim().to_string();
        if !seg_text.is_empty() {
            full_text.push_str(&seg_text);
            full_text.push(' ');
        }
        utterances.push(UtteranceTimestamp {
            text: seg_text,
            start_ms: seg.offsets.from,
            end_ms: seg.offsets.to,
        });
        for token in &seg.tokens {
            // whisper.cpp includes special tokens like [_BEG_], [_TT_*] — skip them.
            let t = token.text.trim();
            if t.is_empty() || (t.starts_with('[') && t.ends_with(']')) {
                continue;
            }
            has_word_data = true;
            words.push(WordTimestamp {
                text: t.to_string(),
                start_ms: token.offsets.from,
                end_ms: token.offsets.to,
                probability: if token.p > 0.0 { Some(token.p) } else { None },
            });
        }
    }

    let full_text = full_text.trim().to_string();
    if has_word_data {
        Ok(TranscriptionResult::word_level(
            full_text, words, utterances, None,
        ))
    } else {
        Ok(TranscriptionResult::utterance_level(
            full_text,
            utterances,
            None,
            vec![
                "whisper.cpp returned no word-level tokens; using utterance timestamps".to_string(),
            ],
        ))
    }
}

fn parse_openai_whisper(json: &serde_json::Value) -> Result<TranscriptionResult> {
    let output: openai_whisper_schema::Output =
        serde_json::from_value(json.clone()).map_err(|e| {
            AgentScribeError::Transcription(format!("OpenAI Whisper output parse error: {}", e))
        })?;

    let mut utterances = Vec::new();
    let mut words = Vec::new();
    let mut has_word_data = false;

    for seg in &output.segments {
        let seg_text = seg.text.trim().to_string();
        utterances.push(UtteranceTimestamp {
            text: seg_text,
            start_ms: (seg.start * 1000.0) as u64,
            end_ms: (seg.end * 1000.0) as u64,
        });
        for w in &seg.words {
            let t = w.word.trim();
            if t.is_empty() {
                continue;
            }
            has_word_data = true;
            words.push(WordTimestamp {
                text: t.to_string(),
                start_ms: (w.start * 1000.0) as u64,
                end_ms: (w.end * 1000.0) as u64,
                probability: if w.probability > 0.0 {
                    Some(w.probability)
                } else {
                    None
                },
            });
        }
    }

    let full_text = output.text.trim().to_string();
    let language = output.language;

    if has_word_data {
        Ok(TranscriptionResult::word_level(
            full_text, words, utterances, language,
        ))
    } else {
        Ok(TranscriptionResult::utterance_level(
            full_text,
            utterances,
            language,
            vec!["OpenAI Whisper returned no word-level timestamps; \
                 pass --word_timestamps True to the executable"
                .to_string()],
        ))
    }
}

/// Auto-detect backend from the output JSON and parse.
fn parse_whisper_output(json_str: &str) -> Result<TranscriptionResult> {
    let value: serde_json::Value = serde_json::from_str(json_str).map_err(|e| {
        AgentScribeError::Transcription(format!("invalid JSON from whisper: {}", e))
    })?;

    if value.get("transcription").is_some() {
        parse_whisper_cpp(&value)
    } else if value.get("segments").is_some() || value.get("text").is_some() {
        parse_openai_whisper(&value)
    } else {
        Err(AgentScribeError::Transcription(
            "unrecognized whisper output format (no 'transcription' or 'segments' key)".to_string(),
        ))
    }
}

// ─── Subprocess runner ────────────────────────────────────────────────────────

/// Run the whisper subprocess and return the raw JSON string output.
async fn run_whisper_subprocess(
    input_path: &Path,
    config: &WhisperConfig,
    out_dir: &Path,
) -> Result<String> {
    let executable = config.executable.as_deref().unwrap_or("whisper");

    let mut cmd = tokio::process::Command::new(executable);

    match config.backend.as_deref().unwrap_or("auto") {
        "whisper_cpp" | "whisper.cpp" => {
            // whisper.cpp CLI: whisper -f input.wav --output-json-full --output-dir dir
            cmd.arg("-f").arg(input_path);
            cmd.arg("--output-json-full");
            cmd.arg("--output-dir").arg(out_dir);
            if let Some(ref model) = config.model_path {
                cmd.arg("-m").arg(shellexpand::tilde(model).as_ref());
            }
            if let Some(ref lang) = config.language {
                cmd.arg("-l").arg(lang);
            }
        }
        _ => {
            // OpenAI Whisper / faster-whisper style (default)
            cmd.arg(input_path);
            cmd.arg("--output_format").arg("json");
            cmd.arg("--output_dir").arg(out_dir);
            if config.word_timestamps {
                cmd.arg("--word_timestamps").arg("True");
            }
            if let Some(ref model) = config.model_path {
                cmd.arg("--model").arg(model);
            }
            if let Some(ref lang) = config.language {
                cmd.arg("--language").arg(lang);
            }
        }
    }

    debug!(executable = %executable, input = %input_path.display(), "invoking whisper");

    let timeout = Duration::from_secs(config.timeout_seconds);
    let output = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| {
            AgentScribeError::Transcription(format!(
                "whisper timed out after {} seconds",
                config.timeout_seconds
            ))
        })?
        .map_err(|e| AgentScribeError::Transcription(format!("failed to launch whisper: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Attempt to salvage partial output before returning an error.
        if let Ok(partial) = find_output_json(input_path, out_dir) {
            warn!(
                exit_code = ?output.status.code(),
                stderr = %stderr,
                "whisper exited non-zero but partial output found — using it"
            );
            return Ok(partial);
        }
        return Err(AgentScribeError::Transcription(format!(
            "whisper exited with {:?}: {}",
            output.status.code(),
            stderr.trim()
        )));
    }

    // Some backends write to stdout rather than a file.
    if !output.stdout.is_empty() {
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        // Only use stdout if it looks like JSON.
        let trimmed = text.trim_start();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            return Ok(text);
        }
    }

    find_output_json(input_path, out_dir)
}

/// Locate the JSON file whisper wrote in the output directory.
fn find_output_json(input_path: &Path, out_dir: &Path) -> Result<String> {
    let stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("audio");

    for candidate in [
        out_dir.join(format!("{}.json", stem)),
        out_dir.join(format!(
            "{}.json",
            input_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("audio")
        )),
    ] {
        if candidate.exists() {
            return std::fs::read_to_string(&candidate).map_err(|e| {
                AgentScribeError::Transcription(format!(
                    "failed to read whisper output {}: {}",
                    candidate.display(),
                    e
                ))
            });
        }
    }

    Err(AgentScribeError::Transcription(
        "whisper output JSON not found in output directory".to_string(),
    ))
}

// ─── Single-attempt transcription ────────────────────────────────────────────

async fn transcribe_once(
    input_path: &Path,
    config: &WhisperConfig,
    scanner: &RedactionScanner,
) -> Result<TranscriptionResult> {
    // Validate format early so the user gets a clear error.
    if AudioFormat::from_path(input_path).is_none() {
        return Err(AgentScribeError::Transcription(format!(
            "unsupported audio format — expected wav, mp3, or m4a: {}",
            input_path.display()
        )));
    }

    // Unique temp directory per attempt to avoid output file collisions.
    let out_dir = std::env::temp_dir().join(format!("agentscribe-whisper-{}", new_job_id()));
    std::fs::create_dir_all(&out_dir).map_err(|e| {
        AgentScribeError::Transcription(format!("could not create temp dir: {}", e))
    })?;

    let whisper_result = run_whisper_subprocess(input_path, config, &out_dir).await;
    let _ = std::fs::remove_dir_all(&out_dir); // best-effort cleanup

    let json_str = whisper_result?;
    let mut transcript = parse_whisper_output(&json_str)?;

    // ── Privacy redaction (§18) ────────────────────────────────────────────
    transcript.full_text = scanner.redact(&transcript.full_text);
    for w in &mut transcript.word_timestamps {
        w.text = scanner.redact(&w.text);
    }
    for u in &mut transcript.utterance_timestamps {
        u.text = scanner.redact(&u.text);
    }

    Ok(transcript)
}

// ─── Retry logic ──────────────────────────────────────────────────────────────

/// Run transcription with retry and exponential back-off.
///
/// On exhaustion, returns the best partial result seen (if any) with warnings,
/// or propagates the last error.
async fn attempt_with_retry(
    job: &mut TranscriptionJob,
    config: &WhisperConfig,
    scanner: &RedactionScanner,
) -> Result<TranscriptionResult> {
    let mut last_error: Option<AgentScribeError> = None;
    let mut partial: Option<TranscriptionResult> = None;

    for attempt in 0..=job.max_retries {
        job.attempts = attempt + 1;

        if attempt > 0 {
            // Exponential back-off: 2, 4, 8 … seconds, capped at 30 s.
            let delay = Duration::from_secs(2u64.pow(attempt.min(4)));
            debug!(
                job_id = %job.id,
                attempt,
                delay_secs = delay.as_secs(),
                "retrying transcription"
            );
            sleep(delay).await;
        }

        match transcribe_once(&job.input_path, config, scanner).await {
            Ok(result) if !result.is_empty() => return Ok(result),
            Ok(result) => {
                // Empty transcript — might be a transient issue, save and retry.
                partial = Some(result);
                last_error = Some(AgentScribeError::Transcription(
                    "whisper produced empty transcript".to_string(),
                ));
            }
            Err(e) => {
                warn!(
                    job_id = %job.id,
                    attempt,
                    error = %e,
                    "transcription attempt failed"
                );
                last_error = Some(e);
            }
        }
    }

    // Return best partial result with warnings, or the last error.
    if let Some(mut p) = partial {
        p.has_warnings = true;
        p.warnings.push(format!(
            "partial transcript saved after {} failed attempt(s)",
            job.max_retries + 1,
        ));
        return Ok(p);
    }

    Err(last_error.unwrap_or_else(|| {
        AgentScribeError::Transcription("transcription failed with no details".to_string())
    }))
}

// ─── Background worker ────────────────────────────────────────────────────────

async fn update_state<F>(state: &Mutex<HashMap<String, TranscriptionJob>>, id: &str, f: F)
where
    F: FnOnce(&mut TranscriptionJob),
{
    let mut map = state.lock().await;
    if let Some(job) = map.get_mut(id) {
        f(job);
        job.updated_at = Utc::now();
    }
}

async fn worker_loop(
    mut rx: mpsc::Receiver<TranscriptionJob>,
    state: Arc<Mutex<HashMap<String, TranscriptionJob>>>,
    config: WhisperConfig,
    redaction_config: RedactionConfig,
) {
    let scanner = RedactionScanner::new(redaction_config);

    while let Some(mut job) = rx.recv().await {
        update_state(&state, &job.id, |j| j.status = JobStatus::Running).await;

        match attempt_with_retry(&mut job, &config, &scanner).await {
            Ok(transcript) => {
                let status = if transcript.has_warnings {
                    JobStatus::PartialFailure
                } else {
                    JobStatus::Completed
                };
                if !transcript.warnings.is_empty() {
                    warn!(
                        job_id = %job.id,
                        warnings = ?transcript.warnings,
                        "transcription completed with warnings"
                    );
                }
                info!(
                    job_id = %job.id,
                    status = ?status,
                    segments = transcript.segment_count(),
                    "transcription done"
                );
                let result = transcript;
                update_state(&state, &job.id, |j| {
                    j.status = status;
                    j.result = Some(result);
                })
                .await;
            }
            Err(e) => {
                error!(job_id = %job.id, error = %e, "transcription failed");
                update_state(&state, &job.id, |j| {
                    j.status = JobStatus::Failed;
                    j.error = Some(e.to_string());
                })
                .await;
            }
        }
    }
}

// ─── Public queue API ─────────────────────────────────────────────────────────

/// Async transcription queue.
///
/// Spawns a single background tokio task that processes jobs serially.
/// CPU-bound work runs in the tokio thread pool via
/// [`tokio::process::Command`] (the whisper subprocess).
pub struct TranscriptionQueue {
    job_tx: mpsc::Sender<TranscriptionJob>,
    state: Arc<Mutex<HashMap<String, TranscriptionJob>>>,
}

impl TranscriptionQueue {
    /// Create a queue and start the background worker.
    pub fn new(config: WhisperConfig, redaction: RedactionConfig) -> Self {
        let (job_tx, job_rx) = mpsc::channel::<TranscriptionJob>(256);
        let state: Arc<Mutex<HashMap<String, TranscriptionJob>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let state_worker = Arc::clone(&state);

        tokio::spawn(worker_loop(job_rx, state_worker, config, redaction));

        TranscriptionQueue { job_tx, state }
    }

    /// Submit an audio file for transcription. Returns the job ID.
    pub async fn submit(&self, input_path: PathBuf, max_retries: u32) -> Result<String> {
        let job = TranscriptionJob::new(input_path, max_retries);
        let id = job.id.clone();
        {
            let mut state = self.state.lock().await;
            state.insert(id.clone(), job.clone());
        }
        self.job_tx.send(job).await.map_err(|_| {
            AgentScribeError::Transcription("transcription queue is closed".to_string())
        })?;
        Ok(id)
    }

    /// Return the current snapshot of a job, or `None` if unknown.
    pub async fn get_job(&self, id: &str) -> Option<TranscriptionJob> {
        self.state.lock().await.get(id).cloned()
    }

    /// Block until the job reaches a terminal state or `timeout` elapses.
    pub async fn wait_for_job(&self, id: &str, timeout: Duration) -> Result<TranscriptionJob> {
        let deadline = tokio::time::Instant::now() + timeout;
        let mut delay = Duration::from_millis(200);

        loop {
            {
                let state = self.state.lock().await;
                if let Some(job) = state.get(id) {
                    match job.status {
                        JobStatus::Completed | JobStatus::PartialFailure | JobStatus::Failed => {
                            return Ok(job.clone())
                        }
                        _ => {}
                    }
                }
            }

            if tokio::time::Instant::now() >= deadline {
                return Err(AgentScribeError::Transcription(format!(
                    "timed out waiting for job {}",
                    id
                )));
            }

            sleep(delay).await;
            delay = (delay * 2).min(Duration::from_secs(5));
        }
    }
}

// ─── Convenience: single-file transcription ───────────────────────────────────

/// Transcribe a single audio file synchronously (for CLI use).
///
/// This is a convenience wrapper around the retry logic without the queue.
/// Use [`TranscriptionQueue`] for batch or daemon-mode workloads.
pub async fn transcribe_file(
    input_path: PathBuf,
    config: &WhisperConfig,
    redaction_config: &RedactionConfig,
) -> Result<TranscriptionResult> {
    if AudioFormat::from_path(&input_path).is_none() {
        return Err(AgentScribeError::Transcription(format!(
            "unsupported format — expected wav, mp3, or m4a: {}",
            input_path.display()
        )));
    }

    let scanner = RedactionScanner::new(redaction_config.clone());
    let mut job = TranscriptionJob::new(input_path, config.max_retries);
    attempt_with_retry(&mut job, config, &scanner).await
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_format_detection() {
        assert_eq!(
            AudioFormat::from_path(Path::new("speech.wav")),
            Some(AudioFormat::Wav)
        );
        assert_eq!(
            AudioFormat::from_path(Path::new("speech.mp3")),
            Some(AudioFormat::Mp3)
        );
        assert_eq!(
            AudioFormat::from_path(Path::new("speech.m4a")),
            Some(AudioFormat::M4a)
        );
        assert_eq!(AudioFormat::from_path(Path::new("speech.ogg")), None);
        assert_eq!(AudioFormat::from_path(Path::new("speech")), None);
    }

    #[test]
    fn test_job_id_uniqueness() {
        let j1 = TranscriptionJob::new(PathBuf::from("a.wav"), 3);
        let j2 = TranscriptionJob::new(PathBuf::from("a.wav"), 3);
        assert_ne!(j1.id, j2.id);
    }

    #[test]
    fn test_parse_whisper_cpp_output() {
        let json = serde_json::json!({
            "transcription": [
                {
                    "timestamps": {"from": "00:00:00,000", "to": "00:00:03,000"},
                    "offsets": {"from": 0, "to": 3000},
                    "text": " Hello world",
                    "tokens": [
                        {
                            "text": " Hello",
                            "timestamps": {"from": "00:00:00,000", "to": "00:00:01,500"},
                            "offsets": {"from": 0, "to": 1500},
                            "p": 0.95,
                            "id": 9906
                        },
                        {
                            "text": " world",
                            "timestamps": {"from": "00:00:01,500", "to": "00:00:03,000"},
                            "offsets": {"from": 1500, "to": 3000},
                            "p": 0.88,
                            "id": 1002
                        }
                    ]
                }
            ]
        });

        let result = parse_whisper_output(&json.to_string()).unwrap();
        assert_eq!(result.timestamp_level, TimestampLevel::Word);
        assert_eq!(result.word_timestamps.len(), 2);
        assert_eq!(result.word_timestamps[0].text, "Hello");
        assert_eq!(result.word_timestamps[0].start_ms, 0);
        assert_eq!(result.word_timestamps[0].end_ms, 1500);
        assert_eq!(result.word_timestamps[1].text, "world");
        assert!(!result.has_warnings);
    }

    #[test]
    fn test_parse_openai_whisper_output() {
        let json = serde_json::json!({
            "text": " Hello world",
            "language": "en",
            "segments": [
                {
                    "id": 0,
                    "start": 0.0,
                    "end": 3.0,
                    "text": " Hello world",
                    "words": [
                        {"word": " Hello", "start": 0.0, "end": 1.5, "probability": 0.95},
                        {"word": " world", "start": 1.5, "end": 3.0, "probability": 0.88}
                    ]
                }
            ]
        });

        let result = parse_whisper_output(&json.to_string()).unwrap();
        assert_eq!(result.timestamp_level, TimestampLevel::Word);
        assert_eq!(result.language, Some("en".to_string()));
        assert_eq!(result.word_timestamps.len(), 2);
        assert_eq!(result.word_timestamps[0].text, "Hello");
        assert_eq!(result.word_timestamps[0].start_ms, 0);
        assert_eq!(result.word_timestamps[1].end_ms, 3000);
    }

    #[test]
    fn test_parse_openai_whisper_utterance_fallback() {
        // Segments present but no word-level timestamps → utterance fallback.
        let json = serde_json::json!({
            "text": "Hello world",
            "segments": [
                {
                    "id": 0,
                    "start": 0.0,
                    "end": 3.0,
                    "text": "Hello world",
                    "words": []
                }
            ]
        });

        let result = parse_whisper_output(&json.to_string()).unwrap();
        assert_eq!(result.timestamp_level, TimestampLevel::Utterance);
        assert!(result.has_warnings);
        assert!(!result.warnings.is_empty());
        assert_eq!(result.utterance_timestamps.len(), 1);
    }

    #[test]
    fn test_parse_unrecognized_format() {
        let json = serde_json::json!({"unexpected_key": []});
        let result = parse_whisper_output(&json.to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_whisper_cpp_skips_special_tokens() {
        let json = serde_json::json!({
            "transcription": [
                {
                    "offsets": {"from": 0, "to": 1000},
                    "text": "Hi",
                    "tokens": [
                        {
                            "text": "[_BEG_]",
                            "offsets": {"from": 0, "to": 0},
                            "p": 1.0,
                            "id": 50364
                        },
                        {
                            "text": "Hi",
                            "offsets": {"from": 0, "to": 1000},
                            "p": 0.9,
                            "id": 2006
                        }
                    ]
                }
            ]
        });

        let result = parse_whisper_output(&json.to_string()).unwrap();
        assert_eq!(result.word_timestamps.len(), 1);
        assert_eq!(result.word_timestamps[0].text, "Hi");
    }

    #[test]
    fn test_result_is_empty() {
        let r = TranscriptionResult::utterance_level("   ".to_string(), vec![], None, vec![]);
        assert!(r.is_empty());
    }
}
