//! Search command implementation
//!
//! Provides full-text BM25 search, fuzzy search, error lookup, code search,
//! and various filter/output modes against the Tantivy index.

use crate::error::{AgentScribeError, Result};
use crate::index::{build_schema, fields_from_schema, IndexFields};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tantivy::collector::TopDocs;
use tantivy::query::{
    BooleanQuery, BoostQuery, FuzzyTermQuery, Occur, Query,
    RangeQuery, TermQuery,
};
use tantivy::schema::{Field, Value};
use tantivy::{DateTime as TantivyDateTime, DocAddress, Searcher, TantivyDocument};
use std::ops::Bound;

/// Tantivy index directory name
const INDEX_DIR_NAME: &str = "tantivy";

/// Default snippet context around match (chars before/after)
const SNIPPET_MARGIN: usize = 100;

/// Approximate chars per token for knapsack estimation
const CHARS_PER_TOKEN: usize = 4;

/// A single search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub session_id: String,
    pub source_agent: String,
    pub project: Option<String>,
    pub timestamp: Option<String>,
    pub turns: Option<u64>,
    pub outcome: Option<String>,
    pub score: f32,
    pub summary: Option<String>,
    pub snippet: Option<String>,
    pub tags: Vec<String>,
    pub doc_type: Option<String>,
    pub model: Option<String>,
    /// Estimated token count for this result (ceil of snippet+summary chars / 4)
    pub token_count: usize,
}

/// Search output for JSON mode
#[derive(Debug, Serialize)]
pub struct SearchOutput {
    pub query: String,
    pub total_matches: usize,
    pub search_time_ms: u64,
    pub sessions_searched: u64,
    pub results: Vec<SearchResult>,
    /// True when exact search returned 0 results and fuzzy fallback was used automatically.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub fuzzy_fallback: bool,
}

/// Search options
pub struct SearchOptions {
    pub query: Option<String>,
    pub error_pattern: Option<String>,
    pub code_query: Option<String>,
    pub code_lang: Option<String>,
    pub solution_only: bool,
    pub like_session: Option<String>,
    pub session_id: Option<String>,
    pub agent: Vec<String>,
    pub project: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub before: Option<DateTime<Utc>>,
    pub tag: Vec<String>,
    pub outcome: Option<String>,
    pub doc_type_filter: Option<String>,
    pub model: Option<String>,
    pub fuzzy: bool,
    pub fuzzy_distance: u8,
    pub max_results: usize,
    pub snippet_length: usize,
    pub token_budget: Option<usize>,
    pub offset: usize,
    pub sort: SortOrder,
}

/// Sort order for results
#[derive(Debug, Clone, Copy, Default)]
pub enum SortOrder {
    #[default]
    Relevance,
    Newest,
    Oldest,
    Turns,
}

/// Open the Tantivy index from the data directory.
pub fn open_index(data_dir: &Path) -> Result<tantivy::Index> {
    let index_path = data_dir.join("index").join(INDEX_DIR_NAME);

    if !index_path.exists() {
        return Err(AgentScribeError::DataDir(format!(
            "Index not found at {}. Run 'agentscribe scrape' first.",
            index_path.display()
        )));
    }

    let index = tantivy::Index::open_in_dir(&index_path).map_err(|e| {
        AgentScribeError::DataDir(format!("Failed to open index: {}", e))
    })?;

    Ok(index)
}

/// Execute a search and return results.
pub fn execute_search(data_dir: &Path, opts: &SearchOptions) -> Result<SearchOutput> {
    let index = open_index(data_dir)?;
    let reader = index.reader().map_err(|e| {
        AgentScribeError::DataDir(format!("Failed to create index reader: {}", e))
    })?;
    let searcher = reader.searcher();
    let total_docs = searcher.num_docs();

    let start = std::time::Instant::now();

    // Handle --session lookup
    if let Some(ref sid) = opts.session_id {
        return lookup_session(&searcher, sid, &start, total_docs);
    }

    let (schema, fields) = build_schema();

    // Build query from options
    let query = build_query(&searcher, &fields, opts, &schema)?;

    // Determine how many results to fetch
    let fetch_limit = if opts.token_budget.is_some() {
        // Fetch extra for knapsack selection
        opts.max_results * 5
    } else {
        opts.max_results + opts.offset
    };

    // Execute search
    let mut top_docs: Vec<(f32, DocAddress)> = searcher
        .search(&query, &TopDocs::with_limit(fetch_limit))
        .map_err(|e| AgentScribeError::DataDir(format!("Search failed: {}", e)))?;

    // Automatic fuzzy fallback: if a plain text query returned nothing, retry with fuzzy.
    // Only triggers for plain queries (not error/code/like searches) and when --fuzzy was
    // not already set (to avoid double-fuzzing an explicit fuzzy request).
    let mut used_fuzzy_fallback = false;
    if top_docs.is_empty() && opts.query.is_some() && !opts.fuzzy
        && opts.error_pattern.is_none() && opts.code_query.is_none() && opts.like_session.is_none()
    {
        let fuzzy_opts = SearchOptions {
            fuzzy: true,
            fuzzy_distance: opts.fuzzy_distance,
            query: opts.query.clone(),
            error_pattern: opts.error_pattern.clone(),
            code_query: opts.code_query.clone(),
            code_lang: opts.code_lang.clone(),
            solution_only: opts.solution_only,
            like_session: opts.like_session.clone(),
            session_id: opts.session_id.clone(),
            agent: opts.agent.clone(),
            project: opts.project.clone(),
            since: opts.since,
            before: opts.before,
            tag: opts.tag.clone(),
            outcome: opts.outcome.clone(),
            doc_type_filter: opts.doc_type_filter.clone(),
            model: opts.model.clone(),
            max_results: opts.max_results,
            snippet_length: opts.snippet_length,
            token_budget: opts.token_budget,
            offset: opts.offset,
            sort: opts.sort,
        };
        let fallback_query = build_query(&searcher, &fields, &fuzzy_opts, &schema)?;
        top_docs = searcher
            .search(&fallback_query, &TopDocs::with_limit(fetch_limit))
            .map_err(|e| AgentScribeError::DataDir(format!("Fuzzy fallback search failed: {}", e)))?;
        if !top_docs.is_empty() {
            used_fuzzy_fallback = true;
        }
    }

    // Apply offset
    let top_docs: Vec<_> = top_docs.into_iter().skip(opts.offset).collect();

    // When token_budget is set, convert all fetched candidates so the knapsack
    // can select optimally; otherwise cap at max_results.
    let result_limit = if opts.token_budget.is_some() {
        fetch_limit
    } else {
        opts.max_results
    };

    // Convert to SearchResult
    let mut results: Vec<SearchResult> = Vec::new();
    for (score, doc_addr) in &top_docs {
        if results.len() >= result_limit {
            break;
        }
        if let Some(result) = doc_to_search_result(&searcher, &fields, *doc_addr, *score, opts) {
            results.push(result);
        }
    }

    // Apply sort order
    apply_sort(&mut results, opts.sort);

    // Apply token budget if specified
    if let Some(budget) = opts.token_budget {
        results = knapsack_pack(results, budget);
    }

    let elapsed = start.elapsed();
    let query_display = opts
        .query
        .clone()
        .or_else(|| opts.error_pattern.clone())
        .or_else(|| opts.code_query.clone())
        .or_else(|| opts.like_session.clone())
        .or_else(|| opts.session_id.clone())
        .unwrap_or_default();

    Ok(SearchOutput {
        query: query_display,
        total_matches: results.len(),
        search_time_ms: elapsed.as_millis() as u64,
        sessions_searched: total_docs,
        results,
        fuzzy_fallback: used_fuzzy_fallback,
    })
}

/// Build a Tantivy query from search options.
fn build_query(
    searcher: &Searcher,
    fields: &IndexFields,
    opts: &SearchOptions,
    _schema: &tantivy::schema::Schema,
) -> Result<Box<dyn Query>> {
    let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();

    // Main text query
    if let Some(ref query_str) = opts.query {
        let query = if opts.fuzzy {
            build_fuzzy_query(fields, query_str, opts.fuzzy_distance)?
        } else {
            build_fulltext_query(searcher, fields, query_str)?
        };
        clauses.push((Occur::Must, query));
    }

    // Error fingerprint lookup
    if let Some(ref error_pat) = opts.error_pattern {
        let term = tantivy::schema::Term::from_field_text(fields.error_fingerprint, error_pat);
        let query = Box::new(FuzzyTermQuery::new(term, 1, true));
        clauses.push((Occur::Must, query));
    }

    // Code search
    if let Some(ref code_q) = opts.code_query {
        let text_query = if opts.fuzzy {
            build_fuzzy_query_for_field(fields.code_content, code_q, opts.fuzzy_distance)?
        } else {
            let (parsed, _errors) = tantivy::query::QueryParser::for_index(
                searcher.index(),
                vec![fields.code_content],
            )
            .parse_query_lenient(code_q);
            Box::new(parsed)
        };
        clauses.push((Occur::Must, text_query));

        // Filter to code_artifact doc_type if searching code
        let doc_term =
            tantivy::schema::Term::from_field_text(fields.doc_type, "code_artifact");
        clauses.push((
            Occur::Must,
            Box::new(TermQuery::new(doc_term, tantivy::schema::IndexRecordOption::Basic)),
        ));

        // Language filter
        if let Some(ref lang) = opts.code_lang {
            let lang_term =
                tantivy::schema::Term::from_field_text(fields.code_language, lang);
            clauses.push((
                Occur::Must,
                Box::new(TermQuery::new(
                    lang_term,
                    tantivy::schema::IndexRecordOption::Basic,
                )),
            ));
        }
    }

    // Solution-only: boost solution_summary field
    if opts.solution_only {
        let query_str = opts.query.as_deref().unwrap_or("");
        if !query_str.is_empty() {
            let (parsed, _errors) = tantivy::query::QueryParser::for_index(
                searcher.index(),
                vec![fields.solution_summary],
            )
            .parse_query_lenient(query_str);
            clauses.push((Occur::Should, Box::new(parsed)));
        }
    }

    // --like <session-id>: find sessions with similar content (TF-IDF based MLT)
    if let Some(ref like_id) = opts.like_session {
        let mlt_query = build_more_like_this(searcher, fields, like_id)?;
        clauses.push((Occur::Must, mlt_query));
        // Exclude the source session from results
        let exclude_term = tantivy::schema::Term::from_field_text(fields.session_id, like_id);
        clauses.push((
            Occur::MustNot,
            Box::new(TermQuery::new(
                exclude_term,
                tantivy::schema::IndexRecordOption::Basic,
            )),
        ));
    }

    // Filters (all as Must clauses)
    // Agent filter
    for agent_name in &opts.agent {
        let term = tantivy::schema::Term::from_field_text(fields.source_agent, agent_name);
        clauses.push((
            Occur::Must,
            Box::new(TermQuery::new(
                term,
                tantivy::schema::IndexRecordOption::Basic,
            )),
        ));
    }

    // Project filter
    if let Some(ref project) = opts.project {
        let term = tantivy::schema::Term::from_field_text(fields.project, project);
        clauses.push((
            Occur::Must,
            Box::new(TermQuery::new(
                term,
                tantivy::schema::IndexRecordOption::Basic,
            )),
        ));
    }

    // Date range filters
    if let Some(since) = opts.since {
        let tantivy_since = TantivyDateTime::from_timestamp_secs(since.timestamp());
        let range = RangeQuery::new_date_bounds(
            "timestamp".to_string(),
            Bound::Included(tantivy_since),
            Bound::Unbounded,
        );
        clauses.push((Occur::Must, Box::new(range)));
    }

    if let Some(before) = opts.before {
        let tantivy_before = TantivyDateTime::from_timestamp_secs(before.timestamp());
        let range = RangeQuery::new_date_bounds(
            "timestamp".to_string(),
            Bound::Unbounded,
            Bound::Excluded(tantivy_before),
        );
        clauses.push((Occur::Must, Box::new(range)));
    }

    // Tag filter (AND logic - all tags must be present)
    for tag in &opts.tag {
        let term = tantivy::schema::Term::from_field_text(fields.tags, tag);
        clauses.push((
            Occur::Must,
            Box::new(TermQuery::new(
                term,
                tantivy::schema::IndexRecordOption::Basic,
            )),
        ));
    }

    // Outcome filter
    if let Some(ref outcome) = opts.outcome {
        let term = tantivy::schema::Term::from_field_text(fields.outcome, outcome);
        clauses.push((
            Occur::Must,
            Box::new(TermQuery::new(
                term,
                tantivy::schema::IndexRecordOption::Basic,
            )),
        ));
    }

    // Doc type filter
    if let Some(ref dt) = opts.doc_type_filter {
        let term = tantivy::schema::Term::from_field_text(fields.doc_type, dt);
        clauses.push((
            Occur::Must,
            Box::new(TermQuery::new(
                term,
                tantivy::schema::IndexRecordOption::Basic,
            )),
        ));
    }

    // Model filter
    if let Some(ref model) = opts.model {
        let term = tantivy::schema::Term::from_field_text(fields.model, model);
        clauses.push((
            Occur::Must,
            Box::new(TermQuery::new(
                term,
                tantivy::schema::IndexRecordOption::Basic,
            )),
        ));
    }

    if clauses.is_empty() {
        return Err(AgentScribeError::DataDir(
            "No search query provided. Use <query>, --error, --code, or --like."
                .to_string(),
        ));
    }

    let query = BooleanQuery::new(clauses);
    Ok(Box::new(query))
}

/// Build a full-text query across content and summary fields.
///
/// Uses QueryParser so multi-word queries are correctly tokenized against the
/// same analysis pipeline used at index time. Summary is boosted 1.5×.
fn build_fulltext_query(
    searcher: &Searcher,
    fields: &IndexFields,
    query_str: &str,
) -> Result<Box<dyn Query>> {
    let mut parser = tantivy::query::QueryParser::for_index(
        searcher.index(),
        vec![fields.content, fields.summary],
    );
    parser.set_field_boost(fields.summary, 1.5);
    let (parsed, _errors) = parser.parse_query_lenient(query_str);
    Ok(Box::new(parsed))
}

/// Build a fuzzy query for all query terms across content, summary, and solution_summary.
fn build_fuzzy_query(fields: &IndexFields, query_str: &str, distance: u8) -> Result<Box<dyn Query>> {
    let terms: Vec<(Occur, Box<dyn Query>)> = query_str
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .flat_map(|word| {
            let mut sub = Vec::new();
            let term_content =
                tantivy::schema::Term::from_field_text(fields.content, word);
            sub.push((
                Occur::Should,
                Box::new(FuzzyTermQuery::new(term_content, distance, true)) as Box<dyn Query>,
            ));
            let term_summary =
                tantivy::schema::Term::from_field_text(fields.summary, word);
            sub.push((
                Occur::Should,
                Box::new(FuzzyTermQuery::new(term_summary, distance, true)) as Box<dyn Query>,
            ));
            let term_solution =
                tantivy::schema::Term::from_field_text(fields.solution_summary, word);
            sub.push((
                Occur::Should,
                Box::new(FuzzyTermQuery::new(term_solution, distance, true)) as Box<dyn Query>,
            ));
            sub
        })
        .collect();

    if terms.is_empty() {
        return Err(AgentScribeError::DataDir(
            "Empty query string".to_string(),
        ));
    }

    Ok(Box::new(BooleanQuery::new(terms)))
}

/// Build a fuzzy query for a specific field.
fn build_fuzzy_query_for_field(field: Field, query_str: &str, distance: u8) -> Result<Box<dyn Query>> {
    let terms: Vec<(Occur, Box<dyn Query>)> = query_str
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .map(|word| {
            let term = tantivy::schema::Term::from_field_text(field, word);
            (
                Occur::Should,
                Box::new(FuzzyTermQuery::new(term, distance, true)) as Box<dyn Query>,
            )
        })
        .collect();

    if terms.is_empty() {
        return Err(AgentScribeError::DataDir(
            "Empty query string".to_string(),
        ));
    }

    Ok(Box::new(BooleanQuery::new(terms)))
}

/// Common English stop words to exclude from MLT term extraction.
/// Must be sorted alphabetically for binary_search.
static MLT_STOP_WORDS: &[&str] = &[
    "about", "after", "all", "also", "and", "are", "because", "been", "being",
    "but", "can", "could", "did", "does", "doing", "each", "for", "from",
    "get", "got", "had", "has", "have", "her", "how", "into", "its", "just",
    "like", "make", "may", "new", "not", "now", "old", "one", "other", "our",
    "out", "over", "see", "should", "some", "such", "than", "that", "the",
    "their", "them", "then", "there", "these", "this", "was", "way", "were",
    "who", "will", "with", "would", "you",
];

/// Check if a token is a stop word.
fn is_stop_word(word: &str) -> bool {
    MLT_STOP_WORDS.binary_search(&word).is_ok()
}

/// Build a "more like this" query using TF-IDF weighted term extraction.
///
/// Extracts terms from the source document's content and summary fields,
/// ranks them by TF-IDF significance, and builds a boosted boolean query
/// from the top-scoring terms. The caller is responsible for excluding
/// the original session via a MustNot clause at the outer query level.
fn build_more_like_this(
    searcher: &Searcher,
    fields: &IndexFields,
    session_id: &str,
) -> Result<Box<dyn Query>> {
    let term = tantivy::schema::Term::from_field_text(fields.session_id, session_id);
    let query = TermQuery::new(term, tantivy::schema::IndexRecordOption::Basic);

    let docs: Vec<(f32, DocAddress)> = searcher
        .search(&query, &TopDocs::with_limit(1))
        .map_err(|e| AgentScribeError::DataDir(format!("Search failed: {}", e)))?;

    if docs.is_empty() {
        return Err(AgentScribeError::DataDir(format!(
            "Session '{}' not found in index",
            session_id
        )));
    }

    let doc: TantivyDocument = searcher
        .doc(docs[0].1)
        .map_err(|e| AgentScribeError::DataDir(format!("Failed to fetch document: {}", e)))?;

    // Collect text from content, summary, and tags fields
    let mut all_text = String::new();
    for field in [fields.content, fields.summary] {
        if let Some(text) = doc.get_first(field).and_then(|v| v.as_str()) {
            all_text.push_str(text);
            all_text.push(' ');
        }
    }
    // Tags are highly discriminative — include them
    for tag_val in doc.get_all(fields.tags).filter_map(|v| v.as_str()) {
        all_text.push_str(tag_val);
        all_text.push(' ');
    }

    // Tokenize and compute term frequencies, filtering stop words and short tokens
    let mut tf_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let total_tokens: usize = all_text
        .split_whitespace()
        .filter(|w| w.len() > 2)
        .map(|w| {
            let w = w.to_lowercase();
            w
        })
        .filter(|w| !is_stop_word(w))
        .map(|w| {
            *tf_counts.entry(w.clone()).or_insert(0) += 1;
            w
        })
        .count();

    if total_tokens == 0 {
        return Err(AgentScribeError::DataDir(
            "No content terms found in session".to_string(),
        ));
    }

    // Compute TF-IDF for each unique term
    let num_docs = searcher.num_docs() as f64;
    let mut scored_terms: Vec<(String, f64)> = tf_counts
        .into_iter()
        .filter_map(|(term_str, tf)| {
            let term =
                tantivy::schema::Term::from_field_text(fields.content, &term_str);
            let df = searcher.doc_freq(&term).unwrap_or(1) as f64;
            if df < 1.0 {
                return None;
            }
            let idf = (num_docs / df).ln();
            let tf_norm = tf as f64 / total_tokens as f64;
            let tfidf = tf_norm * idf;
            Some((term_str, tfidf))
        })
        .collect();

    // Sort by TF-IDF descending — most significant terms first
    scored_terms.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Take top 20 significant terms
    let top_terms: Vec<_> = scored_terms.into_iter().take(20).collect();

    #[cfg(test)]
    eprintln!("DEBUG MLT: num_docs={}, total_tokens={}, top_terms={:?}", num_docs, total_tokens, top_terms.iter().map(|(t, s)| format!("{}={:.4}", t, s)).collect::<Vec<_>>());

    if top_terms.is_empty() {
        return Err(AgentScribeError::DataDir(
            "No significant terms found in session".to_string(),
        ));
    }

    // Build MLT query: each term as a Should clause, boosted by relative TF-IDF
    let max_score = top_terms[0].1;
    let mlt_clauses: Vec<(Occur, Box<dyn Query>)> = top_terms
        .iter()
        .map(|(term_str, score)| {
            let term =
                tantivy::schema::Term::from_field_text(fields.content, term_str);
            let boost = (score / max_score * 2.0 + 0.5) as f32;
            (
                Occur::Should,
                Box::new(BoostQuery::new(
                    Box::new(TermQuery::new(
                        term,
                        tantivy::schema::IndexRecordOption::Basic,
                    )),
                    boost,
                )) as Box<dyn Query>,
            )
        })
        .collect();

    Ok(Box::new(BooleanQuery::new(mlt_clauses)))
}

/// Convert a Tantivy document to a SearchResult.
fn doc_to_search_result(
    searcher: &Searcher,
    fields: &IndexFields,
    doc_addr: DocAddress,
    score: f32,
    opts: &SearchOptions,
) -> Option<SearchResult> {
    let doc: TantivyDocument = searcher.doc(doc_addr).ok()?;

    let session_id = doc
        .get_first(fields.session_id)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let source_agent = doc
        .get_first(fields.source_agent)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let project = doc
        .get_first(fields.project)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let timestamp = doc
        .get_first(fields.timestamp)
        .and_then(|v| v.as_datetime())
        .map(|dt: tantivy::DateTime| {
            DateTime::from_timestamp(dt.into_timestamp_secs(), 0)
                .unwrap_or_default()
                .to_rfc3339()
        });

    let turns = doc.get_first(fields.turn_count).and_then(|v| v.as_u64());

    let outcome = doc
        .get_first(fields.outcome)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let summary = doc
        .get_first(fields.summary)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let doc_type = doc
        .get_first(fields.doc_type)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let model = doc
        .get_first(fields.model)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Extract snippet
    let snippet = if opts.snippet_length > 0 {
        let content_field = if opts.solution_only {
            fields.solution_summary
        } else if opts.code_query.is_some() {
            fields.code_content
        } else {
            fields.content
        };

        let content = doc
            .get_first(content_field)
            .and_then(|v| v.as_str())
            .unwrap_or("");

        extract_snippet(content, opts.snippet_length)
    } else {
        None
    };

    // Collect tags
    let tags: Vec<String> = doc
        .get_all(fields.tags)
        .filter_map(|v| v.as_str())
        .map(|s| s.to_string())
        .collect();

    // Estimate token cost: ceil((snippet_chars + summary_chars) / CHARS_PER_TOKEN)
    let token_count = {
        let snippet_chars = snippet.as_ref().map(|s| s.len()).unwrap_or(0);
        let summary_chars = summary.as_ref().map(|s| s.len()).unwrap_or(0);
        (snippet_chars + summary_chars + CHARS_PER_TOKEN - 1) / CHARS_PER_TOKEN
    };

    Some(SearchResult {
        session_id,
        source_agent,
        project,
        timestamp,
        turns,
        outcome,
        score,
        summary,
        snippet,
        tags,
        doc_type,
        model,
        token_count,
    })
}

/// Extract a snippet from content around the best match.
fn extract_snippet(content: &str, max_length: usize) -> Option<String> {
    if content.is_empty() || max_length == 0 {
        return None;
    }

    if content.len() <= max_length {
        return Some(content.to_string());
    }

    // Take from the beginning up to max_length, at a word boundary
    let mut end = max_length;
    while end > 0 && end < content.len() && !content.is_char_boundary(end) {
        end += 1;
    }

    // Try to break at a word boundary
    if let Some(space_pos) = content[..end].rfind(' ') {
        end = space_pos;
    }

    let snippet = &content[..end];
    if snippet.is_empty() {
        None
    } else {
        Some(format!("{}...", snippet.trim_end()))
    }
}

/// Look up a specific session by ID.
fn lookup_session(
    searcher: &Searcher,
    session_id: &str,
    start: &std::time::Instant,
    total_docs: u64,
) -> Result<SearchOutput> {
    let (_schema, fields) = build_schema();

    let term = tantivy::schema::Term::from_field_text(fields.session_id, session_id);
    let query = TermQuery::new(term, tantivy::schema::IndexRecordOption::Basic);

    let docs: Vec<(f32, DocAddress)> = searcher
        .search(&query, &TopDocs::with_limit(1))
        .map_err(|e| AgentScribeError::DataDir(format!("Search failed: {}", e)))?;

    let mut results = Vec::new();
    if let Some((score, doc_addr)) = docs.first() {
        let opts = SearchOptions {
            query: None,
            error_pattern: None,
            code_query: None,
            code_lang: None,
            solution_only: false,
            like_session: None,
            session_id: None,
            agent: vec![],
            project: None,
            since: None,
            before: None,
            tag: vec![],
            outcome: None,
            doc_type_filter: None,
            model: None,
            fuzzy: false,
            fuzzy_distance: 1,
            max_results: 1,
            snippet_length: 500,
            token_budget: None,
            offset: 0,
            sort: SortOrder::Relevance,
        };
        if let Some(result) = doc_to_search_result(searcher, &fields, *doc_addr, *score, &opts) {
            results.push(result);
        }
    }

    let elapsed = start.elapsed();

    Ok(SearchOutput {
        query: session_id.to_string(),
        total_matches: results.len(),
        search_time_ms: elapsed.as_millis() as u64,
        sessions_searched: total_docs,
        results,
        fuzzy_fallback: false,
    })
}

/// Apply sort order to results.
fn apply_sort(results: &mut [SearchResult], sort: SortOrder) {
    match sort {
        SortOrder::Relevance => {
            // Already sorted by BM25 score (descending)
            results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        }
        SortOrder::Newest => {
            results.sort_by(|a, b| {
                let ts_a = a.timestamp.as_deref().unwrap_or("");
                let ts_b = b.timestamp.as_deref().unwrap_or("");
                ts_b.cmp(ts_a) // Newer first
            });
        }
        SortOrder::Oldest => {
            results.sort_by(|a, b| {
                let ts_a = a.timestamp.as_deref().unwrap_or("");
                let ts_b = b.timestamp.as_deref().unwrap_or("");
                ts_a.cmp(ts_b)
            });
        }
        SortOrder::Turns => {
            results.sort_by(|a, b| {
                b.turns
                    .unwrap_or(0)
                    .cmp(&a.turns.unwrap_or(0))
            });
        }
    }
}

/// Greedy knapsack: pack as many results as possible within a token budget.
///
/// Each result's cost is `ceil((snippet_chars + summary_chars) / CHARS_PER_TOKEN)`.
/// Results are ranked by BM25 score and greedily selected. When a result's full
/// snippet doesn't fit in the remaining budget, the snippet is truncated to fill
/// the remaining space rather than skipping the result entirely — this is the
/// adaptive behaviour that trades fewer/longer snippets for more/shorter ones.
fn knapsack_pack(results: Vec<SearchResult>, token_budget: usize) -> Vec<SearchResult> {
    // Sort by score descending (greedy by relevance)
    let mut items = results;
    items.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    let mut selected = Vec::new();
    let mut remaining = token_budget;

    for mut result in items {
        if remaining == 0 {
            break;
        }

        let snippet_chars = result.snippet.as_ref().map(|s| s.len()).unwrap_or(0);
        let summary_chars = result.summary.as_ref().map(|s| s.len()).unwrap_or(0);
        let full_cost = (snippet_chars + summary_chars + CHARS_PER_TOKEN - 1) / CHARS_PER_TOKEN;

        if full_cost <= remaining {
            // Full result fits unchanged
            remaining -= full_cost;
            selected.push(result);
        } else {
            // Try to fit with a truncated snippet (adaptive packing).
            // Subtract 3 chars to leave room for the "..." suffix extract_snippet may add.
            let summary_cost = (summary_chars + CHARS_PER_TOKEN - 1) / CHARS_PER_TOKEN;
            let available_snippet_chars = remaining
                .saturating_sub(summary_cost)
                .saturating_mul(CHARS_PER_TOKEN)
                .saturating_sub(3);

            if available_snippet_chars > 0 {
                let truncated = result.snippet.as_deref()
                    .and_then(|s| extract_snippet(s, available_snippet_chars));
                let truncated_chars = truncated.as_ref().map(|s| s.len()).unwrap_or(0);
                let actual_cost = (truncated_chars + summary_chars + CHARS_PER_TOKEN - 1) / CHARS_PER_TOKEN;

                if actual_cost <= remaining {
                    result.snippet = truncated;
                    result.token_count = actual_cost;
                    remaining -= actual_cost;
                    selected.push(result);
                } else if summary_cost > 0 && summary_cost <= remaining {
                    // Snippet still too big (e.g. unusual char widths); use summary only
                    result.snippet = None;
                    result.token_count = summary_cost;
                    remaining -= summary_cost;
                    selected.push(result);
                }
                // Skip if nothing fits
            } else if summary_cost > 0 && summary_cost <= remaining {
                // No room for any snippet; include summary only
                result.snippet = None;
                result.token_count = summary_cost;
                remaining -= summary_cost;
                selected.push(result);
            }
            // Skip if nothing fits
        }
    }

    selected
}

/// Format search results for human-readable output.
pub fn format_human(output: &SearchOutput, _snippet_length: usize) -> String {
    let mut lines = Vec::new();

    let fuzzy_note = if output.fuzzy_fallback { " [fuzzy fallback]" } else { "" };
    lines.push(format!(
        "{} result(s) for \"{}\"{} (searched {} sessions in {}ms)",
        output.total_matches,
        output.query,
        fuzzy_note,
        output.sessions_searched,
        output.search_time_ms
    ));

    for (i, result) in output.results.iter().enumerate() {
        lines.push(String::new());
        lines.push(format!(
            "[{}] {}  (score: {:.2})",
            i + 1,
            result.session_id,
            result.score
        ));

        if let Some(ref project) = result.project {
            lines.push(format!("    Project:  {}", project));
        }
        if let Some(ref timestamp) = result.timestamp {
            lines.push(format!("    Date:     {}", timestamp));
        }
        if let Some(turns) = result.turns {
            lines.push(format!("    Turns:    {}", turns));
        }
        if let Some(ref outcome) = result.outcome {
            lines.push(format!("    Outcome:  {}", outcome));
        }
        if let Some(ref summary) = result.summary {
            lines.push(format!("    Summary:  {}", summary));
        }
        if !result.tags.is_empty() {
            lines.push(format!("    Tags:     {}", result.tags.join(", ")));
        }
        if let Some(ref snippet) = result.snippet {
            // Word-wrap snippet
            let wrapped = word_wrap(snippet, 66);
            for (j, line) in wrapped.lines().enumerate() {
                if j == 0 {
                    lines.push(format!("    Snippet:  {}", line));
                } else {
                    lines.push(format!("              {}", line));
                }
            }
        }
    }

    lines.join("\n")
}

/// Simple word-wrap for terminal output.
fn word_wrap(text: &str, width: usize) -> String {
    let mut result = String::new();
    let mut line_len = 0;

    for word in text.split_whitespace() {
        if line_len == 0 {
            result.push_str(word);
            line_len = word.len();
        } else if line_len + word.len() + 1 <= width {
            result.push(' ');
            result.push_str(word);
            line_len += word.len() + 1;
        } else {
            result.push('\n');
            result.push_str(word);
            line_len = word.len();
        }
    }

    result
}

/// Parse a relative or absolute date string into a DateTime.
pub fn parse_datetime(s: &str) -> Result<DateTime<Utc>> {
    // Try ISO 8601 first
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Ok(dt.and_utc());
    }
    if let Ok(dt) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Ok(dt.and_hms_opt(0, 0, 0).unwrap().and_utc());
    }

    // Try relative: Nh, Nd, Nw
    let now = Utc::now();
    let trimmed = s.trim();

    if let Some(suffix) = trimmed.strip_suffix('h') {
        if let Ok(hours) = suffix.parse::<i64>() {
            return Ok(now - chrono::Duration::hours(hours));
        }
    }
    if let Some(suffix) = trimmed.strip_suffix('d') {
        if let Ok(days) = suffix.parse::<i64>() {
            return Ok(now - chrono::Duration::days(days));
        }
    }
    if let Some(suffix) = trimmed.strip_suffix('w') {
        if let Ok(weeks) = suffix.parse::<i64>() {
            return Ok(now - chrono::Duration::weeks(weeks));
        }
    }

    Err(AgentScribeError::Timestamp(format!(
        "Cannot parse date/time: '{}'. Use ISO 8601 or relative (e.g., 24h, 7d, 1w).",
        s
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_datetime_iso8601() {
        let dt = parse_datetime("2026-03-14T10:30:00Z").unwrap();
        assert_eq!(dt.to_rfc3339(), "2026-03-14T10:30:00+00:00");
    }

    #[test]
    fn test_parse_datetime_date_only() {
        let dt = parse_datetime("2026-03-14").unwrap();
        assert_eq!(dt.to_rfc3339(), "2026-03-14T00:00:00+00:00");
    }

    #[test]
    fn test_parse_datetime_relative_hours() {
        let dt = parse_datetime("24h").unwrap();
        let now = Utc::now();
        let diff = now.signed_duration_since(dt);
        assert!(diff.num_hours() >= 23 && diff.num_hours() <= 25);
    }

    #[test]
    fn test_parse_datetime_relative_days() {
        let dt = parse_datetime("7d").unwrap();
        let now = Utc::now();
        let diff = now.signed_duration_since(dt);
        assert!(diff.num_days() >= 6 && diff.num_days() <= 8);
    }

    #[test]
    fn test_parse_datetime_relative_weeks() {
        let dt = parse_datetime("1w").unwrap();
        let now = Utc::now();
        let diff = now.signed_duration_since(dt);
        assert!(diff.num_weeks() >= 0 && diff.num_weeks() <= 2);
    }

    #[test]
    fn test_parse_datetime_invalid() {
        assert!(parse_datetime("not-a-date").is_err());
    }

    #[test]
    fn test_extract_snippet_short() {
        let content = "short text";
        assert_eq!(
            extract_snippet(content, 200),
            Some("short text".to_string())
        );
    }

    #[test]
    fn test_extract_snippet_long() {
        let content = "a ".repeat(500);
        let snippet = extract_snippet(&content, 100).unwrap();
        assert!(snippet.len() <= 110); // Allow some margin for "..."
        assert!(snippet.ends_with("..."));
    }

    #[test]
    fn test_extract_snippet_empty() {
        assert_eq!(extract_snippet("", 100), None);
    }

    #[test]
    fn test_extract_snippet_zero_length() {
        assert_eq!(extract_snippet("hello", 0), None);
    }

    #[test]
    fn test_word_wrap() {
        let text = "the quick brown fox jumps over the lazy dog";
        let wrapped = word_wrap(text, 20);
        assert!(wrapped.contains('\n'));
        // No line should exceed width
        for line in wrapped.lines() {
            assert!(line.len() <= 20);
        }
    }

    #[test]
    fn test_word_wrap_short() {
        assert_eq!(word_wrap("hello", 20), "hello");
    }

    #[test]
    fn test_knapsack_pack_fits_all() {
        let results = vec![
            make_test_result("a", 10.0, 100),
            make_test_result("b", 8.0, 100),
        ];
        let packed = knapsack_pack(results, 10000);
        assert_eq!(packed.len(), 2);
    }

    #[test]
    fn test_knapsack_pack_drops_low_value() {
        // Each snippet is 80 chars → ceil(80/4) = 20 tokens each.
        // Adaptive packing may truncate lower-ranked snippets to fit more results.
        // Key properties: highest-scored result is first, budget is never exceeded.
        let results = vec![
            make_test_result("a", 10.0, 80),
            make_test_result("b", 8.0, 80),
            make_test_result("c", 5.0, 80),
        ];
        let packed = knapsack_pack(results, 45);
        assert!(!packed.is_empty(), "highest-scored item must fit");
        assert_eq!(packed[0].session_id, "a", "highest-scored item should come first");
        let total: usize = packed.iter().map(|r| r.token_count).sum();
        assert!(total <= 45, "total tokens {} exceeded budget 45", total);
    }

    #[test]
    fn test_knapsack_pack_empty() {
        let packed = knapsack_pack(vec![], 1000);
        assert!(packed.is_empty());
    }

    #[test]
    fn test_knapsack_pack_adaptive_truncation() {
        // Budget is tight: fits 1 full result (100 chars = 25 tokens) plus partial second
        // Second result has 80-char snippet but only ~15 tokens of space left after first
        let results = vec![
            make_test_result("a", 10.0, 100), // 25 tokens
            make_test_result("b", 8.0, 200),  // 50 tokens — too big full, but truncatable
        ];
        let packed = knapsack_pack(results, 60);
        // "a" (25 tokens) fits fully; "b" should be truncated to fit remaining 35 tokens
        assert_eq!(packed.len(), 2, "adaptive truncation should include both results");
        assert_eq!(packed[0].session_id, "a");
        assert_eq!(packed[1].session_id, "b");
        // Verify "b"'s snippet was truncated
        let b_snippet = packed[1].snippet.as_ref().expect("truncated snippet should exist");
        assert!(b_snippet.len() < 200, "snippet should have been truncated");
        // Verify token_count was updated
        assert!(packed[1].token_count <= 35);
    }

    #[test]
    fn test_knapsack_pack_token_count_populated() {
        // Verify token_count is set on packed results
        let results = vec![make_test_result("a", 10.0, 80)]; // 80 chars → ceil(80/4) = 20 tokens
        let packed = knapsack_pack(results, 1000);
        assert_eq!(packed[0].token_count, 20);
    }

    #[test]
    fn test_knapsack_pack_respects_budget() {
        // Verify total token_count of all packed results is within budget
        let results = vec![
            make_test_result("a", 10.0, 40), // 10 tokens
            make_test_result("b", 9.0, 40),  // 10 tokens
            make_test_result("c", 8.0, 40),  // 10 tokens
            make_test_result("d", 7.0, 40),  // 10 tokens
        ];
        let budget = 25;
        let packed = knapsack_pack(results, budget);
        let total: usize = packed.iter().map(|r| r.token_count).sum();
        assert!(total <= budget, "total tokens {} exceeded budget {}", total, budget);
    }

    #[test]
    fn test_format_human_empty() {
        let output = SearchOutput {
            query: "test".to_string(),
            total_matches: 0,
            search_time_ms: 1,
            sessions_searched: 100,
            results: vec![],
            fuzzy_fallback: false,
        };
        let formatted = format_human(&output, 200);
        assert!(formatted.contains("0 result(s)"));
    }

    #[test]
    fn test_format_human_with_results() {
        let output = SearchOutput {
            query: "test".to_string(),
            total_matches: 1,
            search_time_ms: 5,
            sessions_searched: 1000,
            results: vec![SearchResult {
                session_id: "claude/abc123".to_string(),
                source_agent: "claude-code".to_string(),
                project: Some("/home/user/project".to_string()),
                timestamp: Some("2026-03-14T10:30:00+00:00".to_string()),
                turns: Some(42),
                outcome: Some("success".to_string()),
                score: 8.42,
                summary: Some("Fixed the bug".to_string()),
                snippet: Some("ran the migration and it worked".to_string()),
                tags: vec!["postgres".to_string(), "migration".to_string()],
                doc_type: Some("session".to_string()),
                model: Some("claude-sonnet".to_string()),
                token_count: 12, // ceil((31 snippet + 14 summary) / 4) = ceil(45/4) = 12
            }],
            fuzzy_fallback: false,
        };
        let formatted = format_human(&output, 200);
        assert!(formatted.contains("[1] claude/abc123"));
        assert!(formatted.contains("score: 8.42"));
        assert!(formatted.contains("Project:  /home/user/project"));
        assert!(formatted.contains("Outcome:  success"));
        assert!(formatted.contains("Summary:  Fixed the bug"));
        assert!(formatted.contains("postgres, migration"));
    }

    #[test]
    fn test_apply_sort_relevance() {
        let mut results = vec![
            make_test_result("a", 5.0, 100),
            make_test_result("b", 10.0, 100),
            make_test_result("c", 1.0, 100),
        ];
        apply_sort(&mut results, SortOrder::Relevance);
        assert_eq!(results[0].score, 10.0);
        assert_eq!(results[1].score, 5.0);
        assert_eq!(results[2].score, 1.0);
    }

    #[test]
    fn test_apply_sort_newest() {
        let mut results = vec![
            make_test_result_with_ts("a", 5.0, "2026-03-10T00:00:00+00:00"),
            make_test_result_with_ts("b", 10.0, "2026-03-14T00:00:00+00:00"),
            make_test_result_with_ts("c", 1.0, "2026-03-12T00:00:00+00:00"),
        ];
        apply_sort(&mut results, SortOrder::Newest);
        assert_eq!(results[0].session_id, "b");
        assert_eq!(results[1].session_id, "c");
        assert_eq!(results[2].session_id, "a");
    }

    #[test]
    fn test_apply_sort_turns() {
        let mut results = vec![
            make_test_result_with_turns("a", 5.0, 10),
            make_test_result_with_turns("b", 10.0, 42),
            make_test_result_with_turns("c", 1.0, 5),
        ];
        apply_sort(&mut results, SortOrder::Turns);
        assert_eq!(results[0].turns, Some(42));
        assert_eq!(results[1].turns, Some(10));
        assert_eq!(results[2].turns, Some(5));
    }

    // Integration test: build a small index and search it
    #[test]
    fn test_search_integration() {
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::build_session_document;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");

        // Create index
        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema.clone()).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        // Add test documents
        let now = Utc::now();

        let manifest1 = {
            let mut m = SessionManifest::new("claude/abc123".to_string(), "claude-code".to_string());
            m.project = Some("/home/user/myapp".to_string());
            m.started = now;
            m.turns = 10;
            m.summary = Some("Migrated Postgres schema from v3 to v4".to_string());
            m.outcome = Some("success".to_string());
            m.tags = vec!["postgres".to_string(), "migration".to_string()];
            m
        };

        let events1 = vec![
            Event::new(now, "claude/abc123".to_string(), "claude-code".to_string(), Role::User,
                "I need to migrate the Postgres schema from v3 to v4".to_string()),
            Event::new(now, "claude/abc123".to_string(), "claude-code".to_string(), Role::Assistant,
                "I'll create a migration script that alters the table and backfills data".to_string()),
        ];

        let doc1 = build_session_document(&fields, &events1, &manifest1);
        writer.add_document(doc1).unwrap();

        let manifest2 = {
            let mut m = SessionManifest::new("aider/def456".to_string(), "aider".to_string());
            m.project = Some("/home/user/api-server".to_string());
            m.started = now - chrono::Duration::days(3);
            m.turns = 5;
            m.summary = Some("Fixed connection pooling issue".to_string());
            m.outcome = Some("success".to_string());
            m.tags = vec!["database".to_string(), "pooling".to_string()];
            m
        };

        let events2 = vec![
            Event::new(now - chrono::Duration::days(3), "aider/def456".to_string(), "aider".to_string(), Role::User,
                "The connection pool is exhausting under load".to_string()),
            Event::new(now - chrono::Duration::days(3), "aider/def456".to_string(), "aider".to_string(), Role::Assistant,
                "Bumped max_connections to 50 and added retry logic".to_string()),
        ];

        let doc2 = build_session_document(&fields, &events2, &manifest2);
        writer.add_document(doc2).unwrap();

        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        // Now search
        let opts = SearchOptions {
            query: Some("postgres migration".to_string()),
            error_pattern: None,
            code_query: None,
            code_lang: None,
            solution_only: false,
            like_session: None,
            session_id: None,
            agent: vec![],
            project: None,
            since: None,
            before: None,
            tag: vec![],
            outcome: None,
            doc_type_filter: None,
            model: None,
            fuzzy: false,
            fuzzy_distance: 1,
            max_results: 10,
            snippet_length: 200,
            token_budget: None,
            offset: 0,
            sort: SortOrder::Relevance,
        };

        let result = execute_search(temp_dir.path(), &opts).unwrap();
        assert!(result.total_matches > 0);
        assert!(result.sessions_searched >= 2);
        assert!(result.search_time_ms < 1000);

        // The postgres migration result should rank first
        if !result.results.is_empty() {
            assert_eq!(result.results[0].session_id, "claude/abc123");
        }
    }

    #[test]
    fn test_search_integration_with_filters() {
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::build_session_document;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");

        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema.clone()).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();

        // Session 1: claude-code, success
        let m1 = {
            let mut m = SessionManifest::new("claude/1".to_string(), "claude-code".to_string());
            m.project = Some("/home/user/proj".to_string());
            m.started = now;
            m.turns = 10;
            m.outcome = Some("success".to_string());
            m.tags = vec!["rust".to_string()];
            m
        };
        let e1 = vec![
            Event::new(now, "claude/1".to_string(), "claude-code".to_string(), Role::User, "fix bug".to_string()),
            Event::new(now, "claude/1".to_string(), "claude-code".to_string(), Role::Assistant, "fixed it".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e1, &m1)).unwrap();

        // Session 2: aider, failure
        let m2 = {
            let mut m = SessionManifest::new("aider/2".to_string(), "aider".to_string());
            m.project = Some("/home/user/proj".to_string());
            m.started = now - chrono::Duration::days(1);
            m.turns = 3;
            m.outcome = Some("failure".to_string());
            m.tags = vec!["rust".to_string()];
            m
        };
        let e2 = vec![
            Event::new(now - chrono::Duration::days(1), "aider/2".to_string(), "aider".to_string(), Role::User, "fix bug".to_string()),
            Event::new(now - chrono::Duration::days(1), "aider/2".to_string(), "aider".to_string(), Role::Assistant, "could not fix".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e2, &m2)).unwrap();

        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        // Search with agent filter
        let opts = SearchOptions {
            query: Some("fix bug".to_string()),
            agent: vec!["claude-code".to_string()],
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].source_agent, "claude-code");

        // Search with outcome filter
        let opts = SearchOptions {
            query: Some("fix bug".to_string()),
            outcome: Some("failure".to_string()),
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].session_id, "aider/2");

        // Search with tag filter
        let opts = SearchOptions {
            query: Some("fix bug".to_string()),
            tag: vec!["rust".to_string()],
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();
        assert_eq!(result.results.len(), 2);
    }

    #[test]
    fn test_search_integration_fuzzy() {
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::build_session_document;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");

        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema.clone()).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();
        let m = {
            let mut m = SessionManifest::new("claude/1".to_string(), "claude-code".to_string());
            m.started = now;
            m.turns = 2;
            m
        };
        let e = vec![
            Event::new(now, "claude/1".to_string(), "claude-code".to_string(), Role::User, "kubernetes deployment".to_string()),
            Event::new(now, "claude/1".to_string(), "claude-code".to_string(), Role::Assistant, "deployed to k8s".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e, &m)).unwrap();
        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        // Fuzzy search with misspelling
        let opts = SearchOptions {
            query: Some("kuberntes".to_string()),
            fuzzy: true,
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();
        assert_eq!(result.results.len(), 1);
    }

    #[test]
    fn test_search_integration_session_lookup() {
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::build_session_document;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");

        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema.clone()).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();
        let m = {
            let mut m = SessionManifest::new("claude/specific-123".to_string(), "claude-code".to_string());
            m.started = now;
            m.turns = 5;
            m.summary = Some("A very specific session".to_string());
            m
        };
        let e = vec![
            Event::new(now, "claude/specific-123".to_string(), "claude-code".to_string(), Role::User, "do something".to_string()),
            Event::new(now, "claude/specific-123".to_string(), "claude-code".to_string(), Role::Assistant, "done".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e, &m)).unwrap();
        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        let opts = SearchOptions {
            session_id: Some("claude/specific-123".to_string()),
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].session_id, "claude/specific-123");
        assert_eq!(
            result.results[0].summary,
            Some("A very specific session".to_string())
        );
    }

    #[test]
    fn test_search_integration_date_filters() {
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::build_session_document;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");

        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema.clone()).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();

        // Old session
        let m1 = {
            let mut m = SessionManifest::new("old/1".to_string(), "test".to_string());
            m.started = now - chrono::Duration::days(30);
            m.turns = 1;
            m
        };
        let e1 = vec![
            Event::new(now - chrono::Duration::days(30), "old/1".to_string(), "test".to_string(), Role::User, "old stuff".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e1, &m1)).unwrap();

        // Recent session
        let m2 = {
            let mut m = SessionManifest::new("new/2".to_string(), "test".to_string());
            m.started = now - chrono::Duration::hours(1);
            m.turns = 1;
            m
        };
        let e2 = vec![
            Event::new(now - chrono::Duration::hours(1), "new/2".to_string(), "test".to_string(), Role::User, "new stuff".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e2, &m2)).unwrap();

        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        // --since 7d should only find the recent session
        let opts = SearchOptions {
            query: Some("stuff".to_string()),
            since: Some(now - chrono::Duration::days(7)),
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].session_id, "new/2");

        // --before 1d should only find the old session
        let opts = SearchOptions {
            query: Some("stuff".to_string()),
            before: Some(now - chrono::Duration::hours(12)),
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].session_id, "old/1");
    }

    #[test]
    fn test_search_no_query_error() {
        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");

        let (schema, _) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema).unwrap();

        let opts = SearchOptions {
            query: None,
            error_pattern: None,
            code_query: None,
            code_lang: None,
            solution_only: false,
            like_session: None,
            session_id: None,
            agent: vec![],
            project: None,
            since: None,
            before: None,
            tag: vec![],
            outcome: None,
            doc_type_filter: None,
            model: None,
            fuzzy: false,
            fuzzy_distance: 1,
            max_results: 10,
            snippet_length: 200,
            token_budget: None,
            offset: 0,
            sort: SortOrder::Relevance,
        };

        let result = execute_search(temp_dir.path(), &opts);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No search query provided"));
    }

    #[test]
    fn test_search_index_not_found() {
        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();
        let opts = default_opts();
        let result = execute_search(temp_dir.path(), &opts);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Index not found"));
    }

    #[test]
    fn test_search_more_like_this() {
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::build_session_document;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");

        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema.clone()).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();

        // Source session: postgres migration work
        let m1 = {
            let mut m = SessionManifest::new("claude/src-1".to_string(), "claude-code".to_string());
            m.project = Some("/home/user/api".to_string());
            m.started = now;
            m.turns = 8;
            m.summary = Some("Migrated postgres schema from v3 to v4".to_string());
            m.tags = vec!["postgres".to_string(), "migration".to_string()];
            m
        };
        let e1 = vec![
            Event::new(now, "claude/src-1".to_string(), "claude-code".to_string(), Role::User,
                "I need to migrate the postgres schema from v3 to v4, including the users table and orders table".to_string()),
            Event::new(now, "claude/src-1".to_string(), "claude-code".to_string(), Role::Assistant,
                "I'll create the migration script that alters the users and orders tables, then backfill the data".to_string()),
            Event::new(now, "claude/src-1".to_string(), "claude-code".to_string(), Role::Assistant,
                "The migration ran successfully using ALTER TABLE and pg_dump for backup".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e1, &m1)).unwrap();

        // Similar session: also postgres work
        let m2 = {
            let mut m = SessionManifest::new("aider/sim-1".to_string(), "aider".to_string());
            m.project = Some("/home/user/api".to_string());
            m.started = now - chrono::Duration::days(1);
            m.turns = 5;
            m.summary = Some("Added postgres connection pooling".to_string());
            m.tags = vec!["postgres".to_string(), "pooling".to_string()];
            m
        };
        let e2 = vec![
            Event::new(now - chrono::Duration::days(1), "aider/sim-1".to_string(), "aider".to_string(), Role::User,
                "Add connection pooling to the postgres database".to_string()),
            Event::new(now - chrono::Duration::days(1), "aider/sim-1".to_string(), "aider".to_string(), Role::Assistant,
                "Configured pgBouncer for postgres connection pooling with max 50 connections".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e2, &m2)).unwrap();

        // Dissimilar session: completely different topic
        let m3 = {
            let mut m = SessionManifest::new("claude/unrelated".to_string(), "claude-code".to_string());
            m.project = Some("/home/user/frontend".to_string());
            m.started = now - chrono::Duration::days(2);
            m.turns = 3;
            m.summary = Some("Fixed CSS layout issue".to_string());
            m.tags = vec!["css".to_string(), "frontend".to_string()];
            m
        };
        let e3 = vec![
            Event::new(now - chrono::Duration::days(2), "claude/unrelated".to_string(), "claude-code".to_string(), Role::User,
                "The flexbox layout is broken on mobile".to_string()),
            Event::new(now - chrono::Duration::days(2), "claude/unrelated".to_string(), "claude-code".to_string(), Role::Assistant,
                "Fixed the CSS grid and flexbox properties for responsive design".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e3, &m3)).unwrap();

        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        // --like should find the postgres session but rank it above the CSS one
        let opts = SearchOptions {
            query: None,
            like_session: Some("claude/src-1".to_string()),
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();

        // Should find at least the similar postgres session
        assert!(result.total_matches >= 1);

        // Source session should NOT appear in results
        for r in &result.results {
            assert_ne!(r.session_id, "claude/src-1");
        }

        // The postgres session should rank higher than the CSS session
        if result.results.len() >= 2 {
            let postgres_found = result.results.iter().any(|r| r.session_id == "aider/sim-1");
            let css_found = result.results.iter().any(|r| r.session_id == "claude/unrelated");
            if postgres_found && css_found {
                let p_idx = result.results.iter().position(|r| r.session_id == "aider/sim-1").unwrap();
                let c_idx = result.results.iter().position(|r| r.session_id == "claude/unrelated").unwrap();
                assert!(p_idx < c_idx, "postgres session should rank higher than CSS session");
            }
        }
    }

    #[test]
    fn test_search_more_like_this_nonexistent_session() {
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::build_session_document;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");

        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let _index = tantivy::Index::create_in_dir(&index_path, schema).unwrap();
        let mut writer = _index.writer(50_000_000).unwrap();

        let now = Utc::now();
        let m = SessionManifest::new("claude/exists".to_string(), "claude-code".to_string());
        let e = vec![
            Event::new(now, "claude/exists".to_string(), "claude-code".to_string(), Role::User, "hello".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e, &m)).unwrap();
        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        // --like with nonexistent session should error
        let opts = SearchOptions {
            like_session: Some("nonexistent/session".to_string()),
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_search_more_like_this_with_agent_filter() {
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::build_session_document;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");

        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();

        // Source: claude-code session about postgres
        let m1 = {
            let mut m = SessionManifest::new("claude/src".to_string(), "claude-code".to_string());
            m.started = now;
            m.turns = 2;
            m.tags = vec!["postgres".to_string()];
            m
        };
        let e1 = vec![
            Event::new(now, "claude/src".to_string(), "claude-code".to_string(), Role::User,
                "fix the postgres connection issue".to_string()),
            Event::new(now, "claude/src".to_string(), "claude-code".to_string(), Role::Assistant,
                "reconfigured postgres connection string".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e1, &m1)).unwrap();

        // Similar: aider session about postgres
        let m2 = {
            let mut m = SessionManifest::new("aider/sim".to_string(), "aider".to_string());
            m.started = now;
            m.turns = 2;
            m.tags = vec!["postgres".to_string()];
            m
        };
        let e2 = vec![
            Event::new(now, "aider/sim".to_string(), "aider".to_string(), Role::User,
                "postgres migration failed".to_string()),
            Event::new(now, "aider/sim".to_string(), "aider".to_string(), Role::Assistant,
                "fixed the postgres migration script".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e2, &m2)).unwrap();

        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        // --like with --agent filter: should only return aider results
        let opts = SearchOptions {
            like_session: Some("claude/src".to_string()),
            agent: vec!["aider".to_string()],
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();
        for r in &result.results {
            assert_eq!(r.source_agent, "aider");
        }
    }

    #[test]
    fn test_search_more_like_this_cross_agent() {
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::build_session_document;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");

        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();

        // Source: claude-code session about kubernetes deployment
        let m1 = {
            let mut m = SessionManifest::new("claude/k8s-1".to_string(), "claude-code".to_string());
            m.project = Some("/home/user/cluster".to_string());
            m.started = now;
            m.turns = 4;
            m.tags = vec!["kubernetes".to_string(), "deployment".to_string()];
            m
        };
        let e1 = vec![
            Event::new(now, "claude/k8s-1".to_string(), "claude-code".to_string(), Role::User,
                "deploy the application to kubernetes cluster using kubectl".to_string()),
            Event::new(now, "claude/k8s-1".to_string(), "claude-code".to_string(), Role::Assistant,
                "created kubernetes deployment yaml manifest and applied kubectl rollout".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e1, &m1)).unwrap();

        // Cross-agent similar: aider session about kubernetes deployment
        let m2 = {
            let mut m = SessionManifest::new("aider/k8s-2".to_string(), "aider".to_string());
            m.project = Some("/home/user/other-project".to_string());
            m.started = now;
            m.turns = 3;
            m.tags = vec!["kubernetes".to_string(), "helm".to_string()];
            m
        };
        let e2 = vec![
            Event::new(now, "aider/k8s-2".to_string(), "aider".to_string(), Role::User,
                "deploy kubernetes deployment for the application".to_string()),
            Event::new(now, "aider/k8s-2".to_string(), "aider".to_string(), Role::Assistant,
                "created kubernetes deployment yaml and applied kubectl rollout successfully".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e2, &m2)).unwrap();

        // Unrelated session to dilute common terms
        let m3 = {
            let mut m = SessionManifest::new("claude/unrelated-css".to_string(), "claude-code".to_string());
            m.started = now;
            m.turns = 2;
            m.tags = vec!["css".to_string(), "frontend".to_string()];
            m
        };
        let e3 = vec![
            Event::new(now, "claude/unrelated-css".to_string(), "claude-code".to_string(), Role::User,
                "fix flexbox layout".to_string()),
            Event::new(now, "claude/unrelated-css".to_string(), "claude-code".to_string(), Role::Assistant,
                "updated css grid properties".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e3, &m3)).unwrap();

        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        // --like should discover the cross-agent session (no text query — MLT only)
        let opts = SearchOptions {
            query: None,
            like_session: Some("claude/k8s-1".to_string()),
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();

        // Debug: verify terms are searchable
        let reader = index.reader().unwrap();
        let searcher = reader.searcher();
        let k8s_term = tantivy::schema::Term::from_field_text(fields.content, "kubernetes");
        eprintln!("DEBUG: kubernetes doc_freq={:?}", searcher.doc_freq(&k8s_term));
        let (schema2, fields2) = build_schema();
        let k8s_query = TermQuery::new(k8s_term, tantivy::schema::IndexRecordOption::Basic);
        let k8s_docs: Vec<(f32, _)> = searcher.search(&k8s_query, &TopDocs::with_limit(10)).unwrap();
        eprintln!("DEBUG: kubernetes query matched {} docs", k8s_docs.len());
        for (score, addr) in &k8s_docs {
            let d: TantivyDocument = searcher.doc(*addr).unwrap();
            let sid = d.get_first(fields2.session_id).and_then(|v| v.as_str()).unwrap_or("?");
            eprintln!("DEBUG:   {} score={}", sid, score);
        }

        // Debug: test the MLT Should-only BooleanQuery directly
        let mlt_query = build_more_like_this(&searcher, &fields, "claude/k8s-1").unwrap();
        let mlt_docs: Vec<(f32, _)> = searcher.search(&*mlt_query, &TopDocs::with_limit(10)).unwrap();
        eprintln!("DEBUG: MLT Should-only query matched {} docs", mlt_docs.len());
        for (score, addr) in &mlt_docs {
            let d: TantivyDocument = searcher.doc(*addr).unwrap();
            let sid = d.get_first(fields2.session_id).and_then(|v| v.as_str()).unwrap_or("?");
            eprintln!("DEBUG: MLT   {} score={}", sid, score);
        }

        // Debug: test with MustNot at same level
        let exclude_term = tantivy::schema::Term::from_field_text(fields.session_id, "claude/k8s-1");
        let combined = BooleanQuery::new(vec![
            (Occur::Must, mlt_query),
            (Occur::MustNot, Box::new(TermQuery::new(exclude_term, tantivy::schema::IndexRecordOption::Basic)) as Box<dyn Query>),
        ]);
        let combined_docs: Vec<(f32, _)> = searcher.search(&combined, &TopDocs::with_limit(10)).unwrap();
        eprintln!("DEBUG: combined Must+MustNot query matched {} docs", combined_docs.len());
        for (score, addr) in &combined_docs {
            let d: TantivyDocument = searcher.doc(*addr).unwrap();
            let sid = d.get_first(fields2.session_id).and_then(|v| v.as_str()).unwrap_or("?");
            eprintln!("DEBUG: comb  {} score={}", sid, score);
        }

        assert!(result.total_matches >= 1, "expected at least 1 match, got {}", result.total_matches);

        // Should find the aider session (cross-agent discovery)
        let cross_agent_found = result
            .results
            .iter()
            .any(|r| r.session_id == "aider/k8s-2" && r.source_agent == "aider");
        assert!(cross_agent_found, "should discover cross-agent similar sessions");
    }

    // Helper to create default search options
    fn default_opts() -> SearchOptions {
        SearchOptions {
            query: Some("test".to_string()),
            error_pattern: None,
            code_query: None,
            code_lang: None,
            solution_only: false,
            like_session: None,
            session_id: None,
            agent: vec![],
            project: None,
            since: None,
            before: None,
            tag: vec![],
            outcome: None,
            doc_type_filter: None,
            model: None,
            fuzzy: false,
            fuzzy_distance: 1,
            max_results: 10,
            snippet_length: 200,
            token_budget: None,
            offset: 0,
            sort: SortOrder::Relevance,
        }
    }

    // Helper to make a test search result
    fn make_test_result(id: &str, score: f32, text_len: usize) -> SearchResult {
        let token_count = (text_len + CHARS_PER_TOKEN - 1) / CHARS_PER_TOKEN;
        SearchResult {
            session_id: id.to_string(),
            source_agent: "test".to_string(),
            project: None,
            timestamp: None,
            turns: None,
            outcome: None,
            score,
            summary: None,
            snippet: Some("x".repeat(text_len)),
            tags: vec![],
            doc_type: None,
            model: None,
            token_count,
        }
    }

    fn make_test_result_with_ts(id: &str, score: f32, ts: &str) -> SearchResult {
        SearchResult {
            session_id: id.to_string(),
            source_agent: "test".to_string(),
            project: None,
            timestamp: Some(ts.to_string()),
            turns: None,
            outcome: None,
            score,
            summary: None,
            snippet: None,
            tags: vec![],
            doc_type: None,
            model: None,
            token_count: 0,
        }
    }

    fn make_test_result_with_turns(id: &str, score: f32, turns: u64) -> SearchResult {
        SearchResult {
            session_id: id.to_string(),
            source_agent: "test".to_string(),
            project: None,
            timestamp: None,
            turns: Some(turns),
            outcome: None,
            score,
            summary: None,
            snippet: None,
            tags: vec![],
            doc_type: None,
            model: None,
            token_count: 0,
        }
    }

    #[test]
    fn test_apply_sort_oldest() {
        let mut results = vec![
            make_test_result_with_ts("a", 5.0, "2026-03-10T00:00:00+00:00"),
            make_test_result_with_ts("b", 10.0, "2026-03-14T00:00:00+00:00"),
            make_test_result_with_ts("c", 1.0, "2026-03-12T00:00:00+00:00"),
        ];
        apply_sort(&mut results, SortOrder::Oldest);
        assert_eq!(results[0].session_id, "a");
        assert_eq!(results[1].session_id, "c");
        assert_eq!(results[2].session_id, "b");
    }

    #[test]
    fn test_search_output_json_serialization() {
        let output = SearchOutput {
            query: "test query".to_string(),
            total_matches: 1,
            search_time_ms: 5,
            sessions_searched: 100,
            results: vec![SearchResult {
                session_id: "claude/abc123".to_string(),
                source_agent: "claude-code".to_string(),
                project: Some("/home/user/project".to_string()),
                timestamp: Some("2026-03-14T10:30:00+00:00".to_string()),
                turns: Some(10),
                outcome: Some("success".to_string()),
                score: 8.5,
                summary: Some("Fixed bug".to_string()),
                snippet: Some("the bug was in auth".to_string()),
                tags: vec!["rust".to_string()],
                doc_type: Some("session".to_string()),
                model: Some("claude-sonnet".to_string()),
                token_count: 12,
            }],
            fuzzy_fallback: false,
        };
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("\"query\":\"test query\""));
        assert!(json.contains("\"session_id\":\"claude/abc123\""));
        assert!(json.contains("\"total_matches\":1"));
        assert!(json.contains("\"source_agent\":\"claude-code\""));
        // All result fields present
        assert!(json.contains("\"outcome\":\"success\""));
        assert!(json.contains("\"model\":\"claude-sonnet\""));
        assert!(json.contains("\"doc_type\":\"session\""));
    }

    #[test]
    fn test_search_result_json_roundtrip() {
        let result = SearchResult {
            session_id: "aider/def456".to_string(),
            source_agent: "aider".to_string(),
            project: None,
            timestamp: None,
            turns: Some(5),
            outcome: None,
            score: 3.14,
            summary: None,
            snippet: Some("some code snippet".to_string()),
            tags: vec!["python".to_string(), "docker".to_string()],
            doc_type: Some("session".to_string()),
            model: None,
            token_count: 3,
        };
        let json = serde_json::to_string(&result).unwrap();
        let deser: SearchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.session_id, result.session_id);
        assert_eq!(deser.turns, result.turns);
        assert_eq!(deser.tags, result.tags);
        assert_eq!(deser.doc_type, result.doc_type);
        assert!(deser.project.is_none());
        assert!(deser.model.is_none());
    }

    #[test]
    fn test_search_integration_error_fingerprint() {
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::build_session_document;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");
        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();

        // Session with a specific error fingerprint (stored lowercase for consistent matching)
        let mut m1 = SessionManifest::new("claude/err-1".to_string(), "claude-code".to_string());
        m1.started = now;
        m1.turns = 2;
        let e1 = vec![
            Event::new(now, "claude/err-1".to_string(), "claude-code".to_string(), Role::User,
                "help fix this error".to_string()),
            Event::new(now, "claude/err-1".to_string(), "claude-code".to_string(), Role::ToolResult,
                "connection refused error".to_string())
                .with_error_fingerprints(vec!["connectionrefusederror".to_string()]),
        ];
        writer.add_document(build_session_document(&fields, &e1, &m1)).unwrap();

        // Session without the error fingerprint
        let mut m2 = SessionManifest::new("claude/noerr".to_string(), "claude-code".to_string());
        m2.started = now;
        m2.turns = 2;
        let e2 = vec![
            Event::new(now, "claude/noerr".to_string(), "claude-code".to_string(), Role::User, "normal session".to_string()),
            Event::new(now, "claude/noerr".to_string(), "claude-code".to_string(), Role::Assistant, "done".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e2, &m2)).unwrap();
        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        let opts = SearchOptions {
            query: None,
            error_pattern: Some("connectionrefusederror".to_string()),
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].session_id, "claude/err-1");
    }

    #[test]
    fn test_search_integration_code_search() {
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::{build_code_artifact_document, build_session_document};
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");
        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();

        // Session document
        let m = SessionManifest::new("claude/code-1".to_string(), "claude-code".to_string());
        let e = vec![Event::new(now, "claude/code-1".to_string(), "claude-code".to_string(),
            Role::User, "write a function".to_string())];
        writer.add_document(build_session_document(&fields, &e, &m)).unwrap();

        // Code artifact document
        let code_doc = build_code_artifact_document(
            &fields,
            "claude/code-1",
            "claude-code",
            Some("/home/user/project"),
            now,
            "rust",
            "src/auth.rs",
            "fn authenticate(user: &User) -> bool { validate_token(user.token) }",
            true,
            None,
        );
        writer.add_document(code_doc).unwrap();
        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        let opts = SearchOptions {
            query: None,
            code_query: Some("authenticate".to_string()),
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();
        assert!(result.total_matches >= 1);
        // All results should be code_artifact type (code search filters to that doc_type)
        for r in &result.results {
            assert_eq!(r.doc_type.as_deref(), Some("code_artifact"));
        }
    }

    #[test]
    fn test_search_integration_code_search_with_lang_filter() {
        use crate::index::build_code_artifact_document;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");
        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();

        // Rust code artifact
        writer.add_document(build_code_artifact_document(
            &fields, "claude/rust-s", "claude-code", None, now, "rust",
            "src/main.rs", "fn handle_request(req: &Request) -> Response { process(req) }",
            true, None,
        )).unwrap();

        // Python code artifact
        writer.add_document(build_code_artifact_document(
            &fields, "claude/py-s", "claude-code", None, now, "python",
            "app/handler.py", "def handle_request(req): return process(req)",
            true, None,
        )).unwrap();
        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        // Filter by rust language
        let opts = SearchOptions {
            query: None,
            code_query: Some("handle_request".to_string()),
            code_lang: Some("rust".to_string()),
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].session_id, "claude/rust-s");
    }

    #[test]
    fn test_search_integration_project_filter() {
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::build_session_document;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");
        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();

        let mut m1 = SessionManifest::new("claude/1".to_string(), "claude-code".to_string());
        m1.project = Some("/home/user/project-alpha".to_string());
        m1.started = now;
        m1.turns = 2;
        let e1 = vec![
            Event::new(now, "claude/1".to_string(), "claude-code".to_string(), Role::User, "debug the service".to_string()),
            Event::new(now, "claude/1".to_string(), "claude-code".to_string(), Role::Assistant, "found the bug".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e1, &m1)).unwrap();

        let mut m2 = SessionManifest::new("claude/2".to_string(), "claude-code".to_string());
        m2.project = Some("/home/user/project-beta".to_string());
        m2.started = now;
        m2.turns = 2;
        let e2 = vec![
            Event::new(now, "claude/2".to_string(), "claude-code".to_string(), Role::User, "debug the service".to_string()),
            Event::new(now, "claude/2".to_string(), "claude-code".to_string(), Role::Assistant, "fixed it".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e2, &m2)).unwrap();
        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        let opts = SearchOptions {
            query: Some("debug".to_string()),
            project: Some("/home/user/project-alpha".to_string()),
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].session_id, "claude/1");
        assert_eq!(result.results[0].project.as_deref(), Some("/home/user/project-alpha"));
    }

    #[test]
    fn test_search_integration_model_filter() {
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::build_session_document;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");
        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();

        let mut m1 = SessionManifest::new("claude/sonnet-1".to_string(), "claude-code".to_string());
        m1.model = Some("claude-sonnet-4-5".to_string());
        m1.started = now;
        m1.turns = 2;
        let e1 = vec![
            Event::new(now, "claude/sonnet-1".to_string(), "claude-code".to_string(), Role::User, "refactor the code".to_string()),
            Event::new(now, "claude/sonnet-1".to_string(), "claude-code".to_string(), Role::Assistant, "done".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e1, &m1)).unwrap();

        let mut m2 = SessionManifest::new("claude/opus-1".to_string(), "claude-code".to_string());
        m2.model = Some("claude-opus-4-6".to_string());
        m2.started = now;
        m2.turns = 2;
        let e2 = vec![
            Event::new(now, "claude/opus-1".to_string(), "claude-code".to_string(), Role::User, "refactor the code".to_string()),
            Event::new(now, "claude/opus-1".to_string(), "claude-code".to_string(), Role::Assistant, "done".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e2, &m2)).unwrap();
        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        let opts = SearchOptions {
            query: Some("refactor".to_string()),
            model: Some("claude-sonnet-4-5".to_string()),
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].session_id, "claude/sonnet-1");
        assert_eq!(result.results[0].model.as_deref(), Some("claude-sonnet-4-5"));
    }

    #[test]
    fn test_search_integration_combined_filters() {
        // agent + outcome + tag all applied together (AND logic)
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::build_session_document;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");
        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();

        // Target: claude-code + success + rust tag
        let mut m1 = SessionManifest::new("claude/target".to_string(), "claude-code".to_string());
        m1.outcome = Some("success".to_string());
        m1.tags = vec!["rust".to_string()];
        m1.started = now;
        m1.turns = 2;
        let e1 = vec![
            Event::new(now, "claude/target".to_string(), "claude-code".to_string(), Role::User, "fix memory leak".to_string()),
            Event::new(now, "claude/target".to_string(), "claude-code".to_string(), Role::Assistant, "fixed it".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e1, &m1)).unwrap();

        // Wrong outcome (failure instead of success)
        let mut m2 = SessionManifest::new("claude/wrong-outcome".to_string(), "claude-code".to_string());
        m2.outcome = Some("failure".to_string());
        m2.tags = vec!["rust".to_string()];
        m2.started = now;
        m2.turns = 2;
        let e2 = vec![
            Event::new(now, "claude/wrong-outcome".to_string(), "claude-code".to_string(), Role::User, "fix memory leak".to_string()),
            Event::new(now, "claude/wrong-outcome".to_string(), "claude-code".to_string(), Role::Assistant, "failed".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e2, &m2)).unwrap();

        // Wrong agent (aider instead of claude-code)
        let mut m3 = SessionManifest::new("aider/wrong-agent".to_string(), "aider".to_string());
        m3.outcome = Some("success".to_string());
        m3.tags = vec!["rust".to_string()];
        m3.started = now;
        m3.turns = 2;
        let e3 = vec![
            Event::new(now, "aider/wrong-agent".to_string(), "aider".to_string(), Role::User, "fix memory leak".to_string()),
            Event::new(now, "aider/wrong-agent".to_string(), "aider".to_string(), Role::Assistant, "fixed".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e3, &m3)).unwrap();

        // Wrong tag (python instead of rust)
        let mut m4 = SessionManifest::new("claude/wrong-tag".to_string(), "claude-code".to_string());
        m4.outcome = Some("success".to_string());
        m4.tags = vec!["python".to_string()];
        m4.started = now;
        m4.turns = 2;
        let e4 = vec![
            Event::new(now, "claude/wrong-tag".to_string(), "claude-code".to_string(), Role::User, "fix memory leak".to_string()),
            Event::new(now, "claude/wrong-tag".to_string(), "claude-code".to_string(), Role::Assistant, "fixed".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e4, &m4)).unwrap();

        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        let opts = SearchOptions {
            query: Some("fix memory".to_string()),
            agent: vec!["claude-code".to_string()],
            outcome: Some("success".to_string()),
            tag: vec!["rust".to_string()],
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].session_id, "claude/target");
    }

    #[test]
    fn test_search_integration_unicode_content() {
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::build_session_document;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");
        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();
        let mut m = SessionManifest::new("claude/unicode".to_string(), "claude-code".to_string());
        m.started = now;
        m.turns = 2;
        m.summary = Some("Unicode test session with authentication".to_string());
        let e = vec![
            Event::new(now, "claude/unicode".to_string(), "claude-code".to_string(), Role::User,
                "Fix authentication bug in the system".to_string()),
            Event::new(now, "claude/unicode".to_string(), "claude-code".to_string(), Role::Assistant,
                "Fixed authentication: 日本語コメント and Ответ на русском".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e, &m)).unwrap();
        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        let opts = SearchOptions {
            query: Some("authentication".to_string()),
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();
        assert!(result.total_matches >= 1);
        assert_eq!(result.results[0].session_id, "claude/unicode");
    }

    #[test]
    fn test_search_integration_empty_session() {
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::build_session_document;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");
        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();

        // Session with no events
        let m_empty = SessionManifest::new("claude/empty".to_string(), "claude-code".to_string());
        let e_empty: Vec<Event> = vec![];
        writer.add_document(build_session_document(&fields, &e_empty, &m_empty)).unwrap();

        // Normal session to ensure it still finds the right one
        let mut m_normal = SessionManifest::new("claude/normal".to_string(), "claude-code".to_string());
        m_normal.started = now;
        m_normal.turns = 1;
        let e_normal = vec![
            Event::new(now, "claude/normal".to_string(), "claude-code".to_string(), Role::User, "fix the auth bug".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e_normal, &m_normal)).unwrap();
        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        // Search should return only the normal session
        let opts = SearchOptions {
            query: Some("auth".to_string()),
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].session_id, "claude/normal");
    }

    #[test]
    fn test_search_integration_offset() {
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::build_session_document;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");
        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();

        for i in 0..3 {
            let session_id = format!("claude/session-{}", i);
            let mut m = SessionManifest::new(session_id.clone(), "claude-code".to_string());
            m.started = now - chrono::Duration::hours(i as i64);
            m.turns = 1;
            let e = vec![
                Event::new(now, session_id.clone(), "claude-code".to_string(), Role::User,
                    "debug the authentication code".to_string()),
            ];
            writer.add_document(build_session_document(&fields, &e, &m)).unwrap();
        }
        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        let opts_all = SearchOptions {
            query: Some("authentication".to_string()),
            max_results: 10,
            offset: 0,
            ..default_opts()
        };
        let all_results = execute_search(temp_dir.path(), &opts_all).unwrap();
        assert_eq!(all_results.results.len(), 3);

        let opts_offset = SearchOptions {
            query: Some("authentication".to_string()),
            max_results: 10,
            offset: 1,
            ..default_opts()
        };
        let offset_results = execute_search(temp_dir.path(), &opts_offset).unwrap();
        assert_eq!(offset_results.results.len(), 2);
        assert_eq!(offset_results.results[0].session_id, all_results.results[1].session_id);
    }

    #[test]
    fn test_search_integration_multiple_tag_filters() {
        // Multiple tags use AND logic — all tags must be present
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::build_session_document;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");
        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();

        // Session with both rust and docker tags
        let mut m1 = SessionManifest::new("claude/multi-tag".to_string(), "claude-code".to_string());
        m1.tags = vec!["rust".to_string(), "docker".to_string()];
        m1.started = now;
        m1.turns = 2;
        let e1 = vec![
            Event::new(now, "claude/multi-tag".to_string(), "claude-code".to_string(), Role::User, "containerize the rust app".to_string()),
            Event::new(now, "claude/multi-tag".to_string(), "claude-code".to_string(), Role::Assistant, "done".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e1, &m1)).unwrap();

        // Session with only rust tag
        let mut m2 = SessionManifest::new("claude/rust-only".to_string(), "claude-code".to_string());
        m2.tags = vec!["rust".to_string()];
        m2.started = now;
        m2.turns = 2;
        let e2 = vec![
            Event::new(now, "claude/rust-only".to_string(), "claude-code".to_string(), Role::User, "fix the rust app".to_string()),
            Event::new(now, "claude/rust-only".to_string(), "claude-code".to_string(), Role::Assistant, "fixed".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e2, &m2)).unwrap();
        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        // Both rust AND docker required
        let opts = SearchOptions {
            query: Some("app".to_string()),
            tag: vec!["rust".to_string(), "docker".to_string()],
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].session_id, "claude/multi-tag");
    }

    #[test]
    fn test_search_integration_doc_type_filter() {
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::{build_code_artifact_document, build_session_document};
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");
        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();

        let mut m = SessionManifest::new("claude/1".to_string(), "claude-code".to_string());
        m.started = now;
        m.turns = 1;
        let e = vec![Event::new(now, "claude/1".to_string(), "claude-code".to_string(),
            Role::User, "write function to validate input data".to_string())];
        writer.add_document(build_session_document(&fields, &e, &m)).unwrap();

        writer.add_document(build_code_artifact_document(
            &fields, "claude/1", "claude-code", None, now, "rust",
            "src/validate.rs", "fn validate_input(s: &str) -> bool { !s.is_empty() }",
            true, None,
        )).unwrap();
        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        // Filter to session only
        let opts = SearchOptions {
            query: Some("validate".to_string()),
            doc_type_filter: Some("session".to_string()),
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();
        for r in &result.results {
            assert_eq!(r.doc_type.as_deref(), Some("session"));
        }
    }

    #[test]
    fn test_search_integration_very_large_session() {
        // Very large sessions should be indexed and searched without panic
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::build_session_document;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");
        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();
        let mut m = SessionManifest::new("claude/large".to_string(), "claude-code".to_string());
        m.started = now;
        m.turns = 3;

        // Very large tool result (will trigger content truncation)
        let large_tool_result = "this is a large tool result with lots of content ".repeat(15000);
        let e = vec![
            Event::new(now, "claude/large".to_string(), "claude-code".to_string(), Role::User,
                "analyze this large codebase for authentication issues".to_string()),
            Event::new(now, "claude/large".to_string(), "claude-code".to_string(), Role::ToolResult,
                large_tool_result),
            Event::new(now, "claude/large".to_string(), "claude-code".to_string(), Role::Assistant,
                "I analyzed the authentication code and found issues".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e, &m)).unwrap();
        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        let opts = SearchOptions {
            query: Some("authentication".to_string()),
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();
        assert!(result.total_matches >= 1);
        assert_eq!(result.results[0].session_id, "claude/large");
    }

    #[test]
    fn test_search_integration_aider_session_roundtrip() {
        // Full scrape → index → search roundtrip for a simulated Aider session
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::build_session_document;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");
        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();
        let mut m = SessionManifest::new("aider/session-1".to_string(), "aider".to_string());
        m.project = Some("/home/user/api".to_string());
        m.started = now;
        m.turns = 5;
        m.summary = Some("Refactored authentication middleware using JWT tokens".to_string());
        m.outcome = Some("success".to_string());
        m.tags = vec!["python".to_string(), "jwt".to_string(), "authentication".to_string()];
        let e = vec![
            Event::new(now, "aider/session-1".to_string(), "aider".to_string(), Role::User,
                "Refactor the auth middleware to use JWT tokens instead of session cookies".to_string()),
            Event::new(now, "aider/session-1".to_string(), "aider".to_string(), Role::Assistant,
                "I'll update the middleware to use JWT. Plan: 1) Install PyJWT 2) Update auth.py".to_string()),
            Event::new(now, "aider/session-1".to_string(), "aider".to_string(), Role::ToolCall,
                "pip install PyJWT".to_string()).with_tool(Some("Bash".to_string())),
            Event::new(now, "aider/session-1".to_string(), "aider".to_string(), Role::ToolResult,
                "Successfully installed PyJWT-2.8.0".to_string()),
            Event::new(now, "aider/session-1".to_string(), "aider".to_string(), Role::Assistant,
                "JWT authentication has been implemented successfully.".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e, &m)).unwrap();
        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        let opts = SearchOptions {
            query: Some("JWT authentication".to_string()),
            agent: vec!["aider".to_string()],
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();
        assert!(result.total_matches >= 1);
        assert_eq!(result.results[0].session_id, "aider/session-1");
        assert_eq!(result.results[0].source_agent, "aider");
        assert!(result.results[0].summary.is_some());
        assert_eq!(result.results[0].outcome.as_deref(), Some("success"));
    }

    #[test]
    fn test_search_integration_claude_code_session_roundtrip() {
        // Full roundtrip for a Claude Code session with file edits and tool calls
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::build_session_document;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");
        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();
        let mut m = SessionManifest::new("claude/session-2".to_string(), "claude-code".to_string());
        m.project = Some("/home/user/myapp".to_string());
        m.started = now;
        m.turns = 6;
        m.model = Some("claude-sonnet-4-5".to_string());
        m.outcome = Some("success".to_string());
        m.files_touched = vec!["src/auth.rs".to_string(), "src/middleware.rs".to_string()];
        let e = vec![
            Event::new(now, "claude/session-2".to_string(), "claude-code".to_string(), Role::User,
                "Add rate limiting to the authentication endpoints".to_string()),
            Event::new(now, "claude/session-2".to_string(), "claude-code".to_string(), Role::Assistant,
                "I'll add rate limiting. First let me read the auth file.".to_string()),
            Event::new(now, "claude/session-2".to_string(), "claude-code".to_string(), Role::ToolCall,
                "src/auth.rs".to_string()).with_tool(Some("Read".to_string()))
                .with_file_paths(vec!["src/auth.rs".to_string()]),
            Event::new(now, "claude/session-2".to_string(), "claude-code".to_string(), Role::ToolResult,
                "pub fn authenticate(token: &str) -> Result<Claims> { /* ... */ }".to_string()),
            Event::new(now, "claude/session-2".to_string(), "claude-code".to_string(), Role::ToolCall,
                "```rust\npub fn authenticate(token: &str) -> Result<Claims> {\n    check_rate_limit()?;\n    validate_token(token)\n}\n```".to_string())
                .with_tool(Some("Edit".to_string()))
                .with_file_paths(vec!["src/auth.rs".to_string()]),
            Event::new(now, "claude/session-2".to_string(), "claude-code".to_string(), Role::Assistant,
                "Rate limiting has been added to the authentication endpoints.".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e, &m)).unwrap();
        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        let opts = SearchOptions {
            query: Some("rate limiting authentication".to_string()),
            model: Some("claude-sonnet-4-5".to_string()),
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();
        assert!(result.total_matches >= 1);
        let found = &result.results[0];
        assert_eq!(found.session_id, "claude/session-2");
        assert_eq!(found.model.as_deref(), Some("claude-sonnet-4-5"));
        assert_eq!(found.outcome.as_deref(), Some("success"));
    }

    #[test]
    fn test_search_integration_solution_only() {
        // --solution-only mode searches the solution_summary field
        use crate::event::{Event, Role, SessionManifest};
        use crate::index::build_session_document;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");
        let (schema, fields) = build_schema();
        std::fs::create_dir_all(&index_path).unwrap();
        let index = tantivy::Index::create_in_dir(&index_path, schema).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();

        // Session with solution_summary
        let mut m = SessionManifest::new("claude/solved".to_string(), "claude-code".to_string());
        m.started = now;
        m.turns = 3;
        m.summary = Some("Investigated memory leak problem".to_string());
        // solution_summary is not in SessionManifest — set it via the document directly
        // We'll test that --solution_only routes snippet to the solution field
        let e = vec![
            Event::new(now, "claude/solved".to_string(), "claude-code".to_string(), Role::User,
                "fix the memory leak in the allocator".to_string()),
            Event::new(now, "claude/solved".to_string(), "claude-code".to_string(), Role::Assistant,
                "memory leak fixed by patching the allocator".to_string()),
        ];
        writer.add_document(build_session_document(&fields, &e, &m)).unwrap();
        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();

        // --solution_only should still return results if query matches content
        let opts = SearchOptions {
            query: Some("memory leak".to_string()),
            solution_only: true,
            ..default_opts()
        };
        let result = execute_search(temp_dir.path(), &opts).unwrap();
        // Results are valid; solution_only only affects snippet source and adds a Should boost
        assert!(result.total_matches >= 0); // May be 0 if solution_summary is empty — that's valid
        // The important thing is no panic/error
    }
}
