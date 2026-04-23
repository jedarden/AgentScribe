//! Project registry — projects.yaml schema and loader.
//!
//! Supports two shapes:
//!
//! **v0.2 (canonical)** — explicit workspace list:
//! ```yaml
//! version: "0.2"
//! projects:
//!   - name: ibkr-mcp
//!     workspaces:
//!       - path: /home/coding/ibkr-mcp
//!         role: primary
//! ```
//!
//! **v0.1 shorthand** — single `path:` field, role defaults to `primary`:
//! ```yaml
//! projects:
//!   - name: ibkr-mcp
//!     path: /home/coding/ibkr-mcp
//! ```
//!
//! Loading normalises v0.1 to v0.2 in memory. `migrate()` rewrites the file
//! to v0.2 format, preserving all data.

use std::collections::HashSet;
use std::path::Path;
use std::{fmt, fs};

use serde::{Deserialize, Serialize};

use crate::error::{AgentScribeError, Result};

/// Schema version emitted by `save()` and `migrate()`.
pub const SCHEMA_VERSION: &str = "0.2";

// ── Public types ──────────────────────────────────────────────────────────────

/// A workspace directory associated with a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub path: String,
    pub role: String,
}

/// A project entry in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub workspaces: Vec<Workspace>,
}

/// Parsed and validated projects.yaml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectsFile {
    pub version: String,
    pub projects: Vec<Project>,
}

/// A non-fatal warning produced during loading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectWarning {
    pub project: String,
    pub message: String,
}

impl fmt::Display for ProjectWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "project '{}': {}", self.project, self.message)
    }
}

// ── Private deserialization types (raw/unvalidated) ───────────────────────────

#[derive(Debug, Deserialize)]
struct ProjectRaw {
    name: String,
    label: Option<String>,
    color: Option<String>,
    /// v0.1 shorthand — single workspace path; role defaults to `primary`.
    path: Option<String>,
    /// v0.2 explicit workspace list.
    #[serde(default)]
    workspaces: Vec<Workspace>,
}

#[derive(Debug, Deserialize)]
struct ProjectsFileRaw {
    version: Option<String>,
    #[serde(default)]
    projects: Vec<ProjectRaw>,
}

// ── Validation helpers ────────────────────────────────────────────────────────

/// Accepts `#RGB` and `#RRGGBB` (case-insensitive).
fn is_valid_hex_color(s: &str) -> bool {
    let Some(hex) = s.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 3 | 6) && hex.chars().all(|c| c.is_ascii_hexdigit())
}

// ── Normalisation ─────────────────────────────────────────────────────────────

fn normalize_project(raw: ProjectRaw) -> Result<(Project, Vec<ProjectWarning>)> {
    let mut warnings = Vec::new();

    let workspaces: Vec<Workspace> = match (raw.path.as_deref(), raw.workspaces.is_empty()) {
        (Some(_), false) => {
            return Err(AgentScribeError::Projects(format!(
                "project '{}': 'path' shorthand and 'workspaces' cannot both be specified",
                raw.name
            )));
        }
        (Some(path), true) => {
            vec![Workspace {
                path: path.to_string(),
                role: "primary".to_string(),
            }]
        }
        (None, false) => raw.workspaces,
        (None, true) => {
            return Err(AgentScribeError::Projects(format!(
                "project '{}': must have at least one workspace \
                 (use 'path' shorthand or 'workspaces' list)",
                raw.name
            )));
        }
    };

    // Validate workspace paths and roles are non-empty.
    for (i, ws) in workspaces.iter().enumerate() {
        if ws.path.trim().is_empty() {
            return Err(AgentScribeError::Projects(format!(
                "project '{}': workspace[{}] has an empty path",
                raw.name, i
            )));
        }
        if ws.role.trim().is_empty() {
            return Err(AgentScribeError::Projects(format!(
                "project '{}': workspace[{}] has an empty role",
                raw.name, i
            )));
        }
    }

    // Warn (but allow) duplicate roles within a project.
    let mut seen_roles: HashSet<&str> = HashSet::new();
    for ws in &workspaces {
        if !seen_roles.insert(ws.role.as_str()) {
            warnings.push(ProjectWarning {
                project: raw.name.clone(),
                message: format!("duplicate workspace role '{}'", ws.role),
            });
        }
    }

    // Validate hex color if present.
    if let Some(ref color) = raw.color {
        if !is_valid_hex_color(color) {
            return Err(AgentScribeError::Projects(format!(
                "project '{}': invalid color '{}' — expected #RGB or #RRGGBB",
                raw.name, color
            )));
        }
    }

    Ok((
        Project {
            name: raw.name,
            label: raw.label,
            color: raw.color,
            workspaces,
        },
        warnings,
    ))
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Load and validate `projects.yaml`, returning the canonical file and any
/// non-fatal warnings.
///
/// v0.1 shorthand is normalised to v0.2 in memory; the file on disk is not
/// modified. Use [`migrate`] to rewrite the file.
pub fn load_with_warnings(path: &Path) -> Result<(ProjectsFile, Vec<ProjectWarning>)> {
    let content = fs::read_to_string(path).map_err(|e| {
        AgentScribeError::Projects(format!("cannot read {}: {}", path.display(), e))
    })?;

    let raw: ProjectsFileRaw = serde_yaml::from_str(&content).map_err(|e| {
        AgentScribeError::Projects(format!("YAML parse error in {}: {}", path.display(), e))
    })?;

    let mut projects = Vec::with_capacity(raw.projects.len());
    let mut all_warnings = Vec::new();
    let mut seen_names: HashSet<String> = HashSet::new();

    for project_raw in raw.projects {
        if !seen_names.insert(project_raw.name.clone()) {
            return Err(AgentScribeError::Projects(format!(
                "duplicate project name '{}'",
                project_raw.name
            )));
        }
        let (project, warnings) = normalize_project(project_raw)?;
        projects.push(project);
        all_warnings.extend(warnings);
    }

    Ok((
        ProjectsFile {
            version: SCHEMA_VERSION.to_string(),
            projects,
        },
        all_warnings,
    ))
}

/// Load and validate `projects.yaml`. Warnings are silently discarded.
pub fn load(path: &Path) -> Result<ProjectsFile> {
    let (file, _warnings) = load_with_warnings(path)?;
    Ok(file)
}

/// Serialize a [`ProjectsFile`] to `path`, creating parent directories as
/// needed.
pub fn save(file: &ProjectsFile, path: &Path) -> Result<()> {
    let content = serde_yaml::to_string(file)
        .map_err(|e| AgentScribeError::Projects(format!("YAML serialization error: {}", e)))?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| AgentScribeError::Projects(format!("cannot create directory: {}", e)))?;
    }

    fs::write(path, content).map_err(|e| {
        AgentScribeError::Projects(format!("cannot write {}: {}", path.display(), e))
    })?;

    Ok(())
}

/// Migrate `projects.yaml` from v0.1 to v0.2 in place.
///
/// - Expands `path:` shorthand to a `workspaces:` list with role `primary`.
/// - Updates the `version` field to `"0.2"`.
/// - Returns `true` if the file was rewritten, `false` if already at v0.2.
pub fn migrate(path: &Path) -> Result<bool> {
    let content = fs::read_to_string(path).map_err(|e| {
        AgentScribeError::Projects(format!("cannot read {}: {}", path.display(), e))
    })?;

    let raw: ProjectsFileRaw = serde_yaml::from_str(&content)
        .map_err(|e| AgentScribeError::Projects(format!("YAML parse error: {}", e)))?;

    let needs_migration = raw.version.as_deref() != Some(SCHEMA_VERSION)
        || raw.projects.iter().any(|p| p.path.is_some());

    if !needs_migration {
        return Ok(false);
    }

    let (canonical, _warnings) = load_with_warnings(path)?;
    save(&canonical, path)?;

    Ok(true)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_yaml(dir: &Path, name: &str, content: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        fs::write(&path, content).unwrap();
        path
    }

    // ── Happy-path loading ──────────────────────────────────────────────────

    #[test]
    fn test_load_v1_shorthand() {
        let dir = tempdir().unwrap();
        let path = write_yaml(
            dir.path(),
            "projects.yaml",
            r"
projects:
  - name: ibkr-mcp
    path: /home/coding/ibkr-mcp
",
        );

        let (file, warnings) = load_with_warnings(&path).unwrap();

        assert_eq!(file.version, "0.2");
        assert_eq!(file.projects.len(), 1);

        let proj = &file.projects[0];
        assert_eq!(proj.name, "ibkr-mcp");
        assert_eq!(proj.workspaces.len(), 1);
        assert_eq!(proj.workspaces[0].path, "/home/coding/ibkr-mcp");
        assert_eq!(proj.workspaces[0].role, "primary");
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_load_v2_multi_workspace() {
        let dir = tempdir().unwrap();
        let path = write_yaml(
            dir.path(),
            "projects.yaml",
            r"
version: '0.2'
projects:
  - name: my-app
    label: My App
    color: '#ff6600'
    workspaces:
      - path: /home/coding/my-app
        role: primary
      - path: /home/coding/my-app-manifests
        role: manifests
      - path: /home/coding/secrets
        role: secrets
",
        );

        let (file, warnings) = load_with_warnings(&path).unwrap();

        assert_eq!(file.version, "0.2");
        let proj = &file.projects[0];
        assert_eq!(proj.name, "my-app");
        assert_eq!(proj.label.as_deref(), Some("My App"));
        assert_eq!(proj.color.as_deref(), Some("#ff6600"));
        assert_eq!(proj.workspaces.len(), 3);
        assert_eq!(proj.workspaces[1].role, "manifests");
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_load_v1_explicit_version_shorthand() {
        let dir = tempdir().unwrap();
        let path = write_yaml(
            dir.path(),
            "projects.yaml",
            r"
version: '0.1'
projects:
  - name: foo
    path: /tmp/foo
",
        );

        let (file, _) = load_with_warnings(&path).unwrap();
        assert_eq!(file.version, "0.2", "always upgraded to 0.2 in memory");
        assert_eq!(file.projects[0].workspaces[0].role, "primary");
    }

    #[test]
    fn test_multiple_projects() {
        let dir = tempdir().unwrap();
        let path = write_yaml(
            dir.path(),
            "projects.yaml",
            r"
projects:
  - name: alpha
    path: /alpha
  - name: beta
    workspaces:
      - path: /beta
        role: primary
",
        );

        let (file, _) = load_with_warnings(&path).unwrap();
        assert_eq!(file.projects.len(), 2);
    }

    // ── Validation errors ───────────────────────────────────────────────────

    #[test]
    fn test_empty_workspaces_array_rejected() {
        let dir = tempdir().unwrap();
        let path = write_yaml(
            dir.path(),
            "projects.yaml",
            r"
projects:
  - name: bad
    workspaces: []
",
        );

        let err = load(&path).unwrap_err();
        assert!(
            err.to_string().contains("at least one workspace"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_no_path_no_workspaces_rejected() {
        let dir = tempdir().unwrap();
        let path = write_yaml(
            dir.path(),
            "projects.yaml",
            r"
projects:
  - name: bad
",
        );

        let err = load(&path).unwrap_err();
        assert!(err.to_string().contains("at least one workspace"));
    }

    #[test]
    fn test_path_and_workspaces_together_rejected() {
        let dir = tempdir().unwrap();
        let path = write_yaml(
            dir.path(),
            "projects.yaml",
            r"
projects:
  - name: bad
    path: /tmp/bad
    workspaces:
      - path: /tmp/also-bad
        role: primary
",
        );

        let err = load(&path).unwrap_err();
        assert!(err.to_string().contains("cannot both be specified"));
    }

    #[test]
    fn test_duplicate_project_names_rejected() {
        let dir = tempdir().unwrap();
        let path = write_yaml(
            dir.path(),
            "projects.yaml",
            r"
projects:
  - name: dup
    path: /a
  - name: dup
    path: /b
",
        );

        let err = load(&path).unwrap_err();
        assert!(err.to_string().contains("duplicate project name"));
    }

    #[test]
    fn test_invalid_hex_color_rejected() {
        let dir = tempdir().unwrap();
        let path = write_yaml(
            dir.path(),
            "projects.yaml",
            r"
projects:
  - name: bad-color
    color: 'red'
    path: /tmp/x
",
        );

        let err = load(&path).unwrap_err();
        assert!(err.to_string().contains("invalid color"));
    }

    #[test]
    fn test_valid_hex_colors() {
        let dir = tempdir().unwrap();
        let path = write_yaml(
            dir.path(),
            "projects.yaml",
            r"
projects:
  - name: short
    color: '#abc'
    path: /tmp/s
  - name: long
    color: '#aabbcc'
    path: /tmp/l
",
        );

        assert!(load(&path).is_ok());
    }

    #[test]
    fn test_empty_workspace_path_rejected() {
        let dir = tempdir().unwrap();
        let path = write_yaml(
            dir.path(),
            "projects.yaml",
            r"
projects:
  - name: bad
    workspaces:
      - path: ''
        role: primary
",
        );

        let err = load(&path).unwrap_err();
        assert!(err.to_string().contains("empty path"));
    }

    #[test]
    fn test_empty_workspace_role_rejected() {
        let dir = tempdir().unwrap();
        let path = write_yaml(
            dir.path(),
            "projects.yaml",
            r"
projects:
  - name: bad
    workspaces:
      - path: /tmp/x
        role: ''
",
        );

        let err = load(&path).unwrap_err();
        assert!(err.to_string().contains("empty role"));
    }

    // ── Warnings ────────────────────────────────────────────────────────────

    #[test]
    fn test_duplicate_roles_allowed_with_warning() {
        let dir = tempdir().unwrap();
        let path = write_yaml(
            dir.path(),
            "projects.yaml",
            r"
projects:
  - name: multi
    workspaces:
      - path: /a
        role: primary
      - path: /b
        role: primary
",
        );

        let (file, warnings) = load_with_warnings(&path).unwrap();
        assert_eq!(file.projects[0].workspaces.len(), 2, "both workspaces kept");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0]
            .to_string()
            .contains("duplicate workspace role 'primary'"));
    }

    #[test]
    fn test_no_warnings_when_roles_unique() {
        let dir = tempdir().unwrap();
        let path = write_yaml(
            dir.path(),
            "projects.yaml",
            r"
projects:
  - name: clean
    workspaces:
      - path: /a
        role: primary
      - path: /b
        role: manifests
",
        );

        let (_, warnings) = load_with_warnings(&path).unwrap();
        assert!(warnings.is_empty());
    }

    // ── Migration ───────────────────────────────────────────────────────────

    #[test]
    fn test_migrate_v1_to_v2_lossless() {
        let dir = tempdir().unwrap();
        let path = write_yaml(
            dir.path(),
            "projects.yaml",
            r"
projects:
  - name: ibkr-mcp
    label: IBKR MCP
    color: '#336699'
    path: /home/coding/ibkr-mcp
  - name: other
    path: /other
",
        );

        let migrated = migrate(&path).unwrap();
        assert!(migrated, "expected migration to run");

        // Read back and verify all fields preserved.
        let file = load(&path).unwrap();
        assert_eq!(file.version, "0.2");
        assert_eq!(file.projects.len(), 2);

        let first = &file.projects[0];
        assert_eq!(first.name, "ibkr-mcp");
        assert_eq!(first.label.as_deref(), Some("IBKR MCP"));
        assert_eq!(first.color.as_deref(), Some("#336699"));
        assert_eq!(first.workspaces.len(), 1);
        assert_eq!(first.workspaces[0].path, "/home/coding/ibkr-mcp");
        assert_eq!(first.workspaces[0].role, "primary");
    }

    #[test]
    fn test_migrate_already_v2_no_op() {
        let dir = tempdir().unwrap();
        let path = write_yaml(
            dir.path(),
            "projects.yaml",
            r"
version: '0.2'
projects:
  - name: already
    workspaces:
      - path: /here
        role: primary
",
        );

        let migrated = migrate(&path).unwrap();
        assert!(!migrated, "should be a no-op");
    }

    #[test]
    fn test_migrate_v1_with_explicit_version_field() {
        let dir = tempdir().unwrap();
        let path = write_yaml(
            dir.path(),
            "projects.yaml",
            r"
version: '0.1'
projects:
  - name: foo
    path: /foo
",
        );

        let migrated = migrate(&path).unwrap();
        assert!(migrated);

        let file = load(&path).unwrap();
        assert_eq!(file.version, "0.2");
        assert_eq!(file.projects[0].workspaces[0].role, "primary");
    }

    // ── Round-trip ──────────────────────────────────────────────────────────

    #[test]
    fn test_save_and_reload_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("projects.yaml");

        let original = ProjectsFile {
            version: SCHEMA_VERSION.to_string(),
            projects: vec![Project {
                name: "test-project".to_string(),
                label: Some("Test Project".to_string()),
                color: Some("#123abc".to_string()),
                workspaces: vec![
                    Workspace {
                        path: "/primary".to_string(),
                        role: "primary".to_string(),
                    },
                    Workspace {
                        path: "/manifests".to_string(),
                        role: "manifests".to_string(),
                    },
                ],
            }],
        };

        save(&original, &path).unwrap();
        let reloaded = load(&path).unwrap();

        assert_eq!(reloaded.version, original.version);
        assert_eq!(reloaded.projects.len(), 1);
        let proj = &reloaded.projects[0];
        assert_eq!(proj.name, "test-project");
        assert_eq!(proj.label.as_deref(), Some("Test Project"));
        assert_eq!(proj.color.as_deref(), Some("#123abc"));
        assert_eq!(proj.workspaces.len(), 2);
        assert_eq!(proj.workspaces[0].role, "primary");
        assert_eq!(proj.workspaces[1].role, "manifests");
    }
}
