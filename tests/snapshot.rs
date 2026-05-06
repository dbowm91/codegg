use codegg::snapshot::{FileSnapshot, Snapshot, SnapshotManager};
use std::collections::HashMap;

fn create_test_manager() -> SnapshotManager {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    SnapshotManager::new(temp_dir.path().to_path_buf())
}

#[test]
fn test_snapshot_manager_new() {
    let manager = create_test_manager();
    assert_eq!(manager.list_for_session("any-session").len(), 0);
}

#[test]
fn test_snapshot_manager_get_nonexistent() {
    let manager = create_test_manager();
    assert!(manager.get("nonexistent-id").is_none());
}

#[test]
fn test_snapshot_manager_latest_none() {
    let manager = create_test_manager();
    assert!(manager.latest("any-session").is_none());
}

#[tokio::test]
async fn test_snapshot_capture_empty_dir() {
    let mut manager = create_test_manager();
    let result = manager.capture("test-session", Some("test-label".to_string())).await;
    assert!(result.is_ok());

    let snapshot = result.unwrap();
    assert_eq!(snapshot.session_id, "test-session");
    assert_eq!(snapshot.label, Some("test-label".to_string()));
    assert!(snapshot.id.len() > 0);
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
fn test_snapshot_creation() {
    let mut files = HashMap::new();
    files.insert(
        "test.txt".to_string(),
        FileSnapshot {
            path: "test.txt".to_string(),
            content: "content".to_string(),
            hash: "hash".to_string(),
            timestamp: 1000,
        },
    );

    let snapshot = Snapshot {
        id: "snapshot-id".to_string(),
        session_id: "session-id".to_string(),
        files,
        created_at: 1000,
        label: Some("label".to_string()),
    };

    assert_eq!(snapshot.id, "snapshot-id");
    assert_eq!(snapshot.session_id, "session-id");
    assert_eq!(snapshot.files.len(), 1);
}
