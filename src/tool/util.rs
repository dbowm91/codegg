use std::path::{Path, PathBuf};

use crate::error::ToolError;

pub fn validate_path(path: &Path, allowed_root: &Path) -> Result<PathBuf, ToolError> {
    check_path_for_symlinks(path)?;
    let canonical = canonicalize_path_internal(path)?;
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
    canonicalize_path_internal(path)
}

fn canonicalize_path_internal(path: &Path) -> Result<PathBuf, ToolError> {
    path.canonicalize()
        .map_err(|_| ToolError::Execution(format!("invalid path: {}", path.display())))
}

pub fn check_path_for_symlinks(path: &Path) -> Result<(), ToolError> {
    let mut current = PathBuf::new();

    for component in path.components() {
        current.push(component);

        if current
            .symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(ToolError::Permission(format!(
                "symlink not allowed in path: {}",
                current.display()
            )));
        }
    }

    Ok(())
}
