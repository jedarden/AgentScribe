//! Import statement parser for Rust source files
//!
//! Extracts and categorizes import statements from Rust files with metadata.
//! Supports `use`, `extern crate`, and `mod` statements with line tracking.

use crate::error::{AgentScribeError, Result};
use std::path::Path;

/// Type of import statement
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum ImportType {
    /// `use` statement - imports from crates, modules, or items
    Use,
    /// `extern crate` statement - declares an external crate dependency
    ExternCrate,
    /// `mod` statement - declares a module
    Mod,
}

impl ImportType {
    /// Convert import type to string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            ImportType::Use => "use",
            ImportType::ExternCrate => "extern crate",
            ImportType::Mod => "mod",
        }
    }
}

/// Structured representation of an import statement
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ImportStatement {
    /// The import path (e.g., `std::collections::HashMap`, `crate::module::Item`)
    pub path: String,
    /// Type of import statement
    pub import_type: ImportType,
    /// Line number where the import appears (1-indexed)
    pub line_number: usize,
    /// Original raw line from the source file
    pub raw_line: String,
}

impl ImportStatement {
    /// Create a new import statement
    pub fn new(
        path: String,
        import_type: ImportType,
        line_number: usize,
        raw_line: String,
    ) -> Self {
        Self {
            path,
            import_type,
            line_number,
            raw_line,
        }
    }

    /// Create a use statement
    pub fn use_statement(path: String, line_number: usize, raw_line: String) -> Self {
        Self::new(path, ImportType::Use, line_number, raw_line)
    }

    /// Create an extern crate statement
    pub fn extern_crate(path: String, line_number: usize, raw_line: String) -> Self {
        Self::new(path, ImportType::ExternCrate, line_number, raw_line)
    }

    /// Create a mod statement
    pub fn mod_statement(path: String, line_number: usize, raw_line: String) -> Self {
        Self::new(path, ImportType::Mod, line_number, raw_line)
    }
}

/// Result of parsing a single file
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ImportParseResult {
    /// All import statements found in the file
    pub imports: Vec<ImportStatement>,
    /// Total number of use statements
    pub use_count: usize,
    /// Total number of extern crate statements
    pub extern_crate_count: usize,
    /// Total number of mod statements
    pub mod_count: usize,
}

impl ImportParseResult {
    /// Create a new parse result from a list of imports
    pub fn new(imports: Vec<ImportStatement>) -> Self {
        let use_count = imports
            .iter()
            .filter(|i| i.import_type == ImportType::Use)
            .count();
        let extern_crate_count = imports
            .iter()
            .filter(|i| i.import_type == ImportType::ExternCrate)
            .count();
        let mod_count = imports
            .iter()
            .filter(|i| i.import_type == ImportType::Mod)
            .count();

        Self {
            imports,
            use_count,
            extern_crate_count,
            mod_count,
        }
    }

    /// Get imports by type
    pub fn imports_by_type(&self, import_type: ImportType) -> Vec<&ImportStatement> {
        self.imports
            .iter()
            .filter(|i| i.import_type == import_type)
            .collect()
    }

    /// Check if any imports were found
    pub fn is_empty(&self) -> bool {
        self.imports.is_empty()
    }

    /// Get total number of imports
    pub fn total_count(&self) -> usize {
        self.imports.len()
    }
}

/// Parser state for tracking context during parsing
#[derive(Debug, Clone, Default)]
struct ParserState {
    /// Current line number (1-indexed)
    line_number: usize,
    /// Whether we're inside a cfg(test) module
    in_test_module: bool,
    /// Brace depth for tracking test module boundaries
    brace_depth: i32,
    /// Whether the current line is part of a multi-line statement
    in_multiline: bool,
    /// Accumulated content for multi-line statements
    multiline_buffer: String,
}

/// Import parser for Rust source files
pub struct ImportParser;

impl ImportParser {
    /// Create a new import parser
    pub fn new() -> Self {
        Self
    }

    /// Parse imports from a file path
    ///
    /// # Arguments
    /// * `file_path` - Path to the Rust source file
    ///
    /// # Returns
    /// * `Result<ImportParseResult>` - Parse result with all imports found
    ///
    /// # Errors
    /// Returns an error if the file cannot be read
    pub fn parse_file(&self, file_path: &Path) -> Result<ImportParseResult> {
        let content = std::fs::read_to_string(file_path).map_err(|e| {
            AgentScribeError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Failed to read file {}: {}", file_path.display(), e),
            ))
        })?;

        Ok(self.parse_content(&content))
    }

    /// Parse imports from a string content
    ///
    /// # Arguments
    /// * `content` - Rust source code content as a string
    ///
    /// # Returns
    /// * `ImportParseResult` - Parse result with all imports found
    pub fn parse_content(&self, content: &str) -> ImportParseResult {
        let mut state = ParserState::default();
        let mut imports = Vec::new();

        for line in content.lines() {
            state.line_number += 1;
            self.process_line(line, &mut state, &mut imports);
        }

        // Handle any remaining multi-line buffer
        if !state.multiline_buffer.is_empty() {
            if let Some(import) = self.try_parse_import(&state.multiline_buffer, state.line_number)
            {
                imports.push(import);
            }
        }

        ImportParseResult::new(imports)
    }

    /// Process a single line during parsing
    fn process_line(
        &self,
        line: &str,
        state: &mut ParserState,
        imports: &mut Vec<ImportStatement>,
    ) {
        let trimmed = line.trim();

        // Skip empty lines and comments (but track cfg(test))
        if trimmed.is_empty() {
            return;
        }

        // Track test module boundaries
        self.track_test_module(trimmed, state);

        // Skip line comments inside test modules (we want those imports too)
        if trimmed.starts_with("//") {
            return;
        }

        // Handle multi-line statements
        if state.in_multiline {
            state.multiline_buffer.push(' ');
            state.multiline_buffer.push_str(trimmed);

            // Check if multi-line statement ends
            if trimmed.contains(';') || trimmed.contains('{') || trimmed.contains('}') {
                state.in_multiline = false;
                let combined = state.multiline_buffer.clone();
                state.multiline_buffer.clear();

                if let Some(import) = self.try_parse_import(&combined, state.line_number) {
                    imports.push(import);
                }
            }
            return;
        }

        // Check if this starts a multi-line statement
        if self.is_multiline_start(trimmed) {
            state.in_multiline = true;
            state.multiline_buffer = trimmed.to_string();
            return;
        }

        // Try to parse as a single-line import
        if let Some(import) = self.try_parse_import(trimmed, state.line_number) {
            imports.push(import);
        }
    }

    /// Track whether we're inside a cfg(test) module
    fn track_test_module(&self, line: &str, state: &mut ParserState) {
        if line.contains("#[cfg(test)]") {
            state.in_test_module = true;
            state.brace_depth = 0;
        } else if state.in_test_module {
            state.brace_depth += line.matches('{').count() as i32;
            state.brace_depth -= line.matches('}').count() as i32;

            if state.brace_depth <= 0 {
                state.in_test_module = false;
                state.brace_depth = 0;
            }
        }
    }

    /// Check if a line starts a multi-line statement
    fn is_multiline_start(&self, line: &str) -> bool {
        // Multi-line use statement with parentheses
        if line.starts_with("use ") && !line.contains(';') && !line.contains('{') {
            return true;
        }
        // Multi-line extern crate with attributes
        if line.starts_with("extern crate") && !line.contains(';') {
            return true;
        }
        // Multi-line mod statement
        if line.starts_with("mod ") && !line.contains(';') && !line.contains('{') {
            return true;
        }
        false
    }

    /// Try to parse a line as an import statement
    fn try_parse_import(&self, line: &str, line_number: usize) -> Option<ImportStatement> {
        let trimmed = line.trim();

        // Skip if it's just a closing brace or semicolon
        if trimmed == "}" || trimmed == ";" || trimmed == "{" {
            return None;
        }

        // Parse extern crate statements
        if let Some(stmt) = self.parse_extern_crate(trimmed, line_number) {
            return Some(stmt);
        }

        // Parse mod statements
        if let Some(stmt) = self.parse_mod_statement(trimmed, line_number) {
            return Some(stmt);
        }

        // Parse use statements
        if let Some(stmt) = self.parse_use_statement(trimmed, line_number) {
            return Some(stmt);
        }

        None
    }

    /// Parse an extern crate statement
    fn parse_extern_crate(&self, line: &str, line_number: usize) -> Option<ImportStatement> {
        if !line.starts_with("extern crate") {
            return None;
        }

        // Extract crate name: "extern crate foo;" or "extern crate foo as bar;"
        let content = line["extern crate".len()..].trim();
        let path = if let Some(as_pos) = content.find(" as ") {
            // Handle "as" clause first - take everything before "as"
            content[..as_pos].trim()
        } else if let Some(semicolon_pos) = content.find(';') {
            content[..semicolon_pos].trim()
        } else {
            content.trim()
        }
        .to_string();

        if path.is_empty() || path == "{" || path == "}" {
            return None;
        }

        Some(ImportStatement::extern_crate(
            path,
            line_number,
            line.to_string(),
        ))
    }

    /// Parse a mod statement
    fn parse_mod_statement(&self, line: &str, line_number: usize) -> Option<ImportStatement> {
        if !line.starts_with("mod ") {
            return None;
        }

        // Extract module name: "mod foo;" or "mod foo { ... }"
        let content = line["mod ".len()..].trim();
        let path = if let Some(semicolon_pos) = content.find(';') {
            content[..semicolon_pos].trim()
        } else if let Some(brace_pos) = content.find('{') {
            content[..brace_pos].trim()
        } else {
            // mod without semicolon or brace (file-based module)
            content.split_whitespace().next().unwrap_or("")
        }
        .to_string();

        if path.is_empty() || path == "{" || path == "}" {
            return None;
        }

        Some(ImportStatement::mod_statement(
            path,
            line_number,
            line.to_string(),
        ))
    }

    /// Parse a use statement
    fn parse_use_statement(&self, line: &str, line_number: usize) -> Option<ImportStatement> {
        if !line.starts_with("use ") {
            return None;
        }

        // Extract import path from use statement
        // Handle: "use foo::bar;", "use foo::{bar, baz};", "use foo::bar as baz;"
        let content = line["use ".len()..].trim();
        let path = if let Some(as_pos) = content.find(" as ") {
            // Handle "as" clause first - take everything before "as"
            content[..as_pos].trim()
        } else if let Some(brace_pos) = content.find('{') {
            // Remove trailing colons before the brace
            content[..brace_pos].trim().trim_end_matches(':')
        } else if let Some(semicolon_pos) = content.find(';') {
            content[..semicolon_pos].trim()
        } else {
            content.trim()
        }
        .to_string();

        if path.is_empty() || path == "{" || path == "}" {
            return None;
        }

        Some(ImportStatement::use_statement(
            path,
            line_number,
            line.to_string(),
        ))
    }
}

impl Default for ImportParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_use_statements() {
        let content = r#"
use std::collections::HashMap;
use crate::module::Item;
use super::ParentModule;
"#;

        let parser = ImportParser::new();
        let result = parser.parse_content(content);

        assert_eq!(result.use_count, 3);
        assert_eq!(result.imports.len(), 3);

        assert_eq!(result.imports[0].path, "std::collections::HashMap");
        assert_eq!(result.imports[0].import_type, ImportType::Use);
        assert_eq!(result.imports[0].line_number, 2);

        assert_eq!(result.imports[1].path, "crate::module::Item");
        assert_eq!(result.imports[2].path, "super::ParentModule");
    }

    #[test]
    fn test_parse_extern_crate() {
        let content = r#"
extern crate serde;
extern crate tokio as tok;
"#;

        let parser = ImportParser::new();
        let result = parser.parse_content(content);

        assert_eq!(result.extern_crate_count, 2);
        assert_eq!(result.imports.len(), 2);

        assert_eq!(result.imports[0].path, "serde");
        assert_eq!(result.imports[0].import_type, ImportType::ExternCrate);

        assert_eq!(result.imports[1].path, "tokio");
    }

    #[test]
    fn test_parse_mod_statements() {
        let content = r#"
mod foo;
mod bar;
mod baz {
    // module contents
}
"#;

        let parser = ImportParser::new();
        let result = parser.parse_content(content);

        assert_eq!(result.mod_count, 3);
        assert_eq!(result.imports.len(), 3);

        assert_eq!(result.imports[0].path, "foo");
        assert_eq!(result.imports[0].import_type, ImportType::Mod);

        assert_eq!(result.imports[1].path, "bar");
        assert_eq!(result.imports[2].path, "baz");
    }

    #[test]
    fn test_parse_use_with_braces() {
        let content = r#"
use std::collections::{HashMap, HashSet};
use crate::module::{self, Item, Another};
"#;

        let parser = ImportParser::new();
        let result = parser.parse_content(content);

        assert_eq!(result.use_count, 2);

        assert_eq!(result.imports[0].path, "std::collections");
        assert_eq!(result.imports[1].path, "crate::module");
    }

    #[test]
    fn test_parse_use_with_as() {
        let content = r#"
use std::collections::HashMap as Map;
use crate::module::Item as Alias;
"#;

        let parser = ImportParser::new();
        let result = parser.parse_content(content);

        assert_eq!(result.use_count, 2);

        // Should preserve the full path before "as"
        assert_eq!(result.imports[0].path, "std::collections::HashMap");
        assert_eq!(result.imports[1].path, "crate::module::Item");
    }

    #[test]
    fn test_skips_comments() {
        let content = r#"
// This is a comment
use std::collections::HashMap;
// Another comment
use crate::module::Item;
"#;

        let parser = ImportParser::new();
        let result = parser.parse_content(content);

        assert_eq!(result.use_count, 2);
        assert_eq!(result.imports.len(), 2);
    }

    #[test]
    fn test_skips_empty_lines() {
        let content = r#"

use std::collections::HashMap;


use crate::module::Item;

"#;

        let parser = ImportParser::new();
        let result = parser.parse_content(content);

        assert_eq!(result.use_count, 2);
    }

    #[test]
    fn test_includes_test_module_imports() {
        let content = r#"
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::test_helpers::*;
}

#[cfg(test)]
mod more_tests {
    use crate::fixtures::setup;
}
"#;

        let parser = ImportParser::new();
        let result = parser.parse_content(content);

        // Should include imports from test modules
        assert!(result.use_count >= 4);

        // Check that test module imports are included
        let test_imports: Vec<_> = result
            .imports
            .iter()
            .filter(|i| i.line_number > 3)
            .collect();
        assert!(test_imports.len() >= 3);
    }

    #[test]
    fn test_line_numbers_are_accurate() {
        let content = r#"
// line 1
use std::collections::HashMap; // line 2
// line 3
extern crate serde; // line 4
"#;

        let parser = ImportParser::new();
        let result = parser.parse_content(content);

        // Line numbers are 1-indexed and count all lines including the initial blank line
        assert_eq!(result.imports[0].line_number, 3);
        assert_eq!(result.imports[1].line_number, 5);
    }

    #[test]
    fn test_raw_line_preserved() {
        let content = "use std::collections::HashMap;";

        let parser = ImportParser::new();
        let result = parser.parse_content(content);

        assert_eq!(result.imports[0].raw_line, content);
    }

    #[test]
    fn test_mixed_imports() {
        let content = r#"
extern crate serde;
use std::collections::HashMap;
mod foo;
use crate::module::Item;
mod bar;
"#;

        let parser = ImportParser::new();
        let result = parser.parse_content(content);

        assert_eq!(result.use_count, 2);
        assert_eq!(result.extern_crate_count, 1);
        assert_eq!(result.mod_count, 2);
        assert_eq!(result.total_count(), 5);
    }

    #[test]
    fn test_imports_by_type_filtering() {
        let content = r#"
use std::collections::HashMap;
extern crate serde;
mod foo;
use crate::module::Item;
"#;

        let parser = ImportParser::new();
        let result = parser.parse_content(content);

        let use_imports = result.imports_by_type(ImportType::Use);
        assert_eq!(use_imports.len(), 2);

        let extern_imports = result.imports_by_type(ImportType::ExternCrate);
        assert_eq!(extern_imports.len(), 1);

        let mod_imports = result.imports_by_type(ImportType::Mod);
        assert_eq!(mod_imports.len(), 1);
    }

    #[test]
    fn test_empty_file() {
        let content = "";

        let parser = ImportParser::new();
        let result = parser.parse_content(content);

        assert!(result.is_empty());
        assert_eq!(result.total_count(), 0);
    }

    #[test]
    fn test_file_with_no_imports() {
        let content = r#"
fn main() {
    println!("Hello, world!");
}
"#;

        let parser = ImportParser::new();
        let result = parser.parse_content(content);

        assert!(result.is_empty());
    }

    #[test]
    fn test_complex_real_world_example() {
        let content = r#"
use crate::error::{AgentScribeError, Result};
use crate::event::Event;
use crate::plugin::Plugin;
use chrono::{DateTime, Utc};
use serde_json::Value;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers;

    #[test]
    fn test_example() {
        // test code
    }
}
"#;

        let parser = ImportParser::new();
        let result = parser.parse_content(content);

        // Top-level imports
        assert!(result.use_count >= 4);

        // Test module imports should also be included
        assert!(result.total_count() >= 5);
    }

    #[test]
    fn test_import_type_as_str() {
        assert_eq!(ImportType::Use.as_str(), "use");
        assert_eq!(ImportType::ExternCrate.as_str(), "extern crate");
        assert_eq!(ImportType::Mod.as_str(), "mod");
    }

    #[test]
    fn test_parse_result_empty_check() {
        let parser = ImportParser::new();

        let result = parser.parse_content("");
        assert!(result.is_empty());

        let result = parser.parse_content("use std::collections::HashMap;");
        assert!(!result.is_empty());
    }
}
