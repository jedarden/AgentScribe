//! Search command implementation
//!
//! Provides full-text BM25 search, fuzzy search, error lookup, code search,
//! and various filter/output modes against the Tantivy index.

use crate::error::{AgentScribeError, Result};
use crate::index::{build_schema, IndexFields};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tantivy::collector::TopDocs;
use tantivy::query::{
    BooleanQuery, BoostQuery, FuzzyTermQuery, Occur, PhrasePrefixQuery, Query,
    RangeQuery, TermQuery,
};
use tantivy::schema::Field;
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
}

/// Search output for JSON mode
#[derive(Debug, Serialize)]
pub struct SearchOutput {
    pub query: String,
    pub total_matches: usize,
    pub search_time_ms: u64,
    pub sessions_searched: u64,
    pub results: Vec<SearchResult>,
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
    let top_docs: Vec<(f32, DocAddress)> = searcher
        .search(&query, &TopDocs::with_limit(fetch_limit))
        .map_err(|e| AgentScribeError::DataDir(format!("Search failed: {}", e)))?;

    // Apply offset
    let top_docs: Vec<_> = top_docs.into_iter().skip(opts.offset).collect();

    // Convert to SearchResult
    let mut results: Vec<SearchResult> = Vec::new();
    for (score, doc_addr) in &top_docs {
        if results.len() >= opts.max_results {
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
        results = knapsack_pack(results, budget, opts.snippet_length);
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
    })
}

/// Build a Tantivy query from search options.
fn build_query(
    searcher: &Searcher,
    fields: &IndexFields,
    opts: &SearchOptions,
    schema: &tantivy::schema::Schema,
) -> Result<Box<dyn Query>> {
    let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();

    // Main text query
    if let Some(ref query_str) = opts.query {
        let query = if opts.fuzzy {
            build_fuzzy_query(fields, query_str)?
        } else {
            build_fulltext_query(fields, query_str)?
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
            build_fuzzy_query_for_field(fields.code_content, code_q)?
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

    // --like <session-id>: find sessions with similar content (term-based similarity)
    if let Some(ref like_id) = opts.like_session {
        let like_term = tantivy::schema::Term::from_field_text(fields.session_id, like_id);
        // Try to find the document to extract terms from it
        if let Ok(term_query) = build_more_like_this(searcher, fields, like_id) {
            clauses.push((Occur::Must, term_query));
        } else {
            // Fallback: just search for the session ID
            clauses.push((
                Occur::Should,
                Box::new(TermQuery::new(
                    like_term,
                    tantivy::schema::IndexRecordOption::Basic,
                )),
            ));
        }
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
fn build_fulltext_query(fields: &IndexFields, query_str: &str) -> Result<Box<dyn Query>> {
    let query = BooleanQuery::new(vec![
        (
            Occur::Should,
            Box::new(BoostQuery::new(
                Box::new(PhrasePrefixQuery::new(vec![
                    tantivy::schema::Term::from_field_text(fields.content, query_str),
                ])),
                1.0,
            )),
        ),
        (
            Occur::Should,
            Box::new(BoostQuery::new(
                Box::new(TermQuery::new(
                    tantivy::schema::Term::from_field_text(fields.summary, query_str),
                    tantivy::schema::IndexRecordOption::Basic,
                )),
                1.5,
            )),
        ),
    ]);
    Ok(Box::new(query))
}

/// Build a fuzzy query for all query terms across content and summary.
fn build_fuzzy_query(fields: &IndexFields, query_str: &str) -> Result<Box<dyn Query>> {
    let terms: Vec<(Occur, Box<dyn Query>)> = query_str
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .flat_map(|word| {
            let mut sub = Vec::new();
            let term_content =
                tantivy::schema::Term::from_field_text(fields.content, word);
            sub.push((
                Occur::Should,
                Box::new(FuzzyTermQuery::new(term_content, 1, true)) as Box<dyn Query>,
            ));
            let term_summary =
                tantivy::schema::Term::from_field_text(fields.summary, word);
            sub.push((
                Occur::Should,
                Box::new(FuzzyTermQuery::new(term_summary, 1, true)) as Box<dyn Query>,
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
fn build_fuzzy_query_for_field(field: Field, query_str: &str) -> Result<Box<dyn Query>> {
    let terms: Vec<(Occur, Box<dyn Query>)> = query_str
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .map(|word| {
            let term = tantivy::schema::Term::from_field_text(field, word);
            (
                Occur::Should,
                Box::new(FuzzyTermQuery::new(term, 1, true)) as Box<dyn Query>,
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

/// Build a "more like this" query by extracting significant terms from a document.
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
            "Session '{}' not found",
            session_id
        )));
    }

    let doc: TantivyDocument = searcher
        .doc(docs[0].1)
        .map_err(|e| AgentScribeError::DataDir(format!("Failed to fetch document: {}", e)))?;

    // Extract text terms from the content field
    let content_terms = doc
        .get_all(fields.content)
        .filter_map(|v| v.as_str())
        .flat_map(|s| {
            s.split_whitespace()
                .filter(|w| w.len() > 3) // Skip short words
                .map(|w| w.to_lowercase())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    // Deduplicate and take top terms
    let mut seen = std::collections::HashSet::new();
    let mut unique_terms = Vec::new();
    for term in content_terms {
        if seen.insert(term.clone()) {
            unique_terms.push(term);
        }
    }

    // Use up to 10 terms for the MLT query
    let top_terms: Vec<_> = unique_terms.into_iter().take(10).collect();

    if top_terms.is_empty() {
        return Err(AgentScribeError::DataDir(
            "No content terms found in session".to_string(),
        ));
    }

    let mlt_clauses: Vec<(Occur, Box<dyn Query>)> = top_terms
        .iter()
        .map(|term_str| {
            let term = tantivy::schema::Term::from_field_text(fields.content, term_str);
            (
                Occur::Should,
                Box::new(TermQuery::new(
                    term,
                    tantivy::schema::IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            )
        })
        .collect();

    // Exclude the original session from results
    let exclude_term = tantivy::schema::Term::from_field_text(fields.session_id, session_id);
    let mut all_clauses = vec![(
        Occur::MustNot,
        Box::new(TermQuery::new(
            exclude_term,
            tantivy::schema::IndexRecordOption::Basic,
        )) as Box<dyn Query>,
    )];
    all_clauses.extend(mlt_clauses);

    Ok(Box::new(BooleanQuery::new(all_clauses)))
}

/// Convert a Tantivy document to a SearchResult.
fn doc_to_search_result(
    searcher: &Searcher,
    fields: &IndexFields,
    doc_addr: DocAddress,
    score: f32,
    opts: &SearchOptions,
) -> Option<SearchResult> {
    let doc = searcher.doc(doc_addr).ok()?;

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
        .and_then(|v| v.as_date())
        .map(|dt| {
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
    let (schema, fields) = build_schema();

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
/// Each result costs tokens proportional to its text length (snippet + summary).
/// Results are sorted by score (value density), then greedily selected.
fn knapsack_pack(results: Vec<SearchResult>, token_budget: usize, snippet_length: usize) -> Vec<SearchResult> {
    let mut items: Vec<(usize, SearchResult)> = results
        .into_iter()
        .enumerate()
        .map(|(i, r)| {
            let text_len = r.snippet.as_ref().map(|s| s.len()).unwrap_or(0)
                + r.summary.as_ref().map(|s| s.len()).unwrap_or(0);
            let token_cost = (text_len / CHARS_PER_TOKEN).max(1);
            (i, (token_cost, r))
        })
        .map(|(i, (cost, r))| {
            // Value = score, cost = token estimate
            (cost, r)
        })
        .collect();

    // Sort by score descending (greedy by value)
    items.sort_by(|a, b| b.1.score.partial_cmp(&a.1.score).unwrap_or(std::cmp::Ordering::Equal));

    let mut selected = Vec::new();
    let mut remaining = token_budget;

    for (cost, result) in items {
        if cost <= remaining {
            // Truncate snippet to fit within remaining budget if needed
            let mut result = result;
            if let Some(ref snippet) = result.snippet {
                let snippet_tokens = snippet.len() / CHARS_PER_TOKEN;
                if snippet_tokens > remaining {
                    let max_chars = remaining * CHARS_PER_TOKEN;
                    result.snippet = extract_snippet(snippet, max_chars);
                }
            }
            remaining -= cost;
            selected.push(result);
        }
    }

    selected
}

/// Format search results for human-readable output.
pub fn format_human(output: &SearchOutput, snippet_length: usize) -> String {
    let mut lines = Vec::new();

    lines.push(format!(
        "{} result(s) for \"{}\" (searched {} sessions in {}ms)",
        output.total_matches,
        output.query,
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
        let packed = knapsack_pack(results, 10000, 200);
        assert_eq!(packed.len(), 2);
    }

    #[test]
    fn test_knapsack_pack_drops_low_value() {
        let results = vec![
            make_test_result("a", 10.0, 1000),
            make_test_result("b", 8.0, 1000),
            make_test_result("c", 5.0, 1000),
        ];
        // Budget of ~50 tokens: only 1-2 results fit
        let packed = knapsack_pack(results, 50, 200);
        assert!(packed.len() <= 2);
        // Highest scored result should be first
        assert_eq!(packed[0].session_id, "a");
    }

    #[test]
    fn test_knapsack_pack_empty() {
        let packed = knapsack_pack(vec![], 1000, 200);
        assert!(packed.is_empty());
    }

    #[test]
    fn test_format_human_empty() {
        let output = SearchOutput {
            query: "test".to_string(),
            total_matches: 0,
            search_time_ms: 1,
            sessions_searched: 100,
            results: vec![],
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
            }],
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
        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index").join("tantivy");

        let (schema, _) = build_schema();
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
        let temp_dir = TempDir::new().unwrap();
        let opts = default_opts();
        let result = execute_search(temp_dir.path(), &opts);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Index not found"));
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
            max_results: 10,
            snippet_length: 200,
            token_budget: None,
            offset: 0,
            sort: SortOrder::Relevance,
        }
    }

    // Helper to make a test search result
    fn make_test_result(id: &str, score: f32, text_len: usize) -> SearchResult {
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
        }
    }
}
