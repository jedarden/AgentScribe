//! AgentScribe CLI entry point.

use agentscribe::cli::run;
use agentscribe::error::Result;

fn main() -> Result<()> {
    run()
}
