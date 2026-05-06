use codegg::session::{
    CreateSession, MessageStore, PartStore, SessionStore, TodoItemInput, TodoStore, UpdateSession,
};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

async fn create_test_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS session (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            workspace_id TEXT,
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
            tags TEXT,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            time_compacting INTEGER,
            time_archived INTEGER,
            time_deleted INTEGER
        )
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS message (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            data TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS part (
            id TEXT PRIMARY KEY,
            message_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            data TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS todo (
            session_id TEXT NOT NULL,
            content TEXT NOT NULL,
            status TEXT NOT NULL,
            priority TEXT NOT NULL,
            position INTEGER NOT NULL,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            PRIMARY KEY (session_id, position)
        )
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS permission (
            project_id TEXT PRIMARY KEY,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            data TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS session_share (
            session_id TEXT PRIMARY KEY,
            id TEXT NOT NULL,
            secret TEXT NOT NULL,
            url TEXT NOT NULL,
            share_expires_at INTEGER,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    pool
}

#[tokio::test]
async fn test_session_create() {
    let pool = create_test_pool().await;
    let store = SessionStore::new(pool);

    let session = store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp/test".to_string(),
            title: Some("Test Session".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    assert_eq!(session.project_id, "proj_1");
    assert_eq!(session.directory, "/tmp/test");
    assert_eq!(session.title, "Test Session");
    assert_eq!(session.slug, "test-session");
    assert_eq!(session.version, "1");
    assert!(session.time_archived.is_none());
}

#[tokio::test]
async fn test_session_create_untitled() {
    let pool = create_test_pool().await;
    let store = SessionStore::new(pool);

    let session = store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: None,
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    assert_eq!(session.title, "Untitled");
    assert_eq!(session.slug, "untitled");
}

#[tokio::test]
async fn test_session_get() {
    let pool = create_test_pool().await;
    let store = SessionStore::new(pool);

    let created = store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Get Test".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    let fetched = store.get(&created.id).await.unwrap().unwrap();
    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.title, "Get Test");
}

#[tokio::test]
async fn test_session_get_nonexistent() {
    let pool = create_test_pool().await;
    let store = SessionStore::new(pool);

    let result = store.get("nonexistent_id").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_session_list() {
    let pool = create_test_pool().await;
    let store = SessionStore::new(pool);

    store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Session 1".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Session 2".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    let sessions = store.list("proj_1", 10).await.unwrap();
    assert_eq!(sessions.len(), 2);
}

#[tokio::test]
async fn test_session_list_filters_archived() {
    let pool = create_test_pool().await;
    let store = SessionStore::new(pool);

    let s1 = store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Active".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Archived".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    store.archive(&s1.id).await.unwrap();

    let sessions = store.list("proj_1", 10).await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].title, "Archived");
}

#[tokio::test]
async fn test_session_list_limit() {
    let pool = create_test_pool().await;
    let store = SessionStore::new(pool);

    for i in 0..5 {
        store
            .create(CreateSession {
                agent: None,
                model: None,
                tags: None,
                project_id: "proj_1".to_string(),
                directory: "/tmp".to_string(),
                title: Some(format!("Session {i}")),
                parent_id: None,
                workspace_id: None,
            })
            .await
            .unwrap();
    }

    let sessions = store.list("proj_1", 2).await.unwrap();
    assert_eq!(sessions.len(), 2);
}

#[tokio::test]
async fn test_session_update() {
    let pool = create_test_pool().await;
    let store = SessionStore::new(pool);

    let session = store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Original".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    let updated = store
        .update(
            &session.id,
            UpdateSession {
                tags: None,
                title: Some("Updated Title".to_string()),
                share_url: None,
                summary_additions: None,
                summary_deletions: None,
                summary_files: None,
                summary_diffs: None,
                revert: None,
                permission: None,
                time_compacting: None,
                time_archived: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(updated.title, "Updated Title");
}

#[tokio::test]
async fn test_session_update_partial() {
    let pool = create_test_pool().await;
    let store = SessionStore::new(pool);

    let session = store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Original".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    let updated = store
        .update(
            &session.id,
            UpdateSession {
                tags: None,
                title: None,
                share_url: Some("https://share.example.com/123".to_string()),
                summary_additions: None,
                summary_deletions: None,
                summary_files: None,
                summary_diffs: None,
                revert: None,
                permission: None,
                time_compacting: None,
                time_archived: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(updated.title, "Original");
    assert_eq!(
        updated.share_url,
        Some("https://share.example.com/123".to_string())
    );
}

#[tokio::test]
async fn test_session_delete() {
    let pool = create_test_pool().await;
    let store = SessionStore::new(pool);

    let session = store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("To Delete".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    store.delete(&session.id).await.unwrap();

    let result = store.get(&session.id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_session_fork() {
    let pool = create_test_pool().await;
    let store = SessionStore::new(pool);

    let parent = store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp/project".to_string(),
            title: Some("Parent".to_string()),
            parent_id: None,
            workspace_id: Some("ws_1".to_string()),
        })
        .await
        .unwrap();

    let fork = store.fork(&parent.id).await.unwrap();

    assert_eq!(fork.project_id, parent.project_id);
    assert_eq!(fork.directory, parent.directory);
    assert_eq!(fork.title, "Parent (fork)");
    assert_eq!(fork.parent_id, Some(parent.id.clone()));
    assert_eq!(fork.workspace_id, parent.workspace_id);
}

#[tokio::test]
async fn test_session_archive() {
    let pool = create_test_pool().await;
    let store = SessionStore::new(pool);

    let session = store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("To Archive".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    let archived = store.archive(&session.id).await.unwrap();
    assert!(archived.time_archived.is_some());

    let sessions = store.list("proj_1", 10).await.unwrap();
    assert!(sessions.is_empty());
}

#[tokio::test]
async fn test_session_unarchive() {
    let pool = create_test_pool().await;
    let store = SessionStore::new(pool);

    let session = store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Unarchive Test".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    store.archive(&session.id).await.unwrap();
    let unarchived = store.unarchive(&session.id).await.unwrap();
    assert!(unarchived.time_archived.is_none());

    let sessions = store.list("proj_1", 10).await.unwrap();
    assert_eq!(sessions.len(), 1);
}

#[tokio::test]
async fn test_session_set_share_url() {
    let pool = create_test_pool().await;
    let store = SessionStore::new(pool);

    let session = store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Share Test".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    let shared = store
        .set_share_url(&session.id, "https://share.example.com/abc")
        .await
        .unwrap();

    assert_eq!(
        shared.share_url,
        Some("https://share.example.com/abc".to_string())
    );
}

#[tokio::test]
async fn test_session_children() {
    let pool = create_test_pool().await;
    let store = SessionStore::new(pool);

    let parent = store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Parent".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Child 1".to_string()),
            parent_id: Some(parent.id.clone()),
            workspace_id: None,
        })
        .await
        .unwrap();

    store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Child 2".to_string()),
            parent_id: Some(parent.id.clone()),
            workspace_id: None,
        })
        .await
        .unwrap();

    let children = store.children(&parent.id).await.unwrap();
    assert_eq!(children.len(), 2);
}

#[tokio::test]
async fn test_session_share_session() {
    let pool = create_test_pool().await;
    let store = SessionStore::new(pool);

    let session = store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Share".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    let shared = store.share_session(&session.id).await.unwrap();
    assert!(shared.share_url.is_some());
    let share_url = shared.share_url.unwrap();
    assert!(
        share_url.starts_with("codegg://share/"),
        "share_url should start with 'codegg://share/' but was: {}",
        share_url
    );
}

#[tokio::test]
async fn test_message_create_and_list() {
    let pool = create_test_pool().await;
    let session_store = SessionStore::new(pool.clone());
    let message_store = MessageStore::new(pool);

    let session = session_store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Message Test".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    let msg_data = serde_json::json!({
        "id": "msg_1",
        "sessionID": session.id,
        "messageID": "msg_1",
        "parts": [
            {
                "id": "part_1",
                "sessionID": session.id,
                "messageID": "msg_1",
                "type": "text",
                "text": "Hello, world!"
            }
        ]
    });

    let _msg = message_store
        .create(&session.id, msg_data.clone())
        .await
        .unwrap();

    let messages = message_store.list(&session.id).await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].data.parts.len(), 1);
}

#[tokio::test]
async fn test_message_get() {
    let pool = create_test_pool().await;
    let session_store = SessionStore::new(pool.clone());
    let message_store = MessageStore::new(pool);

    let session = session_store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Get Message Test".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    let msg_data = serde_json::json!({
        "id": "msg_1",
        "sessionID": session.id,
        "messageID": "msg_1",
        "parts": []
    });

    let created = message_store.create(&session.id, msg_data).await.unwrap();

    let fetched = message_store
        .get(&session.id, &created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.id, created.id);
}

#[tokio::test]
async fn test_message_delete() {
    let pool = create_test_pool().await;
    let session_store = SessionStore::new(pool.clone());
    let message_store = MessageStore::new(pool);

    let session = session_store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Delete Message Test".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    let msg_data = serde_json::json!({
        "id": "msg_1",
        "sessionID": session.id,
        "messageID": "msg_1",
        "parts": []
    });

    let msg = message_store.create(&session.id, msg_data).await.unwrap();

    message_store.delete(&session.id, &msg.id).await.unwrap();

    let messages = message_store.list(&session.id).await.unwrap();
    assert!(messages.is_empty());
}

#[tokio::test]
async fn test_part_create_and_list() {
    let pool = create_test_pool().await;
    let session_store = SessionStore::new(pool.clone());
    let message_store = MessageStore::new(pool.clone());
    let part_store = PartStore::new(pool);

    let session = session_store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Part Test".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    let msg_data = serde_json::json!({
        "id": "msg_1",
        "sessionID": session.id,
        "messageID": "msg_1",
        "parts": []
    });

    let msg = message_store.create(&session.id, msg_data).await.unwrap();

    let part_data = serde_json::json!({
        "type": "text",
        "text": "Part content"
    });

    let part = part_store
        .create(&msg.id, &session.id, part_data)
        .await
        .unwrap();

    let parts = part_store.list_by_message(&msg.id).await.unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].id, part.id);
}

#[tokio::test]
async fn test_todo_set_and_list() {
    let pool = create_test_pool().await;
    let session_store = SessionStore::new(pool.clone());
    let todo_store = TodoStore::new(pool);

    let session = session_store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Todo Test".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    let items = vec![
        TodoItemInput {
            content: "Task 1".to_string(),
            status: "pending".to_string(),
            priority: "high".to_string(),
        },
        TodoItemInput {
            content: "Task 2".to_string(),
            status: "completed".to_string(),
            priority: "low".to_string(),
        },
    ];

    let todos = todo_store.set(&session.id, items).await.unwrap();
    assert_eq!(todos.len(), 2);
    assert_eq!(todos[0].content, "Task 1");
    assert_eq!(todos[1].content, "Task 2");
}

#[tokio::test]
async fn test_todo_add() {
    let pool = create_test_pool().await;
    let session_store = SessionStore::new(pool.clone());
    let todo_store = TodoStore::new(pool);

    let session = session_store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Todo Add Test".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    let item = TodoItemInput {
        content: "New task".to_string(),
        status: "pending".to_string(),
        priority: "medium".to_string(),
    };

    let todo = todo_store.add(&session.id, item).await.unwrap();
    assert_eq!(todo.content, "New task");
}

#[tokio::test]
async fn test_todo_clear() {
    let pool = create_test_pool().await;
    let session_store = SessionStore::new(pool.clone());
    let todo_store = TodoStore::new(pool);

    let session = session_store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Todo Clear Test".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    let items = vec![TodoItemInput {
        content: "Task".to_string(),
        status: "pending".to_string(),
        priority: "medium".to_string(),
    }];

    todo_store.set(&session.id, items).await.unwrap();
    todo_store.clear(&session.id).await.unwrap();

    let todos = todo_store.list(&session.id).await.unwrap();
    assert!(todos.is_empty());
}

#[tokio::test]
async fn test_revert_to_message() {
    let pool = create_test_pool().await;
    let session_store = SessionStore::new(pool.clone());
    let message_store = MessageStore::new(pool);

    let session = session_store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Revert Test".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    let msg1_data = serde_json::json!({
        "id": "msg_1",
        "sessionID": session.id,
        "messageID": "msg_1",
        "parts": [{
            "id": "part_1",
            "sessionID": session.id,
            "messageID": "msg_1",
            "type": "text",
            "text": "First message"
        }]
    });
    message_store.create(&session.id, msg1_data).await.unwrap();

    let msg2_data = serde_json::json!({
        "id": "msg_2",
        "sessionID": session.id,
        "messageID": "msg_2",
        "parts": [{
            "id": "part_2",
            "sessionID": session.id,
            "messageID": "msg_2",
            "type": "text",
            "text": "Second message"
        }]
    });
    message_store.create(&session.id, msg2_data).await.unwrap();

    let msg3_data = serde_json::json!({
        "id": "msg_3",
        "sessionID": session.id,
        "messageID": "msg_3",
        "parts": [{
            "id": "part_3",
            "sessionID": session.id,
            "messageID": "msg_3",
            "type": "text",
            "text": "Third message"
        }]
    });
    message_store.create(&session.id, msg3_data).await.unwrap();

    let messages_before = message_store.list(&session.id).await.unwrap();
    assert_eq!(messages_before.len(), 3);

    let _reverted = session_store
        .revert_to_message(&session.id, &messages_before[1].id)
        .await
        .unwrap();

    let messages_after = message_store.list(&session.id).await.unwrap();
    assert_eq!(messages_after.len(), 2);

    let _unreverted = session_store.unrevert_session(&session.id).await.unwrap();
    let messages_unreverted = message_store.list(&session.id).await.unwrap();
    assert_eq!(messages_unreverted.len(), 3);
}

#[tokio::test]
async fn test_revert_to_message_not_found() {
    let pool = create_test_pool().await;
    let session_store = SessionStore::new(pool.clone());
    let message_store = MessageStore::new(pool);

    let session = session_store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Revert Not Found Test".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    let msg_data = serde_json::json!({
        "id": "msg_1",
        "sessionID": session.id,
        "messageID": "msg_1",
        "parts": [{
            "id": "part_1",
            "sessionID": session.id,
            "messageID": "msg_1",
            "type": "text",
            "text": "First message"
        }]
    });
    message_store.create(&session.id, msg_data).await.unwrap();

    let result = session_store
        .revert_to_message(&session.id, "nonexistent")
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_import_session() {
    let pool = create_test_pool().await;
    let session_store = SessionStore::new(pool.clone());
    let message_store = MessageStore::new(pool);

    let original_session = session_store
        .create(CreateSession {
            agent: None,
            model: None,
            tags: None,
            project_id: "proj_1".to_string(),
            directory: "/tmp".to_string(),
            title: Some("Original Session".to_string()),
            parent_id: None,
            workspace_id: None,
        })
        .await
        .unwrap();

    let msg_data = serde_json::json!({
        "id": "msg_1",
        "sessionID": original_session.id,
        "messageID": "msg_1",
        "parts": [{
            "id": "part_1",
            "sessionID": original_session.id,
            "messageID": "msg_1",
            "type": "text",
            "text": "Hello"
        }]
    });
    message_store
        .create(&original_session.id, msg_data)
        .await
        .unwrap();

    let exported = session_store
        .export_session(&original_session.id)
        .await
        .unwrap();

    let imported = session_store
        .import_session(exported, Some("proj_2"))
        .await
        .unwrap();

    assert_ne!(imported.id, original_session.id);
    assert_eq!(imported.project_id, "proj_2");
    assert_eq!(imported.title, "Original Session");

    let messages = message_store.list(&imported.id).await.unwrap();
    assert_eq!(messages.len(), 1);
}

#[tokio::test]
async fn test_import_session_invalid_data() {
    let pool = create_test_pool().await;
    let session_store = SessionStore::new(pool.clone());

    let result = session_store
        .import_session(serde_json::json!({"invalid": "data"}), None)
        .await;

    assert!(result.is_err());
}
