# CLI Parsing Entry Point

## Location

**Main entry file:** `/home/coding/AgentScribe/src/main.rs`
- Simple entry point that calls `run()` from the cli module
- Only 9 lines of code

**CLI parsing implementation:** `/home/coding/AgentScribe/src/cli.rs`
- 137KB file containing all CLI command definitions
- Uses derive macros from Clap framework

## CLI Framework

**Framework:** Clap (v4) - Rust command-line argument parser
- Dependency: `clap` with features `derive` and `env`
- Used imports: `clap::{CommandFactory, Parser, Subcommand}`
- Also uses: `clap_complete::{generate, Shell}` for shell completions

## Architecture

### Command Structure
- Main struct: `Args` with derive `Parser`
- Commands enum: `Commands` with derive `Subcommand`
- Each CLI command is a variant in the `Commands` enum
- Subcommands use nested enum structures

### Search Options Location
- **CLI definition:** `Commands::Search` variant in `/home/coding/AgentScribe/src/cli.rs` (lines 81-208)
- **Internal struct:** `SearchOptions` in `/home/coding/AgentScribe/src/search.rs` (lines 192-226)
- **Pattern:** clap derive macros with struct fields using attributes like `#[arg(short, long)]`, `#[arg(long)]`, etc.

### Key Components
- `Args` struct - top-level CLI parser
- `Commands` enum - all subcommands (config, plugins, scrape, index, search, etc.)
- `run()` function - main execution entry point in cli.rs
- `SearchOptions` struct - internal search configuration

## Example Pattern

```rust
#[derive(Parser, Debug)]
#[command(name = "agentscribe")]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Search {
        #[arg(long)]
        flag_name: Option<String>,
    },
}
```

## Notes

- The code already has extensive inline documentation about the CLI structure
- Repeatable flags use `Vec<Type>` (e.g., `tag: Vec<String>`)
- Default values use `#[arg(long, default_value = "10")]`
- The SearchOptions struct is already well-documented with field-level comments
