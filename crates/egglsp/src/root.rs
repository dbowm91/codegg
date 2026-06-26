use std::path::{Path, PathBuf};

use tracing::info;

pub fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut current = start
        .canonicalize()
        .ok()
        .unwrap_or_else(|| start.to_path_buf());

    loop {
        if is_project_root(&current) {
            info!(path = ?current, "found project root");
            return Some(current.clone());
        }

        if !current.pop() {
            break;
        }
    }

    let fallback = start
        .canonicalize()
        .ok()
        .unwrap_or_else(|| start.to_path_buf());
    info!(path = ?fallback, "using directory as project root");
    Some(fallback)
}

/// Root markers used by `is_project_root` and `collect_root_markers`.
const ROOT_MARKERS: &[&str] = &[
    ".git",
    "Cargo.toml",
    "package.json",
    "go.mod",
    "pyproject.toml",
    "setup.py",
    "requirements.txt",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "CMakeLists.txt",
    "Makefile",
    "Gemfile",
    "composer.json",
    "mix.exs",
    "rebar.config",
    "project.clj",
    "pubspec.yaml",
    "Package.swift",
    "terraform.tf",
    "main.tf",
    ".terraform",
    "flake.nix",
    "shell.nix",
    "default.nix",
    "stack.yaml",
    "cabal.project",
    "dune-project",
    ".bazelrc",
    "WORKSPACE",
    "BUILD.bazel",
    ".luarc.json",
    ".luacheckrc",
    "tsconfig.json",
    "jsconfig.json",
    ".eslintrc",
    ".eslintrc.json",
    ".eslintrc.js",
    ".prettierrc",
    "vite.config.ts",
    "vite.config.js",
    "next.config.js",
    "next.config.ts",
    "nuxt.config.ts",
    "angular.json",
    ".svelte-kit",
    "astro.config.mjs",
    "remix.config.js",
    "gatsby-config.js",
    ".dockerignore",
    "Dockerfile",
    "docker-compose.yml",
    "docker-compose.yaml",
    ".github",
    ".gitlab-ci.yml",
    ".circleci",
    "Jenkinsfile",
    ".pre-commit-config.yaml",
];

fn is_project_root(dir: &Path) -> bool {
    ROOT_MARKERS.iter().any(|m| dir.join(m).exists())
}

/// Diagnose the root detection for a given file path.
///
/// This is a pure function that does not start any LSP server.
/// It walks up from `input_path` looking for root markers and
/// reports what it finds.
pub fn diagnose_root(input_path: &Path, allowed_root: Option<&Path>) -> RootDiagnosisResult {
    let canonical = input_path
        .canonicalize()
        .unwrap_or_else(|_| input_path.to_path_buf());

    let input_str = canonical.display().to_string();

    let detected_language = crate::language::detect_language(canonical.to_str().unwrap_or(""));

    let mut root_markers_found = Vec::new();
    let mut current = canonical
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| canonical.clone());
    let mut selected_root = None;

    loop {
        if is_project_root(&current) {
            let markers = collect_root_markers(&current);
            root_markers_found.extend(markers);
            selected_root = Some(current);
            break;
        }
        if !current.pop() {
            break;
        }
    }

    let server_profile = detected_language.and_then(profile_for_language);

    let inside_allowed_root = match allowed_root {
        Some(root) => canonical.starts_with(root),
        None => true,
    };

    let mut issues = Vec::new();
    if selected_root.is_none() {
        issues.push("No project root marker found walking up from file".to_string());
    }
    if server_profile.is_none() {
        if let Some(lang) = detected_language {
            issues.push(format!(
                "No LSP server profile configured for language: {lang}"
            ));
        } else {
            issues.push("Could not detect language from file extension".to_string());
        }
    }
    if !inside_allowed_root {
        issues.push(format!(
            "File is outside allowed root: {}",
            allowed_root
                .map(|r| r.display().to_string())
                .unwrap_or_default()
        ));
    }

    RootDiagnosisResult {
        input_path: input_str,
        detected_language: detected_language.map(|s| s.to_string()),
        root_markers_found,
        selected_root: selected_root.map(|p| p.display().to_string()),
        server_profile: server_profile.map(|s| s.to_string()),
        inside_allowed_root,
        issues,
    }
}

/// Result of root diagnosis.
#[derive(Debug, Clone)]
pub struct RootDiagnosisResult {
    pub input_path: String,
    pub detected_language: Option<String>,
    pub root_markers_found: Vec<String>,
    pub selected_root: Option<String>,
    pub server_profile: Option<String>,
    pub inside_allowed_root: bool,
    pub issues: Vec<String>,
}

/// Collect which root markers exist in a directory.
fn collect_root_markers(dir: &Path) -> Vec<String> {
    ROOT_MARKERS
        .iter()
        .filter(|m| dir.join(m).exists())
        .map(|m| m.to_string())
        .collect()
}

/// Map a detected language to an LSP server profile ID.
fn profile_for_language(lang: &str) -> Option<&'static str> {
    crate::language::language_id_to_server_id(lang)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_diagnose_root_with_cargo_toml() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
        let src_dir = tmp.path().join("src");
        fs::create_dir(&src_dir).unwrap();
        let file = src_dir.join("main.rs");
        fs::write(&file, "fn main() {}").unwrap();

        let result = diagnose_root(&file, None);
        assert!(result.detected_language.is_some());
        assert_eq!(result.detected_language.as_deref(), Some("rust"));
        assert!(result.selected_root.is_some());
        assert!(result.server_profile.is_some());
        assert!(result.issues.is_empty());
    }

    #[test]
    fn test_diagnose_root_no_markers() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("orphan.py");
        fs::write(&file, "print('hello')").unwrap();

        let result = diagnose_root(&file, None);
        assert_eq!(result.detected_language.as_deref(), Some("python"));
        assert!(!result.issues.is_empty());
        assert!(result.issues.iter().any(|i| i.contains("No project root")));
    }

    #[test]
    fn test_diagnose_root_outside_allowed() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "").unwrap();
        let file = tmp.path().join("main.rs");
        fs::write(&file, "").unwrap();
        let allowed = Path::new("/completely/different/path");

        let result = diagnose_root(&file, Some(allowed));
        assert!(!result.inside_allowed_root);
        assert!(result
            .issues
            .iter()
            .any(|i| i.contains("outside allowed root")));
    }
}
