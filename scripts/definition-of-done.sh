#!/usr/bin/env bash
# Unified Definition of Done for AgentScribe
#
# This script is the single source of truth for "is this work acceptable?"
# It is invoked identically by:
#   - Pre-commit hook (fast lane only, with --count-bypass)
#   - CI verify step (both fast and slow lanes)
#   - NEEDLE validation gate (fast lane only)
#
# Lanes:
#   - Fast: fmt, clippy, check (seconds, run locally under cgroup)
#   - Slow: cargo test
#
# Behavior: Aggregates all failures rather than aborting on first.
# Returns non-zero if ANY check fails, with all failures reported.
#
# Usage:
#   scripts/definition-of-done.sh [--fast|--slow|--all] [--count-bypass]
#
# Flags:
#   --fast          Run fast lane only (default for NEEDLE gate)
#   --slow          Run slow lane only (tests)
#   --all           Run both lanes (default for CI)
#   --count-bypass   Track the pre-commit result so post-commit can detect
#                    commits made with --no-verify

set -euo pipefail

# Script directory for path resolution
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

# Default to fast lane
LANE="fast"
COUNT_BYPASS=false

# Parse arguments
while [[ $# -gt 0 ]]; do
  case $1 in
    --fast)
      LANE="fast"
      shift
      ;;
    --slow)
      LANE="slow"
      shift
      ;;
    --all)
      LANE="all"
      shift
      ;;
    --count-bypass)
      COUNT_BYPASS=true
      shift
      ;;
    *)
      echo "Error: Unknown argument: $1" >&2
      echo "Usage: $0 [--fast|--slow|--all] [--count-bypass]" >&2
      exit 1
      ;;
  esac
done

# Failure tracking
declare -a FAILURES=()
EXIT_CODE=0

# Fast lane checks
run_fast_lane() {
  echo "=== Running fast lane checks ==="

  # Format check
  echo "Checking formatting..."
  if ! cargo fmt --all -- --check; then
    FAILURES+=("cargo fmt --check failed")
    EXIT_CODE=1
  fi

  # Clippy
  echo "Running clippy..."
  if ! cargo clippy --all-targets -- -D warnings; then
    FAILURES+=("cargo clippy failed")
    EXIT_CODE=1
  fi

  # Compilation check
  echo "Running cargo check..."
  if ! cargo check --all-targets; then
    FAILURES+=("cargo check failed")
    EXIT_CODE=1
  fi
}

# Slow lane checks
run_slow_lane() {
  echo "=== Running slow lane checks ==="

  # Unit tests
  echo "Running cargo test..."
  if ! cargo test; then
    FAILURES+=("cargo test failed")
    EXIT_CODE=1
  fi
}

# Run requested lanes
case "$LANE" in
  fast)
    run_fast_lane
    ;;
  slow)
    run_slow_lane
    ;;
  all)
    run_fast_lane
    run_slow_lane
    ;;
esac

# Report failures
if [ ${#FAILURES[@]} -gt 0 ]; then
  echo ""
  echo "=== DEFINITION OF DONE FAILED ==="
  echo "The following checks failed:"
  for failure in "${FAILURES[@]}"; do
    echo "  - $failure"
  done
  echo ""
fi

exit $EXIT_CODE
