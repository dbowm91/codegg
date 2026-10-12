use super::common;

use codegg::session::{
    CreateSession, MessageStore, PartStore, SessionStore, TodoItemInput, TodoStore, UpdateSession,
};
use sqlx::SqlitePool;

async fn create_test_pool() -> SqlitePool {
    let pool = common::pool::isolated_pool().await;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    for project_id in ["proj_1", "proj_2"] {
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO project (id, name, time_created, time_updated, sandboxes)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(project_id)
        .bind(project_id)
        .bind(now)
        .bind(now)
        .bind("[]")
        .execute(&pool)
        .await
        .expect("failed to seed project");
    }

    pool
}

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
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

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
        })
        .await
        .unwrap();

    assert_eq!(session.title, "Untitled");
    assert_eq!(session.slug, "untitled");
}

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
        })
        .await
        .unwrap();

    let fetched = store.get(&created.id).await.unwrap().unwrap();
    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.title, "Get Test");
}

#[tokio::test(flavor = "current_thread")]
async fn test_session_get_nonexistent() {
    let pool = create_test_pool().await;
    let store = SessionStore::new(pool);

    let result = store.get("nonexistent_id").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
        })
        .await
        .unwrap();

    let sessions = store.list("proj_1", 10).await.unwrap();
    assert_eq!(sessions.len(), 2);
}

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
        })
        .await
        .unwrap();

    store.archive(&s1.id).await.unwrap();

    let sessions = store.list("proj_1", 10).await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].title, "Archived");
}

#[tokio::test(flavor = "current_thread")]
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
                provider_connection_id: None,
                provider_connection_revision: None,
                model_catalog_revision: None,
                selected_model_id: None,
            })
            .await
            .unwrap();
    }

    let sessions = store.list("proj_1", 2).await.unwrap();
    assert_eq!(sessions.len(), 2);
}

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
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
                provider_connection_id: None,
                provider_connection_revision: None,
                model_catalog_revision: None,
                selected_model_id: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(updated.title, "Updated Title");
}

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
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
                provider_connection_id: None,
                provider_connection_revision: None,
                model_catalog_revision: None,
                selected_model_id: None,
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

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
        })
        .await
        .unwrap();

    store.delete(&session.id).await.unwrap();

    let result = store.get(&session.id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
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

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
        })
        .await
        .unwrap();

    let archived = store.archive(&session.id).await.unwrap();
    assert!(archived.time_archived.is_some());

    let sessions = store.list("proj_1", 10).await.unwrap();
    assert!(sessions.is_empty());
}

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
        })
        .await
        .unwrap();

    store.archive(&session.id).await.unwrap();
    let unarchived = store.unarchive(&session.id).await.unwrap();
    assert!(unarchived.time_archived.is_none());

    let sessions = store.list("proj_1", 10).await.unwrap();
    assert_eq!(sessions.len(), 1);
}

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
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

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
        })
        .await
        .unwrap();

    let children = store.children(&parent.id).await.unwrap();
    assert_eq!(children.len(), 2);
}

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
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

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
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

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
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

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
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

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
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

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
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

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
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

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
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

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
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

#[tokio::test(flavor = "current_thread")]
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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
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

#[tokio::test(flavor = "current_thread")]
async fn test_import_session() {
    let pool = create_test_pool().await;
    let session_store = SessionStore::new(pool.clone());
    let _message_store = MessageStore::new(pool);

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
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
        })
        .await
        .unwrap();

    let exported = session_store
        .export_session(&original_session.id)
        .await
        .unwrap();

    let imported = session_store.import_session(exported, None).await.unwrap();

    assert_ne!(imported.id, original_session.id);
    assert_eq!(imported.project_id, "proj_1");
    assert_eq!(imported.title, "Original Session");
}

#[tokio::test(flavor = "current_thread")]
async fn test_import_session_invalid_data() {
    let pool = create_test_pool().await;
    let session_store = SessionStore::new(pool.clone());

    let result = session_store
        .import_session(serde_json::json!({"invalid": "data"}), None)
        .await;

    assert!(result.is_err());
}

/// Regression coverage for session transcript persistence.
///
/// A completed agent run used to write nothing, so reloading a session
/// replaced the transcript with whatever imports had left behind — usually
/// nothing. These tests pin the properties that write depends on: write order
/// survives the read order, an assistant reply stays an assistant reply, and a
/// session that does not exist is never invented.
mod transcript_persistence {
    use super::*;
    use codegg::session::message::{MessageData, MessageRole, PartData, PartInfo};

    async fn session_with_title(store: &SessionStore, title: &str) -> String {
        store
            .create(CreateSession {
                agent: None,
                model: None,
                tags: None,
                project_id: "proj_1".to_string(),
                directory: "/tmp".to_string(),
                title: Some(title.to_string()),
                parent_id: None,
                workspace_id: None,
                provider_connection_id: None,
                provider_connection_revision: None,
                model_catalog_revision: None,
                selected_model_id: None,
            })
            .await
            .unwrap()
            .id
    }

    fn message(
        session_id: &str,
        id: &str,
        role: MessageRole,
        text: &str,
    ) -> (String, serde_json::Value) {
        let data = MessageData {
            id: id.to_string(),
            session_id: session_id.to_string(),
            message_id: id.to_string(),
            parts: vec![PartInfo {
                id: format!("{id}_0"),
                session_id: session_id.to_string(),
                message_id: id.to_string(),
                data: PartData::Text {
                    text: text.to_string(),
                },
            }],
            role: Some(role),
        };
        (id.to_string(), serde_json::to_value(&data).unwrap())
    }

    fn text_of(msg: &codegg::session::message::Message) -> String {
        match &msg.data.parts[0].data {
            PartData::Text { text } => text.clone(),
            other => panic!("expected a text part, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_batch_round_trips_in_write_order_with_roles_intact() {
        let pool = create_test_pool().await;
        let session_store = SessionStore::new(pool.clone());
        let message_store = MessageStore::new(pool);
        let session_id = session_with_title(&session_store, "Ordered Batch").await;

        let written = message_store
            .append_batch(
                &session_id,
                &[
                    message(&session_id, "m_user", MessageRole::User, "fix the bug"),
                    message(&session_id, "m_assistant", MessageRole::Assistant, "on it"),
                ],
            )
            .await
            .unwrap();
        assert_eq!(written, Some(2));

        let messages = message_store.list(&session_id).await.unwrap();
        assert_eq!(messages.len(), 2);
        // `ORDER BY time_created ASC, id ASC` is the only ordering the reader
        // has. Two rows written in the same millisecond must not fall through
        // to the id tiebreak, or the reply loads before the prompt.
        assert_eq!(text_of(&messages[0]), "fix the bug");
        assert_eq!(messages[0].data.role, Some(MessageRole::User));
        assert_eq!(text_of(&messages[1]), "on it");
        assert_eq!(messages[1].data.role, Some(MessageRole::Assistant));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn successive_batches_keep_accumulating_in_order() {
        let pool = create_test_pool().await;
        let session_store = SessionStore::new(pool.clone());
        let message_store = MessageStore::new(pool);
        let session_id = session_with_title(&session_store, "Multi Turn").await;

        for turn in 0..3 {
            message_store
                .append_batch(
                    &session_id,
                    &[
                        message(
                            &session_id,
                            &format!("t{turn}_user"),
                            MessageRole::User,
                            &format!("question {turn}"),
                        ),
                        message(
                            &session_id,
                            &format!("t{turn}_assistant"),
                            MessageRole::Assistant,
                            &format!("answer {turn}"),
                        ),
                    ],
                )
                .await
                .unwrap();
        }

        let messages = message_store.list(&session_id).await.unwrap();
        let rendered: Vec<String> = messages.iter().map(text_of).collect();
        assert_eq!(
            rendered,
            vec![
                "question 0",
                "answer 0",
                "question 1",
                "answer 1",
                "question 2",
                "answer 2",
            ],
            "a reloaded transcript must read in conversation order"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_batch_for_a_session_that_does_not_exist_is_skipped() {
        let pool = create_test_pool().await;
        let message_store = MessageStore::new(pool.clone());

        // Subagent transcripts use a synthetic id with no `session` row, and
        // `message.session_id` is a foreign key.
        let synthetic = "parent-session-sub-task_1";
        let written = message_store
            .append_batch(
                synthetic,
                &[message(synthetic, "m_1", MessageRole::User, "hello")],
            )
            .await
            .unwrap();

        assert_eq!(written, None);
        assert!(message_store.list(synthetic).await.unwrap().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn replaying_a_batch_does_not_duplicate_the_transcript() {
        let pool = create_test_pool().await;
        let session_store = SessionStore::new(pool.clone());
        let message_store = MessageStore::new(pool);
        let session_id = session_with_title(&session_store, "Replayed").await;

        let batch = vec![
            message(&session_id, "m_user", MessageRole::User, "hello"),
            message(&session_id, "m_assistant", MessageRole::Assistant, "hi"),
        ];
        message_store
            .append_batch(&session_id, &batch)
            .await
            .unwrap();
        let second = message_store
            .append_batch(&session_id, &batch)
            .await
            .unwrap();

        assert_eq!(second, Some(0), "an already-stored id is left alone");
        assert_eq!(message_store.list(&session_id).await.unwrap().len(), 2);
    }

    /// Forking a long session must keep the recent conversation.
    ///
    /// The query ordered ascending before its `LIMIT`, so a session past the
    /// limit inherited its *oldest* messages and silently dropped the turn it
    /// was branched from. It also copied every `part` regardless of whether
    /// its message survived, which fails the `part.message_id` foreign key and
    /// loses the fork outright.
    #[tokio::test(flavor = "current_thread")]
    async fn a_fork_keeps_the_newest_messages_and_only_their_parts() {
        let pool = create_test_pool().await;
        let session_store = SessionStore::new(pool.clone());
        let message_store = MessageStore::new(pool.clone());
        let session_id = session_with_title(&session_store, "Long Session").await;

        // Well past the fork window, with ascending timestamps.
        const OVER_LIMIT: usize = 5005;
        let mut batch = Vec::new();
        for index in 0..OVER_LIMIT {
            batch.push(message(
                &session_id,
                &format!("m{index}"),
                MessageRole::User,
                &format!("turn {index}"),
            ));
        }
        // Written in chunks so each call is a realistic batch.
        for chunk in batch.chunks(500) {
            message_store
                .append_batch(&session_id, chunk)
                .await
                .unwrap();
        }

        // An orphan part belonging to a message we will drop.
        sqlx::query(
            r#"
            INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
            VALUES ('p_old', 'm0', ?, 1, 1, '{}')
            "#,
        )
        .bind(&session_id)
        .execute(&pool)
        .await
        .unwrap();

        let forked = session_store.fork(&session_id).await.unwrap();
        let messages = message_store.list(&forked.id).await.unwrap();

        assert_eq!(messages.len(), 5000, "the fork keeps a bounded transcript");
        assert_eq!(
            text_of(&messages[0]),
            format!("turn {}", OVER_LIMIT - 5000),
            "a fork branches from the present, so it keeps the newest turn"
        );
        assert_eq!(
            text_of(messages.last().unwrap()),
            format!("turn {}", OVER_LIMIT - 1),
            "the newest message must survive"
        );

        let orphan_parts: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM part WHERE message_id = 'm0'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(orphan_parts, 1, "the parent keeps its own parts");
        let orphan_in_child: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM part WHERE session_id = ?")
                .bind(&forked.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            orphan_in_child, 0,
            "a part whose message was dropped must not be carried into the fork"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn an_empty_batch_is_a_no_op() {
        let pool = create_test_pool().await;
        let message_store = MessageStore::new(pool);

        assert_eq!(
            message_store
                .append_batch("no-such-session", &[])
                .await
                .unwrap(),
            None
        );
    }
}
