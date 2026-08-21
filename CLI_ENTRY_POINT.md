# AgentScribe CLI Entry Point Documentation

## Location
**Main CLI file**: `/home/coding/AgentScribe/src/cli.rs` (137,209 bytes)

## CLI Framework
- **Framework**: clap version 4.5 with derive macros
- **Features**: Uses `Parser` and `Subcommand` derive macros from `clap`
- **Dependencies**: 
  - `clap = { version = "4.5", features = ["derive"] }`
  - `clap_complete = "4.5"`

## Entry Point Flow
1. **main.rs** (line 6): `fn main() -> Result<()> { run() }`
2. **cli.rs** (line ~1-100+): Main CLI parsing using clap derive macros

## Key Structures

### CLI Arguments (src/cli.rs)
- **Args struct** (line 17): Main CLI parser with `#[derive(Parser)]`
- **Commands enum** (line 26): All subcommands with `#[derive(Subcommand)]`
  - Config, Plugins, Scrape, Index, Embed, Status, Search, etc.

### Search Options (src/search.rs)
- **SearchOptions struct** (lines 192-226): Internal search configuration
  - Maps CLI arguments to search parameters
  - Contains query, filters, output options, etc.

## Pattern Used
The CLI uses Rust clap derive macros with struct-based configuration:
- `#[command(subcommand)]` for nested commands
- `#[arg(short, long)]` for flags (e.g., `-h`, `--help`)
- `Vec<Type>` for repeatable options (e.g., multiple tags)
- `Option<Type>` for optional values
- Default values via `#[arg(long, default_value = "...")]`

## Next Step
This documentation enables finding the SearchOptions struct in the next bead for implementing search-related features.