//! A title change renames the session, so the derived slug must move
//! with it (auto-title and manual rename share `SessionStore::update`).

use codegg_core::session::models::{CreateSession, UpdateSession};
use codegg_core::session::schema;
use codegg_core::session::SessionStore;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

async fn pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("sqlite pool");
    schema::migrate(&pool).await.expect("schema migration");
    pool
}

fn untitled_input() -> CreateSession {
    CreateSession {
        project_id: "legacy-projection".to_string(),
        directory: "/tmp".to_string(),
        title: None,
        ..Default::default()
    }
}

fn retitle(title: &str) -> UpdateSession {
    UpdateSession {
        title: Some(title.to_string()),
        ..Default::default()
    }
}

#[tokio::test(flavor = "current_thread")]
async fn title_update_recomputes_slug() {
    let pool = pool().await;
    let sessions = SessionStore::new(pool.clone());
    let created = sessions
        .create(untitled_input())
        .await
        .expect("create session");
    assert_eq!(created.title, "Untitled");
    assert_eq!(created.slug, "untitled");

    let updated = sessions
        .update(&created.id, retitle("Fix login redirect loop"))
        .await
        .expect("rename session");
    assert_eq!(updated.title, "Fix login redirect loop");
    assert_eq!(updated.slug, "fix-login-redirect-loop");
}

#[tokio::test(flavor = "current_thread")]
async fn non_title_update_leaves_slug_alone() {
    let pool = pool().await;
    let sessions = SessionStore::new(pool.clone());
    let created = sessions
        .create(untitled_input())
        .await
        .expect("create session");

    let updated = sessions
        .update(
            &created.id,
            UpdateSession {
                share_url: Some("https://example.test/share".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("non-title update");
    assert_eq!(updated.title, "Untitled");
    assert_eq!(updated.slug, "untitled");
}
