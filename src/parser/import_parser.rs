//! Import statement parser for Rust source files
//!
//! This module provides functionality for extracting and categorizing import statements
//! from Rust source files. It supports three types of import statements:
//!
//! - **`use` statements**: Import items from crates, modules, or other scopes
//!   (e.g., `use std::collections::HashMap;`)
//! - **`extern crate` statements**: Declare external crate dependencies
//!   (e.g., `extern crate serde;`)
//! - **`mod` statements**: Declare modules, either inline or from separate files
//!   (e.g., `mod foo;` or `mod bar { ... }`)
//!
//! # Module Overview
//!
//! The import parser module provides three main types for working with Rust imports:
//!
//! - **[`ImportType`]**: Enum representing the three kinds of import statements in Rust
//! - **[`ImportStatement`]**: Struct containing complete information about a single import
//! - **[`ImportParseResult`]**: Container for all imports found in a file with convenience methods
//!
//! # Common Use Cases
//!
//! ## Parse a single file
//!
//! ```no_run
//! use agentscribe::parser::ImportParser;
//! use std::path::Path;
//!
//! let parser = ImportParser::new();
//! let result = parser.parse_file(Path::new("src/main.rs")).unwrap();
//! println!("Found {} imports", result.total_count());
//! ```
//!
//! ## Parse from string content
//!
//! ```
//! use agentscribe::parser::ImportParser;
//!
//! let content = r#"
//! use std::collections::HashMap;
//! use crate::module::Item;
//! "#;
//!
//! let parser = ImportParser::new();
//! let result = parser.parse_content(content);
//! assert_eq!(result.use_count, 2);
//! ```
//!
//! ## Filter imports by type
//!
//! ```
//! use agentscribe::parser::{ImportParser, ImportType};
//!
//! let content = r#"
//! use std::collections::HashMap;
//! extern crate serde;
//! mod foo;
//! "#;
//!
//! let parser = ImportParser::new();
//! let result = parser.parse_content(content);
//!
//! let use_imports = result.imports_by_type(ImportType::Use);
//! assert_eq!(use_imports.len(), 1);
//!
//! let extern_imports = result.imports_by_type(ImportType::ExternCrate);
//! assert_eq!(extern_imports.len(), 1);
//! ```
//!
//! # Features
//!
//! - Extracts import paths with support for complex paths (e.g., `use crate::module::Item;`)
//! - Handles multi-line import statements with parentheses
//! - Tracks line numbers for each import
//! - Preserves original raw lines for reference
//! - Distinguishes between different import types
//! - Includes imports from test modules (`#[cfg(test)]`)
//! - Provides convenience methods for filtering and counting imports

use crate::error::{AgentScribeError, Result};
use std::path::Path;

/// Type of import statement in Rust source code
///
/// Represents the three kinds of import statements in Rust:
///
/// - **Use**: The standard `use` statement for importing items from crates, modules, or other scopes.
///   This is the most common import type and includes both simple imports (`use std::collections::HashMap;`)
///   and complex imports with braces (`use std::collections::{HashMap, HashSet};`).
///
/// - **ExternCrate**: The `extern crate` statement declares an external crate dependency. This was required
///   in Rust 2015 edition but is largely obsolete in Rust 2018+ where crate declarations in `Cargo.toml`
///   suffice. Still used in some contexts for explicitly loading external crates.
///
/// - **Mod**: The `mod` statement declares a module, either inline (`mod foo { ... }`) or as a file
///   (`mod bar;` which loads `bar.rs` or `bar/mod.rs`). This helps define the module structure of
///   a crate.
///
/// # Examples
///
/// ```
/// use agentscribe::parser::ImportType;
///
/// // Working with import types
/// let import_type = ImportType::Use;
/// assert_eq!(import_type.as_str(), "use");
///
/// // Matching on import types
/// fn describe_import(import_type: &ImportType) -> &'static str {
///     match import_type {
///         ImportType::Use => "Standard use import",
///         ImportType::ExternCrate => "External crate declaration",
///         ImportType::Mod => "Module declaration",
///     }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ImportType {
    /// `use` statement - imports from crates, modules, or items
    ///
    /// The most common import type in Rust. Used to bring items from other modules,
    /// crates, or scopes into the current scope.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Simple import
    /// use std::collections::HashMap;
    ///
    /// // Complex import with multiple items
    /// use std::collections::{HashMap, HashSet, BTreeMap};
    ///
    /// // Import with renaming
    /// use std::collections::HashMap as Map;
    ///
    /// // Import from crate
    /// use crate::module::Item;
    ///
    /// // Import from parent module
    /// use super::ParentModule;
    ///
    /// // Self import
    /// use crate::module::self;
    /// ```
    Use,

    /// `extern crate` statement - declares an external crate dependency
    ///
    /// This statement was required in Rust 2015 edition to explicitly declare external
    /// crates. In Rust 2018+, this is usually unnecessary as crates listed in `Cargo.toml`
    /// are automatically available. However, it's still useful in some cases like:
    ///
    /// - Loading crates with specific attributes
    /// - Explicitly renaming crates
    /// - Conditional compilation scenarios
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Basic extern crate declaration
    /// extern crate serde;
    ///
    /// // With renaming
    /// extern crate tokio as tok;
    ///
    /// // With macro attributes
    /// #[macro_use]
    /// extern crate lazy_static;
    /// ```
    ExternCrate,

    /// `mod` statement - declares a module
    ///
    /// Modules organize Rust code into separate files or inline blocks. This import type
    /// represents module declarations that define the structure of a crate.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // File-based module (loads foo.rs or foo/mod.rs)
    /// mod foo;
    ///
    /// // File-based with path
    /// mod bar;
    /// // loads: bar.rs or bar/mod.rs
    ///
    /// // Inline module definition
    /// mod baz {
    ///     // module contents here
    /// }
    ///
    /// // Private module (not accessible from outside)
    /// mod private_impl;
    /// ```
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

/// Structured representation of an import statement in Rust source code
///
/// Contains the complete information about a single import statement found in Rust source code,
/// including the import path, type, location, and original text. This struct provides a structured
/// way to work with import statements programmatically.
///
/// # Field Documentation
///
/// ## `path: String`
/// The full import path extracted from the import statement. This includes:
/// - For `use` statements: the full module path (e.g., `std::collections`, `crate::module::Item`)
/// - For `extern crate`: the crate name (e.g., `serde`, `tokio`)
/// - For `mod`: the module name (e.g., `foo`, `bar`)
///
/// ## `import_type: ImportType`
/// The type of import statement (Use, ExternCrate, or Mod). This distinguishes between
/// the three kinds of import statements in Rust.
///
/// ## `line_number: usize`
/// 1-indexed line number where the import appears in the source file. Useful for:
/// - Locating imports in the original source
/// - Reporting positions in error messages
/// - Creating line-aware tools
///
/// ## `raw_line: String`
/// The original text of the import line as it appears in the source, including all formatting,
/// comments, and whitespace. Useful for:
/// - Preserving original formatting
/// - Displaying imports to users
/// - Diffing or comparing import statements
///
/// # Examples
///
/// ```
/// use agentscribe::parser::{ImportStatement, ImportType};
///
/// // Creating a use statement
/// let use_stmt = ImportStatement::use_statement(
///     "std::collections::HashMap".to_string(),
///     5,
///     "use std::collections::HashMap;".to_string()
/// );
///
/// assert_eq!(use_stmt.path, "std::collections::HashMap");
/// assert_eq!(use_stmt.import_type, ImportType::Use);
/// assert_eq!(use_stmt.line_number, 5);
///
/// // Creating an extern crate statement
/// let extern_stmt = ImportStatement::extern_crate(
///     "serde".to_string(),
///     1,
///     "extern crate serde;".to_string()
/// );
///
/// assert_eq!(extern_stmt.import_type, ImportType::ExternCrate);
///
/// // Creating a mod statement
/// let mod_stmt = ImportStatement::mod_statement(
///     "foo".to_string(),
///     10,
///     "mod foo;".to_string()
/// );
///
/// assert_eq!(mod_stmt.import_type, ImportType::Mod);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ImportStatement {
    /// The import path (e.g., `std::collections::HashMap`, `crate::module::Item`)
    ///
    /// For `use` statements: contains the full module path being imported
    /// For `extern crate`: contains the crate name
    /// For `mod`: contains the module name
    ///
    /// # Examples
    ///
    /// ```rust
    /// use agentscribe::parser::ImportStatement;
    ///
    /// let stmt = ImportStatement::use_statement(
    ///     "std::collections::HashMap".to_string(),
    ///     1,
    ///     "use std::collections::HashMap;".to_string()
    /// );
    ///
    /// assert_eq!(stmt.path, "std::collections::HashMap");
    /// ```
    pub path: String,

    /// Type of import statement (Use, ExternCrate, or Mod)
    ///
    /// Distinguishes between the three kinds of import statements in Rust:
    /// - `ImportType::Use`: standard `use` statements
    /// - `ImportType::ExternCrate`: external crate declarations
    /// - `ImportType::Mod`: module declarations
    ///
    /// # Examples
    ///
    /// ```rust
    /// use agentscribe::parser::{ImportStatement, ImportType};
    ///
    /// let use_stmt = ImportStatement::use_statement(
    ///     "std::collections::HashMap".to_string(),
    ///     1,
    ///     "use std::collections::HashMap;".to_string()
    /// );
    ///
    /// assert_eq!(use_stmt.import_type, ImportType::Use);
    /// assert_eq!(use_stmt.import_type.as_str(), "use");
    /// ```
    pub import_type: ImportType,

    /// Line number where the import appears (1-indexed)
    ///
    /// Useful for locating imports in the original source code and for
    /// creating tools that need to report specific line positions.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use agentscribe::parser::ImportStatement;
    ///
    /// let stmt = ImportStatement::use_statement(
    ///     "std::collections::HashMap".to_string(),
    ///     42,
    ///     "use std::collections::HashMap;".to_string()
    /// );
    ///
    /// assert_eq!(stmt.line_number, 42);
    /// // Line numbers are 1-indexed, matching typical editor line numbering
    /// ```
    pub line_number: usize,

    /// Original raw line from the source file
    ///
    /// Preserves the exact formatting of the import as it appears in source code,
    /// including whitespace, comments, and line endings. Useful for display,
    /// diffing, or when you need to show the original import to users.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use agentscribe::parser::ImportStatement;
    ///
    /// let stmt = ImportStatement::use_statement(
    ///     "std::collections::HashMap".to_string(),
    ///     1,
    ///     "use std::collections::HashMap;".to_string()
    /// );
    ///
    /// assert_eq!(stmt.raw_line, "use std::collections::HashMap;");
    /// // The raw line preserves original formatting
    /// ```
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

/// Lightweight representation of a Rust import statement
///
/// A simplified struct containing the essential information about a single import statement
/// found in Rust source code. This struct focuses on the core attributes needed for import
/// analysis and categorization.
///
/// # When to Use Import vs ImportStatement
///
/// The [`Import`] struct is a lightweight alternative to [`ImportStatement`]:
///
/// - **Use [`Import`]** when you need basic import information without the overhead of
///   storing the original source line. Ideal for:
///   - Import categorization and counting
///   - Dependency analysis
///   - Module structure mapping
///   - Lightweight tooling
///
/// - **Use [`ImportStatement`]** when you need the complete original text including:
///   - Exact formatting and whitespace
///   - Comments in import statements
///   - Multi-line import blocks
///   - Source reconstruction
///
/// # Field Documentation
///
/// ## `path: String`
///
/// The import path extracted from the import statement. The format varies by import type:
///
/// - **For `use` statements**: Full module path being imported (e.g., `"std::collections::HashMap"`,
///   `"crate::module::Item"`, `"super::ParentModule"`)
/// - **For `extern crate`**: The crate name (e.g., `"serde"`, `"tokio"`)
/// - **For `mod`**: The module name (e.g., `"foo"`, `"bar"`)
///
/// **Constraints:**
/// - Non-empty string (cannot be `""`)
/// - Contains only valid Rust identifier characters and path separators (`::`)
/// - Case-sensitive (matches Rust's rules)
///
/// **Usage in practice:**
/// ```rust
/// use agentscribe::parser::{Import, ImportType};
///
/// // Standard library import
/// let std_import = Import::new("std::collections::HashMap".to_string(), ImportType::Use, 5);
///
/// // Crate-relative import
/// let crate_import = Import::new("crate::module::Item".to_string(), ImportType::Use, 10);
///
/// // External crate
/// let extern_import = Import::new("serde".to_string(), ImportType::ExternCrate, 1);
/// ```
///
/// ## `import_type: ImportType`
///
/// The type classification of the import statement. This field distinguishes between the three
/// fundamental kinds of import statements in Rust:
///
/// - **[`ImportType::Use`]**: Standard `use` statements (most common)
/// - **[`ImportType::ExternCrate`]**: External crate declarations
/// - **[`ImportType::Mod`]**: Module declarations
///
/// **Valid values:** Only the three variants defined in [`ImportType`]
///
/// **Usage in practice:**
/// ```rust
/// use agentscribe::parser::{Import, ImportType};
///
/// // Categorize imports by type
/// fn is_standard_library(import: &Import) -> bool {
///     match import.import_type {
///         ImportType::Use => import.path.starts_with("std::") || import.path.starts_with("core::"),
///         _ => false,
///     }
/// }
///
/// let std_import = Import::new("std::collections::HashMap".to_string(), ImportType::Use, 5);
/// assert!(is_standard_library(&std_import));
/// ```
///
/// ## `line_number: usize`
///
/// The 1-indexed line number where the import statement appears in the source file.
///
/// **Constraints:**
/// - Must be ≥ 1 (1-indexed, not 0-indexed)
/// - Should be ≤ total lines in the source file
/// - Used for source location and error reporting
///
/// **Usage in practice:**
/// ```rust
/// use agentscribe::parser::{Import, ImportType};
///
/// // Track import location for error reporting
/// let import = Import::new("std::collections::HashMap".to_string(), ImportType::Use, 42);
///
/// println!("Import found at line {}", import.line_number);
/// // Output: Import found at line 42
///
/// // Compare import locations
/// let import1 = Import::new("foo".to_string(), ImportType::Use, 10);
/// let import2 = Import::new("bar".to_string(), ImportType::Use, 20);
///
/// assert!(import1.line_number < import2.line_number);
/// ```
///
/// # Examples
///
/// ## Creating different import types
///
/// ```rust
/// use agentscribe::parser::{Import, ImportType};
///
/// // Create a standard library use import
/// let std_import = Import::new(
///     "std::collections::HashMap".to_string(),
///     ImportType::Use,
///     15
/// );
///
/// // Create an external crate import
/// let extern_import = Import::new(
///     "serde".to_string(),
///     ImportType::ExternCrate,
///     3
/// );
///
/// // Create a module declaration
/// let mod_import = Import::new(
///     "my_module".to_string(),
///     ImportType::Mod,
///     8
/// );
///
/// assert_eq!(std_import.import_type, ImportType::Use);
/// assert_eq!(extern_import.import_type, ImportType::ExternCrate);
/// assert_eq!(mod_import.import_type, ImportType::Mod);
/// ```
///
/// ## Working with imports programmatically
///
/// ```rust
/// use agentscribe::parser::{Import, ImportType};
///
/// let imports = vec![
///     Import::new("std::collections::HashMap".to_string(), ImportType::Use, 5),
///     Import::new("serde".to_string(), ImportType::ExternCrate, 1),
///     Import::new("crate::module::Item".to_string(), ImportType::Use, 10),
/// ];
///
/// // Count imports by type
/// let use_count = imports.iter()
///     .filter(|i| i.import_type == ImportType::Use)
///     .count();
///
/// assert_eq!(use_count, 2);
///
/// // Find standard library imports
/// let std_libs: Vec<_> = imports.iter()
///     .filter(|i| i.path.starts_with("std::"))
///     .collect();
///
/// assert_eq!(std_libs.len(), 1);
/// ```
///
/// ## Import equality and comparison
///
/// ```rust
/// use agentscribe::parser::{Import, ImportType};
///
/// let import1 = Import::new("std::fs".to_string(), ImportType::Use, 5);
/// let import2 = Import::new("std::fs".to_string(), ImportType::Use, 5);
/// let import3 = Import::new("std::fs".to_string(), ImportType::Use, 10);
///
/// // Same path, type, and line number are equal
/// assert_eq!(import1, import2);
///
/// // Different line numbers are not equal
/// assert_ne!(import1, import3);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Import {
    /// The import path (e.g., `"std::collections::HashMap"`, `"crate::module::Item"`)
    ///
    /// Contains the full module path being imported. The format varies by import type:
    ///
    /// - **For `use` statements**: Full module path (e.g., `"std::collections::HashMap"`,
    ///   `"crate::module::Item"`, `"super::ParentModule"`)
    /// - **For `extern crate`**: The crate name (e.g., `"serde"`, `"tokio"`)
    /// - **For `mod`**: The module name (e.g., `"foo"`, `"bar"`)
    ///
    /// # Valid Values
    ///
    /// - Non-empty string (cannot be `""`)
    /// - Contains only valid Rust identifier characters and path separators (`::`)
    /// - Case-sensitive (matches Rust's naming rules)
    /// - May include standard library prefixes (`std::`, `core::`, `alloc::`)
    /// - May include crate-relative paths (`crate::`, `super::`, `self`)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use agentscribe::parser::{Import, ImportType};
    ///
    /// // Standard library import
    /// let std_import = Import::new("std::collections::HashMap".to_string(), ImportType::Use, 5);
    /// assert_eq!(std_import.path, "std::collections::HashMap");
    ///
    /// // External crate import
    /// let extern_import = Import::new("serde".to_string(), ImportType::ExternCrate, 1);
    /// assert_eq!(extern_import.path, "serde");
    ///
    /// // Crate-relative import
    /// let crate_import = Import::new("crate::module::Item".to_string(), ImportType::Use, 10);
    /// assert_eq!(crate_import.path, "crate::module::Item");
    /// ```
    pub path: String,

    /// Type of import statement ([`ImportType::Use`], [`ImportType::ExternCrate`], or [`ImportType::Mod`])
    ///
    /// Distinguishes between the three fundamental kinds of import statements in Rust.
    /// This classification is essential for categorizing imports and understanding their
    /// role in the module structure.
    ///
    /// # Valid Values
    ///
    /// Only the three variants defined in [`ImportType`]:
    ///
    /// - **[`ImportType::Use`]**: Standard `use` statements for importing items from modules,
    ///   crates, or other scopes. Most common import type.
    ///
    /// - **[`ImportType::ExternCrate`]**: External crate declarations. Required in Rust 2015
    ///   edition, largely obsolete in Rust 2018+ but still used in specific contexts.
    ///
    /// - **[`ImportType::Mod`]**: Module declarations that define the module structure,
    ///   either file-based (`mod foo;`) or inline (`mod bar { ... }`).
    ///
    /// # Usage in Practice
    ///
    /// ```rust
    /// use agentscribe::parser::{Import, ImportType};
    ///
    /// // Filter imports by type
    /// fn is_standard_library(import: &Import) -> bool {
    ///     match import.import_type {
    ///         ImportType::Use => import.path.starts_with("std::") || import.path.starts_with("core::"),
    ///         ImportType::ExternCrate => false,  // External crates are not std lib
    ///         ImportType::Mod => false,  // Module declarations are not std lib imports
    ///     }
    /// }
    ///
    /// let std_import = Import::new("std::collections::HashMap".to_string(), ImportType::Use, 5);
    /// assert!(is_standard_library(&std_import));
    ///
    /// let extern_import = Import::new("serde".to_string(), ImportType::ExternCrate, 1);
    /// assert!(!is_standard_library(&extern_import));
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust
    /// use agentscribe::parser::{Import, ImportType};
    ///
    /// let use_import = Import::new("std::fs".to_string(), ImportType::Use, 5);
    /// assert_eq!(use_import.import_type, ImportType::Use);
    ///
    /// let extern_import = Import::new("tokio".to_string(), ImportType::ExternCrate, 1);
    /// assert_eq!(extern_import.import_type, ImportType::ExternCrate);
    ///
    /// let mod_import = Import::new("my_module".to_string(), ImportType::Mod, 10);
    /// assert_eq!(mod_import.import_type, ImportType::Mod);
    /// ```
    pub import_type: ImportType,

    /// Line number where the import appears (1-indexed)
    ///
    /// Represents the position in the source file where this import statement is located.
    /// Line numbers start at 1 (not 0) to match standard text editor and compiler line numbering.
    ///
    /// # Valid Values
    ///
    /// - Must be ≥ 1 (1-indexed, not 0-indexed)
    /// - Should be ≤ total lines in the source file
    /// - Used for source location, error reporting, and tool positioning
    ///
    /// # Usage in Practice
    ///
    /// This field is particularly useful for:
    ///
    /// - **Error reporting**: Point users to the exact location of problematic imports
    /// - **Import organization**: Sort or group imports by their position in the file
    /// - **Tool integration**: IDE features, linters, and code analysis tools
    /// - **Diffs and patches**: Track import location changes between file versions
    ///
    /// # Examples
    ///
    /// ```rust
    /// use agentscribe::parser::{Import, ImportType};
    ///
    /// // Track import location for error reporting
    /// let import = Import::new("std::collections::HashMap".to_string(), ImportType::Use, 42);
    ///
    /// println!("Import found at line {}", import.line_number);
    /// // Output: Import found at line 42
    ///
    /// // Sort imports by line number
    /// let mut imports = vec![
    ///     Import::new("crate::b".to_string(), ImportType::Use, 20),
    ///     Import::new("crate::a".to_string(), ImportType::Use, 10),
    ///     Import::new("std::fs".to_string(), ImportType::Use, 5),
    /// ];
    ///
    /// imports.sort_by(|a, b| a.line_number.cmp(&b.line_number));
    ///
    /// assert_eq!(imports[0].line_number, 5);
    /// assert_eq!(imports[1].line_number, 10);
    /// assert_eq!(imports[2].line_number, 20);
    /// ```
    pub line_number: usize,
}

impl Import {
    /// Create a new import
    pub fn new(path: String, import_type: ImportType, line_number: usize) -> Self {
        Self {
            path,
            import_type,
            line_number,
        }
    }
}

/// Result of parsing a Rust source file for import statements
///
/// Contains all import statements found in a Rust source file, organized by type.
/// Provides convenience methods for querying imports by type and checking if the
/// file contains any imports.
///
/// # Field Documentation
///
/// ## `imports: Vec<ImportStatement>`
/// All import statements found in the file, in the order they appear. This includes
/// `use`, `extern crate`, and `mod` statements from both the main code and test modules.
///
/// ## `use_count: usize`
/// Total number of `use` statements found in the file. This is a pre-computed count
/// for quick access without needing to filter the `imports` vector.
///
/// ## `extern_crate_count: usize`
/// Total number of `extern crate` statements found in the file. This is typically 0
/// for Rust 2018+ code where extern crate declarations are not required.
///
/// ## `mod_count: usize`
/// Total number of `mod` statements found in the file. Includes both file-based
/// modules (`mod foo;`) and inline module definitions (`mod bar { ... }`).
///
/// # Examples
///
/// ```
/// use agentscribe::parser::{ImportParser, ImportType};
///
/// let content = r#"
/// use std::collections::HashMap;
/// extern crate serde;
/// mod foo;
/// use crate::module::Item;
/// "#;
///
/// let parser = ImportParser::new();
/// let result = parser.parse_content(content);
///
/// // Access counts directly
/// assert_eq!(result.use_count, 2);
/// assert_eq!(result.extern_crate_count, 1);
/// assert_eq!(result.mod_count, 1);
///
/// // Get total count
/// assert_eq!(result.total_count(), 4);
///
/// // Check if empty
/// assert!(!result.is_empty());
///
/// // Filter by type
/// let use_imports = result.imports_by_type(ImportType::Use);
/// assert_eq!(use_imports.len(), 2);
///
/// // Access individual imports
/// assert_eq!(result.imports[0].path, "std::collections::HashMap");
/// assert_eq!(result.imports[1].path, "serde");
/// ```
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ImportParseResult {
    /// All import statements found in the file
    ///
    /// Contains every import statement (`use`, `extern crate`, `mod`) in the order
    /// they appear in the source file, including those from test modules.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use agentscribe::parser::ImportParser;
    ///
    /// let content = r#"
    /// use std::collections::HashMap;
    /// extern crate serde;
    /// mod foo;
    /// "#;
    ///
    /// let parser = ImportParser::new();
    /// let result = parser.parse_content(content);
    ///
    /// assert_eq!(result.imports.len(), 3);
    /// assert_eq!(result.imports[0].path, "std::collections::HashMap");
    /// ```
    pub imports: Vec<ImportStatement>,

    /// Total number of use statements
    ///
    /// Pre-computed count for quick access. Use this when you only need to know
    /// how many `use` statements exist without iterating through the imports vector.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use agentscribe::parser::ImportParser;
    ///
    /// let content = r#"
    /// use std::collections::HashMap;
    /// use crate::module::Item;
    /// extern crate serde;
    /// "#;
    ///
    /// let parser = ImportParser::new();
    /// let result = parser.parse_content(content);
    ///
    /// assert_eq!(result.use_count, 2);
    /// ```
    pub use_count: usize,

    /// Total number of extern crate statements
    ///
    /// Pre-computed count of `extern crate` declarations. In Rust 2018+ code,
    /// this is often 0 since extern crate declarations are no longer required.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use agentscribe::parser::ImportParser;
    ///
    /// let content = r#"
    /// extern crate serde;
    /// extern crate tokio;
    /// use std::collections::HashMap;
    /// "#;
    ///
    /// let parser = ImportParser::new();
    /// let result = parser.parse_content(content);
    ///
    /// assert_eq!(result.extern_crate_count, 2);
    /// ```
    pub extern_crate_count: usize,

    /// Total number of mod statements
    ///
    /// Pre-computed count of module declarations. Includes both file-based
    /// modules (`mod foo;`) and inline definitions (`mod bar { ... }`).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use agentscribe::parser::ImportParser;
    ///
    /// let content = r#"
    /// mod foo;
    /// mod bar;
    /// use std::collections::HashMap;
    /// "#;
    ///
    /// let parser = ImportParser::new();
    /// let result = parser.parse_content(content);
    ///
    /// assert_eq!(result.mod_count, 2);
    /// ```
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

    #[test]
    fn test_import_type_creation() {
        // Test creating all ImportType variants
        let use_type = ImportType::Use;
        let extern_crate_type = ImportType::ExternCrate;
        let mod_type = ImportType::Mod;

        // Verify they are different instances
        assert_ne!(use_type, extern_crate_type);
        assert_ne!(use_type, mod_type);
        assert_ne!(extern_crate_type, mod_type);
    }

    #[test]
    fn test_import_creation_with_all_fields() {
        // Test creating Import struct with all fields populated
        let import = Import::new("std::collections::HashMap".to_string(), ImportType::Use, 42);

        assert_eq!(import.path, "std::collections::HashMap");
        assert_eq!(import.import_type, ImportType::Use);
        assert_eq!(import.line_number, 42);
    }

    #[test]
    fn test_import_debug_formatting() {
        // Test Debug trait for Import
        let import = Import::new(
            "crate::module::Item".to_string(),
            ImportType::ExternCrate,
            15,
        );

        let debug_output = format!("{:?}", import);
        assert!(debug_output.contains("crate::module::Item"));
        assert!(debug_output.contains("ExternCrate"));
        assert!(debug_output.contains("15"));
    }

    #[test]
    fn test_import_equality_comparison() {
        // Test PartialEq for Import
        let import1 = Import::new("std::collections::HashMap".to_string(), ImportType::Use, 10);

        let import2 = Import::new("std::collections::HashMap".to_string(), ImportType::Use, 10);

        let import3 = Import::new("std::collections::HashMap".to_string(), ImportType::Use, 20);

        let import4 = Import::new("std::collections::HashSet".to_string(), ImportType::Use, 10);

        // Same fields = equal
        assert_eq!(import1, import2);

        // Different line number = not equal
        assert_ne!(import1, import3);

        // Different name = not equal
        assert_ne!(import1, import4);
    }

    #[test]
    fn test_import_clone() {
        // Test Clone trait for Import
        let original = Import::new("serde::Serialize".to_string(), ImportType::Mod, 99);

        let cloned = original.clone();

        // Verify they are equal
        assert_eq!(original, cloned);

        // Verify they are independent (changes to one don't affect the other)
        assert_eq!(cloned.path, "serde::Serialize");
        assert_eq!(cloned.import_type, ImportType::Mod);
        assert_eq!(cloned.line_number, 99);
    }

    #[test]
    fn test_import_type_equality() {
        // Test PartialEq for ImportType
        assert_eq!(ImportType::Use, ImportType::Use);
        assert_eq!(ImportType::ExternCrate, ImportType::ExternCrate);
        assert_eq!(ImportType::Mod, ImportType::Mod);

        assert_ne!(ImportType::Use, ImportType::ExternCrate);
        assert_ne!(ImportType::Use, ImportType::Mod);
        assert_ne!(ImportType::ExternCrate, ImportType::Mod);
    }

    #[test]
    fn test_import_with_different_import_types() {
        // Test Import with each ImportType variant
        let use_import = Import::new("std::fs".to_string(), ImportType::Use, 1);
        let extern_crate_import = Import::new("serde".to_string(), ImportType::ExternCrate, 2);
        let mod_import = Import::new("my_module".to_string(), ImportType::Mod, 3);

        assert_eq!(use_import.import_type, ImportType::Use);
        assert_eq!(extern_crate_import.import_type, ImportType::ExternCrate);
        assert_eq!(mod_import.import_type, ImportType::Mod);

        // All have different import types
        assert_ne!(use_import.import_type, extern_crate_import.import_type);
        assert_ne!(use_import.import_type, mod_import.import_type);
        assert_ne!(extern_crate_import.import_type, mod_import.import_type);
    }
}
