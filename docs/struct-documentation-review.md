# Struct-Level Documentation Review — AgentScribe

**Date:** 2026-08-13  
**Scope:** All public struct definitions across the AgentScribe codebase  
**Purpose:** Review struct documentation for clarity, consistency, and completeness

## Executive Summary

The AgentScribe codebase demonstrates **excellent documentation practices in key modules** but shows **inconsistent struct-level documentation quality** across the project. While some modules serve as documentation exemplars, others lack comprehensive field documentation and usage guidance.

**Overall Assessment:** **Partially Compliant** - Strong foundation with clear areas for improvement

---

## Documentation Excellence Awards

The following modules demonstrate exemplary documentation practices that should serve as templates for the rest of the codebase:

### 🏆 `src/event.rs` — Gold Standard
- **Module-level docs:** Comprehensive explanation of canonical event schema, core concepts, data flow, and examples
- **Struct docs:** Every struct (`Event`, `SessionManifest`, `Role`, `TokenCounts`, `SourceFileState`, `ScrapeState`) has detailed documentation
- **Field docs:** Every field is documented with purpose and usage
- **Usage guidance:** Clear explanations of when to use each type, field availability, and lifecycle
- **Examples:** Multiple code examples showing proper usage

### 🏆 `src/plugin.rs` — Strong Module Documentation
- **Module-level docs:** Excellent overview of plugin system, structure, bundled plugins, and validation
- **Struct docs:** Good documentation for core structs (`Plugin`, `PluginMeta`, `Envelope`)
- **Field docs:** Most fields documented with clear explanations
- **Examples:** TOML examples for plugin configuration

### 🏆 `src/analytics.rs` — Excellent Type Documentation
- **Module-level docs:** Clear explanation of analytics capabilities and problem type classification
- **Enum docs:** `ProblemType` enum has exceptional documentation with detailed classification logic
- **Usage guidance:** Clear examples of CLI usage and classification signals

---

## Critical Documentation Issues

### Issue 1: Missing Struct-Level Documentation

**Files Affected:** `src/config.rs`, `src/index.rs`, `src/cli.rs`, `src/search.rs`

#### Specific Problems:

**`src/config.rs` (Lines 80-436):**
```rust
// ❌ NO struct-level doc
pub struct ModelPricing {
    pub input_per_1m: f64,
    pub output_per_1m: f64,
}

// ❌ NO struct-level doc
pub struct CostConfig {
    #[serde(default)]
    pub models: HashMap<String, ModelPricing>,
}

// ❌ NO struct-level doc
pub struct ShellHookConfig {
    /// Whether to run search in a background subprocess (recommended; false = blocking)
    #[serde(default = "default_true")]
    pub background: bool,
    /// Whether to capture stderr of the failed command (fragile, not recommended)
    #[serde(default)]
    pub stderr_capture: bool,
}
```

**Expected:**
```rust
/// Model pricing configuration for cost estimation.
///
/// Defines per-1M-token costs for input and output tokens. Used by analytics
/// modules to estimate session costs and compute cost-per-success metrics.
/// Pricing data is loaded from `[cost.models]` in config.toml.
pub struct ModelPricing {
    /// Cost per million input tokens in USD
    pub input_per_1m: f64,
    /// Cost per million output tokens in USD
    pub output_per_1m: f64,
}
```

**`src/index.rs` (Lines 26-672):**
```rust
// ❌ Minimal struct-level doc
#[derive(Clone)]
#[allow(dead_code)]
pub struct IndexFields {
    // Full-text searchable + stored
    pub content: Field,
    pub summary: Field,
    // ... 20+ fields with no individual documentation
}
```

**Expected:**
```rust
/// Named field handles for the Tantivy schema.
///
/// Provides strongly-typed access to Tantivy schema fields by name. Used
/// throughout the indexing and search code to reference fields without
/// re-resolving them by string name.
///
/// # Field Categories
///
/// - **Full-text searchable:** content, summary, solution_summary, code_content
/// - **Exact match + faceted:** session_id, source_agent, project, tags, outcome
/// - **Analytics:** model, session_type, parent_session_id
/// - **Date + numeric:** timestamp, turn_count
#[derive(Clone)]
pub struct IndexFields {
    /// Primary search content field (indexed but not stored per ADR-2)
    pub content: Field,
    /// One-line session summary (indexed and stored for results display)
    pub summary: Field,
    // ... etc with field-level docs
}
```

**`src/search.rs` (Lines 178-227):**
```rust
// ❌ NO struct-level doc
#[derive(Debug, Serialize)]
pub struct SearchOutput {
    pub query: String,
    pub total_matches: usize,
    // ... other fields
}

// ❌ Minimal struct-level doc
pub struct SearchOptions {
    pub query: Option<String>,
    pub error_pattern: Option<String>,
    // ... 20+ fields with no individual documentation
}
```

---

### Issue 2: Incomplete Field Documentation

**Files Affected:** `src/analytics.rs`, `src/search.rs`, `src/config.rs`

#### Specific Problems:

**`src/analytics.rs` (Lines 217-275):**
```rust
// ❌ Fields lack individual documentation
pub struct AnalyticsOptions {
    pub agent: Option<String>,
    pub project: Option<String>,
    pub since: Option<DateTime<Utc>>,
}

// ❌ Complex struct with minimal field docs
#[derive(Debug, Clone, Serialize)]
pub struct AgentMetrics {
    pub agent: String,
    pub total_sessions: usize,
    pub success_count: usize,
    pub failure_count: usize,
    pub abandoned_count: usize,
    pub unknown_count: usize,
    pub success_rate: f64,
    pub avg_turns_success: f64,
    pub avg_turns_all: f64,
    pub avg_tokens_success: f64,
    pub specialization: HashMap<String, usize>,
    pub estimated_cost: f64,
    pub cost_per_success: f64,
}
```

**Expected:**
```rust
/// Configuration options for analytics queries.
///
/// Used to filter the dataset for specific analytics reports. When all options
/// are `None`, analytics are computed across all sessions in the index.
pub struct AnalyticsOptions {
    /// Filter to sessions from this specific agent only (e.g., "claude-code")
    pub agent: Option<String>,
    /// Filter to sessions within this project directory path
    pub project: Option<String>,
    /// Only include sessions after this timestamp (exclusive)
    pub since: Option<DateTime<Utc>>,
}

/// Per-agent analytics summary with performance metrics.
///
/// Aggregates all session data for a single agent type to compute success rates,
/// efficiency metrics, specialization patterns, and cost estimates. Used for
/// agent comparison reports and CLI output.
pub struct AgentMetrics {
    /// Agent name (e.g., "claude-code", "aider")
    pub agent: String,
    /// Total number of sessions analyzed for this agent
    pub total_sessions: usize,
    /// Number of sessions with outcome="success"
    pub success_count: usize,
    /// Number of sessions with outcome="failure"
    pub failure_count: usize,
    /// Number of sessions with outcome="abandoned"
    pub abandoned_count: usize,
    /// Number of sessions with outcome="unknown"
    pub unknown_count: usize,
    /// Success rate as percentage (0.0-100.0)
    pub success_rate: f64,
    /// Average turns per successful session
    pub avg_turns_success: f64,
    /// Average turns across all sessions
    pub avg_turns_all: f64,
    /// Average tokens per successful session
    pub avg_tokens_success: f64,
    /// Count of successful sessions by problem type (debug, feature, refactor, etc.)
    pub specialization: HashMap<String, usize>,
    /// Estimated total cost in USD for all sessions by this agent
    pub estimated_cost: f64,
    /// Estimated cost per successful session in USD
    pub cost_per_success: f64,
}
```

**`src/plugin.rs` (Lines 199-360+):**
```rust
// ❌ Large struct with minimal documentation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub paths: Vec<String>,
    pub exclude: Vec<String>,
    pub format: String,
    pub session_detection: SessionDetection,
    // ... more fields
}
```

---

### Issue 3: Inconsistent Documentation Patterns

**Files Affected:** Multiple modules

#### Problems Identified:

1. **Inconsistent field ordering:** Some structs document fields in declaration order, others group by category
2. **Inconsistent doc comment styles:** Some use `///` for all docs, others mix `///` and `//`
3. **Inconsistent examples:** Some structs have examples, most don't
4. **Inconsistent "Field Availability" sections:** Only `event.rs` and a few others document when fields are `None` vs. populated

#### Example of Inconsistency:

**`src/event.rs` (Consistent pattern):**
```rust
/// Role of the entity that produced this event.
///
/// Defines whether this is a user message, assistant response, system message, tool call,
/// or tool result. Critical for understanding conversation flow and for enrichment.
pub role: Role,
```

**`src/config.rs` (Inconsistent pattern):**
```rust
// ❌ No doc comment at all
pub data_dir: PathBuf,

// ❌ Minimal inline comment only
#[serde(default = "default_true")]  // default: true
pub background: bool,
```

---

### Issue 4: Missing Usage Guidance

**Files Affected:** Most struct definitions lack clear "When should I use this?" guidance

#### Examples of Missing Usage Guidance:

**`src/enrichment/mod.rs` (Line 36):**
```rust
// ❌ No explanation of when to use this struct
pub struct EnrichmentResult {
    pub outcome: Outcome,
    pub summary: String,
    pub solution_summary: Option<String>,
    // ...
}
```

**Expected:**
```rust
/// Result of enriching a session with intelligence.
///
/// Produced by the enrichment pipeline (`enrich_session`) after analyzing raw
/// session events. Contains detected outcomes, generated summaries, extracted
/// solutions, and other intelligence extracted from the conversation.
///
/// # When to Use
///
/// - **After scraping:** Enrichment runs automatically after sessions are scraped
/// - **For analytics:** Use this struct's fields for agent performance metrics
/// - **For search:** The `outcome` and `summary` fields are indexed for faceted search
/// - **For CLI output:** `agentscribe search` displays `summary` and `outcome` in results
pub struct EnrichmentResult {
    // ... fields
}
```

---

## Files Requiring Immediate Attention

### High Priority (User-Facing Documentation)

1. **`src/config.rs`** — All structs (13 structs) need comprehensive documentation
2. **`src/search.rs`** — `SearchOutput`, `SearchOptions`, `SearchResult` need field docs
3. **`src/index.rs`** — `IndexFields`, `IndexManager` need detailed documentation
4. **`src/cli.rs`** — Command structs need usage guidance

### Medium Priority (Internal Documentation)

5. **`src/analytics.rs`** — `AnalyticsOptions`, `AgentMetrics`, output structs need field docs
6. **`src/plugin.rs`** — `Source`, `Parser`, `Metadata` need comprehensive docs
7. **`src/enrichment/mod.rs`** — `EnrichmentResult` needs usage guidance

### Low Priority (Well-Documented)

8. **`src/event.rs`** — ✅ Exemplary documentation, use as template
9. **`src/plugin.rs`** — ✅ Good module docs, minor improvements needed
10. **`src/transcription.rs`** — ✅ Generally well documented

---

## Documentation Template

Based on the exemplary documentation in `src/event.rs`, here's a template for struct documentation:

```rust
/// [One-sentence summary of what the struct does].
///
/// [Detailed paragraph explaining the struct's purpose and role in the system].
/// Include information about:
/// - When/where this struct is used
/// - Who creates/consumes it
/// - Any important invariants or constraints
///
/// # [Optional Section Title]
///
/// [Additional details about usage patterns, lifecycle, or relationships]
///
/// # When to Use
///
/// - **Use case 1:** When X happens, use this struct for Y purpose
/// - **Use case 2:** For Z scenarios, this struct provides ...
///
/// # Field Availability
///
/// [For structs with Option fields]: Explain when fields are None vs. populated
///
/// # Examples
///
/// ```no_run
/// use agentscribe::module::StructName;
///
/// // Create a new instance (typically done by X, not users)
/// let instance = StructName::new(...);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructName {
    /// [Field purpose and usage]. Include units/ranges if applicable
    pub field_name: FieldType,
    
    /// [Field purpose]. Explain when this field is None/Some
    pub optional_field: Option<FieldType>,
}
```

---

## Recommended Action Plan

### Phase 1: Critical Structs (Week 1)
- [ ] Document all config structs (`src/config.rs`)
- [ ] Document search output structs (`src/search.rs`)
- [ ] Document index structs (`src/index.rs`)

### Phase 2: Analytics & Plugin Structs (Week 2)
- [ ] Document analytics structs (`src/analytics.rs`)
- [ ] Document plugin parser structs (`src/plugin.rs`)
- [ ] Document enrichment result structs (`src/enrichment/mod.rs`)

### Phase 3: Remaining Modules (Week 3)
- [ ] Review and document remaining structs across all modules
- [ ] Add examples to key structs
- [ ] Standardize documentation patterns

### Phase 4: Verification (Week 4)
- [ ] Run `cargo doc` and review all generated docs
- [ ] Ensure all public structs have `///` doc comments
- [ ] Verify all fields have individual doc comments
- [ ] Check for missing "When to Use" sections on complex structs

---

## Metrics for Success

### Documentation Coverage
- **Current:** ~60% of public structs have comprehensive documentation
- **Target:** 100% of public structs have comprehensive documentation

### Quality Criteria
Each struct must have:
1. ✅ Struct-level doc comment explaining purpose and usage
2. ✅ All fields documented with their purpose
3. ✅ Complex structs include "When to Use" guidance
4. ✅ Structs with Option fields explain availability rules
5. ✅ Public-facing structs include usage examples

### Validation Commands
```bash
# Generate and review documentation
cargo doc --no-deps --open

# Check for undocumented public items (requires cargo-doc)
cargo doc --no-deps 2>&1 | grep "not documented"

# Count documented vs undocumented structs
grep -r "pub struct" src/ | wc -l  # Total structs
grep -B2 "pub struct" src/ | grep "///" | wc -l  # Documented structs
```

---

## Conclusion

The AgentScribe codebase has a **strong foundation** in documentation, with `src/event.rs` serving as an exemplary model. However, **systematic gaps** exist in struct-level documentation across many modules, particularly for configuration, search, and analytics data structures.

**Priority:** Implement documentation for high-priority user-facing structs (config, search, index) first, then address internal-facing structs in subsequent phases.

**Estimated Effort:** 3-4 weeks to bring all public struct documentation to the exemplary standard set by `src/event.rs`.

**Impact:** Improved developer experience, easier onboarding for contributors, and better API discoverability for users integrating AgentScribe as a library.
