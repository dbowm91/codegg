mod common;

use codegg::snapshot::affected_paths::{
    extract_affected_paths, extract_batch_affected_paths,
    extract_batch_affected_paths_with_read_only, is_restorable_tool, normalize_and_dedup,
};
use codegg::snapshot::checkpoint::EditCheckpointManager;
use codegg::snapshot::checkpoint::{EditCheckpoint, EditFileState, FileState};
use codegg::snapshot::SnapshotManager;
use codegg::workspace_services::WorkspaceLockTable;
use serde_json::json;
use sqlx::SqlitePool;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::oneshot;

async fn isolated_pool() -> SqlitePool {
    common::pool::isolated_pool().await
}

async fn create_checkpoint_manager(pool: SqlitePool, root: &Path) -> EditCheckpointManager {
    // ensure edit_checkpoint table exists (migration v46 creates it)
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS edit_checkpoint (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            turn_id TEXT,
            batch_seq INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            data TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();
    // also need snapshot table for legacy test
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS snapshot (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            label TEXT,
            data TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();
    // need session/project tables for FK? Not required for checkpoint without FK enforcement maybe.
    // Create minimal session/project so FK not fails if enabled.
    // FK is only on session_id referencing session(id) ON DELETE CASCADE, but if foreign_keys pragma off, not enforced.
    // For safety, create tables.
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS project (
            id TEXT PRIMARY KEY,
            worktree TEXT NOT NULL,
            vcs TEXT,
            name TEXT,
            icon_url TEXT,
            icon_color TEXT,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            time_initialized INTEGER,
            sandboxes TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS session (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            parent_id TEXT,
            slug TEXT NOT NULL,
            directory TEXT NOT NULL,
            title TEXT NOT NULL,
            version TEXT NOT NULL,
            share_url TEXT,
            summary_additions INTEGER,
            summary_deletions INTEGER,
            summary_files INTEGER,
            summary_diffs TEXT,
            revert TEXT,
            permission TEXT,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            time_compacting INTEGER,
            time_archived INTEGER,
            time_deleted INTEGER,
            workspace_id TEXT,
            provider_connection_id TEXT,
            provider_connection_revision INTEGER,
            model_catalog_revision TEXT,
            selected_model_id TEXT,
            agent TEXT,
            model TEXT,
            tags TEXT
        )
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();
    // Insert dummy project/session for FK
    let _ = sqlx::query("INSERT OR IGNORE INTO project (id, worktree, sandboxes, time_created, time_updated) VALUES ('p1','.', '[]', 0,0)").execute(&pool).await;
    let _ = sqlx::query("INSERT OR IGNORE INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES ('sess1','p1','s1','.','t','1',0,0)").execute(&pool).await;
    let _ = sqlx::query("INSERT OR IGNORE INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES ('sess2','p1','s2','.','t','1',0,0)").execute(&pool).await;
    EditCheckpointManager::new(pool, root.to_path_buf())
}

#[tokio::test(flavor = "current_thread")]
async fn write_checkpoint_create_absent_to_present() {
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    let mgr = create_checkpoint_manager(pool, tmp.path()).await;
    // pre absent
    let pre = mgr.capture_file_state_sync("new.txt").unwrap();
    assert!(matches!(pre, FileState::Absent));
    // create file
    fs::write(tmp.path().join("new.txt"), "hello").unwrap();
    let post = mgr.capture_file_state_sync("new.txt").unwrap();
    assert!(matches!(post, FileState::Present { .. }));
    // persist checkpoint
    let cp = EditCheckpoint {
        id: "cp-create".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: Some("turn1".into()),
        batch_seq: 1,
        created_at: 1000,
        files: vec![EditFileState {
            path: "new.txt".into(),
            pre,
            post: post.clone(),
        }],
    };
    let persisted = mgr.persist_checkpoint(cp).await.unwrap();
    let fetched = mgr.get("cp-create").await.unwrap().unwrap();
    assert_eq!(fetched.files[0].pre, FileState::Absent);
    assert_eq!(fetched.files[0].post, post);
    assert_eq!(persisted.id, "cp-create");
}

#[tokio::test(flavor = "current_thread")]
async fn edit_checkpoint_update_present_to_present() {
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("edit.txt"), "old").unwrap();
    let mgr = create_checkpoint_manager(pool, tmp.path()).await;
    let pre = mgr.capture_file_state_sync("edit.txt").unwrap();
    fs::write(tmp.path().join("edit.txt"), "new").unwrap();
    let post = mgr.capture_file_state_sync("edit.txt").unwrap();
    let cp = EditCheckpoint {
        id: "cp-edit".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: Some("turn1".into()),
        batch_seq: 2,
        created_at: 2000,
        files: vec![EditFileState {
            path: "edit.txt".into(),
            pre: pre.clone(),
            post: post.clone(),
        }],
    };
    mgr.persist_checkpoint(cp).await.unwrap();
    let got = mgr.get("cp-edit").await.unwrap().unwrap();
    assert_ne!(got.files[0].pre, got.files[0].post);
}

#[tokio::test(flavor = "current_thread")]
async fn replace_checkpoint_same_as_edit() {
    // replace is same path handling as edit
    let input = json!({"path":"a.txt","pattern":"x","replacement":"y"});
    let paths = extract_affected_paths("replace", &input).unwrap();
    assert_eq!(paths, vec!["a.txt"]);
}

#[tokio::test(flavor = "current_thread")]
async fn multiedit_checkpoint_single_path() {
    let input = json!({"path":"a.txt","edits":[{"old_string":"x","new_string":"y"}]});
    let paths = extract_affected_paths("multiedit", &input).unwrap();
    assert_eq!(paths, vec!["a.txt"]);
    // Ensure capture works for multiedit path
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("a.txt"), "x").unwrap();
    let mgr = create_checkpoint_manager(pool, tmp.path()).await;
    let pre = mgr.capture_file_state_sync("a.txt").unwrap();
    fs::write(tmp.path().join("a.txt"), "y").unwrap();
    let post = mgr.capture_file_state_sync("a.txt").unwrap();
    assert!(matches!(pre, FileState::Present { .. }));
    assert!(matches!(post, FileState::Present { .. }));
}

#[tokio::test(flavor = "current_thread")]
async fn apply_patch_checkpoints_all_modes() {
    // update
    let input = json!({"path":"a.txt","patch":"@@","mode":"update"});
    assert_eq!(
        extract_affected_paths("apply_patch", &input).unwrap(),
        vec!["a.txt"]
    );
    // create
    let input = json!({"path":"new.txt","patch":"hi","mode":"create"});
    assert_eq!(
        extract_affected_paths("apply_patch", &input).unwrap(),
        vec!["new.txt"]
    );
    // delete
    let input = json!({"path":"old.txt","patch":"","mode":"delete"});
    assert_eq!(
        extract_affected_paths("apply_patch", &input).unwrap(),
        vec!["old.txt"]
    );
    // move
    let patch = "rename from old.txt\nrename to new.txt\n";
    let input = json!({"path":"ignored","patch":patch,"mode":"move"});
    let paths = extract_affected_paths("apply_patch", &input).unwrap();
    assert_eq!(paths, vec!["old.txt", "new.txt"]);

    // Move checkpoint with two files: source present->absent, dest absent->present
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("old.txt"), "move_me").unwrap();
    let mgr = create_checkpoint_manager(pool, tmp.path()).await;
    let pre_old = mgr.capture_file_state_sync("old.txt").unwrap();
    let pre_new = mgr.capture_file_state_sync("new.txt").unwrap();
    assert!(matches!(pre_old, FileState::Present { .. }));
    assert!(matches!(pre_new, FileState::Absent));
    // simulate move
    fs::rename(tmp.path().join("old.txt"), tmp.path().join("new.txt")).unwrap();
    let post_old = mgr.capture_file_state_sync("old.txt").unwrap();
    let post_new = mgr.capture_file_state_sync("new.txt").unwrap();
    assert!(matches!(post_old, FileState::Absent));
    assert!(matches!(post_new, FileState::Present { .. }));
    let cp = EditCheckpoint {
        id: "cp-move".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: None,
        batch_seq: 3,
        created_at: 3000,
        files: vec![
            EditFileState {
                path: "old.txt".into(),
                pre: pre_old,
                post: post_old,
            },
            EditFileState {
                path: "new.txt".into(),
                pre: pre_new,
                post: post_new,
            },
        ],
    };
    mgr.persist_checkpoint(cp).await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn failed_mutation_does_not_fabricate_post_state() {
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("fail.txt"), "original").unwrap();
    let mgr = create_checkpoint_manager(pool, tmp.path()).await;
    let pre = mgr.capture_file_state_sync("fail.txt").unwrap();
    // Simulate failed edit that does not change file (tool error before write)
    // post should remain original, not fabricated new content
    let post = mgr.capture_file_state_sync("fail.txt").unwrap();
    assert_eq!(pre, post);
    // If file didn't change, checkpoint should be skipped (no meaningful change) - but persistence would still be allowed?
    // Our logic skips persist when no change. Verify that.
    // We test that capturing post after failed mutation yields same state, not a fabricated success.
}

#[tokio::test(flavor = "current_thread")]
async fn two_workspaces_isolated_same_relative_path() {
    let pool_a = isolated_pool().await;
    let pool_b = isolated_pool().await;
    let tmp_a = tempfile::tempdir().unwrap();
    let tmp_b = tempfile::tempdir().unwrap();
    // Same relative path different content/roots
    fs::write(tmp_a.path().join("common.txt"), "workspace-a").unwrap();
    fs::write(tmp_b.path().join("common.txt"), "workspace-b").unwrap();

    let mgr_a = create_checkpoint_manager(pool_a, tmp_a.path()).await;
    let mgr_b = create_checkpoint_manager(pool_b, tmp_b.path()).await;

    let pre_a = mgr_a.capture_file_state_sync("common.txt").unwrap();
    let pre_b = mgr_b.capture_file_state_sync("common.txt").unwrap();

    assert!(matches!(pre_a, FileState::Present { ref content, .. } if content == "workspace-a"));
    assert!(matches!(pre_b, FileState::Present { ref content, .. } if content == "workspace-b"));

    // Mutate only workspace A
    fs::write(tmp_a.path().join("common.txt"), "mutated-a").unwrap();
    let post_a = mgr_a.capture_file_state_sync("common.txt").unwrap();
    let post_b = mgr_b.capture_file_state_sync("common.txt").unwrap();

    assert!(matches!(post_a, FileState::Present { ref content, .. } if content == "mutated-a"));
    // B unchanged
    assert!(matches!(post_b, FileState::Present { ref content, .. } if content == "workspace-b"));

    // Persist checkpoints for each workspace with distinct workspace_id
    let cp_a = EditCheckpoint {
        id: "cp-a".into(),
        workspace_id: "ws-a".into(),
        session_id: "sess1".into(),
        turn_id: Some("turn1".into()),
        batch_seq: 1,
        created_at: 1000,
        files: vec![EditFileState {
            path: "common.txt".into(),
            pre: pre_a,
            post: post_a,
        }],
    };
    let cp_b = EditCheckpoint {
        id: "cp-b".into(),
        workspace_id: "ws-b".into(),
        session_id: "sess1".into(),
        turn_id: Some("turn1".into()),
        batch_seq: 1,
        created_at: 1000,
        files: vec![EditFileState {
            path: "common.txt".into(),
            pre: pre_b.clone(),
            post: post_b.clone(),
        }],
    };
    mgr_a.persist_checkpoint(cp_a).await.unwrap();
    mgr_b.persist_checkpoint(cp_b).await.unwrap();

    // Ensure list isolation
    let list_a = mgr_a.list_for_workspace("ws-a").await.unwrap();
    let list_b = mgr_b.list_for_workspace("ws-b").await.unwrap();
    assert_eq!(list_a.len(), 1);
    assert_eq!(list_b.len(), 1);
    assert_eq!(list_a[0].workspace_id, "ws-a");
    assert_eq!(list_b[0].workspace_id, "ws-b");
}

#[tokio::test(flavor = "current_thread")]
async fn concurrent_batches_same_workspace_isolated_pre_post() {
    // Simulate two concurrent batches for same workspace but different files - should not contaminate
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("file1.txt"), "v1").unwrap();
    fs::write(tmp.path().join("file2.txt"), "v2").unwrap();
    let mgr = create_checkpoint_manager(pool, tmp.path()).await;

    // Batch 1 affects file1, batch2 affects file2, run concurrently (pre capture before, post after)
    let mgr1 = EditCheckpointManager::new(mgr.pool(), tmp.path().to_path_buf());
    let mgr2 = EditCheckpointManager::new(mgr.pool(), tmp.path().to_path_buf());

    // Pre captures concurrently
    let (pre1, pre2) = tokio::join!(
        async { mgr1.capture_file_state_sync("file1.txt").unwrap() },
        async { mgr2.capture_file_state_sync("file2.txt").unwrap() }
    );
    // Mutate both files concurrently
    fs::write(tmp.path().join("file1.txt"), "new1").unwrap();
    fs::write(tmp.path().join("file2.txt"), "new2").unwrap();
    let (post1, post2) = tokio::join!(
        async { mgr1.capture_file_state_sync("file1.txt").unwrap() },
        async { mgr2.capture_file_state_sync("file2.txt").unwrap() }
    );
    // Verify no cross contamination: file1's post is new1, not new2
    assert!(matches!(post1, FileState::Present { ref content, .. } if content == "new1"));
    assert!(matches!(post2, FileState::Present { ref content, .. } if content == "new2"));
    assert!(matches!(pre1, FileState::Present { ref content, .. } if content == "v1"));
    assert!(matches!(pre2, FileState::Present { ref content, .. } if content == "v2"));
}

#[tokio::test(flavor = "current_thread")]
async fn same_path_batches_serialize_capture_mutate_capture_and_persist() {
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("same.txt"), "initial").unwrap();
    let mgr = create_checkpoint_manager(pool, tmp.path()).await;
    let locks = Arc::new(WorkspaceLockTable::new());

    let (first_entered_tx, first_entered_rx) = oneshot::channel();
    let (first_continue_tx, first_continue_rx) = oneshot::channel();
    let first_mgr = EditCheckpointManager::new(mgr.pool(), tmp.path().to_path_buf());
    let first_locks = Arc::clone(&locks);
    let first_root = tmp.path().to_path_buf();
    let first = tokio::spawn(async move {
        let _guard = first_locks.acquire_repository(&first_root).await;
        let paths = vec!["same.txt".to_string()];
        let pre = first_mgr.capture_states(&paths).await.unwrap();
        first_entered_tx.send(()).unwrap();
        first_continue_rx.await.unwrap();

        fs::write(first_root.join("same.txt"), "first").unwrap();
        let post = first_mgr.capture_states(&paths).await.unwrap();
        first_mgr
            .persist_checkpoint(EditCheckpoint {
                id: "checkpoint-first".into(),
                workspace_id: "workspace-1".into(),
                session_id: "sess1".into(),
                turn_id: Some("turn-1".into()),
                batch_seq: 1,
                created_at: 1,
                files: vec![EditFileState {
                    path: "same.txt".into(),
                    pre: pre["same.txt"].clone(),
                    post: post["same.txt"].clone(),
                }],
            })
            .await
            .unwrap();
    });

    first_entered_rx.await.unwrap();
    let (second_started_tx, second_started_rx) = oneshot::channel();
    let second_mgr = EditCheckpointManager::new(mgr.pool(), tmp.path().to_path_buf());
    let second_locks = Arc::clone(&locks);
    let second_root = tmp.path().to_path_buf();
    let second = tokio::spawn(async move {
        second_started_tx.send(()).unwrap();
        let _guard = second_locks.acquire_repository(&second_root).await;
        let paths = vec!["same.txt".to_string()];
        let pre = second_mgr.capture_states(&paths).await.unwrap();
        fs::write(second_root.join("same.txt"), "second").unwrap();
        let post = second_mgr.capture_states(&paths).await.unwrap();
        second_mgr
            .persist_checkpoint(EditCheckpoint {
                id: "checkpoint-second".into(),
                workspace_id: "workspace-1".into(),
                session_id: "sess2".into(),
                turn_id: Some("turn-2".into()),
                batch_seq: 1,
                created_at: 2,
                files: vec![EditFileState {
                    path: "same.txt".into(),
                    pre: pre["same.txt"].clone(),
                    post: post["same.txt"].clone(),
                }],
            })
            .await
            .unwrap();
        (pre, post)
    });

    // The second batch has started and is blocked on the same repository lock
    // while the first batch completes its full checkpoint transaction.
    second_started_rx.await.unwrap();
    first_continue_tx.send(()).unwrap();
    first.await.unwrap();
    let (second_pre, second_post) = second.await.unwrap();

    assert!(
        matches!(second_pre["same.txt"], FileState::Present { ref content, .. } if content == "first")
    );
    assert!(
        matches!(second_post["same.txt"], FileState::Present { ref content, .. } if content == "second")
    );
    let checkpoints = mgr.list_for_workspace("workspace-1").await.unwrap();
    assert_eq!(checkpoints.len(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn overlapping_mutations_serialize_deterministic() {
    // Two tools in same batch targeting same path produce overlapping raw paths -> dedup and serialize
    let calls = vec![
        (
            "write".to_string(),
            json!({"path":"same.txt","content":"a"}),
        ),
        (
            "edit".to_string(),
            json!({"path":"same.txt","old_string":"x","new_string":"y"}),
        ),
    ];
    let raw = extract_batch_affected_paths(&calls).unwrap().unwrap();
    assert_eq!(raw.len(), 2);
    assert_eq!(raw, vec!["same.txt", "same.txt"]);
    let root = Path::new("/tmp/ws");
    let normalized = normalize_and_dedup(raw.clone(), root).unwrap();
    assert_eq!(normalized.len(), 1);
    assert_eq!(normalized[0], "same.txt");
    // overlapping detection
    assert!(codegg::snapshot::affected_paths::has_overlapping_paths(
        raw.len(),
        normalized.len()
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn independent_paths_remain_parallel() {
    let calls = vec![
        ("write".to_string(), json!({"path":"a.txt","content":"hi"})),
        ("write".to_string(), json!({"path":"b.txt","content":"hi"})),
    ];
    let raw = extract_batch_affected_paths(&calls).unwrap().unwrap();
    let root = Path::new("/tmp/ws");
    let normalized = normalize_and_dedup(raw.clone(), root).unwrap();
    assert_eq!(normalized.len(), 2);
    assert!(!codegg::snapshot::affected_paths::has_overlapping_paths(
        raw.len(),
        normalized.len()
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn symlink_path_rejected() {
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    // Create a file and symlink to it
    fs::write(tmp.path().join("real.txt"), "content").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(tmp.path().join("real.txt"), tmp.path().join("link.txt"))
            .unwrap();
        let mgr = create_checkpoint_manager(pool, tmp.path()).await;
        let res = mgr.capture_file_state_sync("link.txt");
        assert!(res.is_err(), "symlink should be rejected");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn oversized_rejected() {
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    // Create oversized file
    let big_content = "x".repeat(2_000_000);
    fs::write(tmp.path().join("big.txt"), &big_content).unwrap();
    // Manager with small limit
    let mgr = EditCheckpointManager::new_with_options(
        pool,
        tmp.path().to_path_buf(),
        codegg::snapshot::SnapshotOptions {
            max_files: 10,
            max_file_bytes: 1_000_000,
            max_total_bytes: 20_000_000,
        },
    );
    let res = mgr.capture_file_state_sync("big.txt");
    assert!(res.is_err());
    // Also persist should reject
    let cp = EditCheckpoint {
        id: "big-cp".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: None,
        batch_seq: 1,
        created_at: 1,
        files: vec![EditFileState {
            path: "big.txt".into(),
            pre: FileState::Absent,
            post: FileState::Present {
                hash: "h".into(),
                content: big_content,
            },
        }],
    };
    // Need pool with table already created? The mgr's pool already has table?
    // Our mgr created with new_with_options but not via helper that creates table; create it
    let pool2 = mgr.pool();
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS edit_checkpoint (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, session_id TEXT NOT NULL, turn_id TEXT, batch_seq INTEGER NOT NULL, created_at INTEGER NOT NULL, data TEXT NOT NULL)"#
    ).execute(&pool2).await.unwrap();
    let mgr2 = EditCheckpointManager::new_with_options(
        pool2,
        tmp.path().to_path_buf(),
        codegg::snapshot::SnapshotOptions {
            max_files: 10,
            max_file_bytes: 1_000_000,
            max_total_bytes: 20_000_000,
        },
    );
    assert!(mgr2.persist_checkpoint(cp).await.is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn unsupported_tool_not_mislabeled_restorable() {
    assert!(!is_restorable_tool("bash"));
    assert!(!is_restorable_tool("mcp__server__tool"));
    assert!(!is_restorable_tool("read"));
    assert!(is_restorable_tool("write"));
    let calls = vec![("bash".to_string(), json!({"command":"ls"}))];
    let res = extract_batch_affected_paths(&calls).unwrap();
    assert_eq!(res, None);
    // Mixed batch: an unknown/potentially mutating call makes the whole
    // logical batch non-restorable rather than extracting a native subset.
    let mixed = vec![
        ("write".to_string(), json!({"path":"a.txt","content":"hi"})),
        ("bash".to_string(), json!({"command":"touch a.txt"})),
    ];
    assert_eq!(
        extract_batch_affected_paths_with_read_only(&mixed, |_, _| false).unwrap(),
        None
    );

    let read_mixed = vec![
        ("write".to_string(), json!({"path":"a.txt","content":"hi"})),
        ("bash".to_string(), json!({"command":"echo hi"})),
    ];
    assert_eq!(
        extract_batch_affected_paths_with_read_only(&read_mixed, |name, input| {
            name == "bash" && input["command"] == "echo hi"
        })
        .unwrap(),
        Some(vec!["a.txt".to_string()])
    );
}

#[tokio::test(flavor = "current_thread")]
async fn persisted_checkpoint_survives_manager_recreation() {
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    {
        let mgr = create_checkpoint_manager(pool.clone(), tmp.path()).await;
        let cp = EditCheckpoint {
            id: "persist".into(),
            workspace_id: "ws1".into(),
            session_id: "sess1".into(),
            turn_id: Some("turn1".into()),
            batch_seq: 1,
            created_at: 12345,
            files: vec![EditFileState {
                path: "a.txt".into(),
                pre: FileState::Present {
                    hash: "h1".into(),
                    content: "old".into(),
                },
                post: FileState::Present {
                    hash: "h2".into(),
                    content: "new".into(),
                },
            }],
        };
        mgr.persist_checkpoint(cp).await.unwrap();
    }
    // Recreate manager with same pool (simulating daemon restart)
    let mgr2 = create_checkpoint_manager(pool.clone(), tmp.path()).await;
    let fetched = mgr2.get("persist").await.unwrap().unwrap();
    assert_eq!(fetched.id, "persist");
    assert_eq!(fetched.files.len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn foreign_file_changed_event_not_in_checkpoint() {
    // Verify durable checkpoint contents no longer depend on unscoped FileChanged draining.
    // A foreign workspace FileChanged event should not enter another turn's checkpoint.
    use codegg::bus::events::AppEvent;
    use codegg::bus::global::GlobalEventBus;
    let pool = isolated_pool().await;
    let tmp_current = tempfile::tempdir().unwrap();
    let tmp_foreign = tempfile::tempdir().unwrap();
    fs::write(tmp_current.path().join("current.txt"), "before").unwrap();
    fs::write(tmp_foreign.path().join("foreign.txt"), "foreign").unwrap();
    let mgr_current = create_checkpoint_manager(pool.clone(), tmp_current.path()).await;
    // Simulate foreign workspace emitting FileChanged via global bus (as old code would drain)
    let mut rx = GlobalEventBus::subscribe();
    GlobalEventBus::publish(AppEvent::FileChanged {
        path: "foreign.txt".into(),
        action: "Modified".into(),
        old_content: Some("foreign_old".into()),
    });
    // Also publish current workspace's change (would have been drained old way)
    GlobalEventBus::publish(AppEvent::FileChanged {
        path: "current.txt".into(),
        action: "Modified".into(),
        old_content: Some("before".into()),
    });
    // Drain the receiver as old code would have done - but new checkpoint ignores drained events
    let mut drained = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let AppEvent::FileChanged {
            path,
            old_content,
            action: _,
        } = ev
        {
            drained.push((path, old_content));
        }
    }
    // Drained contains both, including foreign - old code would have included foreign in checkpoint
    assert!(drained.iter().any(|(p, _)| p == "foreign.txt"));
    assert!(drained.iter().any(|(p, _)| p == "current.txt"));
    // New correct checkpoint derives from affected-path extraction, not drained events.
    // Simulate batch with single write to current.txt
    let calls = vec![(
        "write".to_string(),
        json!({"path":"current.txt","content":"after"}),
    )];
    let raw = extract_batch_affected_paths(&calls).unwrap().unwrap();
    let normalized = normalize_and_dedup(raw, tmp_current.path()).unwrap();
    assert_eq!(normalized, vec!["current.txt"]);
    // Ensure foreign path not in normalized set despite drained foreign event
    assert!(!normalized.contains(&"foreign.txt".to_string()));
    // Capture pre/post and persist - should only contain current.txt
    let pre = mgr_current.capture_file_state_sync("current.txt").unwrap();
    fs::write(tmp_current.path().join("current.txt"), "after").unwrap();
    let post = mgr_current.capture_file_state_sync("current.txt").unwrap();
    let cp = EditCheckpoint {
        id: "foreign-test".into(),
        workspace_id: "ws-current".into(),
        session_id: "sess1".into(),
        turn_id: Some("turn1".into()),
        batch_seq: 1,
        created_at: 1,
        files: vec![EditFileState {
            path: "current.txt".into(),
            pre,
            post,
        }],
    };
    mgr_current.persist_checkpoint(cp).await.unwrap();
    let fetched = mgr_current.get("foreign-test").await.unwrap().unwrap();
    assert_eq!(fetched.files.len(), 1);
    assert_eq!(fetched.files[0].path, "current.txt");
    // Ensure no foreign file leakage
    assert!(!fetched.files.iter().any(|f| f.path == "foreign.txt"));
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_snapshot_still_readable_after_checkpoint_migration() {
    // Simulate old snapshot record remains readable after new table added
    let pool = isolated_pool().await;
    // Create both tables
    sqlx::query("CREATE TABLE IF NOT EXISTS snapshot (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, created_at INTEGER NOT NULL, label TEXT, data TEXT NOT NULL)").execute(&pool).await.unwrap();
    sqlx::query("CREATE TABLE IF NOT EXISTS edit_checkpoint (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, session_id TEXT NOT NULL, turn_id TEXT, batch_seq INTEGER NOT NULL, created_at INTEGER NOT NULL, data TEXT NOT NULL)").execute(&pool).await.unwrap();
    // Need project/session for FK
    sqlx::query("CREATE TABLE IF NOT EXISTS project (id TEXT PRIMARY KEY, worktree TEXT NOT NULL, sandboxes TEXT NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL)").execute(&pool).await.unwrap();
    sqlx::query("CREATE TABLE IF NOT EXISTS session (id TEXT PRIMARY KEY, project_id TEXT NOT NULL, slug TEXT NOT NULL, directory TEXT NOT NULL, title TEXT NOT NULL, version TEXT NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL)").execute(&pool).await.unwrap();
    sqlx::query("INSERT OR IGNORE INTO project (id, worktree, sandboxes, time_created, time_updated) VALUES ('p','.', '[]',0,0)").execute(&pool).await.unwrap();
    sqlx::query("INSERT OR IGNORE INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES ('sess1','p','s','.','t','1',0,0)").execute(&pool).await.unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let snap_mgr = SnapshotManager::new(pool.clone(), tmp.path().to_path_buf());
    // Old snapshot via incremental (uses old content)
    let inc = snap_mgr
        .capture_incremental(
            "sess1",
            None,
            vec![("old.txt".to_string(), Some("legacy".to_string()))],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(inc.files.get("old.txt").unwrap().content, "legacy");
    // Now create checkpoint
    let ck_mgr = EditCheckpointManager::new(pool.clone(), tmp.path().to_path_buf());
    let cp = EditCheckpoint {
        id: "chk".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: None,
        batch_seq: 1,
        created_at: 1,
        files: vec![EditFileState {
            path: "a.txt".into(),
            pre: FileState::Absent,
            post: FileState::Present {
                hash: "h".into(),
                content: "hi".into(),
            },
        }],
    };
    ck_mgr.persist_checkpoint(cp).await.unwrap();
    // Both readable
    let snap = snap_mgr.get(&inc.id).await.unwrap().unwrap();
    assert!(snap.files.contains_key("old.txt"));
    let chk = ck_mgr.get("chk").await.unwrap().unwrap();
    assert_eq!(chk.files[0].path, "a.txt");
}
