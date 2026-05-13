use std::path::Path;

use codegg::error::ToolError;
use codegg::tool::util::validate_path;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::Builder;

    fn temp_root() -> tempfile::TempDir {
        Builder::new()
            .prefix("codegg-path-validation-")
            .tempdir_in("/private/tmp")
            .unwrap()
    }

    #[test]
    fn test_valid_path_inside_root() {
        let temp_dir = temp_root();
        let root = temp_dir.path();
        let test_file = root.join("test_file.txt");
        std::fs::write(&test_file, "test").unwrap();
        let result = validate_path(&test_file, root);
        assert!(result.is_ok());
    }

    #[test]
    fn test_path_outside_root() {
        let temp_dir = temp_root();
        let other_dir = Path::new("/tmp/../../../etc");
        let result = validate_path(other_dir, temp_dir.path());
        assert!(matches!(result, Err(ToolError::Permission(_))));
    }

    #[test]
    fn test_invalid_path() {
        let result = validate_path(
            Path::new("/nonexistent/path/that/does/not/exist"),
            Path::new("/"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_nested_path_inside_root() {
        let temp_dir = temp_root();
        let nested = temp_dir.path().join("a").join("b").join("c");
        std::fs::create_dir_all(&nested).unwrap();
        let result = validate_path(&nested, temp_dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_symlink_inside_root() {
        let temp_dir = temp_root();
        let root = temp_dir.path();
        let target = root.join("target.txt");
        std::fs::write(&target, "test").unwrap();
        let symlink = root.join("symlink.txt");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &symlink).ok();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&target, &symlink).ok();

        let result = validate_path(&symlink, root);
        assert!(matches!(result, Err(ToolError::Permission(_))));
    }

    #[test]
    fn test_path_equals_root() {
        let temp_dir = temp_root();
        let root = temp_dir.path();
        let result = validate_path(root, root);
        assert!(result.is_ok());
    }
}
