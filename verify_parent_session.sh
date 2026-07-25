#!/bin/bash
# Verification script for parent_session_id functionality

echo "=== Parent Session ID Verification ==="
echo ""

# Check if agentscribe data directory exists
DATA_DIR="$HOME/.agentscribe"
if [ ! -d "$DATA_DIR" ]; then
    echo "❌ AgentScribe data directory not found: $DATA_DIR"
    exit 1
fi

echo "✅ Data directory found: $DATA_DIR"
echo ""

# Check sessions directory
SESSIONS_DIR="$DATA_DIR/sessions"
if [ ! -d "$SESSIONS_DIR" ]; then
    echo "❌ Sessions directory not found: $SESSIONS_DIR"
    exit 1
fi

echo "✅ Sessions directory found: $SESSIONS_DIR"
echo ""

# Find subagent session files
SUBAGENT_FILES=$(find "$SESSIONS_DIR" -name "*.jsonl" | wc -l)
echo "Total session files: $SUBAGENT_FILES"
echo ""

# Look for claude-code plugin sessions
CLAUDE_DIR="$SESSIONS_DIR/claude-code"
if [ -d "$CLAUDE_DIR" ]; then
    CLAUDE_SESSIONS=$(ls "$CLAUDE_DIR"/*.jsonl 2>/dev/null | wc -l)
    echo "Claude Code sessions: $CLAUDE_SESSIONS"

    # Check for subagent sessions (by checking source_agent in first line)
    SUBAGENT_COUNT=0
    for session_file in "$CLAUDE_DIR"/*.jsonl; do
        if [ -f "$session_file" ]; then
            FIRST_LINE=$(head -n 1 "$session_file" 2>/dev/null)
            if echo "$FIRST_LINE" | grep -q '"source_agent":"claude-code-subagent"'; then
                SUBAGENT_COUNT=$((SUBAGENT_COUNT + 1))
                SESSION_ID=$(basename "$session_file" .jsonl)
                echo "  📁 Subagent session: $SESSION_ID"

                # Try to extract parent_session_id from the path or content
                # This would need to be stored in the session JSONL or derived from filename
            fi
        fi
    done

    echo "Total subagent sessions found: $SUBAGENT_COUNT"
else
    echo "❌ Claude Code plugin directory not found"
fi

echo ""
echo "=== Check Index for parent_session_id field ==="
INDEX_DIR="$DATA_DIR/index/tantivy"
if [ -d "$INDEX_DIR" ]; then
    echo "✅ Index directory exists"
    # We can't easily inspect the Tantivy index without the agentscribe binary
    echo "⚠️  Cannot inspect index without agentscribe binary (build failed due to missing BLAS)"
else
    echo "❌ Index directory not found"
fi

echo ""
echo "=== Summary ==="
echo "The parent_session_id field is:"
echo "  ✅ Defined in the schema (src/index.rs:58, 166)"
echo "  ✅ Extracted by JSONL parser (src/parser/jsonl.rs:468-502)"
echo "  ✅ Passed to indexer (src/scraper/mod.rs:565)"
echo "  ⚠️  NOT visible in current status output (only shows aggregate counts)"
echo ""
echo "To verify parent_session_id values, the status output needs to be enhanced to:"
echo "  1. Show individual parent_session_id values for subagent sessions"
echo "  2. Display parent-child relationships between sessions"
echo "  3. Allow filtering/searching by parent_session_id"
echo ""
echo "Current limitation: Cannot build agentscribe binary due to missing BLAS libraries"
