use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer};

use crate::error::ToolError;

/// Well-known macOS aliases for system directories that are exposed
/// as symlinks under `/private/*`. We allow these so that paths
/// under `/var/...`, `/tmp/...`, and `/etc/...` continue to work on
/// macOS, where the public prefix is a symlink to the real
/// directory under `/private/`.
const MACOS_SYSTEM_ALIASES: &[&str] = &[
    "/var",
    "/tmp",
    "/etc",
    "/private/var",
    "/private/tmp",
    "/private/etc",
];

/// Resolve an existing path against the workspace root.
///
/// A **relative** path is relative to `allowed_root`, never to the process
/// working directory. CodeGG runs every turn inside a long-lived user-scoped
/// daemon whose cwd is wherever that daemon happened to be launched, so
/// canonicalizing the raw relative path resolved it against that arbitrary
/// directory instead of the session's own workspace. A `read` of
/// `src/http/ratelimit.rs` from a session rooted at one repository then failed
/// while the file sat at the expected workspace path — and the failure hint,
/// which *did* join the root, would suggest the very file it claimed was
/// missing.
///
/// [`validate_target_path`] already joined the root for missing-suffix targets;
/// this is the same rule for paths that must already exist. Keeping the two
/// consistent matters: `write` resolved a relative path correctly while `read`
/// did not, so a write→read round-trip of one relative path could not read
/// back what it had just written.
pub fn validate_path(path: &Path, allowed_root: &Path) -> Result<PathBuf, ToolError> {
    let root_canonical = allowed_root
        .canonicalize()
        .map_err(|_| ToolError::Execution("invalid allowed root".to_string()))?;

    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root_canonical.join(path)
    };
    // Normalize before the symlink walk so a `..` cannot walk the check out of
    // the root, then again on the canonical result below.
    let candidate = lexically_normalize(&candidate);

    // The check runs on the resolved absolute path. On the raw relative path it
    // inspected the daemon's cwd and was checking a path nobody asked for.
    check_path_for_symlinks(&candidate)?;

    // Errors still name the path the caller sent, not the derived one.
    let canonical = candidate
        .canonicalize()
        .map_err(|_| ToolError::Execution(format!("invalid path: {}", path.display())))?;

    if !canonical.starts_with(&root_canonical) {
        return Err(ToolError::Permission(format!(
            "path '{}' is outside allowed directory",
            path.display()
        )));
    }

    Ok(canonical)
}

/// Validate a file target whose leaf and/or parent directories may not exist.
/// Existing path prefixes are canonicalized so symlinks and `..` cannot escape
/// the allowed root, while the missing suffix is resolved lexically.
pub fn validate_target_path(path: &Path, allowed_root: &Path) -> Result<PathBuf, ToolError> {
    let root_canonical = allowed_root
        .canonicalize()
        .map_err(|_| ToolError::Execution("invalid allowed root".to_string()))?;
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root_canonical.join(path)
    };
    let candidate = lexically_normalize(&candidate);

    check_path_for_symlinks(&candidate)?;

    let mut existing = candidate.clone();
    let mut suffix = Vec::new();
    while !existing.exists() {
        let component = existing
            .file_name()
            .ok_or_else(|| ToolError::Execution("invalid path".to_string()))?
            .to_os_string();
        suffix.push(component);
        existing = existing
            .parent()
            .ok_or_else(|| ToolError::Execution("invalid path".to_string()))?
            .to_path_buf();
    }

    check_path_for_symlinks(&existing)?;
    let existing_canonical = existing
        .canonicalize()
        .map_err(|_| ToolError::Execution(format!("invalid path: {}", existing.display())))?;
    if !existing_canonical.starts_with(&root_canonical) {
        return Err(ToolError::Permission(format!(
            "path '{}' is outside allowed directory",
            path.display()
        )));
    }

    let mut normalized = existing_canonical;
    for component in suffix.iter().rev() {
        if component == std::ffi::OsStr::new(".") {
            continue;
        }
        if component == std::ffi::OsStr::new("..") {
            normalized.pop();
        } else {
            normalized.push(component);
        }
        if !normalized.starts_with(&root_canonical) {
            return Err(ToolError::Permission(format!(
                "path '{}' is outside allowed directory",
                path.display()
            )));
        }
    }

    Ok(normalized)
}

/// Resolve a caller-supplied working directory against the workspace root.
///
/// Delegates to [`validate_target_path`] rather than repeating its containment
/// walk, so a `workdir` cannot diverge from the rule every file path obeys. The
/// directory need not exist yet — that is the runner's problem to report, not
/// a containment question. Only the error label is restated, because "path is
/// outside allowed directory" reads as a mistake when the caller sent a
/// perfectly ordinary `workdir`.
pub fn resolve_workdir(raw: &Path, workspace_root: &Path) -> Result<PathBuf, ToolError> {
    validate_target_path(raw, workspace_root).map_err(|error| match error {
        ToolError::Permission(message) => {
            ToolError::Permission(message.replacen("path '", "workdir '", 1))
        }
        other => other,
    })
}

fn lexically_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

pub fn canonicalize_path(path: &Path) -> Result<PathBuf, ToolError> {
    check_path_for_symlinks(path)?;
    path.canonicalize()
        .map_err(|_| ToolError::Execution(format!("invalid path: {}", path.display())))
}

/// Check that no component of `path` is a user-created symlink that
/// would redirect the lookup somewhere unexpected. System-level
/// symlinks (macOS `/var`, `/tmp`, `/etc` -> `/private/...`) are
/// allowed so that paths under them continue to work on macOS.
pub fn check_path_for_symlinks(path: &Path) -> Result<(), ToolError> {
    let mut current = PathBuf::new();

    for component in path.components() {
        current.push(component);

        // Skip non-existent components (path traverses a directory
        // that does not yet exist). The caller will resolve or
        // create it later.
        let Ok(meta) = current.symlink_metadata() else {
            continue;
        };

        if meta.file_type().is_symlink() {
            // Allow the macOS system aliases themselves
            // (`/var`, `/tmp`, `/etc`), but NOT arbitrary children
            // of those directories, which may be user-created
            // symlinks.
            if is_macos_system_alias(&current) {
                continue;
            }
            return Err(ToolError::Permission(format!(
                "symlink not allowed in path: {}",
                current.display()
            )));
        }
    }

    Ok(())
}

fn is_macos_system_alias(path: &Path) -> bool {
    let s = path.to_string_lossy();
    MACOS_SYSTEM_ALIASES.iter().any(|alias| s == *alias)
}

/// Read a whole-valued JSON number as `usize`.
///
/// Tool schemas declared integer parameters as `"type": "number"`, and a
/// model that follows one literally sends `500.0` for a count. `as_u64()`
/// rejects that, and a derived `usize` field rejects it harder — the call
/// became a hard tool error for a value that carries exactly the integer the
/// caller meant. A fractional number is still rejected: rounding a real
/// `1.5` would hide a caller bug rather than absorb a formatting quirk.
pub fn json_usize(value: &serde_json::Value) -> Option<usize> {
    let number = value.as_number()?;
    if let Some(exact) = number.as_u64() {
        return usize::try_from(exact).ok();
    }
    let float = number.as_f64()?;
    if !float.is_finite() || float < 0.0 || float.fract() != 0.0 {
        return None;
    }
    usize::try_from(float as u64).ok()
}

fn whole_number_error(value: &serde_json::Value) -> String {
    format!("expected a whole number, got {value}")
}

/// `#[serde(deserialize_with = "tolerant_usize")]` for a required count.
pub fn tolerant_usize<'de, D>(deserializer: D) -> Result<usize, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    json_usize(&value).ok_or_else(|| serde::de::Error::custom(whole_number_error(&value)))
}

/// `#[serde(default, deserialize_with = "tolerant_opt_usize")]` for an
/// optional count. An explicit `null` and an absent key are both "unset".
pub fn tolerant_opt_usize<'de, D>(deserializer: D) -> Result<Option<usize>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    if value.is_null() {
        return Ok(None);
    }
    json_usize(&value)
        .map(Some)
        .ok_or_else(|| serde::de::Error::custom(whole_number_error(&value)))
}

/// 32-bit variant of [`tolerant_opt_usize`] for LSP position fields, which
/// are `u32` rather than `usize` upstream.
pub fn tolerant_opt_u32<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    if value.is_null() {
        return Ok(None);
    }
    json_usize(&value)
        .and_then(|v| u32::try_from(v).ok())
        .map(Some)
        .ok_or_else(|| serde::de::Error::custom(whole_number_error(&value)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: `max_files: 500.0` used to fail the whole call with
    /// "invalid type: floating point `500.0`, expected usize". The value was
    /// never malformed — a schema reading `"type": "number"` is exactly what
    /// makes a model emit the `.0`.
    #[test]
    fn a_whole_valued_float_is_an_integer() {
        assert_eq!(json_usize(&serde_json::json!(500.0)), Some(500));
        assert_eq!(json_usize(&serde_json::json!(0.0)), Some(0));
        assert_eq!(json_usize(&serde_json::json!(12)), Some(12));
    }

    #[test]
    fn a_fractional_or_negative_count_is_still_rejected() {
        assert_eq!(json_usize(&serde_json::json!(1.5)), None);
        assert_eq!(json_usize(&serde_json::json!(-1.0)), None);
        assert_eq!(json_usize(&serde_json::json!("500")), None);
        assert_eq!(json_usize(&serde_json::Value::Null), None);
    }

    #[test]
    fn the_error_names_the_type_the_caller_sent() {
        #[derive(serde::Deserialize, Debug)]
        struct Input {
            #[serde(deserialize_with = "tolerant_opt_usize")]
            max_files: Option<usize>,
        }
        let error = serde_json::from_value::<Input>(serde_json::json!({ "max_files": 1.5 }))
            .expect_err("a fractional count is a caller bug, not a formatting quirk");
        assert!(
            error.to_string().contains("whole number"),
            "the message must say what was wrong; got {error}"
        );
        assert!(serde_json::from_value::<Input>(serde_json::json!({ "max_files": 500.0 })).is_ok());
        assert!(
            serde_json::from_value::<Input>(serde_json::json!({ "max_files": null }))
                .expect("an explicit null is unset")
                .max_files
                .is_none()
        );
    }

    /// Build a workspace root that is deliberately *not* reachable as a
    /// relative path from the process cwd, so a cwd-resolving
    /// implementation cannot accidentally pass.
    fn fixture_root() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("workspace fixture");
        std::fs::create_dir_all(dir.path().join("src/http")).expect("fixture layout");
        std::fs::write(dir.path().join("src/http/ratelimit.rs"), b"fn main() {}")
            .expect("fixture file");
        dir
    }

    /// The reported defect: a session rooted at one repository asked to read
    /// `src/http/ratelimit.rs`, and the tool looked in the daemon's launch
    /// directory instead. `validate_path` canonicalized the raw relative path,
    /// which resolves against the process cwd — so the read failed while the
    /// file sat at the expected workspace path.
    #[test]
    fn a_relative_path_resolves_against_the_workspace_root_not_the_process_cwd() {
        let root = fixture_root();

        let resolved = validate_path(Path::new("src/http/ratelimit.rs"), root.path())
            .expect("a relative path resolves under the workspace root");

        assert!(
            resolved.starts_with(root.path().canonicalize().expect("canonical root")),
            "resolved path must live under the workspace root; got {}",
            resolved.display()
        );
        assert!(
            resolved.is_file(),
            "and must be the file that was asked for"
        );
    }

    /// A `write` already resolved relative paths under the root
    /// (`validate_target_path`), so the two must agree. Otherwise writing a
    /// relative path and reading it back cannot round-trip.
    #[test]
    fn relative_read_and_write_agree_on_the_same_target() {
        let root = fixture_root();

        let written = validate_target_path(Path::new("src/http/ratelimit.rs"), root.path())
            .expect("write target resolves under the root");
        let read = validate_path(Path::new("src/http/ratelimit.rs"), root.path())
            .expect("read target resolves under the root");

        assert_eq!(
            written, read,
            "one relative path must name one location for both tools"
        );
    }

    /// Containment must survive the join: `..` cannot climb out, and an
    /// absolute path outside the root is still refused.
    #[test]
    fn the_join_does_not_weaken_containment() {
        let root = fixture_root();
        let outside = tempfile::tempdir().expect("outside fixture");
        let outside_file = outside.path().join("secret.txt");
        std::fs::write(&outside_file, b"nope").expect("outside file");

        assert!(
            validate_path(Path::new("../secret.txt"), root.path()).is_err(),
            "a relative traversal out of the root must be refused"
        );
        assert!(
            matches!(
                validate_path(&outside_file, root.path()),
                Err(ToolError::Permission(_))
            ),
            "an absolute path outside the root must stay a permission error"
        );
        assert!(
            validate_path(Path::new("src/http/missing.rs"), root.path()).is_err(),
            "a genuinely missing file is still an error"
        );
    }

    /// The error must name what the caller sent, not the derived absolute
    /// path — otherwise a failure is unactionable for the caller.
    #[test]
    fn a_missing_relative_path_reports_the_relative_path_the_caller_sent() {
        let root = fixture_root();
        let error = validate_path(Path::new("src/http/nope.rs"), root.path())
            .expect_err("the file does not exist");
        assert!(
            error.to_string().contains("src/http/nope.rs"),
            "the message must quote the caller's own path; got {error}"
        );
    }

    /// A `workdir` need not exist yet, so containment has to be decided by the
    /// deepest existing ancestor plus a lexical suffix — the same walk
    /// `validate_target_path` performs.
    #[test]
    fn a_workdir_may_not_exist_yet_but_must_stay_inside_the_workspace() {
        let root = fixture_root();

        let nested = resolve_workdir(Path::new("crates/inner/does-not-exist"), root.path())
            .expect("a not-yet-created workdir inside the workspace is allowed");
        assert!(
            nested.starts_with(root.path().canonicalize().expect("canonical root")),
            "the resolved workdir must stay under the root; got {}",
            nested.display()
        );
    }

    #[test]
    fn a_workdir_traversal_is_refused_as_a_permission_error() {
        let root = fixture_root();
        let error = resolve_workdir(Path::new("../elsewhere"), root.path())
            .expect_err("a traversal out of the workspace must be refused");
        assert!(
            matches!(error, ToolError::Permission(_)),
            "expected a permission error, got {error:?}"
        );
        assert!(
            error.to_string().contains("workdir '"),
            "the message must name a workdir, not a path: {error}"
        );
    }
}
