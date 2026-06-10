use codegg::worktree::{create_worktree, find_git_root, list_worktrees, remove_worktree, Worktree};
use std::path::Path;
use std::process::Command;

fn git(args: &[&str], dir: &Path) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .expect("failed to run git");
    assert!(status.success(), "git command failed: git {:?}", args);
}

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

    let result = find_git_root(temp_dir.path());
    assert!(result.is_some());
    assert_eq!(result.unwrap(), temp_dir.path().to_path_buf());
}

#[test]
fn test_find_git_root_with_git_file() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let git_file = temp_dir.path().join(".git");
    std::fs::write(&git_file, "gitdir: /tmp/fake-gitdir\n").expect("failed to create .git file");

    let result = find_git_root(temp_dir.path());
    assert!(result.is_some());
    assert_eq!(result.unwrap(), temp_dir.path().to_path_buf());
}

#[tokio::test]
async fn test_list_worktrees_non_git_dir() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let result = list_worktrees(temp_dir.path()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_list_worktrees_parses_current_and_detached() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let repo_dir = temp_dir.path().join("repo");
    let wt_dir = temp_dir.path().join("wt1");
    std::fs::create_dir_all(&repo_dir).expect("failed to create repo dir");

    git(&["init"], &repo_dir);
    git(&["config", "user.name", "Test User"], &repo_dir);
    git(&["config", "user.email", "test@example.com"], &repo_dir);
    std::fs::write(repo_dir.join("README.md"), "hello\n").expect("failed to write README");
    git(&["add", "README.md"], &repo_dir);
    git(&["commit", "-m", "initial"], &repo_dir);

    let wt_dir_str = wt_dir.to_string_lossy().to_string();
    git(
        &["worktree", "add", "-b", "feature/test", &wt_dir_str],
        &repo_dir,
    );
    git(&["checkout", "HEAD^0"], &wt_dir);

    let trees = list_worktrees(&repo_dir)
        .await
        .expect("list_worktrees failed");
    assert_eq!(trees.len(), 2);

    let main_tree = trees
        .iter()
        .find(|t| t.is_current)
        .expect("main worktree not found");
    assert!(main_tree.is_current);
    assert!(!main_tree.is_detached);
    assert!(!main_tree.branch.is_empty());

    let detached_tree = trees
        .iter()
        .find(|t| t.path.ends_with("/wt1"))
        .expect("secondary worktree not found");
    assert!(!detached_tree.is_current);
    assert!(detached_tree.is_detached);
    assert!(detached_tree.branch.starts_with("detached@"));
}

#[tokio::test]
async fn test_create_and_remove_worktree() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let repo_dir = temp_dir.path().join("repo");
    let wt_dir = temp_dir.path().join("wt-create-remove");
    std::fs::create_dir_all(&repo_dir).expect("failed to create repo dir");

    git(&["init"], &repo_dir);
    git(&["config", "user.name", "Test User"], &repo_dir);
    git(&["config", "user.email", "test@example.com"], &repo_dir);
    std::fs::write(repo_dir.join("README.md"), "hello\n").expect("failed to write README");
    git(&["add", "README.md"], &repo_dir);
    git(&["commit", "-m", "initial"], &repo_dir);

    create_worktree(&repo_dir, &wt_dir, "feature/create-remove", true)
        .expect("create_worktree failed");

    let trees_after_create = list_worktrees(&repo_dir)
        .await
        .expect("list_worktrees after create failed");
    assert!(
        trees_after_create
            .iter()
            .any(|t| t.path.ends_with("/wt-create-remove")),
        "created worktree not found in list"
    );

    remove_worktree(&repo_dir, &wt_dir, false).expect("remove_worktree failed");
    let trees_after_remove = list_worktrees(&repo_dir)
        .await
        .expect("list_worktrees after remove failed");
    assert!(
        !trees_after_remove
            .iter()
            .any(|t| t.path.ends_with("/wt-create-remove")),
        "removed worktree still present in list"
    );
}

#[test]
fn test_is_git_worktree_with_git_dir() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let git_dir = temp_dir.path().join(".git");
    std::fs::create_dir_all(&git_dir).expect("failed to create .git dir");

    let result = codegg::worktree::is_git_worktree(temp_dir.path());
    assert!(
        !result,
        "regular .git directory should not be detected as worktree"
    );
}

#[test]
fn test_is_git_worktree_with_git_file() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let git_file = temp_dir.path().join(".git");
    std::fs::write(&git_file, "gitdir: /tmp/fake-gitdir\n").expect("failed to create .git file");

    let result = codegg::worktree::is_git_worktree(temp_dir.path());
    assert!(
        result,
        ".git file with gitdir: prefix should be detected as worktree"
    );
}

#[test]
fn test_is_git_worktree_non_git_dir() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");

    let result = codegg::worktree::is_git_worktree(temp_dir.path());
    assert!(
        !result,
        "non-git directory should not be detected as worktree"
    );
}

#[test]
fn test_is_git_file_with_gitdir_prefix() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let git_file = temp_dir.path().join(".git");
    std::fs::write(&git_file, "gitdir: /tmp/fake-gitdir\n").expect("failed to create .git file");

    let result = codegg::worktree::is_git_file(&git_file);
    assert!(result, "file with gitdir: prefix should return true");
}

#[test]
fn test_is_git_file_without_gitdir_prefix() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let git_file = temp_dir.path().join(".git");
    std::fs::write(&git_file, "just some content\n").expect("failed to create .git file");

    let result = codegg::worktree::is_git_file(&git_file);
    assert!(!result, "file without gitdir: prefix should return false");
}

#[cfg(unix)]
#[tokio::test]
async fn test_list_worktrees_symlink_detection() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let repo_dir = temp_dir.path().join("repo");
    let symlink_dir = temp_dir.path().join("repo-link");
    std::fs::create_dir_all(&repo_dir).expect("failed to create repo dir");

    git(&["init"], &repo_dir);
    git(&["config", "user.name", "Test User"], &repo_dir);
    git(&["config", "user.email", "test@example.com"], &repo_dir);
    std::fs::write(repo_dir.join("README.md"), "hello\n").expect("failed to write README");
    git(&["add", "README.md"], &repo_dir);
    git(&["commit", "-m", "initial"], &repo_dir);

    // Create symlink to repo
    #[cfg(unix)]
    std::os::unix::fs::symlink(&repo_dir, &symlink_dir).expect("failed to create symlink");

    // List worktrees from symlinked path - should still detect current worktree
    let trees = list_worktrees(&symlink_dir)
        .await
        .expect("list_worktrees failed");
    assert_eq!(trees.len(), 1);

    let main_tree = &trees[0];
    assert!(
        main_tree.is_current,
        "worktree accessed via symlink should be detected as current"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_list_worktrees_symlink_worktree_path() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let repo_dir = temp_dir.path().join("repo");
    let wt_dir = temp_dir.path().join("wt-real");
    let wt_symlink = temp_dir.path().join("wt-link");
    std::fs::create_dir_all(&repo_dir).expect("failed to create repo dir");

    git(&["init"], &repo_dir);
    git(&["config", "user.name", "Test User"], &repo_dir);
    git(&["config", "user.email", "test@example.com"], &repo_dir);
    std::fs::write(repo_dir.join("README.md"), "hello\n").expect("failed to write README");
    git(&["add", "README.md"], &repo_dir);
    git(&["commit", "-m", "initial"], &repo_dir);

    // Create worktree
    let wt_dir_str = wt_dir.to_string_lossy().to_string();
    git(
        &["worktree", "add", "-b", "feature/test", &wt_dir_str],
        &repo_dir,
    );

    // Create symlink to worktree
    #[cfg(unix)]
    std::os::unix::fs::symlink(&wt_dir, &wt_symlink).expect("failed to create symlink");

    // Create another worktree that uses the symlink as its path
    let wt2_dir = temp_dir.path().join("wt2");
    let wt2_dir_str = wt2_dir.to_string_lossy().to_string();
    git(
        &["worktree", "add", "-b", "feature/two", &wt2_dir_str],
        &repo_dir,
    );

    // List worktrees and verify detection works with symlinks
    let trees = list_worktrees(&repo_dir)
        .await
        .expect("list_worktrees failed");
    assert!(trees.len() >= 2, "should have at least 2 worktrees");

    // The main worktree should be detected as current
    let main_tree = trees
        .iter()
        .find(|t| t.is_current)
        .expect("no current worktree found");
    assert!(!main_tree.is_detached);
}
