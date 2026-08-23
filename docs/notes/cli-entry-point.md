# CLI Parsing Entry Point

## Location

**Main entry:** `src/main.rs` (minimal - just calls `run()` from CLI module)

**Primary CLI definition:** `src/cli.rs` (137KB - all command parsing logic)

**CLI Framework:** Clap (derive API with Parser, Subcommand, CommandFactory)

## Key Structures

### cli.rs
- `Args` struct (line 20) - top-level command parser with `#[command(subcommand)]`
- `Commands` enum (line 26) - all subcommands (Config, Plugins, Scrape, Index, Search, etc.)
- Each subcommand has its own clap-derived struct for flags

### SearchOptions struct
**Location:** `src/search.rs` (lines 192-226)

**Purpose:** Internal search query options struct that maps CLI flags to search parameters

**Key fields:**
- `query: Option<String>` - main search query
- `agent: Vec<String>` - filter by agent type (repeatable)
- `tag: Vec<String>` - filter by tag (repeatable, AND logic)
- `project: Option<String>` - filter by project path
- `since/before: Option<DateTime<Utc>>` - date range filters
- `outcome: Option<String>` - filter by outcome (success/failure/abandoned/unknown)
- `max_results: usize` - maximum results
- `token_budget: Option<usize>` - context budget packing
- `semantic: bool` - enable vector search
- `hybrid: bool` - enable BM25 + semantic hybrid
- `file_path: Option<String>` - filter by file path
- And more...

## Pattern for Adding CLI Flags

1. Add field to the appropriate command struct in `cli.rs` with clap attributes:
   - `#[arg(short, long)]` for short+long flags
   - `#[arg(long)]` for long-only
   - `Vec<Type>` for repeatable flags
   - `#[arg(long, default_value = "10")]` for defaults

2. Map the field to internal options struct (e.g., SearchOptions)

3. Add processing logic in the command handler function

## Why This Matters

This entry point is critical for the next bead: finding the SearchOptions struct so we can add tag filtering support to the search CLI.
