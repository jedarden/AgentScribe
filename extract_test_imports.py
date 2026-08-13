#!/usr/bin/env python3
"""
Extract import statements from AgentScribe test files.

Parses all test files identified in test_files_catalog.json and extracts
all use, extern crate, and mod statements with source file attribution.
"""

import json
import re
from pathlib import Path
from typing import List, Dict, Any
from datetime import datetime


def extract_use_statements(content: str) -> List[Dict[str, Any]]:
    """Extract use statements from Rust source code.

    Handles:
    - Single-line use statements: use crate::foo::Bar;
    - Multi-line use statements with braces:
      use crate::foo::{
          Bar,
          Baz,
      };
    - Nested use statements: use crate::foo::{self, Bar, Baz};
    - Self-referential: use crate::foo::self as Bar;
    """
    imports = []

    # Pattern for use statements - handles both single and multi-line
    # This pattern captures from 'use' to the final ';'
    use_pattern = r'use\s+(.+?);'

    for match in re.finditer(use_pattern, content, re.MULTILINE | re.DOTALL):
        full_statement = match.group(0).strip()
        import_path = match.group(1).strip()

        # Clean up multi-line formatting
        import_path = re.sub(r'\s+', ' ', import_path)

        imports.append({
            'type': 'use',
            'statement': full_statement,
            'path': import_path,
            'is_nested': '{' in import_path,
            'is_self': 'self' in import_path or import_path.endswith('::self'),
        })

    return imports


def extract_extern_crate_statements(content: str) -> List[Dict[str, Any]]:
    """Extract extern crate statements."""
    imports = []

    pattern = r'extern\s+crate\s+(\w+)(?:\s+as\s+(\w+))?;'

    for match in re.finditer(pattern, content):
        crate_name = match.group(1)
        alias = match.group(2)

        imports.append({
            'type': 'extern_crate',
            'statement': match.group(0).strip(),
            'crate_name': crate_name,
            'alias': alias,
        })

    return imports


def extract_mod_statements(content: str) -> List[Dict[str, Any]]:
    """Extract mod statements (module declarations)."""
    imports = []

    # Pattern for mod statements
    # Handles: mod foo; and mod foo { ... }
    pattern = r'mod\s+(\w+)\s*(?:;\s*//.*?$|\{)'

    for match in re.finditer(pattern, content, re.MULTILINE):
        mod_name = match.group(1)
        full_line = match.group(0).strip()

        imports.append({
            'type': 'mod',
            'statement': full_line,
            'module_name': mod_name,
            'is_inline': '{' in full_line,
        })

    return imports


def parse_file_for_imports(file_path: Path) -> List[Dict[str, Any]]:
    """Parse a single Rust file and extract all import statements."""
    try:
        content = file_path.read_text(encoding='utf-8')
    except Exception as e:
        print(f"Warning: Could not read {file_path}: {e}")
        return []

    all_imports = []

    # Extract all types of imports
    all_imports.extend(extract_use_statements(content))
    all_imports.extend(extract_extern_crate_statements(content))
    all_imports.extend(extract_mod_statements(content))

    # Add source file information to each import
    for imp in all_imports:
        imp['source_file'] = str(file_path)
        # Try to get relative path, fall back to absolute if not possible
        try:
            imp['relative_path'] = str(file_path.relative_to('/home/coding/AgentScribe'))
        except ValueError:
            imp['relative_path'] = str(file_path)

    return all_imports


def main():
    """Main entry point."""
    # Load the test files catalog
    catalog_path = Path('/home/coding/AgentScribe/test_files_catalog.json')
    with open(catalog_path, 'r') as f:
        catalog = json.load(f)

    all_imports = []
    files_processed = 0
    files_with_errors = 0

    # Process standalone test files
    for entry in catalog.get('standalone_test_files', []):
        file_path = Path(entry['path'])
        if file_path.exists():
            imports = parse_file_for_imports(file_path)
            all_imports.extend(imports)
            files_processed += 1
            print(f"Processed: {entry['path']} ({len(imports)} imports)")
        else:
            print(f"Warning: File not found: {entry['path']}")
            files_with_errors += 1

    # Process source files with test modules
    for entry in catalog.get('source_files_with_test_modules', []):
        file_path = Path(entry['path'])
        if file_path.exists():
            imports = parse_file_for_imports(file_path)
            all_imports.extend(imports)
            files_processed += 1
            print(f"Processed: {entry['path']} ({len(imports)} imports)")
        else:
            print(f"Warning: File not found: {entry['path']}")
            files_with_errors += 1

    # Prepare output data
    output = {
        'extraction_generated_at': datetime.utcnow().isoformat() + 'Z',
        'project': 'AgentScribe',
        'summary': {
            'total_files_processed': files_processed,
            'total_files_with_errors': files_with_errors,
            'total_imports_extracted': len(all_imports),
        },
        'imports_by_type': {
            'use': len([i for i in all_imports if i['type'] == 'use']),
            'extern_crate': len([i for i in all_imports if i['type'] == 'extern_crate']),
            'mod': len([i for i in all_imports if i['type'] == 'mod']),
        },
        'imports': all_imports,
    }

    # Write output
    output_path = Path('/home/coding/AgentScribe/test_imports.json')
    with open(output_path, 'w') as f:
        json.dump(output, f, indent=2)

    print(f"\n{'='*60}")
    print(f"Extraction complete!")
    print(f"Files processed: {files_processed}")
    print(f"Total imports extracted: {len(all_imports)}")
    print(f"Output: {output_path}")
    print(f"{'='*60}")


if __name__ == '__main__':
    main()
