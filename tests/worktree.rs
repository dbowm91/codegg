use codegg::worktree::{find_git_root, list_worktrees, Worktree};

#[test]
fn test_worktree_struct() {
    let worktree = Worktree {
        path: "/path/to/worktree".to_string(),
        branch: "main".to_string(),
        is_current: true,
        is_detached: false,
    };

    assert_eq!(worktree.path, "/path/to/worktree");
    assert_eq!(worktree.branch, "main");
    assert!(worktree.is_current);
    assert!(!worktree.is_detached);
}

#[test]
fn test_worktree_detached() {
    let worktree = Worktree {
        path: "/path/to/worktree".to_string(),
        branch: "detached@/path/to/worktree".to_string(),
        is_current: false,
        is_detached: true,
    };

    assert!(worktree.is_detached);
    assert!(worktree.branch.starts_with("detached@"));
}

#[test]
fn test_find_git_root_with_git_dir() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let git_dir = temp_dir.path().join(".git");
    std::fs::create_dir_all(&git_dir).expect("failed to create .git dir");

    let result = find_git_root(&temp_dir.path().to_path_buf());
    assert!(result.is_some());
    assert_eq!(result.unwrap(), temp_dir.path().to_path_buf());
}

#[test]
fn test_list_worktrees_non_git_dir() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let result = list_worktrees(temp_dir.path());
    assert!(result.is_err());
}
