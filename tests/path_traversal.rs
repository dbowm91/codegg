#[cfg(feature = "server")]
mod tests {
    use std::fs;
    use tempfile::tempdir;

    fn setup_test_dir() -> (tempfile::TempDir, String) {
        let dir = tempdir().unwrap();
        let root = dir.path().to_str().unwrap().to_string();

        // Create test directory structure
        fs::create_dir_all(dir.path().join("src/nested/deep")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(dir.path().join("src/nested/deep/file.txt"), "test").unwrap();

        (dir, root)
    }

    #[test]
    fn test_sanitize_path_normal_file() {
        let (_dir, root) = setup_test_dir();
        let requested = "src/main.rs";
        let result = codegg::server::routes::file::sanitize_path(&root, requested);
        assert!(result.is_ok());
    }

    #[test]
    fn test_sanitize_path_nested_file() {
        let (_dir, root) = setup_test_dir();
        let requested = "src/nested/deep/file.txt";
        let result = codegg::server::routes::file::sanitize_path(&root, requested);
        assert!(result.is_ok());
    }

    #[test]
    fn test_sanitize_path_blocked_traversal() {
        let (_dir, root) = setup_test_dir();
        let requested = "../../../etc/passwd";
        let result = codegg::server::routes::file::sanitize_path(&root, requested);
        assert!(result.is_err());
    }

    #[test]
    fn test_sanitize_path_partial_traversal() {
        let (_dir, root) = setup_test_dir();
        let requested = "../secret.txt";
        let result = codegg::server::routes::file::sanitize_path(&root, requested);
        assert!(result.is_err());
    }

    #[test]
    fn test_sanitize_path_traversal_with_file() {
        let (_dir, root) = setup_test_dir();
        let requested = "src/../../root.txt";
        let result = codegg::server::routes::file::sanitize_path(&root, requested);
        assert!(result.is_err());
    }

    #[test]
    fn test_sanitize_path_absolute_path_rejected() {
        let (_dir, root) = setup_test_dir();
        let requested = "/etc/passwd";
        let result = codegg::server::routes::file::sanitize_path(&root, requested);
        assert!(result.is_err());
    }

    #[test]
    fn test_sanitize_path_empty_request() {
        let (_dir, root) = setup_test_dir();
        let requested = "";
        let result = codegg::server::routes::file::sanitize_path(&root, requested);
        assert!(result.is_ok());
    }
}
