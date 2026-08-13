#!/usr/bin/env bash

# Extract test imports for pattern analysis
OUTPUT_FILE="test_imports_analysis.json"
OUTPUT_MD="docs/test-imports-analysis.md"

echo "Extracting test imports from AgentScribe codebase..."

# Find all test files
TEST_FILES=$(find /home/coding/AgentScribe -type f -name "*.rs" | grep -E "(test|tests)" || find /home/coding/AgentScribe -type f -name "*test*.rs")

TOTAL_FILES=0
TOTAL_IMPORTS=0

# Initialize JSON structure
echo "{" > "$OUTPUT_FILE"
echo "  \"metadata\": {" >> "$OUTPUT_FILE"
echo "    \"generated_at\": \"$(date -u +"%Y-%m-%dT%H:%M:%SZ")\"," >> "$OUTPUT_FILE"
echo "    \"total_test_files\": 0," >> "$OUTPUT_FILE"
echo "    \"total_imports\": 0" >> "$OUTPUT_FILE"
echo "  }," >> "$OUTPUT_FILE"
echo "  \"by_category\": {}," >> "$OUTPUT_FILE"
echo "  \"by_framework\": {}," >> "$OUTPUT_FILE"
echo "  \"by_file\": {}" >> "$OUTPUT_FILE"
echo "}" >> "$OUTPUT_FILE"

# Categories and frameworks mapping
declare -A CATEGORIES
declare -A FRAMEWORKS

# Process each test file
while IFS= read -r file; do
    if [[ -f "$file" ]]; then
        FILE_IMPORTS=$(grep -E "^(use |extern crate |mod )" "$file" 2>/dev/null || echo "")

        if [[ -n "$FILE_IMPORTS" ]]; then
            ((TOTAL_FILES++))

            # Extract imports and categorize
            while IFS= read -r import_line; do
                if [[ -n "$import_line" ]]; then
                    ((TOTAL_IMPORTS++))

                    # Categorize import
                    CATEGORY="unknown"
                    FRAMEWORK=""

                    case "$import_line" in
                        *tokio*test*)
                            CATEGORY="tokio_test"
                            FRAMEWORK="tokio"
                            ;;
                        *rstest*)
                            CATEGORY="rstest"
                            FRAMEWORK="rstest"
                            ;;
                        *proptest*)
                            CATEGORY="proptest"
                            FRAMEWORK="proptest"
                            ;;
                        *criterion*)
                            CATEGORY="criterion"
                            FRAMEWORK="criterion"
                            ;;
                        *mockall*)
                            CATEGORY="mockall"
                            FRAMEWORK="mockall"
                            ;;
                        *assert*)
                            CATEGORY="assert_macro"
                            FRAMEWORK="assert"
                            ;;
                        std::*|core::*)
                            CATEGORY="std_lib"
                            ;;
                        crate::*|super::*)
                            CATEGORY="local_module"
                            ;;
                        *::*::*)
                            CATEGORY="external_crate"
                            ;;
                        *)
                            CATEGORY="unknown"
                            ;;
                    esac

                    # Track categories and frameworks
                    CATEGORIES["$CATEGORY"]=$((${CATEGORIES[$CATEGORY]:-0} + 1))
                    if [[ -n "$FRAMEWORK" ]]; then
                        FRAMEWORKS["$FRAMEWORK"]=$((${FRAMEWORKS[$FRAMEWORK]:-0} + 1))
                    fi
                fi
            done <<< "$FILE_IMPORTS"
        fi
    fi
done <<< "$TEST_FILES"

# Create markdown output
cat > "$OUTPUT_MD" << EOF
# Test Imports Analysis

Generated: $(date -u +"%Y-%m-%dT%H:%M:%SZ")

## Summary

- **Total test files processed**: $TOTAL_FILES
- **Total imports found**: $TOTAL_IMPORTS

## Imports by Category

EOF

for category in "${!CATEGORIES[@]}"; do
    count="${CATEGORIES[$category]}"
    echo "- **$category**: $count imports" >> "$OUTPUT_MD"
done

cat >> "$OUTPUT_MD" << EOF

## Imports by Framework

EOF

for framework in "${!FRAMEWORKS[@]}"; do
    count="${FRAMEWORKS[$framework]}"
    echo "- **$framework**: $count imports" >> "$OUTPUT_MD"
done

cat >> "$OUTPUT_MD" << EOF

## Files Analyzed

EOF

while IFS= read -r file; do
    if [[ -f "$file" ]]; then
        import_count=$(grep -cE "^(use |extern crate |mod )" "$file" 2>/dev/null || echo "0")
        if [[ "$import_count" -gt 0 ]]; then
            echo "- \`$file\`: $import_count imports" >> "$OUTPUT_MD"
        fi
    fi
done <<< "$TEST_FILES"

echo "Analysis complete!"
echo "Markdown output: $OUTPUT_MD"
echo "Found $TOTAL_FILES test files with $TOTAL_IMPORTS total imports"