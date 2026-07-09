use std::path::Path;

use thiserror::Error;

use crate::test_runner::custom::{
    validate_custom_command, CustomCommandValidationError, ValidatedCustomCommand,
};
use crate::test_runner::index::{self, TestIndexError};
use crate::test_runner::types::{ResolvedTestCommand, TestLanguage, TestRunRequest, TestScope};

#[derive(Error, Debug)]
pub enum TestResolveError {
    #[error("missing workdir")]
    MissingWorkdir,
    #[error("missing package name for package scope")]
    MissingPackageName,
    #[error("missing file path for file scope")]
    MissingFilePath,
    #[error("empty custom command")]
    EmptyCustomCommand,
    #[error("custom command validation failed: {0}")]
    CustomCommandInvalid(#[from] CustomCommandValidationError),
    #[error("ambiguous ecosystem: both Rust and Python detected; use an explicit scope")]
    AmbiguousEcosystem,
    #[error("no supported test ecosystem detected in {0}")]
    UnsupportedEcosystem(String),
    #[error("unsupported scope {scope:?} for {language:?} ecosystem")]
    UnsupportedScopeForEcosystem {
        scope: &'static str,
        language: TestLanguage,
    },
    #[error(transparent)]
    PreviousFailures(#[from] TestIndexError),
}

pub type Result<T> = std::result::Result<T, TestResolveError>;

pub fn resolve_test_command(request: &TestRunRequest) -> Result<ResolvedTestCommand> {
    let workdir = &request.workdir;
    if !workdir.exists() {
        return Err(TestResolveError::MissingWorkdir);
    }

    match &request.scope {
        TestScope::Auto => resolve_auto(workdir),
        TestScope::Workspace => resolve_workspace(workdir),
        TestScope::Changed => resolve_auto(workdir),
        TestScope::Package(name) => resolve_package(workdir, name),
        TestScope::File(path) => resolve_file(workdir, path),
        TestScope::PreviousFailures => resolve_previous_failures(workdir),
        TestScope::CustomCommand(cmd) => {
            let validated = resolve_validated_custom_command(cmd)?;
            Ok(ResolvedTestCommand {
                language: TestLanguage::Generic,
                argv: validated.argv,
                cwd: workdir.to_path_buf(),
                scope_label: format!("custom:{}", validated.label),
            })
        }
    }
}

fn resolve_previous_failures(workdir: &Path) -> Result<ResolvedTestCommand> {
    let index = index::load_index(workdir)?;
    let entry = index::find_newest_actionable_failure(&index, workdir)
        .ok_or(TestIndexError::NoPreviousFailures)?;

    index::validate_indexed_rerun_command(&entry.argv, workdir, &entry.cwd)?;

    Ok(ResolvedTestCommand {
        language: TestLanguage::Generic,
        argv: entry.argv.clone(),
        cwd: entry.cwd.clone(),
        scope_label: format!("previous-failures:{}", entry.run_id),
    })
}

fn resolve_auto(workdir: &Path) -> Result<ResolvedTestCommand> {
    let has_rust = has_cargo_manifest(workdir);
    let has_python = has_python_test_markers(workdir);

    match (has_rust, has_python) {
        (true, false) => Ok(ResolvedTestCommand {
            language: TestLanguage::Rust,
            argv: vec!["cargo".into(), "test".into()],
            cwd: workdir.to_path_buf(),
            scope_label: "auto-rust".to_string(),
        }),
        (false, true) => Ok(ResolvedTestCommand {
            language: TestLanguage::Python,
            argv: vec!["pytest".into()],
            cwd: workdir.to_path_buf(),
            scope_label: "auto-python".to_string(),
        }),
        (true, true) => Err(TestResolveError::AmbiguousEcosystem),
        (false, false) => Err(TestResolveError::UnsupportedEcosystem(
            workdir.display().to_string(),
        )),
    }
}

fn resolve_workspace(workdir: &Path) -> Result<ResolvedTestCommand> {
    if has_cargo_manifest(workdir) {
        return Ok(ResolvedTestCommand {
            language: TestLanguage::Rust,
            argv: vec!["cargo".into(), "test".into()],
            cwd: workdir.to_path_buf(),
            scope_label: "workspace".to_string(),
        });
    }
    if has_python_test_markers(workdir) {
        return Ok(ResolvedTestCommand {
            language: TestLanguage::Python,
            argv: vec!["pytest".into()],
            cwd: workdir.to_path_buf(),
            scope_label: "workspace".to_string(),
        });
    }
    Err(TestResolveError::UnsupportedEcosystem(
        workdir.display().to_string(),
    ))
}

fn resolve_package(workdir: &Path, name: &str) -> Result<ResolvedTestCommand> {
    if has_cargo_manifest(workdir) {
        return Ok(ResolvedTestCommand {
            language: TestLanguage::Rust,
            argv: vec!["cargo".into(), "test".into(), "-p".into(), name.to_string()],
            cwd: workdir.to_path_buf(),
            scope_label: format!("package:{name}"),
        });
    }
    if has_python_test_markers(workdir) {
        return Err(TestResolveError::UnsupportedScopeForEcosystem {
            scope: "Package",
            language: TestLanguage::Python,
        });
    }
    Err(TestResolveError::UnsupportedEcosystem(
        workdir.display().to_string(),
    ))
}

fn resolve_file(workdir: &Path, path: &Path) -> Result<ResolvedTestCommand> {
    if has_cargo_manifest(workdir) {
        let label = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".to_string());
        return Ok(ResolvedTestCommand {
            language: TestLanguage::Rust,
            argv: vec!["cargo".into(), "test".into()],
            cwd: workdir.to_path_buf(),
            scope_label: format!("file:{label}"),
        });
    }
    if has_python_test_markers(workdir) {
        let argv = vec!["pytest".into(), path.to_string_lossy().into_owned()];
        let label = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".to_string());
        return Ok(ResolvedTestCommand {
            language: TestLanguage::Python,
            argv,
            cwd: workdir.to_path_buf(),
            scope_label: format!("file:{label}"),
        });
    }
    Err(TestResolveError::UnsupportedEcosystem(
        workdir.display().to_string(),
    ))
}

/// Validate and tokenize a custom command, returning the argv vector
/// ready for direct (non-shell) execution.
///
/// The resolver re-runs the strict validator as a defense-in-depth
/// measure — even if a caller forgets to validate at the boundary,
/// the resolver still rejects shell metacharacters, redirection,
/// command substitution, and allowlist-prefix smuggling.
///
/// On `Empty` this returns `EmptyCustomCommand` (the legacy variant)
/// so existing callers that distinguish empty input keep working.
pub fn resolve_validated_custom_command(
    cmd: &str,
) -> std::result::Result<ValidatedCustomCommand, TestResolveError> {
    match validate_custom_command(cmd) {
        Ok(v) => Ok(v),
        Err(CustomCommandValidationError::Empty) => Err(TestResolveError::EmptyCustomCommand),
        Err(other) => Err(TestResolveError::CustomCommandInvalid(other)),
    }
}

pub fn has_cargo_manifest(workdir: &Path) -> bool {
    workdir.join("Cargo.toml").exists()
}

pub fn has_python_test_markers(workdir: &Path) -> bool {
    workdir.join("pyproject.toml").exists()
        || workdir.join("pytest.ini").exists()
        || workdir.join("tox.ini").exists()
        || workdir.join("noxfile.py").exists()
        || workdir.join("tests").is_dir()
}

pub fn detect_language_for_auto(
    workdir: &Path,
) -> std::result::Result<TestLanguage, TestResolveError> {
    let has_rust = has_cargo_manifest(workdir);
    let has_python = has_python_test_markers(workdir);
    match (has_rust, has_python) {
        (true, false) => Ok(TestLanguage::Rust),
        (false, true) => Ok(TestLanguage::Python),
        (true, true) => Err(TestResolveError::AmbiguousEcosystem),
        (false, false) => Err(TestResolveError::UnsupportedEcosystem(
            workdir.display().to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_runner::custom::CustomCommandValidationError;
    use std::fs;

    fn temp_dir_with_files(_name: &str, files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for file in files {
            let path = dir.path().join(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&path, "").unwrap();
        }
        dir
    }

    fn request(scope: TestScope, workdir: &Path) -> TestRunRequest {
        TestRunRequest {
            scope,
            workdir: workdir.to_path_buf(),
            timeout_secs: None,
            stall_timeout_secs: None,
            max_report_bytes: None,
            session_id: None,
        }
    }

    #[test]
    fn resolves_auto_rust_when_cargo_toml_exists() {
        let dir = temp_dir_with_files("rust", &["Cargo.toml"]);
        let result = resolve_test_command(&request(TestScope::Auto, dir.path())).unwrap();
        assert_eq!(result.language, TestLanguage::Rust);
        assert_eq!(result.argv, vec!["cargo", "test"]);
        assert_eq!(result.scope_label, "auto-rust");
    }

    #[test]
    fn resolves_auto_python_when_pytest_markers_exist() {
        let dir = temp_dir_with_files("py", &["pyproject.toml"]);
        let result = resolve_test_command(&request(TestScope::Auto, dir.path())).unwrap();
        assert_eq!(result.language, TestLanguage::Python);
        assert_eq!(result.argv, vec!["pytest"]);
        assert_eq!(result.scope_label, "auto-python");
    }

    #[test]
    fn returns_ambiguity_for_mixed_rust_python_root() {
        let dir = temp_dir_with_files("mixed", &["Cargo.toml", "pyproject.toml"]);
        let result = resolve_test_command(&request(TestScope::Auto, dir.path()));
        assert!(matches!(result, Err(TestResolveError::AmbiguousEcosystem)));
    }

    #[test]
    fn resolves_rust_package_scope() {
        let dir = temp_dir_with_files("rust-pkg", &["Cargo.toml"]);
        let result =
            resolve_test_command(&request(TestScope::Package("my-crate".into()), dir.path()))
                .unwrap();
        assert_eq!(result.language, TestLanguage::Rust);
        assert_eq!(result.argv, vec!["cargo", "test", "-p", "my-crate"]);
        assert_eq!(result.scope_label, "package:my-crate");
    }

    #[test]
    fn resolves_python_file_scope() {
        let dir = temp_dir_with_files("py-file", &["tests/__init__.py"]);
        let path = dir.path().join("tests/test_foo.py");
        fs::write(&path, "").unwrap();
        let result =
            resolve_test_command(&request(TestScope::File(path.clone()), dir.path())).unwrap();
        assert_eq!(result.language, TestLanguage::Python);
        assert_eq!(result.argv, vec!["pytest", path.to_string_lossy().as_ref()]);
        assert_eq!(result.scope_label, "file:test_foo.py");
    }

    #[test]
    fn changed_scope_uses_auto_fallback() {
        let dir = temp_dir_with_files("changed-rust", &["Cargo.toml"]);
        let result = resolve_test_command(&request(TestScope::Changed, dir.path())).unwrap();
        assert_eq!(result.language, TestLanguage::Rust);
        assert_eq!(result.scope_label, "auto-rust");
    }

    #[test]
    fn custom_command_is_tokenized_into_argv() {
        let dir = temp_dir_with_files("custom", &[]);
        let result = resolve_test_command(&request(
            TestScope::CustomCommand("cargo test --lib".into()),
            dir.path(),
        ))
        .unwrap();
        assert_eq!(result.language, TestLanguage::Generic);
        assert_eq!(result.argv, vec!["cargo", "test", "--lib"]);
        assert_eq!(result.scope_label, "custom:cargo test");
    }

    #[test]
    fn custom_command_rejects_forbidden_shell_syntax() {
        let dir = temp_dir_with_files("custom-bypass", &[]);
        let result = resolve_test_command(&request(
            TestScope::CustomCommand("cargo test; rm -rf /".into()),
            dir.path(),
        ));
        assert!(matches!(
            result,
            Err(TestResolveError::CustomCommandInvalid(
                CustomCommandValidationError::ForbiddenShellSyntax
            ))
        ));
    }

    #[test]
    fn custom_command_rejects_unsupported_command() {
        let dir = temp_dir_with_files("custom-unsupported", &[]);
        let result = resolve_test_command(&request(
            TestScope::CustomCommand("rm -rf /".into()),
            dir.path(),
        ));
        assert!(matches!(
            result,
            Err(TestResolveError::CustomCommandInvalid(
                CustomCommandValidationError::UnsupportedCommand
            ))
        ));
    }

    #[test]
    fn custom_command_empty_maps_to_empty_custom_error() {
        let dir = temp_dir_with_files("custom-empty", &[]);
        let result =
            resolve_test_command(&request(TestScope::CustomCommand("".into()), dir.path()));
        assert!(matches!(result, Err(TestResolveError::EmptyCustomCommand)));
    }

    #[test]
    fn custom_command_rejects_prefix_collision() {
        let dir = temp_dir_with_files("custom-collision", &[]);
        let result = resolve_test_command(&request(
            TestScope::CustomCommand("pytestevil".into()),
            dir.path(),
        ));
        assert!(matches!(
            result,
            Err(TestResolveError::CustomCommandInvalid(
                CustomCommandValidationError::UnsupportedCommand
            ))
        ));
    }
}
