# Search CLI Options Structure Documentation

## Overview

This document describes where the search command CLI options are defined in the AgentScribe codebase and how to add new flags.

## File Locations

### 1. CLI Command Definition
**File:** `/home/coding/AgentScribe/src/cli.rs`

**Lines:** 82-193 (Search command variant)

**Structure:** The search command is defined as a variant in the `Commands` enum:

```rust
#[derive(Subcommand, Debug)]
enum Commands {
    // ... other commands ...
    
    /// Query the Tantivy index for matching sessions
    Search {
        /// Search query string (Tantivy query syntax)
        query: Option<String>,

        /// Filter by tag (repeatable, AND logic)
        #[arg(short = 't', long)]
        tag: Vec<String>,
        
        /// Filter by source agent type (repeatable)
        #[arg(short, long)]
        agent: Vec<String>,
        
        // ... more fields ...
    },
}
```

### 2. Internal Search Options Struct
**File:** `/home/coding/AgentScribe/src/search.rs`

**Lines:** 192-226

**Structure:** The internal search options struct that receives the CLI parameters:

```rust
pub struct SearchOptions {
    pub query: Option<String>,
    pub tag: Vec<String>,
    pub agent: Vec<String>,
    pub project: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub before: Option<DateTime<Utc>>,
    pub outcome: Option<String>,
    // ... more fields ...
}
```

## How CLI Flags Are Implemented

### Pattern: Clap Field Attributes

All CLI options use **clap** (a Rust argument parsing library) with these patterns:

#### 1. Simple Optional String
```rust
/// Project path filter
#[arg(long)]
project: Option<String>,
```
Usage: `--project /path/to/project`

#### 2. Short and Long Flags
```rust
/// Filter by tag (repeatable, AND logic)
#[arg(short = 't', long)]
tag: Vec<String>,
```
Usage: `-t rust -t postgres` or `--tag rust --tag postgres`

#### 3. Repeatable Flags (Vec)
```rust
/// Filter by source agent type (repeatable)
#[arg(short, long)]
agent: Vec<String>,
```
Usage: `-a claude-code -a aider` or `--agent claude-code --agent aider`

#### 4. Boolean Flags
```rust
/// Enable fuzzy matching on all query terms
#[arg(long)]
fuzzy: bool,
```
Usage: `--fuzzy` (true) or omit (false)

#### 5. Default Values
```rust
/// Maximum number of results
#[arg(short = 'n', long, default_value = "10")]
max_results: usize,
```
Usage: `-n 20` or `--max-results 20` (defaults to 10)

## Location of `--tag` Flag

### ✓ ALREADY IMPLEMENTED

The `--tag` flag is **already implemented** in the search CLI:

**Location:** `src/cli.rs` lines 126-128

```rust
/// Filter by tag (repeatable, AND logic)
#[arg(short = 't', long)]
tag: Vec<String>,
```

**Usage Examples:**
```bash
# Single tag
agentscribe search "database" --tag rust

# Multiple tags (AND logic - all must match)
agentscribe search "database" -t rust -t postgres
agentscribe search "database" --tag rust --tag postgres --tag migration

# With other filters
agentscribe search "auth" --tag rust --agent claude-code --outcome success
```

## How to Add a New CLI Flag

### Step 1: Add to CLI Definition (src/cli.rs)

Add the field to the `Commands::Search` variant:

```rust
Search {
    // ... existing fields ...
    
    /// Your new flag description
    #[arg(long)]
    new_flag: Option<String>,
    
    // ... more fields ...
}
```

### Step 2: Add to run_search Function Parameters

Update the function signature at line ~1202:

```rust
fn run_search(
    // ... existing parameters ...
    new_flag: Option<String>,
    // ... more parameters ...
) -> Result<()> {
```

### Step 3: Add to SearchOptions Struct (src/search.rs)

Add the field to the `SearchOptions` struct at line 192:

```rust
pub struct SearchOptions {
    // ... existing fields ...
    
    /// Your new flag description
    pub new_flag: Option<String>,
    
    // ... more fields ...
}
```

### Step 4: Update Default Implementation

Add the default value in the `Default` implementation at line 229:

```rust
impl Default for SearchOptions {
    fn default() -> Self {
        SearchOptions {
            // ... existing fields ...
            
            new_flag: None,
            
            // ... more fields ...
        }
    }
}
```

### Step 5: Wire It Up in run_search

Add the parameter to the SearchOptions construction at line ~1247:

```rust
let opts = SearchOptions {
    // ... existing fields ...
    
    new_flag,
    
    // ... more fields ...
};
```

### Step 6: Implement the Logic

Add the actual filtering/search logic in the `execute_search` function in `src/search.rs`.

## Complete Search CLI Flag Reference

| Flag | Short | Type | Repeatable | Description |
|------|-------|------|------------|-------------|
| `--tag` | `-t` | `Vec<String>` | ✓ | Filter by tags (AND logic) |
| `--agent` | `-a` | `Vec<String>` | ✓ | Filter by source agent type |
| `--project` | | `Option<String>` | | Filter by project path |
| `--since` | | `Option<String>` | | Only match sessions after timestamp |
| `--before` | | `Option<String>` | | Only match sessions before timestamp |
| `--outcome` | | `Option<String>` | | Filter by outcome (success/failure/abandoned/unknown) |
| `--model` | | `Option<String>` | | Filter by LLM model name |
| `--session-type` | | `Option<String>` | | Filter by session type (debug/feature/refactor/etc) |
| `--type` | | `Option<String>` | | Filter by doc type (session/code_artifact) |
| `--error` | | `Option<String>` | | Error fingerprint pattern to search |
| `--code` | | `Option<String>` | | Code content query |
| `--lang` | | `Option<String>` | | Language filter for code search |
| `--solution-only` | | `bool` | | Return only extracted solutions |
| `--like` | | `Option<String>` | | Find sessions similar to this session ID |
| `--session` | | `Option<String>` | | Retrieve a specific session by ID |
| `--anti-patterns` | | `bool` | | Filter to sessions with anti-patterns detected |
| `--semantic` | | `bool` | | Enable semantic vector search (STUB/NON-FUNCTIONAL) |
| `--hybrid` | | `bool` | | Enable hybrid search (STUB/NON-FUNCTIONAL) |
| `--fuzzy` | | `bool` | | Enable fuzzy matching on all query terms |
| `--edit-distance` | | `Option<u8>` | | Levenshtein edit distance for fuzzy matching |
| `--max-results` | `-n` | `usize` | | Maximum number of results (default: 10) |
| `--snippet-length` | | `usize` | | Maximum snippet length per result (default: 200) |
| `--token-budget` | | `Option<usize>` | | Token budget for greedy knapsack context packing |
| `--offset` | | `usize` | | Skip first N results (pagination, default: 0) |
| `--sort` | `-s` | `String` | | Sort order: relevance/newest/oldest/turns (default: relevance) |
| `--hint` | | `bool` | | Output a single-line hint (for shell hook integration) |
| `--json` | | `bool` | | JSON structured output |

## Data Flow

```
CLI Input (Command Line)
    ↓
clap::Parser (src/cli.rs::Args::parse)
    ↓
Commands::Search enum variant
    ↓
run_search() function (src/cli.rs)
    ↓
SearchOptions struct (src/search.rs)
    ↓
execute_search() function (src/search.rs)
    ↓
SearchOutput struct (results)
    ↓
CLI Output (human or JSON format)
```

## Testing New Flags

After adding a new flag, test it:

```bash
# Test the flag appears in help
agentscribe search --help

# Test the flag works
agentscribe search "query" --your-flag value

# Test with JSON output
agentscribe search "query" --your-flag value --json

# Test with other filters
agentscribe search "query" --your-flag value --tag rust --agent claude-code
```

## Related Files

- `src/cli.rs` - CLI command definitions and argument parsing
- `src/search.rs` - Search execution and SearchOptions struct
- `src/index.rs` - Tantivy index schema and field definitions
- `src/config.rs` - Configuration options and defaults

## Notes

- All CLI options use **clap** derive macros (`Parser`, `Subcommand`)
- Boolean flags are `false` by default, become `true` when present
- `Vec<String>` fields are repeatable and use AND logic
- `Option<T>` fields are optional
- Default values are specified with `default_value` or `default_value_t`
- Short flags are single characters (e.g., `-t`, `-n`)
- Long flags are full words (e.g., `--tag`, `--max-results`)
