use std::path::{Path, PathBuf};

use crate::error::ToolError;

/// Well-known macOS aliases for system directories that are exposed
/// as symlinks under `/private/*`. We allow these so that paths
/// under `/var/...`, `/tmp/...`, and `/etc/...` continue to work on
/// macOS, where the public prefix is a symlink to the real
/// directory under `/private/`.
const MACOS_SYSTEM_ALIASES: &[&str] = &["/var", "/tmp", "/etc"];

pub fn validate_path(path: &Path, allowed_root: &Path) -> Result<PathBuf, ToolError> {
    check_path_for_symlinks(path)?;
    let canonical = path
        .canonicalize()
        .map_err(|_| ToolError::Execution(format!("invalid path: {}", path.display())))?;
    let root_canonical = allowed_root
        .canonicalize()
        .map_err(|_| ToolError::Execution("invalid allowed root".to_string()))?;

    if !canonical.starts_with(&root_canonical) {
        return Err(ToolError::Permission(format!(
            "path '{}' is outside allowed directory",
            path.display()
        )));
    }

    Ok(canonical)
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
