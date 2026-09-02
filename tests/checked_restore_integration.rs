mod common;

use codegg::snapshot::checked_restore::CheckedRestoreOutcome;
use codegg::snapshot::checkpoint::{
    EditCheckpoint, EditCheckpointManager, EditFileState, FileState,
};
use sqlx::SqlitePool;
use std::fs;
use std::path::Path;

async fn isolated_pool() -> SqlitePool {
    common::pool::isolated_pool().await
}

async fn create_checkpoint_manager(pool: SqlitePool, root: &Path) -> EditCheckpointManager {
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
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS edit_restore_operation (
            id TEXT PRIMARY KEY,
            checkpoint_id TEXT NOT NULL,
            workspace_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            turn_id TEXT,
            direction TEXT NOT NULL CHECK(direction IN ('undo','reapply')),
            result TEXT NOT NULL,
            conflict_paths TEXT NOT NULL DEFAULT '[]',
            applied_paths TEXT NOT NULL DEFAULT '[]',
            failed_paths TEXT NOT NULL DEFAULT '[]',
            error_message TEXT,
            created_at INTEGER NOT NULL
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
            workspace_id TEXT
        )
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();
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
    let _ = sqlx::query("INSERT OR IGNORE INTO project (id, worktree, sandboxes, time_created, time_updated) VALUES ('p1','.', '[]', 0,0)").execute(&pool).await;
    let _ = sqlx::query("INSERT OR IGNORE INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES ('sess1','p1','s1','.','t','1',0,0)").execute(&pool).await;
    let _ = sqlx::query("INSERT OR IGNORE INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES ('sess2','p1','s2','.','t','1',0,0)").execute(&pool).await;
    EditCheckpointManager::new(pool, root.to_path_buf())
}

fn hash_of(content: &str) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(content.as_bytes()))
}

// ---- Focused unit tests ----

#[tokio::test(flavor = "current_thread")]
async fn compare_present_absent_states() {
    let absent = FileState::Absent;
    let present = FileState::Present {
        hash: hash_of("hi"),
        content: "hi".into(),
    };
    assert!(codegg::snapshot::checked_restore::file_states_equal(
        &absent,
        &FileState::Absent
    ));
    assert!(!codegg::snapshot::checked_restore::file_states_equal(
        &absent, &present
    ));
    assert!(codegg::snapshot::checked_restore::file_states_equal(
        &present, &present
    ));
    let present2 = FileState::Present {
        hash: hash_of("bye"),
        content: "bye".into(),
    };
    assert!(!codegg::snapshot::checked_restore::file_states_equal(
        &present, &present2
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn conflict_aggregation_before_mutation_no_partial_write() {
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    let mgr = create_checkpoint_manager(pool, tmp.path()).await;

    // Create two files
    fs::write(tmp.path().join("a.txt"), "v1").unwrap();
    fs::write(tmp.path().join("b.txt"), "w1").unwrap();
    let pre_a = mgr.capture_file_state_sync("a.txt").unwrap();
    let pre_b = mgr.capture_file_state_sync("b.txt").unwrap();
    // mutate both
    fs::write(tmp.path().join("a.txt"), "v2").unwrap();
    fs::write(tmp.path().join("b.txt"), "w2").unwrap();
    let post_a = mgr.capture_file_state_sync("a.txt").unwrap();
    let post_b = mgr.capture_file_state_sync("b.txt").unwrap();

    let cp = EditCheckpoint {
        id: "cp2".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: Some("turn1".into()),
        batch_seq: 1,
        created_at: 1000,
        files: vec![
            EditFileState {
                path: "a.txt".into(),
                pre: pre_a.clone(),
                post: post_a.clone(),
            },
            EditFileState {
                path: "b.txt".into(),
                pre: pre_b.clone(),
                post: post_b.clone(),
            },
        ],
    };
    mgr.persist_checkpoint(cp).await.unwrap();

    // External edit to b.txt makes it stale
    fs::write(tmp.path().join("b.txt"), "external").unwrap();

    // Attempt undo: should conflict and mutate zero files
    // b.txt now != post_b, a.txt == post_a, but one stale should prevent every path
    let outcome = mgr.checked_undo("cp2", "ws1", Some("sess1")).await.unwrap();
    assert!(
        matches!(outcome, CheckedRestoreOutcome::Conflict { .. }),
        "got {:?}",
        outcome
    );
    // Ensure a.txt was not reverted despite being non-stale (all-or-nothing)
    let cur_a = mgr.capture_file_state_sync("a.txt").unwrap();
    assert_eq!(
        cur_a, post_a,
        "a.txt should remain at post state because b stale blocks whole operation"
    );
    let cur_b = mgr.capture_file_state_sync("b.txt").unwrap();
    assert!(matches!(cur_b, FileState::Present { .. }));
    // Ensure b still external
    if let FileState::Present { content, .. } = cur_b {
        assert_eq!(content, "external");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn inverse_mapping_create_update_delete_move() {
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    let mgr = create_checkpoint_manager(pool, tmp.path()).await;

    // create: absent -> present
    let pre_absent = FileState::Absent;
    let post_present = FileState::Present {
        hash: hash_of("hello"),
        content: "hello".into(),
    };
    let cp_create = EditCheckpoint {
        id: "cp-create".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: None,
        batch_seq: 1,
        created_at: 1,
        files: vec![EditFileState {
            path: "new.txt".into(),
            pre: pre_absent.clone(),
            post: post_present.clone(),
        }],
    };
    mgr.persist_checkpoint(cp_create).await.unwrap();
    // Need file at post state to undo
    fs::write(tmp.path().join("new.txt"), "hello").unwrap();
    let outcome = mgr
        .checked_undo("cp-create", "ws1", Some("sess1"))
        .await
        .unwrap();
    assert!(
        matches!(outcome, CheckedRestoreOutcome::Applied { .. }),
        "create undo {:?}",
        outcome
    );
    assert!(
        !tmp.path().join("new.txt").exists(),
        "undo create should delete file"
    );
    // reapply should recreate
    let outcome2 = mgr
        .checked_reapply("cp-create", "ws1", Some("sess1"))
        .await
        .unwrap();
    assert!(
        matches!(outcome2, CheckedRestoreOutcome::Applied { .. }),
        "create reapply {:?}",
        outcome2
    );
    assert!(tmp.path().join("new.txt").exists());
    assert_eq!(
        fs::read_to_string(tmp.path().join("new.txt")).unwrap(),
        "hello"
    );

    // update: present -> present
    fs::write(tmp.path().join("upd.txt"), "old").unwrap();
    let pre = mgr.capture_file_state_sync("upd.txt").unwrap();
    fs::write(tmp.path().join("upd.txt"), "new").unwrap();
    let post = mgr.capture_file_state_sync("upd.txt").unwrap();
    let cp_upd = EditCheckpoint {
        id: "cp-upd".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: None,
        batch_seq: 2,
        created_at: 2,
        files: vec![EditFileState {
            path: "upd.txt".into(),
            pre: pre.clone(),
            post: post.clone(),
        }],
    };
    mgr.persist_checkpoint(cp_upd).await.unwrap();
    let outcome = mgr
        .checked_undo("cp-upd", "ws1", Some("sess1"))
        .await
        .unwrap();
    assert!(matches!(outcome, CheckedRestoreOutcome::Applied { .. }));
    assert_eq!(
        fs::read_to_string(tmp.path().join("upd.txt")).unwrap(),
        "old"
    );
    let outcome = mgr
        .checked_reapply("cp-upd", "ws1", Some("sess1"))
        .await
        .unwrap();
    assert!(matches!(outcome, CheckedRestoreOutcome::Applied { .. }));
    assert_eq!(
        fs::read_to_string(tmp.path().join("upd.txt")).unwrap(),
        "new"
    );

    // delete: present -> absent
    fs::write(tmp.path().join("del.txt"), "to_delete").unwrap();
    let pre_del = mgr.capture_file_state_sync("del.txt").unwrap();
    fs::remove_file(tmp.path().join("del.txt")).unwrap();
    let post_del = mgr.capture_file_state_sync("del.txt").unwrap();
    assert!(matches!(post_del, FileState::Absent));
    let cp_del = EditCheckpoint {
        id: "cp-del".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: None,
        batch_seq: 3,
        created_at: 3,
        files: vec![EditFileState {
            path: "del.txt".into(),
            pre: pre_del.clone(),
            post: post_del.clone(),
        }],
    };
    mgr.persist_checkpoint(cp_del).await.unwrap();
    // Current is absent (post), undo should restore present
    let outcome = mgr
        .checked_undo("cp-del", "ws1", Some("sess1"))
        .await
        .unwrap();
    assert!(
        matches!(outcome, CheckedRestoreOutcome::Applied { .. }),
        "delete undo {:?}",
        outcome
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("del.txt")).unwrap(),
        "to_delete"
    );
    // reapply should delete again
    let outcome = mgr
        .checked_reapply("cp-del", "ws1", Some("sess1"))
        .await
        .unwrap();
    assert!(matches!(outcome, CheckedRestoreOutcome::Applied { .. }));
    assert!(!tmp.path().join("del.txt").exists());

    // move: two files source present->absent, dest absent->present
    fs::write(tmp.path().join("old.txt"), "move_me").unwrap();
    let pre_old = mgr.capture_file_state_sync("old.txt").unwrap();
    let pre_new = mgr.capture_file_state_sync("new2.txt").unwrap();
    assert!(matches!(pre_new, FileState::Absent));
    fs::rename(tmp.path().join("old.txt"), tmp.path().join("new2.txt")).unwrap();
    let post_old = mgr.capture_file_state_sync("old.txt").unwrap();
    let post_new = mgr.capture_file_state_sync("new2.txt").unwrap();
    let cp_move = EditCheckpoint {
        id: "cp-move".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: None,
        batch_seq: 4,
        created_at: 4,
        files: vec![
            EditFileState {
                path: "old.txt".into(),
                pre: pre_old.clone(),
                post: post_old.clone(),
            },
            EditFileState {
                path: "new2.txt".into(),
                pre: pre_new.clone(),
                post: post_new.clone(),
            },
        ],
    };
    mgr.persist_checkpoint(cp_move).await.unwrap();
    let outcome = mgr
        .checked_undo("cp-move", "ws1", Some("sess1"))
        .await
        .unwrap();
    assert!(
        matches!(outcome, CheckedRestoreOutcome::Applied { .. }),
        "move undo {:?}",
        outcome
    );
    assert!(tmp.path().join("old.txt").exists());
    assert!(!tmp.path().join("new2.txt").exists());
    let outcome = mgr
        .checked_reapply("cp-move", "ws1", Some("sess1"))
        .await
        .unwrap();
    assert!(matches!(outcome, CheckedRestoreOutcome::Applied { .. }));
    assert!(!tmp.path().join("old.txt").exists());
    assert!(tmp.path().join("new2.txt").exists());
}

#[tokio::test(flavor = "current_thread")]
async fn wrong_workspace_rejected() {
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    let mgr = create_checkpoint_manager(pool, tmp.path()).await;
    fs::write(tmp.path().join("a.txt"), "x").unwrap();
    let pre = mgr.capture_file_state_sync("a.txt").unwrap();
    fs::write(tmp.path().join("a.txt"), "y").unwrap();
    let post = mgr.capture_file_state_sync("a.txt").unwrap();
    let cp = EditCheckpoint {
        id: "cp-ws".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: None,
        batch_seq: 1,
        created_at: 1,
        files: vec![EditFileState {
            path: "a.txt".into(),
            pre,
            post,
        }],
    };
    mgr.persist_checkpoint(cp).await.unwrap();
    let outcome = mgr
        .checked_undo("cp-ws", "ws2", Some("sess1"))
        .await
        .unwrap();
    assert!(
        matches!(outcome, CheckedRestoreOutcome::WrongWorkspace { .. }),
        "got {:?}",
        outcome
    );
    // Ensure file not mutated
    assert_eq!(fs::read_to_string(tmp.path().join("a.txt")).unwrap(), "y");
}

#[tokio::test(flavor = "current_thread")]
async fn wrong_session_rejected() {
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    let mgr = create_checkpoint_manager(pool, tmp.path()).await;
    fs::write(tmp.path().join("a.txt"), "x").unwrap();
    let pre = mgr.capture_file_state_sync("a.txt").unwrap();
    fs::write(tmp.path().join("a.txt"), "y").unwrap();
    let post = mgr.capture_file_state_sync("a.txt").unwrap();
    let cp = EditCheckpoint {
        id: "cp-sess".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: None,
        batch_seq: 1,
        created_at: 1,
        files: vec![EditFileState {
            path: "a.txt".into(),
            pre,
            post,
        }],
    };
    mgr.persist_checkpoint(cp).await.unwrap();
    let outcome = mgr
        .checked_undo("cp-sess", "ws1", Some("sess2"))
        .await
        .unwrap();
    assert!(
        matches!(outcome, CheckedRestoreOutcome::WrongSession { .. }),
        "got {:?}",
        outcome
    );
}

#[tokio::test(flavor = "current_thread")]
async fn duplicate_idempotent_requests() {
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    let mgr = create_checkpoint_manager(pool, tmp.path()).await;
    fs::write(tmp.path().join("a.txt"), "old").unwrap();
    let pre = mgr.capture_file_state_sync("a.txt").unwrap();
    fs::write(tmp.path().join("a.txt"), "new").unwrap();
    let post = mgr.capture_file_state_sync("a.txt").unwrap();
    let cp = EditCheckpoint {
        id: "cp-dup".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: None,
        batch_seq: 1,
        created_at: 1,
        files: vec![EditFileState {
            path: "a.txt".into(),
            pre,
            post,
        }],
    };
    mgr.persist_checkpoint(cp).await.unwrap();
    // First undo succeeds
    let out1 = mgr
        .checked_undo("cp-dup", "ws1", Some("sess1"))
        .await
        .unwrap();
    assert!(matches!(out1, CheckedRestoreOutcome::Applied { .. }));
    assert_eq!(fs::read_to_string(tmp.path().join("a.txt")).unwrap(), "old");
    // Second undo should be conflict (already undone, current is pre not post)
    let out2 = mgr
        .checked_undo("cp-dup", "ws1", Some("sess1"))
        .await
        .unwrap();
    assert!(
        matches!(out2, CheckedRestoreOutcome::Conflict { .. }),
        "second undo should conflict, got {:?}",
        out2
    );
    // Ensure still old (no double apply)
    assert_eq!(fs::read_to_string(tmp.path().join("a.txt")).unwrap(), "old");
    // Reapply should succeed
    let out3 = mgr
        .checked_reapply("cp-dup", "ws1", Some("sess1"))
        .await
        .unwrap();
    assert!(matches!(out3, CheckedRestoreOutcome::Applied { .. }));
    assert_eq!(fs::read_to_string(tmp.path().join("a.txt")).unwrap(), "new");
    // Second reapply should conflict
    let out4 = mgr
        .checked_reapply("cp-dup", "ws1", Some("sess1"))
        .await
        .unwrap();
    assert!(matches!(out4, CheckedRestoreOutcome::Conflict { .. }));
}

// ---- Integration tests ----

#[tokio::test(flavor = "current_thread")]
async fn undo_and_reapply_single_file() {
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    let mgr = create_checkpoint_manager(pool, tmp.path()).await;
    fs::write(tmp.path().join("single.txt"), "before").unwrap();
    let pre = mgr.capture_file_state_sync("single.txt").unwrap();
    fs::write(tmp.path().join("single.txt"), "after").unwrap();
    let post = mgr.capture_file_state_sync("single.txt").unwrap();
    let cp = EditCheckpoint {
        id: "single".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: None,
        batch_seq: 1,
        created_at: 1,
        files: vec![EditFileState {
            path: "single.txt".into(),
            pre,
            post,
        }],
    };
    mgr.persist_checkpoint(cp).await.unwrap();
    let out = mgr
        .checked_undo("single", "ws1", Some("sess1"))
        .await
        .unwrap();
    assert!(matches!(out, CheckedRestoreOutcome::Applied { .. }));
    assert_eq!(
        fs::read_to_string(tmp.path().join("single.txt")).unwrap(),
        "before"
    );
    let out2 = mgr
        .checked_reapply("single", "ws1", Some("sess1"))
        .await
        .unwrap();
    assert!(matches!(out2, CheckedRestoreOutcome::Applied { .. }));
    assert_eq!(
        fs::read_to_string(tmp.path().join("single.txt")).unwrap(),
        "after"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn multi_file_move_batch() {
    // Already covered in inverse_mapping test, but explicit multi-file batch
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    let mgr = create_checkpoint_manager(pool, tmp.path()).await;
    fs::write(tmp.path().join("a.txt"), "a1").unwrap();
    fs::write(tmp.path().join("b.txt"), "b1").unwrap();
    let pre_a = mgr.capture_file_state_sync("a.txt").unwrap();
    let pre_b = mgr.capture_file_state_sync("b.txt").unwrap();
    fs::write(tmp.path().join("a.txt"), "a2").unwrap();
    fs::write(tmp.path().join("b.txt"), "b2").unwrap();
    let post_a = mgr.capture_file_state_sync("a.txt").unwrap();
    let post_b = mgr.capture_file_state_sync("b.txt").unwrap();
    let cp = EditCheckpoint {
        id: "multi".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: None,
        batch_seq: 1,
        created_at: 1,
        files: vec![
            EditFileState {
                path: "a.txt".into(),
                pre: pre_a,
                post: post_a,
            },
            EditFileState {
                path: "b.txt".into(),
                pre: pre_b,
                post: post_b,
            },
        ],
    };
    mgr.persist_checkpoint(cp).await.unwrap();
    let out = mgr
        .checked_undo("multi", "ws1", Some("sess1"))
        .await
        .unwrap();
    assert!(matches!(out, CheckedRestoreOutcome::Applied { .. }));
    assert_eq!(fs::read_to_string(tmp.path().join("a.txt")).unwrap(), "a1");
    assert_eq!(fs::read_to_string(tmp.path().join("b.txt")).unwrap(), "b1");
    let out2 = mgr
        .checked_reapply("multi", "ws1", Some("sess1"))
        .await
        .unwrap();
    assert!(matches!(out2, CheckedRestoreOutcome::Applied { .. }));
    assert_eq!(fs::read_to_string(tmp.path().join("a.txt")).unwrap(), "a2");
    assert_eq!(fs::read_to_string(tmp.path().join("b.txt")).unwrap(), "b2");
}

#[tokio::test(flavor = "current_thread")]
async fn human_external_edit_blocks_undo() {
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    let mgr = create_checkpoint_manager(pool, tmp.path()).await;
    fs::write(tmp.path().join("file.txt"), "original").unwrap();
    let pre = mgr.capture_file_state_sync("file.txt").unwrap();
    fs::write(tmp.path().join("file.txt"), "tool_edit").unwrap();
    let post = mgr.capture_file_state_sync("file.txt").unwrap();
    let cp = EditCheckpoint {
        id: "human".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: None,
        batch_seq: 1,
        created_at: 1,
        files: vec![EditFileState {
            path: "file.txt".into(),
            pre,
            post,
        }],
    };
    mgr.persist_checkpoint(cp).await.unwrap();
    // Human edits after tool completion
    fs::write(tmp.path().join("file.txt"), "human_edit").unwrap();
    let out = mgr
        .checked_undo("human", "ws1", Some("sess1"))
        .await
        .unwrap();
    assert!(matches!(out, CheckedRestoreOutcome::Conflict { .. }));
    // Ensure human edit not overwritten
    assert_eq!(
        fs::read_to_string(tmp.path().join("file.txt")).unwrap(),
        "human_edit"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn stale_one_of_many_prevents_all() {
    // Already tested in conflict_aggregation, duplicate here for coverage
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    let mgr = create_checkpoint_manager(pool, tmp.path()).await;
    for name in ["x.txt", "y.txt", "z.txt"] {
        fs::write(tmp.path().join(name), "before").unwrap();
    }
    let mut files = vec![];
    for name in ["x.txt", "y.txt", "z.txt"] {
        let pre = mgr.capture_file_state_sync(name).unwrap();
        fs::write(tmp.path().join(name), "after").unwrap();
        let post = mgr.capture_file_state_sync(name).unwrap();
        files.push(EditFileState {
            path: name.into(),
            pre,
            post,
        });
    }
    let cp = EditCheckpoint {
        id: "many".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: None,
        batch_seq: 1,
        created_at: 1,
        files,
    };
    mgr.persist_checkpoint(cp).await.unwrap();
    // Stale only y
    fs::write(tmp.path().join("y.txt"), "stale").unwrap();
    let out = mgr
        .checked_undo("many", "ws1", Some("sess1"))
        .await
        .unwrap();
    assert!(matches!(out, CheckedRestoreOutcome::Conflict { .. }));
    // x and z should still be after
    assert_eq!(
        fs::read_to_string(tmp.path().join("x.txt")).unwrap(),
        "after"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("z.txt")).unwrap(),
        "after"
    );
}

// Restart and recovery

#[tokio::test(flavor = "current_thread")]
async fn successful_undo_then_restart_still_permits_reapply() {
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    // Create checkpoint and undo
    {
        let mgr = create_checkpoint_manager(pool.clone(), tmp.path()).await;
        fs::write(tmp.path().join("r.txt"), "pre").unwrap();
        let pre = mgr.capture_file_state_sync("r.txt").unwrap();
        fs::write(tmp.path().join("r.txt"), "post").unwrap();
        let post = mgr.capture_file_state_sync("r.txt").unwrap();
        let cp = EditCheckpoint {
            id: "restart".into(),
            workspace_id: "ws1".into(),
            session_id: "sess1".into(),
            turn_id: None,
            batch_seq: 1,
            created_at: 1,
            files: vec![EditFileState {
                path: "r.txt".into(),
                pre,
                post,
            }],
        };
        mgr.persist_checkpoint(cp).await.unwrap();
        let out = mgr
            .checked_undo("restart", "ws1", Some("sess1"))
            .await
            .unwrap();
        assert!(matches!(out, CheckedRestoreOutcome::Applied { .. }));
        assert_eq!(fs::read_to_string(tmp.path().join("r.txt")).unwrap(), "pre");
    }
    // Recreate manager from same pool (simulating daemon restart)
    {
        let mgr2 = create_checkpoint_manager(pool.clone(), tmp.path()).await;
        // Should still see checkpoint and be able to reapply
        let out = mgr2
            .checked_reapply("restart", "ws1", Some("sess1"))
            .await
            .unwrap();
        assert!(
            matches!(out, CheckedRestoreOutcome::Applied { .. }),
            "reapply after restart {:?}",
            out
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join("r.txt")).unwrap(),
            "post"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn partial_degraded_does_not_expose_normal_reapply() {
    // We can't easily force a partial I/O failure without mocking filesystem,
    // but we can verify that a conflict (which is zero mutation) does not
    // create a successful undo log that would allow reapply.
    // Ensure latest_successful_undo is None after conflict.
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    let mgr = create_checkpoint_manager(pool, tmp.path()).await;
    fs::write(tmp.path().join("p.txt"), "a").unwrap();
    let pre = mgr.capture_file_state_sync("p.txt").unwrap();
    fs::write(tmp.path().join("p.txt"), "b").unwrap();
    let post = mgr.capture_file_state_sync("p.txt").unwrap();
    let cp = EditCheckpoint {
        id: "partial".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: None,
        batch_seq: 1,
        created_at: 1,
        files: vec![EditFileState {
            path: "p.txt".into(),
            pre,
            post,
        }],
    };
    mgr.persist_checkpoint(cp).await.unwrap();
    // Make stale so undo conflicts
    fs::write(tmp.path().join("p.txt"), "external").unwrap();
    let out = mgr
        .checked_undo("partial", "ws1", Some("sess1"))
        .await
        .unwrap();
    assert!(matches!(out, CheckedRestoreOutcome::Conflict { .. }));
    // latest successful undo should be None, so reapply_latest should be NotFound
    let reapply = mgr
        .reapply_latest_undone_for_session("sess1", "ws1")
        .await
        .unwrap();
    assert!(
        matches!(reapply, CheckedRestoreOutcome::NotFound { .. }),
        "got {:?}",
        reapply
    );
}

// Security

#[tokio::test(flavor = "current_thread")]
async fn checkpoint_path_traversal_rejected() {
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    let mgr = create_checkpoint_manager(pool, tmp.path()).await;
    // Manually persist a checkpoint with traversal path (bypass persist validation? but persist should reject)
    let cp = EditCheckpoint {
        id: "traversal".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: None,
        batch_seq: 1,
        created_at: 1,
        files: vec![EditFileState {
            path: "../evil.txt".into(),
            pre: FileState::Absent,
            post: FileState::Present {
                hash: hash_of("evil"),
                content: "evil".into(),
            },
        }],
    };
    // Persist should reject, but if it somehow exists, checked restore should reject
    let persist_res = mgr.persist_checkpoint(cp).await;
    assert!(persist_res.is_err(), "persist should reject traversal");

    // Try to create checkpoint via DB directly with traversal and then undo should fail validation
    // Insert raw row bypassing validation
    let data = serde_json::to_string(&vec![EditFileState {
        path: "../evil.txt".into(),
        pre: FileState::Absent,
        post: FileState::Present {
            hash: hash_of("evil"),
            content: "evil".into(),
        },
    }])
    .unwrap();
    sqlx::query("INSERT INTO edit_checkpoint (id, workspace_id, session_id, turn_id, batch_seq, created_at, data) VALUES (?, ?, ?, ?, ?, ?, ?)")
        .bind("traversal2")
        .bind("ws1")
        .bind("sess1")
        .bind(Option::<String>::None)
        .bind(2)
        .bind(1000)
        .bind(&data)
        .execute(&mgr.pool())
        .await
        .unwrap();
    // Ensure current file doesn't exist
    let out = mgr
        .checked_undo("traversal2", "ws1", Some("sess1"))
        .await
        .unwrap();
    assert!(
        matches!(out, CheckedRestoreOutcome::PathValidationFailed { .. }),
        "got {:?}",
        out
    );
    // Ensure no file escaped
    assert!(!tmp.path().join("../evil.txt").exists());
    // Also ensure within workdir not created
    assert!(!tmp.path().join("evil.txt").exists());
}

#[tokio::test(flavor = "current_thread")]
#[cfg(unix)]
async fn symlink_rejected() {
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    let mgr = create_checkpoint_manager(pool, tmp.path()).await;
    fs::write(tmp.path().join("real.txt"), "content").unwrap();
    std::os::unix::fs::symlink(tmp.path().join("real.txt"), tmp.path().join("link.txt")).unwrap();
    // Capture should reject symlink
    let res = mgr.capture_file_state_sync("link.txt");
    assert!(res.is_err(), "symlink capture should be rejected");

    // Now create a valid checkpoint, then replace file with symlink before undo
    fs::remove_file(tmp.path().join("link.txt")).unwrap();
    fs::write(tmp.path().join("link.txt"), "orig").unwrap();
    let pre = mgr.capture_file_state_sync("link.txt").unwrap();
    fs::write(tmp.path().join("link.txt"), "modified").unwrap();
    let post = mgr.capture_file_state_sync("link.txt").unwrap();
    let cp = EditCheckpoint {
        id: "sym".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: None,
        batch_seq: 1,
        created_at: 1,
        files: vec![EditFileState {
            path: "link.txt".into(),
            pre,
            post,
        }],
    };
    mgr.persist_checkpoint(cp).await.unwrap();
    // Replace with symlink
    fs::remove_file(tmp.path().join("link.txt")).unwrap();
    std::os::unix::fs::symlink(tmp.path().join("real.txt"), tmp.path().join("link.txt")).unwrap();
    let out = mgr.checked_undo("sym", "ws1", Some("sess1")).await.unwrap();
    assert!(
        matches!(out, CheckedRestoreOutcome::PathValidationFailed { .. }),
        "symlink should cause path validation failure, got {:?}",
        out
    );
    // Ensure no overwrite
    assert!(fs::symlink_metadata(tmp.path().join("link.txt"))
        .unwrap()
        .file_type()
        .is_symlink());
}

#[tokio::test(flavor = "current_thread")]
async fn no_file_bodies_in_conflict_output() {
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    let mgr = create_checkpoint_manager(pool, tmp.path()).await;
    let secret_content = "super_secret_password_123";
    fs::write(tmp.path().join("sec.txt"), "old").unwrap();
    let pre = mgr.capture_file_state_sync("sec.txt").unwrap();
    fs::write(tmp.path().join("sec.txt"), secret_content).unwrap();
    let post = mgr.capture_file_state_sync("sec.txt").unwrap();
    let cp = EditCheckpoint {
        id: "secret".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: None,
        batch_seq: 1,
        created_at: 1,
        files: vec![EditFileState {
            path: "sec.txt".into(),
            pre,
            post,
        }],
    };
    mgr.persist_checkpoint(cp).await.unwrap();
    // Make stale
    fs::write(tmp.path().join("sec.txt"), "different").unwrap();
    let out = mgr
        .checked_undo("secret", "ws1", Some("sess1"))
        .await
        .unwrap();
    let json = serde_json::to_string(&out).unwrap();
    // Ensure secret not leaked in JSON (conflict should only contain paths)
    assert!(
        !json.contains(secret_content),
        "conflict output should not contain file bodies"
    );
    assert!(json.contains("sec.txt"), "should contain path");
}

// Workspace isolation already tested via wrong_workspace, but also test isolation of files

#[tokio::test(flavor = "current_thread")]
async fn workspace_isolation_files() {
    let pool = isolated_pool().await;
    let tmp_a = tempfile::tempdir().unwrap();
    let tmp_b = tempfile::tempdir().unwrap();
    // Use same pool but different workspace roots
    let mgr_a = create_checkpoint_manager(pool.clone(), tmp_a.path()).await;
    let mgr_b = create_checkpoint_manager(pool.clone(), tmp_b.path()).await;
    fs::write(tmp_a.path().join("common.txt"), "a_before").unwrap();
    fs::write(tmp_b.path().join("common.txt"), "b_before").unwrap();
    let pre_a = mgr_a.capture_file_state_sync("common.txt").unwrap();
    let pre_b = mgr_b.capture_file_state_sync("common.txt").unwrap();
    fs::write(tmp_a.path().join("common.txt"), "a_after").unwrap();
    fs::write(tmp_b.path().join("common.txt"), "b_after").unwrap();
    let post_a = mgr_a.capture_file_state_sync("common.txt").unwrap();
    let post_b = mgr_b.capture_file_state_sync("common.txt").unwrap();
    let cp_a = EditCheckpoint {
        id: "ws-a".into(),
        workspace_id: "ws-a".into(),
        session_id: "sess1".into(),
        turn_id: None,
        batch_seq: 1,
        created_at: 1,
        files: vec![EditFileState {
            path: "common.txt".into(),
            pre: pre_a,
            post: post_a,
        }],
    };
    let cp_b = EditCheckpoint {
        id: "ws-b".into(),
        workspace_id: "ws-b".into(),
        session_id: "sess1".into(),
        turn_id: None,
        batch_seq: 1,
        created_at: 1,
        files: vec![EditFileState {
            path: "common.txt".into(),
            pre: pre_b,
            post: post_b,
        }],
    };
    mgr_a.persist_checkpoint(cp_a).await.unwrap();
    mgr_b.persist_checkpoint(cp_b).await.unwrap();
    // Undo A should not affect B
    let out_a = mgr_a
        .checked_undo("ws-a", "ws-a", Some("sess1"))
        .await
        .unwrap();
    assert!(matches!(out_a, CheckedRestoreOutcome::Applied { .. }));
    assert_eq!(
        fs::read_to_string(tmp_a.path().join("common.txt")).unwrap(),
        "a_before"
    );
    assert_eq!(
        fs::read_to_string(tmp_b.path().join("common.txt")).unwrap(),
        "b_after"
    );
    // Cross-workspace attempt should be rejected
    let out_cross = mgr_a
        .checked_undo("ws-b", "ws-a", Some("sess1"))
        .await
        .unwrap();
    assert!(matches!(
        out_cross,
        CheckedRestoreOutcome::WrongWorkspace { .. }
    ));
}

// Ensure undo/reapply lineage survives manager recreation

#[tokio::test(flavor = "current_thread")]
async fn reapply_lineage_via_latest_undone() {
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    let mgr = create_checkpoint_manager(pool.clone(), tmp.path()).await;
    fs::write(tmp.path().join("line.txt"), "v1").unwrap();
    let pre = mgr.capture_file_state_sync("line.txt").unwrap();
    fs::write(tmp.path().join("line.txt"), "v2").unwrap();
    let post = mgr.capture_file_state_sync("line.txt").unwrap();
    let cp = EditCheckpoint {
        id: "line".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: None,
        batch_seq: 1,
        created_at: 1,
        files: vec![EditFileState {
            path: "line.txt".into(),
            pre,
            post,
        }],
    };
    mgr.persist_checkpoint(cp).await.unwrap();
    let out = mgr
        .checked_undo("line", "ws1", Some("sess1"))
        .await
        .unwrap();
    assert!(matches!(out, CheckedRestoreOutcome::Applied { .. }));
    // Now latest undone should be findable
    let latest = mgr
        .latest_successful_undo_for_session("sess1")
        .await
        .unwrap();
    assert!(latest.is_some());
    assert_eq!(latest.unwrap().checkpoint_id, "line");
    // Recreate manager
    let mgr2 = create_checkpoint_manager(pool.clone(), tmp.path()).await;
    let out2 = mgr2
        .reapply_latest_undone_for_session("sess1", "ws1")
        .await
        .unwrap();
    assert!(
        matches!(out2, CheckedRestoreOutcome::Applied { .. }),
        "got {:?}",
        out2
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("line.txt")).unwrap(),
        "v2"
    );
}

// Concurrency: two undos same checkpoint, only one should apply

#[tokio::test(flavor = "current_thread")]
async fn concurrent_undo_serialization() {
    let pool = isolated_pool().await;
    let tmp = tempfile::tempdir().unwrap();
    let mgr = create_checkpoint_manager(pool.clone(), tmp.path()).await;
    fs::write(tmp.path().join("conc.txt"), "pre").unwrap();
    let pre = mgr.capture_file_state_sync("conc.txt").unwrap();
    fs::write(tmp.path().join("conc.txt"), "post").unwrap();
    let post = mgr.capture_file_state_sync("conc.txt").unwrap();
    let cp = EditCheckpoint {
        id: "conc".into(),
        workspace_id: "ws1".into(),
        session_id: "sess1".into(),
        turn_id: None,
        batch_seq: 1,
        created_at: 1,
        files: vec![EditFileState {
            path: "conc.txt".into(),
            pre,
            post,
        }],
    };
    mgr.persist_checkpoint(cp).await.unwrap();

    // Simulate two concurrent undos (without lock, race could allow both to think they are at post)
    // With our compare-before-mutate, second will see current != post after first applies, so conflict.
    // Run sequentially to emulate serialization: first succeeds, second conflicts.
    let mgr1 = create_checkpoint_manager(pool.clone(), tmp.path()).await;
    let mgr2 = create_checkpoint_manager(pool.clone(), tmp.path()).await;
    let out1 = mgr1
        .checked_undo("conc", "ws1", Some("sess1"))
        .await
        .unwrap();
    let out2 = mgr2
        .checked_undo("conc", "ws1", Some("sess1"))
        .await
        .unwrap();
    // One should be applied, one conflict (sequential emulation)
    let applied_count = [&out1, &out2]
        .iter()
        .filter(|o| matches!(**o, CheckedRestoreOutcome::Applied { .. }))
        .count();
    assert_eq!(
        applied_count, 1,
        "exactly one should be applied, got {:?} and {:?}",
        out1, out2
    );
    // Verify file is at pre (not double toggled)
    let cur = fs::read_to_string(tmp.path().join("conc.txt")).unwrap();
    assert_eq!(cur, "pre");
}
