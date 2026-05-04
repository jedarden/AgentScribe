//! Per-account capacity utilization from Claude Code JSONL logs
//!
//! Computes per-account 5h and 7d rolling utilization meters matching Claude
//! Code's `/status` output. Each Claude credential directory is one account.
//!
//! Data sources (in priority order):
//! 1. Cached API response (`~/.cache/claude-usage/usage.json`) — exact, matches `/status`
//! 2. JSONL-based estimation — fallback when cache is stale or missing
//!
//! The JSONL fallback uses cost-equivalent token weighting to approximate
//! Claude's internal rate-limit accounting. It is inherently approximate
//! because the exact weighting formula is proprietary. The cached API
//! response should be preferred whenever available.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use tracing::debug;

/// Deserializes `Option<Option<T>>` so that a present-but-null JSON field
/// becomes `Some(None)` (distinguishable from an absent field which is `None`).
fn deserialize_option_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Ok(Some(Option::deserialize(deserializer)?))
}

/// Per-model 7d utilization window
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelWindow {
    pub model: String,
    pub utilization: f64,
    pub resets_at: Option<DateTime<Utc>>,
}

/// Utilization data for a single account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountCapacity {
    /// Account identifier (derived from credential dir path)
    pub account_id: String,
    /// Adapter name (always "claude" for now)
    pub adapter: String,
    /// Plan type from credentials (e.g. "max", "pro")
    pub plan_type: String,
    /// Rate limit tier from credentials (e.g. "default_claude_max_20x")
    pub rate_limit_tier: String,
    /// 5-hour rolling utilization (0–100)
    pub utilization_5h: f64,
    /// 7-day rolling utilization (0–100)
    pub utilization_7d: f64,
    /// When the 5h window resets
    pub resets_at_5h: Option<DateTime<Utc>>,
    /// When the 7d window resets
    pub resets_at_7d: Option<DateTime<Utc>>,
    /// Per-model 7d windows (sonnet, opus, etc.)
    pub model_windows_7d: Vec<ModelWindow>,
    /// Cost-equivalent tokens counted in the 5h window (from JSONL)
    pub tokens_5h: u64,
    /// Cost-equivalent tokens counted in the 7d window (from JSONL)
    pub tokens_7d: u64,
    /// Total assistant turns in 5h window
    pub turns_5h: u64,
    /// Total assistant turns in 7d window
    pub turns_7d: u64,
    /// Burn rate: cost-equivalent tokens per minute over the last hour
    pub burn_rate_per_min: f64,
    /// Forecast: minutes until 5h utilization hits 100% at current burn rate
    pub forecast_full_5h_min: Option<f64>,
    /// Forecast: minutes until 7d utilization hits 100% at current burn rate
    pub forecast_full_7d_min: Option<f64>,
    /// Source of the utilization numbers ("api_cache" or "jsonl_estimate")
    pub source: String,
    /// When this data was computed
    pub computed_at: DateTime<Utc>,
}

/// Cached API usage response written by Claude Code.
///
/// Matches the structure that `/status` reads from
/// `~/.cache/claude-usage/usage.json`.
#[derive(Debug, Deserialize)]
struct CachedUsageResponse {
    #[serde(default)]
    five_hour: Option<WindowUsage>,
    #[serde(default)]
    seven_day: Option<WindowUsage>,
    #[serde(default, deserialize_with = "deserialize_option_option")]
    seven_day_sonnet: Option<Option<WindowUsage>>,
    #[serde(default, deserialize_with = "deserialize_option_option")]
    seven_day_opus: Option<Option<WindowUsage>>,
    #[serde(default, deserialize_with = "deserialize_option_option")]
    seven_day_cowork: Option<Option<WindowUsage>>,
    #[serde(default, deserialize_with = "deserialize_option_option")]
    seven_day_omelette: Option<Option<WindowUsage>>,
}

#[derive(Debug, Deserialize, Clone)]
struct WindowUsage {
    #[serde(default)]
    utilization: f64,
    #[serde(default)]
    resets_at: Option<String>,
}

/// Claude credentials structure
#[allow(non_snake_case)]
#[derive(Debug, Deserialize)]
struct Credentials {
    #[serde(default)]
    claudeAiOauth: Option<OAuthCreds>,
}

#[allow(non_snake_case)]
#[derive(Debug, Deserialize)]
struct OAuthCreds {
    #[serde(default)]
    subscriptionType: Option<String>,
    #[serde(default)]
    rateLimitTier: Option<String>,
}

/// A single JSONL assistant turn with parsed timestamp and usage
#[derive(Debug)]
struct ParsedTurn {
    ts: DateTime<Utc>,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    #[allow(dead_code)]
    model: Option<String>,
}

impl ParsedTurn {
    /// Cost-equivalent token count for utilization estimation.
    ///
    /// Claude's rate limiting uses a cost-weighted token count where output
    /// tokens count more than input tokens (reflecting API pricing). The
    /// exact ratio is proprietary, but empirically:
    ///
    /// - `input_tokens` at full weight
    /// - `output_tokens` at ~5× weight (matching the ~5:1 output:input price ratio)
    /// - `cache_read` at ~0.1× (cache reads are discounted)
    /// - `cache_write` at ~0.25× (cache writes are partially discounted)
    fn cost_equivalent_tokens(&self) -> u64 {
        let input = self.input_tokens as f64;
        let cache_read = self.cache_read_tokens as f64;
        let cache_write = self.cache_write_tokens as f64;
        let output = self.output_tokens as f64;

        (input + cache_read * 0.10 + cache_write * 0.25 + output * 5.0) as u64
    }
}

/// Plan-specific token limits for rate-limit windows.
///
/// Only used in the JSONL fallback path; the cached API response is preferred.
struct PlanLimits {
    tokens_5h: u64,
    tokens_7d: u64,
}

fn get_plan_limits(plan_type: &str, tier: &str) -> PlanLimits {
    match (plan_type, tier) {
        ("max", t) if t.contains("20x") => PlanLimits {
            tokens_5h: 1_000_000,
            tokens_7d: 15_000_000,
        },
        ("max", t) if t.contains("10x") => PlanLimits {
            tokens_5h: 500_000,
            tokens_7d: 7_500_000,
        },
        ("max", t) if t.contains("5x") => PlanLimits {
            tokens_5h: 250_000,
            tokens_7d: 3_750_000,
        },
        ("max", _) => PlanLimits {
            tokens_5h: 100_000,
            tokens_7d: 1_500_000,
        },
        ("pro", _) => PlanLimits {
            tokens_5h: 44_000,
            tokens_7d: 660_000,
        },
        _ => PlanLimits {
            tokens_5h: 44_000,
            tokens_7d: 660_000,
        },
    }
}

/// Resolved filesystem paths for a single Claude account
#[derive(Debug, Clone)]
struct AccountPaths {
    credential_dir: PathBuf,
    projects_dir: PathBuf,
    cached_usage_path: PathBuf,
}

/// Configuration for the capacity meter
#[derive(Debug, Clone)]
pub struct CapacityMeterConfig {
    /// Claude config directories to scan (each = one account).
    /// Defaults to `~/.claude` plus any auto-discovered `~/.claude-*` dirs.
    pub account_dirs: Vec<PathBuf>,
    /// Maximum age of cached usage.json before treating it as stale (seconds)
    pub cache_max_age_secs: u64,
    /// Override base cache directory (defaults to `~/.cache`). Useful in tests.
    pub cache_base_dir: Option<PathBuf>,
}

impl Default for CapacityMeterConfig {
    fn default() -> Self {
        let home = home_dir();
        let mut account_dirs = vec![home.join(".claude")];

        // Auto-discover additional Claude config dirs (~/.claude-*)
        if let Ok(entries) = fs::read_dir(&home) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with(".claude-")
                    && entry.path().join(".credentials.json").exists()
                {
                    account_dirs.push(entry.path());
                }
            }
        }

        Self {
            account_dirs,
            cache_max_age_secs: 600,
            cache_base_dir: None,
        }
    }
}

impl CapacityMeterConfig {
    fn resolve_account_paths(&self, account_dir: &Path) -> AccountPaths {
        let cache_base = self
            .cache_base_dir
            .as_ref()
            .cloned()
            .unwrap_or_else(cache_dir);

        let dir_name = account_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        let cached_usage_path = if dir_name == ".claude" {
            cache_base.join("claude-usage").join("usage.json")
        } else {
            cache_base
                .join("claude-usage")
                .join(format!("{}-usage.json", dir_name))
        };

        AccountPaths {
            credential_dir: account_dir.to_path_buf(),
            projects_dir: account_dir.join("projects"),
            cached_usage_path,
        }
    }
}

/// Capacity meter: computes per-account utilization from Claude Code JSONL logs
pub struct CapacityMeter {
    config: CapacityMeterConfig,
}

impl CapacityMeter {
    pub fn new(config: CapacityMeterConfig) -> Self {
        Self { config }
    }

    /// Compute capacity for all configured accounts
    pub fn compute(&self) -> Vec<AccountCapacity> {
        let mut accounts = Vec::new();

        for account_dir in &self.config.account_dirs {
            let paths = self.config.resolve_account_paths(account_dir);
            match self.compute_account(&paths) {
                Ok(cap) => accounts.push(cap),
                Err(e) => {
                    tracing::warn!(
                        "Failed to compute capacity for {}: {}",
                        paths.credential_dir.display(),
                        e
                    );
                }
            }
        }

        accounts
    }

    fn compute_account(&self, paths: &AccountPaths) -> Result<AccountCapacity> {
        let account_id = Self::derive_account_id(&paths.credential_dir);
        let now = Utc::now();

        let (plan_type, rate_limit_tier) = Self::read_credentials(&paths.credential_dir)?;

        // Try cached API response first (exact numbers matching /status)
        let cached =
            Self::read_cached_usage(&paths.cached_usage_path, self.config.cache_max_age_secs);

        // Parse JSONL for token counts (used for burn rate and as fallback)
        let turns = Self::parse_all_jsonl(&paths.projects_dir)?;

        // Compute rolling windows
        let cutoff_5h = now - Duration::hours(5);
        let cutoff_7d = now - Duration::days(7);
        let cutoff_1h = now - Duration::hours(1);

        let mut tokens_5h: u64 = 0;
        let mut tokens_7d: u64 = 0;
        let mut turns_5h: u64 = 0;
        let mut turns_7d: u64 = 0;
        let mut tokens_last_hour: u64 = 0;

        for turn in &turns {
            let weighted = turn.cost_equivalent_tokens();
            if turn.ts > cutoff_5h {
                tokens_5h += weighted;
                turns_5h += 1;
            }
            if turn.ts > cutoff_7d {
                tokens_7d += weighted;
                turns_7d += 1;
            }
            if turn.ts > cutoff_1h {
                tokens_last_hour += weighted;
            }
        }

        let burn_rate_per_min = if tokens_last_hour > 0 {
            tokens_last_hour as f64 / 60.0
        } else {
            0.0
        };

        // Determine utilization: prefer cached API, fall back to JSONL estimate
        let (util_5h, util_7d, resets_5h, resets_7d, model_windows, source) =
            if let Some(ref c) = cached {
                let u5 = c.five_hour.as_ref().map(|w| w.utilization).unwrap_or(0.0);
                let u7 = c.seven_day.as_ref().map(|w| w.utilization).unwrap_or(0.0);
                let r5 = parse_resets_at(c.five_hour.as_ref());
                let r7 = parse_resets_at(c.seven_day.as_ref());

                let mut windows = Vec::new();
                if let Some(Some(w)) = &c.seven_day_sonnet {
                    windows.push(ModelWindow {
                        model: "sonnet".to_string(),
                        utilization: w.utilization,
                        resets_at: parse_resets_at(Some(w)),
                    });
                }
                if let Some(Some(w)) = &c.seven_day_opus {
                    windows.push(ModelWindow {
                        model: "opus".to_string(),
                        utilization: w.utilization,
                        resets_at: parse_resets_at(Some(w)),
                    });
                }
                if let Some(Some(w)) = &c.seven_day_cowork {
                    windows.push(ModelWindow {
                        model: "cowork".to_string(),
                        utilization: w.utilization,
                        resets_at: parse_resets_at(Some(w)),
                    });
                }
                if let Some(Some(w)) = &c.seven_day_omelette {
                    windows.push(ModelWindow {
                        model: "omelette".to_string(),
                        utilization: w.utilization,
                        resets_at: parse_resets_at(Some(w)),
                    });
                }

                (u5, u7, r5, r7, windows, "api_cache".to_string())
            } else {
                let limits = get_plan_limits(&plan_type, &rate_limit_tier);
                let u5 = if limits.tokens_5h > 0 {
                    (tokens_5h as f64 / limits.tokens_5h as f64 * 100.0).min(100.0)
                } else {
                    0.0
                };
                let u7 = if limits.tokens_7d > 0 {
                    (tokens_7d as f64 / limits.tokens_7d as f64 * 100.0).min(100.0)
                } else {
                    0.0
                };
                (u5, u7, None, None, Vec::new(), "jsonl_estimate".to_string())
            };

        let limits = get_plan_limits(&plan_type, &rate_limit_tier);
        let forecast_full_5h = if burn_rate_per_min > 0.0 && util_5h < 100.0 {
            let remaining = limits.tokens_5h as f64 * (1.0 - util_5h / 100.0);
            Some(remaining / burn_rate_per_min)
        } else if util_5h >= 100.0 {
            Some(0.0)
        } else {
            None
        };

        let forecast_full_7d = if burn_rate_per_min > 0.0 && util_7d < 100.0 {
            let remaining = limits.tokens_7d as f64 * (1.0 - util_7d / 100.0);
            Some(remaining / burn_rate_per_min)
        } else if util_7d >= 100.0 {
            Some(0.0)
        } else {
            None
        };

        Ok(AccountCapacity {
            account_id,
            adapter: "claude".to_string(),
            plan_type,
            rate_limit_tier,
            utilization_5h: util_5h,
            utilization_7d: util_7d,
            resets_at_5h: resets_5h,
            resets_at_7d: resets_7d,
            model_windows_7d: model_windows,
            tokens_5h,
            tokens_7d,
            turns_5h,
            turns_7d,
            burn_rate_per_min,
            forecast_full_5h_min: forecast_full_5h,
            forecast_full_7d_min: forecast_full_7d,
            source,
            computed_at: now,
        })
    }

    fn derive_account_id(cred_dir: &Path) -> String {
        let dir_name = cred_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        if dir_name == ".claude" {
            "claude-default".to_string()
        } else {
            dir_name.to_string()
        }
    }

    fn read_credentials(cred_dir: &Path) -> Result<(String, String)> {
        let creds_path = cred_dir.join(".credentials.json");
        if !creds_path.exists() {
            return Ok(("unknown".to_string(), "unknown".to_string()));
        }

        let content = fs::read_to_string(&creds_path)?;
        let creds: Credentials = serde_json::from_str(&content)?;

        let oauth = creds.claudeAiOauth.unwrap_or(OAuthCreds {
            subscriptionType: None,
            rateLimitTier: None,
        });

        Ok((
            oauth
                .subscriptionType
                .unwrap_or_else(|| "unknown".to_string()),
            oauth.rateLimitTier.unwrap_or_else(|| "unknown".to_string()),
        ))
    }

    fn read_cached_usage(path: &Path, max_age_secs: u64) -> Option<CachedUsageResponse> {
        if !path.exists() {
            debug!("No cached usage at {}", path.display());
            return None;
        }

        let metadata = fs::metadata(path).ok()?;
        if let Ok(modified) = metadata.modified() {
            let modified_dt: DateTime<Utc> = modified.into();
            let age = Utc::now() - modified_dt;
            if age > Duration::seconds(max_age_secs as i64) {
                debug!(
                    "Cached usage data is {}s old (max {}s), ignoring",
                    age.num_seconds(),
                    max_age_secs
                );
                return None;
            }
        }

        let content = fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    fn parse_all_jsonl(projects_dir: &Path) -> Result<Vec<ParsedTurn>> {
        if !projects_dir.exists() {
            return Ok(Vec::new());
        }

        let mut turns = Vec::new();
        Self::scan_jsonl_recursive(projects_dir, &mut turns)?;

        debug!("Parsed {} assistant turns from JSONL files", turns.len());
        Ok(turns)
    }

    fn scan_jsonl_recursive(dir: &Path, turns: &mut Vec<ParsedTurn>) -> Result<()> {
        if !dir.exists() {
            return Ok(());
        }

        let entries = fs::read_dir(dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                // Skip subagents directories — they share token budgets with the parent session
                if path.file_name().map(|n| n == "subagents").unwrap_or(false) {
                    continue;
                }
                Self::scan_jsonl_recursive(&path, turns)?;
            } else if path.extension().map(|e| e == "jsonl").unwrap_or(false) {
                if let Err(e) = Self::parse_jsonl_file(&path, turns) {
                    debug!("Error parsing {}: {}", path.display(), e);
                }
            }
        }

        Ok(())
    }

    fn parse_jsonl_file(path: &Path, turns: &mut Vec<ParsedTurn>) -> Result<()> {
        let file = fs::File::open(path)?;
        let reader = BufReader::new(file);

        // Deduplicate by message ID within this file to avoid counting resumed
        // sessions twice (the same event may be re-written at the top of a
        // continued JSONL file).
        let mut seen_message_ids: HashMap<String, bool> = HashMap::new();

        for line in reader.lines() {
            let line = line?;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // Fast pre-filter: skip lines that can't be assistant events
            if !line.contains("\"type\":\"assistant\"") {
                continue;
            }

            let entry: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            if entry.get("type").and_then(|v| v.as_str()) != Some("assistant") {
                continue;
            }

            let ts_str = match entry.get("timestamp").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => continue,
            };
            let ts: DateTime<Utc> = match ts_str.parse() {
                Ok(t) => t,
                Err(_) => continue,
            };

            let message = match entry.get("message") {
                Some(m) => m,
                None => continue,
            };

            // Deduplicate: same message ID = same response, don't double-count
            if let Some(msg_id) = message.get("id").and_then(|v| v.as_str()) {
                if seen_message_ids.contains_key(msg_id) {
                    continue;
                }
                seen_message_ids.insert(msg_id.to_string(), true);
            }

            // Skip synthetic tool-use scaffolding injected by Claude Code
            let model = message.get("model").and_then(|v| v.as_str()).unwrap_or("");
            if model == "<synthetic>" {
                continue;
            }

            let usage = match message.get("usage") {
                Some(u) => u,
                None => continue,
            };

            let input_tokens = usage
                .get("input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let output_tokens = usage
                .get("output_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cache_read = usage
                .get("cache_read_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cache_write = usage
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            // Skip zero-token events (can appear at session open/close)
            if input_tokens == 0 && output_tokens == 0 && cache_read == 0 && cache_write == 0 {
                continue;
            }

            turns.push(ParsedTurn {
                ts,
                input_tokens,
                output_tokens,
                cache_read_tokens: cache_read,
                cache_write_tokens: cache_write,
                model: if model.is_empty() {
                    None
                } else {
                    Some(model.to_string())
                },
            });
        }

        Ok(())
    }
}

fn parse_resets_at(window: Option<&WindowUsage>) -> Option<DateTime<Utc>> {
    window
        .and_then(|w| w.resets_at.as_deref())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn cache_dir() -> PathBuf {
    std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".cache"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn make_assistant_jsonl(
        timestamp: &str,
        input: u64,
        output: u64,
        cache_read: u64,
        cache_write: u64,
        model: &str,
    ) -> String {
        format!(
            concat!(
                r#"{{"parentUuid":"p1","isSidechain":false,"type":"assistant","uuid":"u1","timestamp":"{}","#,
                r#""userType":"external","entrypoint":"sdk-cli","cwd":"/home/test","sessionId":"s1","version":"2.1.117","#,
                r#""gitBranch":"main","message":{{"model":"{}","id":"msg_{}","type":"message","role":"assistant","#,
                r#""content":[],"stop_reason":"end_turn","stop_sequence":null,"usage":{{"input_tokens":{},"#,
                r#""cache_creation_input_tokens":{},"cache_read_input_tokens":{},"output_tokens":{}}}}}}}"#,
            ),
            timestamp, model, timestamp, input, cache_write, cache_read, output
        )
    }

    #[test]
    fn test_parse_jsonl_file_basic() {
        let dir = TempDir::new().unwrap();
        let jsonl_path = dir.path().join("test.jsonl");

        let mut f = fs::File::create(&jsonl_path).unwrap();
        writeln!(
            f,
            "{}",
            make_assistant_jsonl(
                "2026-04-22T20:00:00Z",
                100,
                50,
                200,
                10,
                "claude-sonnet-4-6"
            )
        )
        .unwrap();
        writeln!(f, r#"{{"type":"user","timestamp":"2026-04-22T20:00:01Z"}}"#).unwrap();
        writeln!(
            f,
            "{}",
            make_assistant_jsonl("2026-04-22T20:01:00Z", 200, 100, 0, 0, "claude-opus-4-7")
        )
        .unwrap();
        // Synthetic — should be skipped
        writeln!(
            f,
            "{}",
            make_assistant_jsonl("2026-04-22T20:02:00Z", 0, 0, 0, 0, "<synthetic>")
        )
        .unwrap();
        // All-zero — should be skipped
        writeln!(
            f,
            "{}",
            make_assistant_jsonl("2026-04-22T20:03:00Z", 0, 0, 0, 0, "claude-sonnet-4-6")
        )
        .unwrap();

        let mut turns = Vec::new();
        CapacityMeter::parse_jsonl_file(&jsonl_path, &mut turns).unwrap();

        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].input_tokens, 100);
        assert_eq!(turns[0].output_tokens, 50);
        assert_eq!(turns[0].cache_read_tokens, 200);
        assert_eq!(turns[0].cache_write_tokens, 10);
        assert_eq!(turns[0].model.as_deref(), Some("claude-sonnet-4-6"));
        assert_eq!(turns[1].input_tokens, 200);
        assert_eq!(turns[1].output_tokens, 100);
        assert_eq!(turns[1].model.as_deref(), Some("claude-opus-4-7"));
    }

    #[test]
    fn test_cost_equivalent_tokens() {
        let turn = ParsedTurn {
            ts: Utc::now(),
            input_tokens: 1000,
            output_tokens: 300,
            cache_read_tokens: 5000,
            cache_write_tokens: 500,
            model: None,
        };
        let weighted = turn.cost_equivalent_tokens();
        // Expected: 1000 + 5000*0.1 + 500*0.25 + 300*5.0
        // = 1000 + 500 + 125 + 1500 = 3125
        assert!(
            weighted > 2500 && weighted < 3500,
            "cost equivalent = {}",
            weighted
        );
    }

    #[test]
    fn test_rolling_windows() {
        let dir = TempDir::new().unwrap();
        let jsonl_path = dir.path().join("test.jsonl");

        let now = Utc::now();
        let mut f = fs::File::create(&jsonl_path).unwrap();

        let ts_3h = (now - Duration::hours(3)).to_rfc3339();
        writeln!(
            f,
            "{}",
            make_assistant_jsonl(&ts_3h, 1000, 100, 0, 0, "claude-sonnet-4-6")
        )
        .unwrap();

        let ts_6h = (now - Duration::hours(6)).to_rfc3339();
        writeln!(
            f,
            "{}",
            make_assistant_jsonl(&ts_6h, 2000, 200, 0, 0, "claude-sonnet-4-6")
        )
        .unwrap();

        let ts_8d = (now - Duration::days(8)).to_rfc3339();
        writeln!(
            f,
            "{}",
            make_assistant_jsonl(&ts_8d, 5000, 500, 0, 0, "claude-sonnet-4-6")
        )
        .unwrap();

        let turns = CapacityMeter::parse_all_jsonl(dir.path()).unwrap();
        assert_eq!(turns.len(), 3);

        let cutoff_5h = now - Duration::hours(5);
        let cutoff_7d = now - Duration::days(7);
        let in_5h: Vec<_> = turns.iter().filter(|t| t.ts > cutoff_5h).collect();
        let in_7d: Vec<_> = turns.iter().filter(|t| t.ts > cutoff_7d).collect();

        assert_eq!(in_5h.len(), 1, "Only 3h-ago should be in 5h window");
        assert_eq!(in_7d.len(), 2, "3h-ago and 6h-ago should be in 7d window");
    }

    #[test]
    fn test_5h_window_boundary() {
        let dir = TempDir::new().unwrap();
        let jsonl_path = dir.path().join("test.jsonl");
        let now = Utc::now();
        let mut f = fs::File::create(&jsonl_path).unwrap();

        // Exactly 5h ago — outside (> not >=)
        let ts_5h = (now - Duration::hours(5)).to_rfc3339();
        writeln!(
            f,
            "{}",
            make_assistant_jsonl(&ts_5h, 1000, 100, 0, 0, "claude-sonnet-4-6")
        )
        .unwrap();

        // Just inside
        let ts_4h59 = (now - Duration::hours(4) - Duration::minutes(59)).to_rfc3339();
        writeln!(
            f,
            "{}",
            make_assistant_jsonl(&ts_4h59, 1000, 100, 0, 0, "claude-sonnet-4-6")
        )
        .unwrap();

        let turns = CapacityMeter::parse_all_jsonl(dir.path()).unwrap();
        let cutoff_5h = now - Duration::hours(5);
        let in_5h: Vec<_> = turns.iter().filter(|t| t.ts > cutoff_5h).collect();

        assert_eq!(in_5h.len(), 1, "Exactly 5h-ago excluded, 4h59 included");
    }

    #[test]
    fn test_7d_window_boundary() {
        let dir = TempDir::new().unwrap();
        let jsonl_path = dir.path().join("test.jsonl");
        let now = Utc::now();
        let mut f = fs::File::create(&jsonl_path).unwrap();

        let ts_7d = (now - Duration::days(7)).to_rfc3339();
        writeln!(
            f,
            "{}",
            make_assistant_jsonl(&ts_7d, 1000, 100, 0, 0, "claude-sonnet-4-6")
        )
        .unwrap();

        let ts_6d23h = (now - Duration::days(6) - Duration::hours(23)).to_rfc3339();
        writeln!(
            f,
            "{}",
            make_assistant_jsonl(&ts_6d23h, 1000, 100, 0, 0, "claude-sonnet-4-6")
        )
        .unwrap();

        let turns = CapacityMeter::parse_all_jsonl(dir.path()).unwrap();
        let cutoff_7d = now - Duration::days(7);
        let in_7d: Vec<_> = turns.iter().filter(|t| t.ts > cutoff_7d).collect();

        assert_eq!(in_7d.len(), 1, "Exactly 7d-ago excluded, 6d23h included");
    }

    #[test]
    fn test_plan_limits() {
        let max_20x = get_plan_limits("max", "default_claude_max_20x");
        assert!(max_20x.tokens_5h > 0);
        assert!(max_20x.tokens_7d > max_20x.tokens_5h);

        let pro = get_plan_limits("pro", "default");
        assert!(pro.tokens_5h > 0);
        assert!(pro.tokens_5h < max_20x.tokens_5h);
    }

    #[test]
    fn test_derive_account_id() {
        assert_eq!(
            CapacityMeter::derive_account_id(&PathBuf::from("/home/user/.claude")),
            "claude-default"
        );
        assert_eq!(
            CapacityMeter::derive_account_id(&PathBuf::from("/home/user/.claude-work")),
            ".claude-work"
        );
    }

    #[test]
    fn test_deduplicate_by_message_id() {
        let dir = TempDir::new().unwrap();
        let jsonl_path = dir.path().join("test.jsonl");

        let mut f = fs::File::create(&jsonl_path).unwrap();
        let entry =
            make_assistant_jsonl("2026-04-22T20:00:00Z", 100, 50, 0, 0, "claude-sonnet-4-6");
        writeln!(f, "{}", entry).unwrap();
        writeln!(f, "{}", entry).unwrap(); // duplicate

        let mut turns = Vec::new();
        CapacityMeter::parse_jsonl_file(&jsonl_path, &mut turns).unwrap();
        assert_eq!(
            turns.len(),
            1,
            "Duplicate message IDs should be deduplicated"
        );
    }

    #[test]
    fn test_cached_usage_parse() {
        let cached_json = r#"{"five_hour":{"utilization":24.0,"resets_at":"2026-04-23T02:00:00.803167+00:00"},"seven_day":{"utilization":94.0,"resets_at":"2026-04-23T19:00:00.803185+00:00"},"seven_day_sonnet":{"utilization":82.0,"resets_at":"2026-04-23T19:00:00.803192+00:00"}}"#;
        let parsed: CachedUsageResponse = serde_json::from_str(cached_json).unwrap();
        assert_eq!(parsed.five_hour.unwrap().utilization, 24.0);
        assert_eq!(parsed.seven_day.unwrap().utilization, 94.0);
        let sonnet = parsed.seven_day_sonnet.unwrap().unwrap();
        assert_eq!(sonnet.utilization, 82.0);
    }

    #[test]
    fn test_cached_usage_null_model_window() {
        let cached_json = r#"{"five_hour":{"utilization":10.0},"seven_day":{"utilization":50.0},"seven_day_opus":null,"seven_day_sonnet":{"utilization":40.0}}"#;
        let parsed: CachedUsageResponse = serde_json::from_str(cached_json).unwrap();
        assert!(parsed.seven_day_opus.unwrap().is_none());
        let sonnet = parsed.seven_day_sonnet.unwrap().unwrap();
        assert_eq!(sonnet.utilization, 40.0);
    }

    #[test]
    fn test_full_compute_with_cache() {
        let dir = TempDir::new().unwrap();

        let cache_dir_path = dir.path().join("cache").join("claude-usage");
        fs::create_dir_all(&cache_dir_path).unwrap();
        let cached = r#"{"five_hour":{"utilization":42.5,"resets_at":"2026-04-23T02:00:00Z"},"seven_day":{"utilization":88.0,"resets_at":"2026-04-23T19:00:00Z"},"seven_day_sonnet":{"utilization":75.0,"resets_at":"2026-04-23T19:00:00Z"}}"#;
        fs::write(cache_dir_path.join("usage.json"), cached).unwrap();

        let claude_dir = dir.path().join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::write(
            claude_dir.join(".credentials.json"),
            r#"{"claudeAiOauth":{"subscriptionType":"max","rateLimitTier":"default_claude_max_20x"}}"#,
        )
        .unwrap();

        let config = CapacityMeterConfig {
            account_dirs: vec![claude_dir],
            cache_max_age_secs: 600,
            cache_base_dir: Some(dir.path().join("cache")),
        };

        let meter = CapacityMeter::new(config);
        let accounts = meter.compute();
        assert_eq!(accounts.len(), 1);

        let acct = &accounts[0];
        assert_eq!(acct.source, "api_cache");
        assert!((acct.utilization_5h - 42.5).abs() < 0.01);
        assert!((acct.utilization_7d - 88.0).abs() < 0.01);
        assert_eq!(acct.model_windows_7d.len(), 1);
        assert_eq!(acct.model_windows_7d[0].model, "sonnet");
        assert!((acct.model_windows_7d[0].utilization - 75.0).abs() < 0.01);
    }

    #[test]
    fn test_full_compute_jsonl_fallback() {
        let dir = TempDir::new().unwrap();

        let claude_dir = dir.path().join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::write(
            claude_dir.join(".credentials.json"),
            r#"{"claudeAiOauth":{"subscriptionType":"max","rateLimitTier":"default_claude_max_20x"}}"#,
        )
        .unwrap();

        let projects_dir = claude_dir.join("projects");
        fs::create_dir_all(&projects_dir).unwrap();
        let now = Utc::now();
        let ts = (now - Duration::hours(1)).to_rfc3339();
        let mut f = fs::File::create(projects_dir.join("test.jsonl")).unwrap();
        writeln!(
            f,
            "{}",
            make_assistant_jsonl(&ts, 50000, 5000, 0, 0, "claude-sonnet-4-6")
        )
        .unwrap();

        let config = CapacityMeterConfig {
            account_dirs: vec![claude_dir],
            cache_max_age_secs: 600,
            cache_base_dir: Some(dir.path().join("cache")),
        };

        let meter = CapacityMeter::new(config);
        let accounts = meter.compute();
        assert_eq!(accounts.len(), 1);

        let acct = &accounts[0];
        assert_eq!(acct.source, "jsonl_estimate");
        assert!(acct.utilization_5h > 0.0);
        assert!(acct.utilization_7d > 0.0);
        assert_eq!(acct.turns_5h, 1);
        assert_eq!(acct.turns_7d, 1);
    }

    #[test]
    fn test_multi_account_separate_dirs() {
        let dir = TempDir::new().unwrap();

        // Account 1: Max 20x
        let claude1 = dir.path().join(".claude");
        fs::create_dir_all(&claude1).unwrap();
        fs::write(
            claude1.join(".credentials.json"),
            r#"{"claudeAiOauth":{"subscriptionType":"max","rateLimitTier":"default_claude_max_20x"}}"#,
        )
        .unwrap();
        let projects1 = claude1.join("projects");
        fs::create_dir_all(&projects1).unwrap();
        let now = Utc::now();
        let ts1 = (now - Duration::hours(1)).to_rfc3339();
        let mut f1 = fs::File::create(projects1.join("account1.jsonl")).unwrap();
        writeln!(
            f1,
            "{}",
            make_assistant_jsonl(&ts1, 100000, 20000, 0, 0, "claude-sonnet-4-6")
        )
        .unwrap();

        // Account 2: Max 10x
        let claude2 = dir.path().join(".claude-work");
        fs::create_dir_all(&claude2).unwrap();
        fs::write(
            claude2.join(".credentials.json"),
            r#"{"claudeAiOauth":{"subscriptionType":"max","rateLimitTier":"default_claude_max_10x"}}"#,
        )
        .unwrap();
        let projects2 = claude2.join("projects");
        fs::create_dir_all(&projects2).unwrap();
        let ts2 = (now - Duration::hours(2)).to_rfc3339();
        let mut f2 = fs::File::create(projects2.join("account2.jsonl")).unwrap();
        writeln!(
            f2,
            "{}",
            make_assistant_jsonl(&ts2, 10000, 1000, 0, 0, "claude-sonnet-4-6")
        )
        .unwrap();

        let config = CapacityMeterConfig {
            account_dirs: vec![claude1, claude2],
            cache_max_age_secs: 600,
            cache_base_dir: Some(dir.path().join("cache")),
        };

        let meter = CapacityMeter::new(config);
        let accounts = meter.compute();
        assert_eq!(accounts.len(), 2, "Should have two accounts");

        let acct1 = accounts
            .iter()
            .find(|a| a.account_id == "claude-default")
            .expect("account 1");
        let acct2 = accounts
            .iter()
            .find(|a| a.account_id == ".claude-work")
            .expect("account 2");

        assert_eq!(acct1.source, "jsonl_estimate");
        assert_eq!(acct2.source, "jsonl_estimate");

        // Account 1 has more usage
        assert!(acct1.tokens_5h > acct2.tokens_5h);
        assert_eq!(acct1.plan_type, "max");
        assert_eq!(acct2.plan_type, "max");
        assert!(acct1.rate_limit_tier.contains("20x"));
        assert!(acct2.rate_limit_tier.contains("10x"));
    }

    #[test]
    fn test_cached_api_priority_over_jsonl() {
        let dir = TempDir::new().unwrap();

        let cache_dir_path = dir.path().join("cache").join("claude-usage");
        fs::create_dir_all(&cache_dir_path).unwrap();
        fs::write(
            cache_dir_path.join("usage.json"),
            r#"{"five_hour":{"utilization":47.0,"resets_at":"2026-04-23T02:00:00Z"},"seven_day":{"utilization":97.0,"resets_at":"2026-04-23T19:00:00Z"}}"#,
        )
        .unwrap();

        let claude_dir = dir.path().join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::write(
            claude_dir.join(".credentials.json"),
            r#"{"claudeAiOauth":{"subscriptionType":"max","rateLimitTier":"default_claude_max_20x"}}"#,
        )
        .unwrap();

        // JSONL with very different usage — should NOT override cached API values
        let projects_dir = claude_dir.join("projects");
        fs::create_dir_all(&projects_dir).unwrap();
        let now = Utc::now();
        let ts = (now - Duration::hours(1)).to_rfc3339();
        let mut f = fs::File::create(projects_dir.join("test.jsonl")).unwrap();
        writeln!(
            f,
            "{}",
            make_assistant_jsonl(&ts, 500000, 50000, 0, 0, "claude-sonnet-4-6")
        )
        .unwrap();

        let config = CapacityMeterConfig {
            account_dirs: vec![claude_dir],
            cache_max_age_secs: 600,
            cache_base_dir: Some(dir.path().join("cache")),
        };

        let meter = CapacityMeter::new(config);
        let accounts = meter.compute();
        let acct = &accounts[0];

        assert_eq!(acct.source, "api_cache");
        assert!((acct.utilization_5h - 47.0).abs() < 0.01);
        assert!((acct.utilization_7d - 97.0).abs() < 0.01);
    }

    #[test]
    fn test_burn_rate_and_forecast() {
        let dir = TempDir::new().unwrap();

        let claude_dir = dir.path().join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::write(
            claude_dir.join(".credentials.json"),
            r#"{"claudeAiOauth":{"subscriptionType":"max","rateLimitTier":"default_claude_max_20x"}}"#,
        )
        .unwrap();

        let projects_dir = claude_dir.join("projects");
        fs::create_dir_all(&projects_dir).unwrap();
        let now = Utc::now();

        // Add some usage in the last hour to establish a burn rate
        let mut f = fs::File::create(projects_dir.join("test.jsonl")).unwrap();

        // 3 assistant turns in the last hour, each using ~100k weighted tokens
        for i in 0..3 {
            let ts = (now - Duration::minutes(20 * (i + 1))).to_rfc3339();
            writeln!(
                f,
                "{}",
                make_assistant_jsonl(&ts, 20000, 10000, 0, 0, "claude-sonnet-4-6")
            )
            .unwrap();
        }

        let config = CapacityMeterConfig {
            account_dirs: vec![claude_dir],
            cache_max_age_secs: 600,
            cache_base_dir: Some(dir.path().join("cache")),
        };

        let meter = CapacityMeter::new(config);
        let accounts = meter.compute();
        let acct = &accounts[0];

        // Should have positive burn rate from JSONL usage
        assert!(
            acct.burn_rate_per_min > 0.0,
            "burn_rate_per_min should be positive: {}",
            acct.burn_rate_per_min
        );

        // With positive utilization and burn rate, forecast should be Some
        assert!(
            acct.forecast_full_5h_min.is_some() || acct.utilization_5h >= 100.0,
            "forecast_full_5h_min should be Some when utilization < 100%"
        );

        // Verify forecast is reasonable (should be > 0 if utilization < 100%)
        if let Some(forecast) = acct.forecast_full_5h_min {
            assert!(
                forecast > 0.0 || acct.utilization_5h >= 100.0,
                "forecast should be positive unless at 100% utilization"
            );
        }
    }

    #[test]
    fn test_stale_cache_falls_back_to_jsonl() {
        let dir = TempDir::new().unwrap();

        let cache_dir_path = dir.path().join("cache").join("claude-usage");
        fs::create_dir_all(&cache_dir_path).unwrap();

        // Create a cached file but make it old (simulate staleness)
        let cached_path = cache_dir_path.join("usage.json");
        fs::write(
            &cached_path,
            r#"{"five_hour":{"utilization":50.0},"seven_day":{"utilization":75.0}}"#,
        )
        .unwrap();

        // Set file modification time to > 10 minutes ago
        let old_time = std::time::SystemTime::now() - std::time::Duration::from_secs(700);
        filetime::set_file_mtime(&cached_path, old_time.into()).unwrap();

        let claude_dir = dir.path().join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::write(
            claude_dir.join(".credentials.json"),
            r#"{"claudeAiOauth":{"subscriptionType":"max","rateLimitTier":"default_claude_max_20x"}}"#,
        )
        .unwrap();

        // Add JSONL data to ensure fallback works
        let projects_dir = claude_dir.join("projects");
        fs::create_dir_all(&projects_dir).unwrap();
        let now = Utc::now();
        let ts = (now - Duration::hours(1)).to_rfc3339();
        let mut f = fs::File::create(projects_dir.join("test.jsonl")).unwrap();
        writeln!(
            f,
            "{}",
            make_assistant_jsonl(&ts, 50000, 5000, 0, 0, "claude-sonnet-4-6")
        )
        .unwrap();

        let config = CapacityMeterConfig {
            account_dirs: vec![claude_dir],
            cache_max_age_secs: 600, // 10 minutes
            cache_base_dir: Some(dir.path().join("cache")),
        };

        let meter = CapacityMeter::new(config);
        let accounts = meter.compute();
        let acct = &accounts[0];

        // Should use JSONL estimate since cache is stale
        assert_eq!(acct.source, "jsonl_estimate");
    }
}
