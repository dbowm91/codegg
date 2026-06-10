mod common;

use codegg::snapshot::{FileSnapshot, SnapshotManager, SnapshotView};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::fs;

async fn create_test_pool() -> SqlitePool {
    common::pool::isolated_pool().await
}

#[tokio::test]
async fn test_snapshot_capture_empty_dir() {
    let (mut manager, pool) = create_test_manager_with_pool().await;

    // Create project and session
    sqlx::query("INSERT INTO project (id, worktree, sandboxes, time_created, time_updated) VALUES (?, ?, ?, ?, ?)")
        .bind("test-project")
        .bind(".")
        .bind("[]")
        .bind(0)
        .bind(0)
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
        .bind("test-session")
        .bind("test-project")
        .bind("test-session")
        .bind(".")
        .bind("Test Session")
        .bind("1")
        .bind(0)
        .bind(0)
        .execute(&pool)
        .await
        .unwrap();

    let result = manager
        .capture("test-session", Some("test-label".to_string()))
        .await;
    assert!(result.is_ok(), "Capture failed: {:?}", result.err());

    let snapshot = result.unwrap();
    assert_eq!(snapshot.session_id, "test-session");
    assert_eq!(snapshot.label, Some("test-label".to_string()));
    assert!(!snapshot.id.is_empty());
}

async fn create_test_manager_with_pool() -> (SnapshotManager, SqlitePool) {
    let pool = create_test_pool().await;
    // Run migrations
    codegg::session::schema::migrate(&pool)
        .await
        .expect("failed to run migrations");

    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    (
        SnapshotManager::new(pool.clone(), temp_dir.path().to_path_buf()),
        pool,
    )
}

async fn insert_test_project_and_session(pool: &SqlitePool) {
    sqlx::query("INSERT INTO project (id, worktree, sandboxes, time_created, time_updated) VALUES (?, ?, ?, ?, ?)")
        .bind("test-project")
        .bind(".")
        .bind("[]")
        .bind(0)
        .bind(0)
        .execute(pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
        .bind("test-session")
        .bind("test-project")
        .bind("test-session")
        .bind(".")
        .bind("Test Session")
        .bind("1")
        .bind(0)
        .bind(0)
        .execute(pool)
        .await
        .unwrap();
}

#[test]
fn test_file_snapshot_creation() {
    let snapshot = FileSnapshot {
        path: "test/path.txt".to_string(),
        content: "file content".to_string(),
        hash: "abc123".to_string(),
        timestamp: 1234567890,
    };

    assert_eq!(snapshot.path, "test/path.txt");
    assert_eq!(snapshot.content, "file content");
    assert_eq!(snapshot.hash, "abc123");
}

#[test]
fn test_snapshot_view_creation() {
    let mut files = HashMap::new();
    files.insert(
        "test.txt".to_string(),
        FileSnapshot {
            path: "test.txt".to_string(),
            content: "content".to_string(),
            hash: "hash".to_string(),
            timestamp: 100,
        },
    );

    let snapshot = SnapshotView {
        id: "id".to_string(),
        session_id: "session".to_string(),
        files,
        created_at: 200,
        label: Some("label".to_string()),
    };

    assert_eq!(snapshot.id, "id");
    assert_eq!(snapshot.files.len(), 1);
}

#[tokio::test]
async fn test_capture_incremental_uses_old_content() {
    let pool = create_test_pool().await;
    codegg::session::schema::migrate(&pool)
        .await
        .expect("failed to run migrations");
    insert_test_project_and_session(&pool).await;

    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let manager = SnapshotManager::new(pool.clone(), temp_dir.path().to_path_buf());

    let result = manager
        .capture_incremental(
            "test-session",
            Some("incremental-pre-change".to_string()),
            vec![
                ("src/main.rs".to_string(), Some("old-main".to_string())),
                ("new/file.txt".to_string(), None),
            ],
        )
        .await
        .unwrap()
        .expect("expected incremental snapshot");

    assert_eq!(result.files.len(), 1);
    assert_eq!(result.label, Some("incremental-pre-change".to_string()));
    let old = result.files.get("src/main.rs").expect("missing file");
    assert_eq!(old.content, "old-main");
}

#[tokio::test]
async fn test_capture_skips_binary_and_large_files() {
    let pool = create_test_pool().await;
    codegg::session::schema::migrate(&pool)
        .await
        .expect("failed to run migrations");
    insert_test_project_and_session(&pool).await;

    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let root = temp_dir.path();
    fs::create_dir_all(root.join("src")).unwrap();

    fs::write(root.join("src").join("ok.txt"), "hello world").unwrap();
    fs::write(root.join("src").join("bin.dat"), [0_u8, 159, 146, 150]).unwrap();
    fs::write(root.join("src").join("big.txt"), vec![b'a'; 1_000_001]).unwrap();

    let mut manager = SnapshotManager::new(pool, root.to_path_buf());
    let snapshot = manager
        .capture("test-session", Some("full".to_string()))
        .await
        .unwrap();

    assert!(snapshot.files.contains_key("src/ok.txt"));
    assert!(!snapshot.files.contains_key("src/bin.dat"));
    assert!(!snapshot.files.contains_key("src/big.txt"));
}

#[tokio::test]
async fn test_capture_file_count_limit() {
    let pool = create_test_pool().await;
    codegg::session::schema::migrate(&pool)
        .await
        .expect("failed to run migrations");
    insert_test_project_and_session(&pool).await;

    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let root = temp_dir.path();
    fs::create_dir_all(root.join("many")).unwrap();

    for i in 0..5_100 {
        fs::write(root.join("many").join(format!("f{i}.txt")), "x").unwrap();
    }

    let mut manager = SnapshotManager::new(pool, root.to_path_buf());
    let snapshot = manager
        .capture("test-session", Some("full".to_string()))
        .await
        .unwrap();

    assert!(snapshot.files.len() <= 5_000);
}
