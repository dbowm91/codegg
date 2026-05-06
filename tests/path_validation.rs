use std::path::Path;

use codegg::error::ToolError;
use codegg::tool::util::validate_path;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_path_inside_root() {
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test_file.txt");
        std::fs::write(&test_file, "test").unwrap();
        let result = validate_path(&test_file, &temp_dir);
        assert!(result.is_ok());
        std::fs::remove_file(&test_file).ok();
    }

    #[test]
    fn test_path_outside_root() {
        let temp_dir = std::env::temp_dir();
        let other_dir = Path::new("/tmp/../../../etc");
        let result = validate_path(other_dir, &temp_dir);
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
        let temp_dir = std::env::temp_dir();
        let nested = temp_dir.join("a").join("b").join("c");
        std::fs::create_dir_all(&nested).unwrap();
        let result = validate_path(&nested, &temp_dir);
        assert!(result.is_ok());
        std::fs::remove_dir_all(&temp_dir.join("a")).ok();
    }

    #[test]
    fn test_symlink_inside_root() {
        let temp_dir = std::env::temp_dir();
        let target = temp_dir.join("target.txt");
        std::fs::write(&target, "test").unwrap();
        let symlink = temp_dir.join("symlink.txt");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &symlink).ok();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&target, &symlink).ok();

        let result = validate_path(&symlink, &temp_dir);
        assert!(result.is_ok());

        std::fs::remove_file(&target).ok();
        std::fs::remove_file(&symlink).ok();
    }

    #[test]
    fn test_path_equals_root() {
        let temp_dir = std::env::temp_dir();
        let result = validate_path(&temp_dir, &temp_dir);
        assert!(result.is_ok());
    }
}
