//! M002b (ADR-0037, accepted — Option B): the queue mutation slice.
//!
//! Durable session-scoped queue ownership, the four mutations, and the refusal
//! semantics around them. The case this milestone exists to prove is **idle**
//! mutation: a queue authored while no turn runs and no lease row exists.
//!
//! A separate file from M002a's `user_prompt_queue.rs` on purpose — that file is
//! closed and reviewed. Every test name carries the `user_prompt_queue` prefix
//! so the plan's §11 filter selects them by name rather than by module path.

use codegg_core::session::user_prompt_queue::{
    UserPromptQueueError, UserPromptQueueOwnership, UserPromptQueueStatus, UserPromptQueueStore,
    USER_PROMPT_QUEUE_MAX_ENTRY_BYTES, USER_PROMPT_QUEUE_MAX_PENDING,
    USER_PROMPT_QUEUE_RENUMBER_BAND_OFFSET, USER_PROMPT_QUEUE_TTL_MS,
};
use codegg_core::storage::STORAGE_LAYOUT_VERSION;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

const SESSION_ID: &str = "session-queue-2b";
const OWNER: &str = "principal-owner";
const OTHER: &str = "principal-other";
const T0: i64 = 1_700_000_000_000;

async fn pool_at(path: &str) -> SqlitePool {
    let opts = SqliteConnectOptions::from_str(path)
        .expect("sqlite options")
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await
        .expect("connect sqlite");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate");
    pool
}

async fn isolated_pool() -> SqlitePool {
    let name = format!("user_prompt_queue_mut_{}", uuid::Uuid::new_v4().simple());
    pool_at(&format!("file:{name}?mode=memory&cache=shared")).await
}

async fn ensure_session(pool: &SqlitePool, session_id: &str) {
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT OR IGNORE INTO project (id, worktree, sandboxes, time_created, time_updated) \
         VALUES ('project-queue-2b', '/tmp/project-queue-2b', '[]', ?, ?)",
    )
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("insert project");
    sqlx::query(
        "INSERT OR IGNORE INTO session \
         (id, project_id, slug, directory, title, version, time_created, time_updated) \
         VALUES (?, 'project-queue-2b', 'queue-2b', '/tmp/project-queue-2b', 'Queue', '1', ?, ?)",
    )
    .bind(session_id)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("insert session");
}

/// Write a queue row directly, bypassing the owner gate.
///
/// This is how the tests construct the `Unowned` state ADR-0037 calls an
/// invalid one. It is unreachable through M002b's own code paths, because the
/// only path that creates entries also creates the owner in the same
/// transaction — which is precisely why the state has to be built by hand to be
/// tested at all.
async fn insert_entry_raw(
    pool: &SqlitePool,
    session_id: &str,
    id: &str,
    ordinal: i64,
    text: &str,
    status: &str,
    created_at_ms: i64,
    expires_at_ms: i64,
) {
    sqlx::query(
        "INSERT INTO user_prompt_queue_entry \
         (id, session_id, ordinal, text, text_bytes, status, failure_reason, \
          created_at_ms, updated_at_ms, expires_at_ms, delivered_turn_id, delivered_at_ms) \
         VALUES (?, ?, ?, ?, ?, ?, NULL, ?, ?, ?, NULL, NULL)",
    )
    .bind(id)
    .bind(session_id)
    .bind(ordinal)
    .bind(text)
    .bind(text.len() as i64)
    .bind(status)
    .bind(created_at_ms)
    .bind(created_at_ms)
    .bind(expires_at_ms)
    .execute(pool)
    .await
    .expect("insert queue entry");
}

async fn owner_row(pool: &SqlitePool, session_id: &str) -> Option<(String, i64)> {
    sqlx::query_as(
        "SELECT owner_principal, revision FROM user_prompt_queue_owner WHERE session_id = ?",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await
    .expect("read owner row")
}

// ---------------------------------------------------------------------------
// Migration and compatibility.
// ---------------------------------------------------------------------------

/// v81 carries the owner binding and nothing else. The table is the whole
/// migration: an owner column on the v80 queue table would have meant editing a
/// shipped migration, which ADR-0037's Compatibility section forbids.
#[tokio::test]
async fn user_prompt_queue_v81_migration_creates_the_owner_table() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;

    // This is a tripwire: it pins the *current* number so a silent bump is
    // visible in review, and the assertion above proves the constant still
    // equals the highest wired migration.
    //
    // It is currently v84: M007 took v82 for `run_at_ms`, managed scratch
    // reserved v83, and Linux M005 takes v84 for the additive network-intent
    // preference. M002b's own claim — that the owner binding bumped the
    // layout at all — is unchanged and is what the table assertions below prove.
    assert_eq!(
        STORAGE_LAYOUT_VERSION, 84,
        "M002b bumps the layout to the owner binding; M007 (ADR-0039) advanced it \
         to v82 for the optional due time; managed scratch advanced it to v83; \
         M005 added network intent at v84"
    );

    let table: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' \
         AND name='user_prompt_queue_owner'",
    )
    .fetch_one(&pool)
    .await
    .expect("inspect table");
    assert_eq!(table, 1, "v81 must create user_prompt_queue_owner");

    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM pragma_table_info('user_prompt_queue_owner') ORDER BY cid",
    )
    .fetch_all(&pool)
    .await
    .expect("inspect columns");
    assert_eq!(
        columns,
        vec![
            "session_id",
            "owner_principal",
            "revision",
            "adopted_at_ms",
            "updated_at_ms"
        ],
        "v81 owns the owner binding only — no extra column may ride along"
    );

    // The revision fence is a `CHECK`, so a negative revision cannot exist.
    let negative = sqlx::query(
        "INSERT INTO user_prompt_queue_owner \
         (session_id, owner_principal, revision, adopted_at_ms, updated_at_ms) \
         VALUES (?, 'p', -1, 1, 1)",
    )
    .bind(SESSION_ID)
    .execute(&pool)
    .await;
    assert!(
        negative.is_err(),
        "the revision CHECK is the backstop that survives a missed call site"
    );
}

/// v81 is additive: v80's queue rows survive it byte-for-byte.
#[tokio::test]
async fn user_prompt_queue_v80_rows_survive_v81_intact() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    insert_entry_raw(
        &pool,
        SESSION_ID,
        "v80-entry",
        0,
        "written under v80",
        "pending",
        T0,
        T0 + USER_PROMPT_QUEUE_TTL_MS,
    )
    .await;

    // Force the ladder to run the way a restart would, then read back.
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("re-run migration ladder");

    let store = UserPromptQueueStore::new(pool.clone());
    let record = store
        .get("v80-entry")
        .await
        .expect("v80 row must survive v81");
    assert_eq!(record.text, "written under v80");
    assert_eq!(record.ordinal, 0);

    // The owner table starts empty: v81 seeds nothing. An existing queue with no
    // owner row is exactly the `Unowned` state, and it must fail closed rather
    // than be back-filled with the session creator.
    assert_eq!(owner_row(&pool, SESSION_ID).await, None);
    assert!(matches!(
        store.ownership(SESSION_ID).await.expect("ownership"),
        UserPromptQueueOwnership::Unowned
    ));
}

// ---------------------------------------------------------------------------
// The headline: mutation while idle.
// ---------------------------------------------------------------------------

/// **The milestone's headline evidence.** A queue entry is added, edited,
/// deleted, and reordered while the session is idle — no turn, no lease row.
///
/// This is the case ADR-0035's clause could not express and ADR-0037 was
/// written to make expressible: the turn-scoped controller lease does not exist
/// between turns, so it answers for nobody there, including the legitimate
/// owner.
#[tokio::test]
async fn user_prompt_queue_mutation_is_authorized_while_the_session_is_idle() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());

    // No lease row exists. The whole store API is reachable with none, which is
    // the point: nothing in this path consults turn state.
    let leases: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM session_turn_controller")
        .fetch_one(&pool)
        .await
        .expect("count leases");
    assert_eq!(leases, 0, "no turn and no lease row");

    let first = store
        .add(OWNER, SESSION_ID, "e1", "first", None, T0)
        .await
        .expect("idle add must be authorized");
    assert_eq!(first.ordinal, 0);

    store
        .add(OWNER, SESSION_ID, "e2", "second", None, T0)
        .await
        .expect("idle add must be authorized");
    store
        .add(OWNER, SESSION_ID, "e3", "third", None, T0)
        .await
        .expect("idle add must be authorized");

    let edited = store
        .edit(OWNER, SESSION_ID, "e2", "second, revised", None, T0)
        .await
        .expect("idle edit must be authorized");
    assert_eq!(edited.text, "second, revised");
    assert_eq!(edited.ordinal, 1, "edit must not move an entry");

    let reordered = store
        .reorder(
            OWNER,
            SESSION_ID,
            &["e3".to_owned(), "e1".to_owned(), "e2".to_owned()],
            T0,
        )
        .await
        .expect("idle reorder must be authorized");
    let order: Vec<&str> = reordered.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(order, vec!["e3", "e1", "e2"]);

    let after_delete = store
        .delete(OWNER, SESSION_ID, "e1", T0)
        .await
        .expect("idle delete must be authorized");
    let order: Vec<&str> = after_delete.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(order, vec!["e3", "e2"], "delete leaves the rest in order");
}

/// ADR-0037 deferred the seeding policy to this milestone. The recorded audit
/// decision is: **the calling principal becomes the owner** — not the session
/// creator, and not the local interactive owner.
#[tokio::test]
async fn user_prompt_queue_first_add_adopts_the_calling_principal() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());

    assert!(matches!(
        store.ownership(SESSION_ID).await.expect("ownership"),
        UserPromptQueueOwnership::Absent
    ));

    store
        .add(OWNER, SESSION_ID, "e1", "hello", None, T0)
        .await
        .expect("first add");

    let ownership = store.ownership(SESSION_ID).await.expect("ownership");
    assert_eq!(
        ownership.owner_principal(),
        Some(OWNER),
        "the author becomes the owner, which is the policy ADR-0037 deferred here"
    );
}

// ---------------------------------------------------------------------------
// Three-valued ownership.
// ---------------------------------------------------------------------------

/// An unowned queue has **no** authorized mutator, including for `add`, and is
/// never implicitly owned by the session creator or the local interactive
/// owner. First-add must not adopt here: that would silently widen authority
/// onto whoever happened to touch an invalidated queue.
#[tokio::test]
async fn user_prompt_queue_unowned_queue_is_refused_for_every_mutation() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    // Entries with no owner row — the defensive invalid state.
    insert_entry_raw(
        &pool,
        SESSION_ID,
        "orphan",
        0,
        "orphaned",
        "pending",
        T0,
        T0 + USER_PROMPT_QUEUE_TTL_MS,
    )
    .await;
    let store = UserPromptQueueStore::new(pool.clone());

    let refused = [
        (
            "add",
            store
                .add(OWNER, SESSION_ID, "new", "x", None, T0)
                .await
                .map(|_| ()),
        ),
        (
            "edit",
            store
                .edit(OWNER, SESSION_ID, "orphan", "y", None, T0)
                .await
                .map(|_| ()),
        ),
        (
            "delete",
            store
                .delete(OWNER, SESSION_ID, "orphan", T0)
                .await
                .map(|_| ()),
        ),
        (
            "reorder",
            store
                .reorder(OWNER, SESSION_ID, &["orphan".to_owned()], T0)
                .await
                .map(|_| ()),
        ),
        (
            "transfer",
            store
                .transfer(OWNER, OTHER, 0, SESSION_ID, T0)
                .await
                .map(|_| ()),
        ),
    ];

    for (label, result) in refused {
        let error = result.expect_err(&format!("{label} on an unowned queue must refuse"));
        assert_eq!(
            error.code(),
            "user_prompt_queue_unowned",
            "{label} must name the unowned state, not silently adopt"
        );
    }

    // Nothing was created by the refused first-add.
    assert_eq!(owner_row(&pool, SESSION_ID).await, None);
    assert_eq!(
        store
            .list_all_for_session(SESSION_ID)
            .await
            .expect("list")
            .len(),
        1,
        "the refused add wrote no second row"
    );
}

/// Only the explicit adoption repairs an unowned queue.
#[tokio::test]
async fn user_prompt_queue_explicit_adoption_repairs_an_unowned_queue() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    insert_entry_raw(
        &pool,
        SESSION_ID,
        "orphan",
        0,
        "orphaned",
        "pending",
        T0,
        T0 + USER_PROMPT_QUEUE_TTL_MS,
    )
    .await;
    let store = UserPromptQueueStore::new(pool.clone());

    let record = store
        .adopt(OWNER, SESSION_ID, T0)
        .await
        .expect("explicit adoption repairs");
    assert_eq!(record.owner_principal, OWNER);
    assert_eq!(record.revision, 0);

    // And the queue is now mutable by its owner.
    store
        .add(OWNER, SESSION_ID, "e1", "now allowed", None, T0)
        .await
        .expect("mutation is authorized after repair");
}

/// Adoption never seizes a queue that already has an owner. That is what
/// transfer is for; collapsing the two would let any `agent.invoke` principal
/// steal a queue.
#[tokio::test]
async fn user_prompt_queue_adoption_never_seizes_a_queue_owned_by_another() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());
    store
        .add(OWNER, SESSION_ID, "e1", "mine", None, T0)
        .await
        .expect("first add");

    let error = store
        .adopt(OTHER, SESSION_ID, T0)
        .await
        .expect_err("adoption must not seize");
    assert_eq!(error.code(), "user_prompt_queue_not_owner");
    assert_eq!(
        owner_row(&pool, SESSION_ID).await.expect("owner").0,
        OWNER,
        "ownership is unchanged by the refused adoption"
    );
}

/// A repeated adopt is idempotent and leaves the revision fence alone, so it
/// cannot churn the revision out from under a concurrent transfer.
#[tokio::test]
async fn user_prompt_queue_adoption_is_idempotent_and_preserves_the_revision() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());
    let first = store.adopt(OWNER, SESSION_ID, T0).await.expect("adopt");
    let second = store
        .adopt(OWNER, SESSION_ID, T0 + 5)
        .await
        .expect("re-adopt");

    assert_eq!(first.revision, second.revision);
    assert_eq!(first.revision, 0);
    assert_eq!(second.adopted_at_ms, first.adopted_at_ms);
}

// ---------------------------------------------------------------------------
// Transfer: the revision fence.
// ---------------------------------------------------------------------------

/// Two transfers computed from the **same** observed revision — the CAS
/// equivalent of two concurrent transfers — converge on exactly one winner, and
/// the loser is refused by name. This is the fence that keeps a queue from ever
/// having two owners.
#[tokio::test]
async fn user_prompt_queue_two_transfers_from_one_revision_converge_on_one_winner() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());
    store.adopt(OWNER, SESSION_ID, T0).await.expect("adopt");

    let observed = store
        .ownership(SESSION_ID)
        .await
        .expect("ownership")
        .revision()
        .expect("owner row has a revision");

    let winner = store
        .transfer(OWNER, OTHER, observed, SESSION_ID, T0)
        .await
        .expect("first transfer wins");
    assert_eq!(winner.owner_principal, OTHER);
    assert_eq!(winner.revision, observed + 1);

    // The loser must still BE the owner, or the ownership gate would fire first
    // and the fence would never be exercised. OTHER now owns the queue and
    // attempts a transfer carrying the same stale revision it could have
    // observed a moment earlier.
    let loser = store
        .transfer(OTHER, "principal-third", observed, SESSION_ID, T0)
        .await
        .expect_err("second transfer from the same revision must lose");
    match loser {
        UserPromptQueueError::OwnerRevisionConflict { expected, current } => {
            assert_eq!(expected, observed);
            assert_eq!(current, observed + 1);
        }
        other => panic!("expected a revision conflict, got {other:?}"),
    }
    assert_eq!(
        owner_row(&pool, SESSION_ID).await.expect("owner").0,
        OTHER,
        "the loser's stale transfer must not have overwritten the winner"
    );

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_prompt_queue_owner")
        .fetch_one(&pool)
        .await
        .expect("count owners");
    assert_eq!(rows, 1, "a transfer must never produce two owners");
}

/// Truly concurrent transfers. Whichever wins, the invariant is that the queue
/// never holds two owners and the loser never silently overwrites the winner.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_prompt_queue_concurrent_transfers_never_produce_two_owners() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());
    store.adopt(OWNER, SESSION_ID, T0).await.expect("adopt");

    let (a, b) = tokio::join!(
        store.transfer(OWNER, OTHER, 0, SESSION_ID, T0),
        store.transfer(OWNER, "principal-third", 0, SESSION_ID, T0)
    );

    let succeeded = [a.is_ok(), b.is_ok()].iter().filter(|ok| **ok).count();
    assert!(
        succeeded <= 1,
        "two transfers from one revision must not both win: {a:?} / {b:?}"
    );

    let owners: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_prompt_queue_owner")
        .fetch_one(&pool)
        .await
        .expect("count owners");
    assert_eq!(owners, 1, "a queue must never have two owner rows");

    // Whichever outcome, the surviving owner is one of the two candidates and
    // the row is internally consistent.
    let (owner, revision) = owner_row(&pool, SESSION_ID)
        .await
        .expect("owner row survives");
    assert!(
        [OTHER, "principal-third"].contains(&owner.as_str()) || owner == OWNER,
        "unexpected owner {owner}"
    );
    assert_eq!(revision, 1, "exactly one transfer revision was consumed");
}

/// Only the current owner may transfer, matching `session_control_transfer`'s
/// controller-only rule.
#[tokio::test]
async fn user_prompt_queue_transfer_requires_the_current_owner() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());
    store.adopt(OWNER, SESSION_ID, T0).await.expect("adopt");

    let error = store
        .transfer(OTHER, OWNER, 0, SESSION_ID, T0)
        .await
        .expect_err("a non-owner cannot transfer");
    assert_eq!(error.code(), "user_prompt_queue_not_owner");
}

/// Ownership is durable. The queue survives restart, so its authority must too:
/// a restart cannot silently widen or narrow the mutator set.
#[tokio::test]
async fn user_prompt_queue_ownership_and_transfer_survive_restart() {
    let dir = std::env::temp_dir().join(format!("upq_restart_{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("queue.db");
    let url = format!("sqlite://{}", path.display());

    {
        let pool = pool_at(&url).await;
        ensure_session(&pool, SESSION_ID).await;
        let store = UserPromptQueueStore::new(pool.clone());
        store
            .add(OWNER, SESSION_ID, "e1", "durable", None, T0)
            .await
            .expect("add");
        store.adopt(OWNER, SESSION_ID, T0).await.expect("adopt");
        store
            .transfer(OWNER, OTHER, 0, SESSION_ID, T0)
            .await
            .expect("transfer");
        pool.close().await;
    }

    // A genuinely new pool over the same file — not the same in-memory handle.
    let reopened = pool_at(&url).await;
    let store = UserPromptQueueStore::new(reopened.clone());
    let ownership = store.ownership(SESSION_ID).await.expect("ownership");
    assert_eq!(
        ownership.owner_principal(),
        Some(OTHER),
        "a restart must not change who may mutate the queue"
    );
    assert_eq!(ownership.revision(), Some(1), "the fence survives too");

    let entries = store.list_live(SESSION_ID, T0).await.expect("entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].text, "durable");

    // And the restored owner is still the only mutator.
    assert!(
        store
            .add(OWNER, SESSION_ID, "e2", "not mine", None, T0)
            .await
            .is_err(),
        "the pre-transfer owner is no longer authorized after restart"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Mutations.
// ---------------------------------------------------------------------------

/// `entry_id` is the at-most-once key, so a duplicate retry converges on the
/// existing entry rather than creating a second one — at-most-once by identity,
/// with no receipt namespace (which ADR-0035 forbids).
#[tokio::test]
async fn user_prompt_queue_duplicate_add_converges_instead_of_duplicating() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());

    let first = store
        .add(OWNER, SESSION_ID, "e1", "once", None, T0)
        .await
        .expect("add");
    let retry = store
        .add(OWNER, SESSION_ID, "e1", "once", None, T0)
        .await
        .expect("retry");

    assert_eq!(first.id, retry.id);
    assert_eq!(
        first.ordinal, retry.ordinal,
        "no second row and no new position"
    );
    assert_eq!(
        store.list_all_for_session(SESSION_ID).await.unwrap().len(),
        1
    );
}

/// Convergence must not resurrect a terminal row, and must not hand one
/// session's entry to another.
#[tokio::test]
async fn user_prompt_queue_duplicate_add_refuses_to_resurrect_an_expired_entry() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    ensure_session(&pool, "session-other-queue").await;
    let store = UserPromptQueueStore::new(pool.clone());
    store
        .add(OWNER, SESSION_ID, "e1", "once", None, T0)
        .await
        .expect("add");

    let expired_at = T0 + USER_PROMPT_QUEUE_TTL_MS + 1;
    let error = store
        .add(OWNER, SESSION_ID, "e1", "once", None, expired_at)
        .await
        .expect_err("an expired entry must not be resurrected by a retry");
    assert_eq!(error.code(), "user_prompt_queue_expired");

    let record = store.get("e1").await.expect("row still terminal");
    assert_eq!(
        record.expires_at_ms,
        T0 + USER_PROMPT_QUEUE_TTL_MS,
        "the retry must not extend the expiry"
    );
}

/// Edit replaces text and nothing else.
#[tokio::test]
async fn user_prompt_queue_edit_preserves_identity_and_position() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());
    store
        .add(OWNER, SESSION_ID, "e1", "one", None, T0)
        .await
        .expect("add");
    store
        .add(OWNER, SESSION_ID, "e2", "two", None, T0)
        .await
        .expect("add");

    let before = store.get("e2").await.expect("before");
    let edited = store
        .edit(OWNER, SESSION_ID, "e2", "two revised", None, T0)
        .await
        .expect("edit");
    let after = store.get("e2").await.expect("after");

    assert_eq!(edited.id, before.id, "identity is unchanged");
    assert_eq!(edited.ordinal, before.ordinal, "position is unchanged");
    assert_eq!(after.created_at_ms, before.created_at_ms);
    assert_eq!(edited.text, "two revised");
    assert_eq!(edited.text_bytes, "two revised".len());
}

/// A bounded resource refuses rather than truncates — including on edit, where
/// silently shortening would produce a different prompt from the one written.
#[tokio::test]
async fn user_prompt_queue_edit_refuses_rather_than_truncating() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());
    store
        .add(OWNER, SESSION_ID, "e1", "short", None, T0)
        .await
        .expect("add");

    let oversized = "x".repeat(USER_PROMPT_QUEUE_MAX_ENTRY_BYTES + 1);
    let error = store
        .edit(OWNER, SESSION_ID, "e1", &oversized, None, T0)
        .await
        .expect_err("an over-bound edit must refuse");
    assert_eq!(error.code(), "user_prompt_queue_entry_too_large");

    let record = store.get("e1").await.expect("unchanged");
    assert_eq!(record.text, "short", "the refused edit wrote nothing");
}

/// Expiry wins over edit: an expired entry is terminal and an edit cannot bring
/// it back.
#[tokio::test]
async fn user_prompt_queue_edit_never_resurrects_an_expired_entry() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());
    store
        .add(OWNER, SESSION_ID, "e1", "original", None, T0)
        .await
        .expect("add");

    let expired_at = T0 + USER_PROMPT_QUEUE_TTL_MS + 1;
    let error = store
        .edit(OWNER, SESSION_ID, "e1", "revived", None, expired_at)
        .await
        .expect_err("an expired entry must refuse an edit");
    assert_eq!(error.code(), "user_prompt_queue_expired");

    // The refused edit rolled back, including its expiry sweep -- which is the
    // point: an unauthorized or invalid mutation has zero side effects. What
    // must hold is that the entry is not live and its text is untouched, so the
    // read projection cannot resurrect it either.
    let record = store.get("e1").await.expect("row still present");
    assert_ne!(record.text, "revived", "the refused edit wrote nothing");
    let live = store.list_live(SESSION_ID, expired_at).await.expect("live");
    assert!(
        live.iter().all(|e| e.id != "e1"),
        "an expired entry must not appear in the projection after a refused edit"
    );
    assert!(
        store
            .edit(OWNER, SESSION_ID, "e1", "second attempt", None, expired_at)
            .await
            .is_err(),
        "an expired entry stays terminal across repeated edits"
    );
}

/// Delete keeps the queue a dense total order.
#[tokio::test]
async fn user_prompt_queue_delete_renumbers_densely() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());
    for (id, text) in [("e1", "a"), ("e2", "b"), ("e3", "c"), ("e4", "d")] {
        store
            .add(OWNER, SESSION_ID, id, text, None, T0)
            .await
            .expect("add");
    }

    store
        .delete(OWNER, SESSION_ID, "e2", T0)
        .await
        .expect("delete");

    let live = store.list_live(SESSION_ID, T0).await.expect("live");
    let dense: Vec<(String, i64)> = live.iter().map(|e| (e.id.clone(), e.ordinal)).collect();
    assert_eq!(
        dense,
        vec![
            ("e1".to_owned(), 0),
            ("e3".to_owned(), 1),
            ("e4".to_owned(), 2),
        ],
        "a gap at the delete site must be closed"
    );
}

/// Reorder writes a dense total order, starting from a deliberately sparse one
/// so the renumber has to do real work.
#[tokio::test]
async fn user_prompt_queue_reorder_is_collision_free_from_a_sparse_order() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    // A deliberately colliding intermediate state: ordinals 10, 20, 30 rather
    // than a dense 0,1,2. A naive walk to the targets would collide here.
    for (id, ordinal) in [("e1", 10), ("e2", 20), ("e3", 30)] {
        insert_entry_raw(
            &pool,
            SESSION_ID,
            id,
            ordinal,
            id,
            "pending",
            T0,
            T0 + USER_PROMPT_QUEUE_TTL_MS,
        )
        .await;
    }
    let store = UserPromptQueueStore::new(pool.clone());
    store.adopt(OWNER, SESSION_ID, T0).await.expect("adopt");

    let result = store
        .reorder(
            OWNER,
            SESSION_ID,
            &["e3".to_owned(), "e2".to_owned(), "e1".to_owned()],
            T0,
        )
        .await
        .expect("reorder from a sparse order");

    let dense: Vec<(String, i64)> = result.iter().map(|e| (e.id.clone(), e.ordinal)).collect();
    assert_eq!(
        dense,
        vec![
            ("e3".to_owned(), 0),
            ("e2".to_owned(), 1),
            ("e1".to_owned(), 2),
        ],
        "the renumber must be collision-free and dense"
    );
}

/// Reorder takes the **complete** order. A stale client view that dropped or
/// invented an entry is refused rather than partially applied, because a partial
/// reorder is exactly the interleaving ADR-0037 forbids.
#[tokio::test]
async fn user_prompt_queue_reorder_refuses_a_partial_entry_list() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());
    for id in ["e1", "e2", "e3"] {
        store
            .add(OWNER, SESSION_ID, id, id, None, T0)
            .await
            .expect("add");
    }

    for (label, target) in [
        ("dropped entry", vec!["e2".to_owned(), "e3".to_owned()]),
        (
            "invented entry",
            vec!["e2".to_owned(), "e3".to_owned(), "e4".to_owned()],
        ),
        (
            "duplicated entry",
            vec!["e1".to_owned(), "e1".to_owned(), "e3".to_owned()],
        ),
        (
            "unknown entry",
            vec!["e1".to_owned(), "e2".to_owned(), "e9".to_owned()],
        ),
    ] {
        let error = store
            .reorder(OWNER, SESSION_ID, &target, T0)
            .await
            .expect_err(&format!("{label} must refuse"));
        assert_eq!(error.code(), "user_prompt_queue_order_mismatch", "{label}");

        let live = store.list_live(SESSION_ID, T0).await.expect("live");
        assert_eq!(live.len(), 3, "{label} must not have applied partially");
    }
}

/// Concurrent reorders interleave into **one** serialised outcome: a dense
/// order equal to one of the submitted ones. No interleaved or partial order is
/// observable.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_prompt_queue_concurrent_reorder_converges_on_one_order() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());
    for id in ["e1", "e2", "e3", "e4"] {
        store
            .add(OWNER, SESSION_ID, id, id, None, T0)
            .await
            .expect("add");
    }

    let forward = vec![
        "e1".to_owned(),
        "e2".to_owned(),
        "e3".to_owned(),
        "e4".to_owned(),
    ];
    let reverse = vec![
        "e4".to_owned(),
        "e3".to_owned(),
        "e2".to_owned(),
        "e1".to_owned(),
    ];
    let (a, b) = tokio::join!(
        store.reorder(OWNER, SESSION_ID, &forward, T0),
        store.reorder(OWNER, SESSION_ID, &reverse, T0)
    );

    // A SQLite lock contention refusal is legitimate; a *wrong order* is not.
    for projection in [a, b].into_iter().flatten() {
        let ids: Vec<String> = projection.into_iter().map(|e| e.id).collect();
        assert!(
            ids == forward || ids == reverse,
            "an interleaved order became observable: {ids:?}"
        );
    }

    let live = store.list_live(SESSION_ID, T0).await.expect("live");
    let ids: Vec<String> = live.iter().map(|e| e.id.clone()).collect();
    assert!(
        ids == forward || ids == reverse,
        "the surviving order must be one submitted order, got {ids:?}"
    );
    let ordinals: Vec<i64> = live.iter().map(|e| e.ordinal).collect();
    assert_eq!(ordinals, vec![0, 1, 2, 3], "the order stays dense");
}

/// Bounds refuse on the mutation path, not only on the read path M002a shipped.
#[tokio::test]
async fn user_prompt_queue_bounds_refuse_on_the_add_path() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());
    store.adopt(OWNER, SESSION_ID, T0).await.expect("adopt");

    for i in 0..USER_PROMPT_QUEUE_MAX_PENDING {
        store
            .add(OWNER, SESSION_ID, &format!("e{i}"), "x", None, T0)
            .await
            .unwrap_or_else(|error| panic!("entry {i} must fit: {error}"));
    }

    let error = store
        .add(OWNER, SESSION_ID, "one-too-many", "x", None, T0)
        .await
        .expect_err("the pending bound must refuse");
    assert_eq!(error.code(), "user_prompt_queue_too_many_entries");
    assert_eq!(
        store.list_all_for_session(SESSION_ID).await.unwrap().len() as i64,
        USER_PROMPT_QUEUE_MAX_PENDING,
        "the refused add wrote nothing"
    );
}

// ---------------------------------------------------------------------------
// Security and negative.
// ---------------------------------------------------------------------------

/// A refusal to a non-owner must not disclose who the owner is: naming it would
/// itself be the leak the refusal exists to prevent.
#[tokio::test]
async fn user_prompt_queue_not_owner_refusal_leaks_no_owner_identity() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());
    store
        .add(OWNER, SESSION_ID, "e1", "private prompt", None, T0)
        .await
        .expect("add");

    let refusals = [
        store
            .edit(OTHER, SESSION_ID, "e1", "x", None, T0)
            .await
            .err(),
        store.delete(OTHER, SESSION_ID, "e1", T0).await.err(),
        store
            .reorder(OTHER, SESSION_ID, &["e1".to_owned()], T0)
            .await
            .err(),
        store.adopt(OTHER, SESSION_ID, T0).await.err(),
        store.transfer(OTHER, OWNER, 0, SESSION_ID, T0).await.err(),
    ];

    for error in refusals.into_iter().flatten() {
        let rendered = format!("{error} {error:?}");
        assert_eq!(error.code(), "user_prompt_queue_not_owner");
        assert!(
            !rendered.contains(OWNER),
            "the refusal disclosed the owner identity: {rendered}"
        );
    }
}

/// Entry text is user-authored prompt content with the same sensitivity as a
/// submitted turn. No refusal path may carry it.
#[tokio::test]
async fn user_prompt_queue_no_refusal_carries_entry_text() {
    const SENTINEL: &str = "SENTINEL-QUEUE-BODY-4b7e1a";
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());
    store
        .add(OWNER, SESSION_ID, "e1", SENTINEL, None, T0)
        .await
        .expect("add");

    let oversized = format!(
        "{SENTINEL}{}",
        "x".repeat(USER_PROMPT_QUEUE_MAX_ENTRY_BYTES)
    );
    let refusals = [
        store
            .edit(OTHER, SESSION_ID, "e1", "y", None, T0)
            .await
            .err(),
        store
            .edit(OWNER, SESSION_ID, "e1", &oversized, None, T0)
            .await
            .err(),
        store.delete(OTHER, SESSION_ID, "e1", T0).await.err(),
        store.add(OWNER, SESSION_ID, "e2", "", None, T0).await.err(),
        store.delete(OWNER, SESSION_ID, "e-missing", T0).await.err(),
    ];

    for error in refusals.into_iter().flatten() {
        let rendered = format!("{error} {error:?}");
        assert!(
            !rendered.contains(SENTINEL),
            "a refusal leaked entry text: {rendered}"
        );
    }
}

/// Every typed refusal has its own stable wire code. A shared code would make two
/// different refusals indistinguishable to a caller.
#[test]
fn user_prompt_queue_mutation_variants_have_distinct_codes() {
    let errors = [
        UserPromptQueueError::NotQueueOwner,
        UserPromptQueueError::QueueUnowned,
        UserPromptQueueError::OrderMismatch,
        UserPromptQueueError::OwnerRevisionConflict {
            expected: 0,
            current: 1,
        },
        UserPromptQueueError::NotFound,
        UserPromptQueueError::Expired,
    ];
    let mut codes: Vec<&str> = errors.iter().map(|e| e.code()).collect();
    codes.sort_unstable();
    let total = codes.len();
    codes.dedup();
    assert_eq!(codes.len(), total, "two refusals share a wire code");
}

// ---------------------------------------------------------------------------
// C001: what a reorder does to a `delivering` row, and where the entry lands if
// that delivery is released.
//
// M005 left this as an open question and recorded the answer rather than
// engineering around it, because the only fix was in `renumber_dense` — closed
// M002b/M003 code — and that needed a maintainer decision. C001 is that
// decision, and these tests are where the recorded answer is replaced.
//
// **These two tests supersede measurements, not contracts.** M005's
// `..._displaces_a_delivering_row_above_the_pending_band` and
// `..._a_released_delivery_returns_to_the_back_of_the_queue` asserted the
// broken behaviour *correctly* — they were written to record what the store did,
// which is why they were safe to correct. They are replaced here rather than
// left in place, because a green suite that describes a fixed defect as correct
// is worse than no suite.
//
// M005's closure record is not contradicted by this section: it measured the
// behaviour accurately and disposed of it honestly within its own scope.
// ---------------------------------------------------------------------------

/// Seed four entries, retire the first, and leave the **second** one claimed.
///
/// The second claim has to happen after `a` is retired, because
/// `claim_next_for_delivery` takes the lowest ordinal — a first claim would
/// always land on the head and could not exercise a row that is genuinely
/// *among* the queue.
async fn seeded_with_a_delivering_second_entry(store: &UserPromptQueueStore) {
    for (id, text) in [
        ("a", "first"),
        ("b", "second"),
        ("c", "third"),
        ("d", "fourth"),
    ] {
        store
            .add(OWNER, SESSION_ID, id, text, None, T0)
            .await
            .expect("seed");
    }
    let first = store
        .claim_next_for_delivery(SESSION_ID, T0)
        .await
        .expect("claim")
        .expect("a row to claim");
    assert!(store
        .mark_delivered(&first.entry.id, "turn-a", T0)
        .await
        .expect("mark delivered"));

    let second = store
        .claim_next_for_delivery(SESSION_ID, T0 + 1)
        .await
        .expect("claim")
        .expect("the next row to claim");
    assert_eq!(second.entry.id, "b", "`b` is the lowest surviving ordinal");
}

/// A reorder holds a `delivering` row at **its own rank** instead of pushing it
/// above the pending band.
///
/// The client still sends the **pending subset** — this is what M005's
/// `pending_permutation` does, and the only shape `is_permutation` accepts. `b`
/// was the lowest live ordinal when it was claimed, so it holds rank 0 across the
/// reorder, and the pending rows reorder around it.
#[tokio::test]
async fn user_prompt_queue_a_reorder_holds_a_delivering_row_at_its_rank() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded_with_a_delivering_second_entry(&store).await;

    let reordered = store
        .reorder(
            OWNER,
            SESSION_ID,
            &["d".to_string(), "c".to_string()],
            T0 + 2,
        )
        .await
        .expect("reorder with a delivering row must succeed");

    let order: Vec<&str> = reordered.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(
        order,
        vec!["b", "d", "c"],
        "SUPERSEDES M005's measurement that a `delivering` row is displaced above \
         the band and renders last. `b` holds the rank it had when it was \
         claimed, and the pending rows reorder around it."
    );

    let b = reordered
        .iter()
        .find(|e| e.id == "b")
        .expect("`b` is still in the projection");
    assert_eq!(
        b.ordinal, 0,
        "the delivering row keeps its rank rather than inheriting the displacement \
         band; got {}",
        b.ordinal
    );

    // Every live row is inside the dense band. This is the assertion M005 could
    // not make: it only ever held for the pending rows.
    let out_of_band: Vec<&str> = reordered
        .iter()
        .filter(|e| e.ordinal >= USER_PROMPT_QUEUE_RENUMBER_BAND_OFFSET)
        .map(|e| e.id.as_str())
        .collect();
    assert!(
        out_of_band.is_empty(),
        "no live row may be left above the dense band: {out_of_band:?}"
    );

    // And the order the *next* delivery will use follows the client's reorder,
    // not the claim order.
    let next = store
        .claim_next_for_delivery(SESSION_ID, T0 + 3)
        .await
        .expect("claim")
        .expect("a pending row to claim");
    assert_eq!(
        next.entry.id, "d",
        "delivery order follows the daemon ordinal, which the client just wrote"
    );
}

/// A released delivery resumes **at its own position** — and, precisely because
/// the reorder no longer moved it, the release moves nothing at all.
///
/// This is the sharpest form of the invariant: the released queue is identical
/// to the queue the reorder produced. M005's control test
/// (`..._a_release_without_a_reorder_keeps_its_place`) established that a release
/// with no reorder is order-preserving; C001 makes the reorder case behave the
/// same way, which is what removes the documented workaround.
#[tokio::test]
async fn user_prompt_queue_a_released_delivery_resumes_at_its_own_position() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded_with_a_delivering_second_entry(&store).await;

    store
        .reorder(
            OWNER,
            SESSION_ID,
            &["d".to_string(), "c".to_string()],
            T0 + 2,
        )
        .await
        .expect("reorder");

    // The delivery is refused transiently and released rather than failed.
    assert!(
        store.release_delivery("b", T0 + 3).await.expect("release"),
        "the release must apply"
    );

    let live = store.list_live(SESSION_ID, T0 + 4).await.expect("list");
    let order: Vec<&str> = live.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(
        order,
        vec!["b", "d", "c"],
        "SUPERSEDES M005's recorded answer that a released entry resumes at the \
         END of the queue. `renumber_dense` now writes the whole live set, so \
         `b` kept a dense ordinal while delivering and the release had nothing \
         to move."
    );

    let b = live
        .iter()
        .find(|e| e.id == "b")
        .expect("`b` is still live");
    assert_eq!(
        b.ordinal, 0,
        "the release restores the status and the ordinal was never lost, so the \
         entry is dense rather than a band offset above its old value; got {}",
        b.ordinal
    );

    // The sharper claim M005 could not make: the release does not merely put the
    // entry back, it leaves **every** row's ordinal alone.
    assert_eq!(
        live.iter().map(|e| e.ordinal).collect::<Vec<i64>>(),
        vec![0, 1, 2],
        "the released queue must be exactly as dense as the queue before the \
         release; the in-band gap M005 measured is gone"
    );

    // And the revived entry is genuinely claimable at the head, rather than
    // waiting behind everything the user put after it.
    let reclaimed = store
        .claim_next_for_delivery(SESSION_ID, T0 + 5)
        .await
        .expect("reclaim")
        .expect("the released entry is claimable again");
    assert_eq!(reclaimed.entry.id, "b");
}

/// The second of the three return paths: startup reconciliation of a stranded
/// `delivering` row resolves to `Retry` and returns it at its own position.
///
/// Reconcile revives a row that is **still `delivering`**, so it is in the live
/// set the merge reads, and Option A reaches it with no change to the store
/// function itself.
#[tokio::test]
async fn user_prompt_queue_a_reconciled_delivery_resumes_at_its_own_position() {
    use codegg_core::session::user_prompt_queue::UserPromptQueueDeliveryResolution;

    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded_with_a_delivering_second_entry(&store).await;

    store
        .reorder(
            OWNER,
            SESSION_ID,
            &["d".to_string(), "c".to_string()],
            T0 + 2,
        )
        .await
        .expect("reorder");

    // The daemon restarts, finds `b` stranded with no receipt, and retries it.
    assert!(
        store
            .resolve_stranded_delivery("b", &UserPromptQueueDeliveryResolution::Retry, T0 + 3,)
            .await
            .expect("reconcile"),
        "the reconciliation must apply"
    );

    let live = store.list_live(SESSION_ID, T0 + 4).await.expect("list");
    let order: Vec<&str> = live.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(
        order,
        vec!["b", "d", "c"],
        "a reconciled stranded entry resumes in place for the same reason a \
         released one does: it held a dense ordinal while stranded"
    );
    assert_eq!(
        live.iter().map(|e| e.ordinal).collect::<Vec<i64>>(),
        vec![0, 1, 2],
        "the reconciled queue stays dense"
    );
}

/// **C001 §14 stop condition, reported rather than patched.**
///
/// `retry_failed` is the one return path Option A does *not* reach, and the
/// reason is structural rather than an oversight: it revives a row that was
/// **terminal** (`failed`) while the reorder ran. A terminal row is not in the
/// live set, so the merge never gave it a dense ordinal, and reviving it restores
/// a status it no longer owns an ordinal for.
///
/// C001's §6 claims all three return paths are fixed with no change to those
/// functions. That claim holds for `release_delivery` and for
/// `resolve_stranded_delivery(Retry)`, and this test is the measurement that
/// falsifies it for `retry_failed`.
///
/// Two facts bound the severity, and both are why this is reported rather than
/// fixed here:
/// 1. `retry_failed` has **no production caller** — the daemon's non-terminal
///    path is `release_delivery`, and reconciliation uses
///    `resolve_stranded_delivery`. Only a store-level test calls it today.
/// 2. A `failed` entry is not in the client's projection, so no user ever saw it
///    move. There is no observed position to preserve.
///
/// What a maintainer decision still has to choose is where a revived `failed`
/// entry *should* land: its stale ordinal, the head, or the tail of the band.
/// Until that is chosen this test pins the measured truth, so whoever chooses is
/// met by a red test rather than by a silent behaviour change.
#[tokio::test]
async fn user_prompt_queue_a_retried_failure_is_not_reached_by_the_c001_merge() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());

    for (id, text) in [("a", "first"), ("b", "second"), ("c", "third")] {
        store
            .add(OWNER, SESSION_ID, id, text, None, T0)
            .await
            .expect("seed");
    }
    let claim = store
        .claim_next_for_delivery(SESSION_ID, T0)
        .await
        .expect("claim")
        .expect("a row to claim");
    assert!(store
        .mark_delivery_failed(&claim.entry.id, "transient", T0 + 1)
        .await
        .expect("fail"));

    // A reorder while `a` is terminal: `a` is not in the live set, so it is
    // displaced and never written back.
    store
        .reorder(
            OWNER,
            SESSION_ID,
            &["c".to_string(), "b".to_string()],
            T0 + 2,
        )
        .await
        .expect("reorder");

    assert!(
        store.retry_failed("a", T0 + 3).await.expect("retry"),
        "the retry must apply"
    );

    let live = store.list_live(SESSION_ID, T0 + 4).await.expect("list");
    let order: Vec<&str> = live.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(
        order,
        vec!["c", "b", "a"],
        "MEASURED, NOT ENDORSED: reviving a `failed` entry returns it above the \
         band, because a terminal row is absent from the merge's live set. This \
         is the one return path C001 does not reach; see the test doc comment."
    );

    // The entry is not lost — which is why this is a reported gap and not a
    // data-loss defect. It is delivered **last**, behind rows that were queued
    // after it failed, which is the whole of the remaining symptom.
    let first = store
        .claim_next_for_delivery(SESSION_ID, T0 + 5)
        .await
        .expect("claim")
        .expect("a pending row to claim");
    assert_eq!(
        first.entry.id, "c",
        "the revived entry sits behind the rows that were queued after it failed"
    );
    assert!(store
        .mark_delivered(&first.entry.id, "turn-c", T0 + 6)
        .await
        .expect("mark delivered"));

    let second = store
        .claim_next_for_delivery(SESSION_ID, T0 + 7)
        .await
        .expect("claim")
        .expect("a pending row to claim");
    assert_eq!(
        second.entry.id, "b",
        "`b` follows the order the client wrote"
    );
    assert!(store
        .mark_delivered(&second.entry.id, "turn-b", T0 + 8)
        .await
        .expect("mark delivered"));

    let third = store
        .claim_next_for_delivery(SESSION_ID, T0 + 9)
        .await
        .expect("claim")
        .expect("a pending row to claim");
    assert_eq!(
        third.entry.id, "a",
        "the revived entry is still delivered, just not in the user's order"
    );
}

/// A delete's survivor renumbering now covers a `delivering` row, so the next
/// delete re-densifies a returning entry instead of stranding it further.
#[tokio::test]
async fn user_prompt_queue_delete_densifies_a_delivering_row() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded_with_a_delivering_second_entry(&store).await;

    let after = store
        .delete(OWNER, SESSION_ID, "c", T0 + 2)
        .await
        .expect("delete a pending row while another delivers");

    let order: Vec<&str> = after.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(order, vec!["b", "d"], "the gap the delete left is closed");
    assert_eq!(
        after.iter().map(|e| e.ordinal).collect::<Vec<i64>>(),
        vec![0, 1],
        "delete's renumber covers the whole live set, `delivering` included"
    );
}

/// The same release with **no** reorder in between leaves the entry where it
/// was — which is what isolates the reorder as the cause rather than the
/// release itself.
#[tokio::test]
async fn user_prompt_queue_a_release_without_a_reorder_keeps_its_place() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());

    for (id, text) in [("a", "first"), ("b", "second"), ("c", "third")] {
        store
            .add(OWNER, SESSION_ID, id, text, None, T0)
            .await
            .expect("seed");
    }
    store
        .claim_next_for_delivery(SESSION_ID, T0)
        .await
        .expect("claim")
        .expect("a row to claim");
    assert!(store.release_delivery("a", T0 + 1).await.expect("release"));

    let live = store.list_live(SESSION_ID, T0 + 2).await.expect("list");
    let order: Vec<&str> = live.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(
        order,
        vec!["a", "b", "c"],
        "without a reorder there is no displacement to inherit, so the entry \
         resumes in place — which is what makes the reorder the cause"
    );
}

/// The uniqueness backstop: a reorder while one row is delivering must not
/// violate `UNIQUE (session_id, ordinal)`, which is the whole reason the
/// displacement exists. This is the assertion that would fail if the band
/// offset were ever removed.
#[tokio::test]
async fn user_prompt_queue_reorder_with_a_delivering_row_keeps_ordinals_unique() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());

    for (id, text) in [("a", "first"), ("b", "second"), ("c", "third")] {
        store
            .add(OWNER, SESSION_ID, id, text, None, T0)
            .await
            .expect("seed");
    }
    store
        .claim_next_for_delivery(SESSION_ID, T0)
        .await
        .expect("claim")
        .expect("a row to claim");

    store
        .reorder(OWNER, SESSION_ID, &["c".to_string(), "b".to_string()], T0)
        .await
        .expect("reorder");

    let rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT id, ordinal FROM user_prompt_queue_entry WHERE session_id = ?")
            .bind(SESSION_ID)
            .fetch_all(&pool)
            .await
            .expect("read ordinals");
    let mut ordinals: Vec<i64> = rows.iter().map(|(_, o)| *o).collect();
    let total = ordinals.len();
    ordinals.sort_unstable();
    ordinals.dedup();
    assert_eq!(
        ordinals.len(),
        total,
        "two rows share an ordinal after a reorder with a delivering row: {rows:?}"
    );
}

/// The refusal M005's client avoids by construction, observed: naming a
/// `delivering` row in the permutation is refused with `order_mismatch`.
#[tokio::test]
async fn user_prompt_queue_a_permutation_naming_a_delivering_row_is_refused() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());

    for (id, text) in [("a", "first"), ("b", "second")] {
        store
            .add(OWNER, SESSION_ID, id, text, None, T0)
            .await
            .expect("seed");
    }
    store
        .claim_next_for_delivery(SESSION_ID, T0)
        .await
        .expect("claim")
        .expect("a row to claim");

    // Exactly what a client that sends every rendered id would send.
    let error = store
        .reorder(OWNER, SESSION_ID, &["b".to_string(), "a".to_string()], T0)
        .await
        .expect_err("a delivering row must not be nameable");
    assert_eq!(
        error.code(),
        UserPromptQueueError::OrderMismatch.code(),
        "this is the refusal M005's pending-only permutation exists to avoid"
    );

    // The pending subset alone is accepted — the shape the box actually sends.
    store
        .reorder(OWNER, SESSION_ID, &["b".to_string()], T0)
        .await
        .expect("the pending subset is the accepted shape");
}

// ---------------------------------------------------------------------------
// C001: the invariant, over every interleaving rather than one hand-picked one.
//
// This is exhaustive, not random. The state space here is small — three entries
// and five operations — so enumerating all 780 sequences of length 1..=4 covers
// it completely, which is stronger evidence than sampling it. It is also
// deterministic: the same failure is reproduced on every run, with no shrinking
// and no seed to report.
//
// `retry_failed` is deliberately absent from the alphabet. It is the one path
// C001 does not reach (see
// `user_prompt_queue_a_retried_failure_is_not_reached_by_the_c001_merge`), and
// including it here would make the density property false by design rather than
// by defect. A `claim_fail` step still retires a row to terminal — so terminal
// rows are displaced and the `UNIQUE` backstop is genuinely exercised — it just
// does not revive one.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum QueueOp {
    /// Reorder the pending rows into their reverse: always a valid permutation,
    /// whatever the current order happens to be.
    ReversePending,
    /// Delete the lowest-pending row, closing a gap.
    DeleteHead,
    /// Claim, then release — the interrupted-delivery path.
    ClaimRelease,
    /// Claim and leave the row `delivering`.
    ///
    /// This op exists because the defect needs it. Without a step that holds a
    /// row mid-delivery, no two operations in the alphabet ever overlap a
    /// reorder with a `delivering` row — the exact state C001 is about — and the
    /// enumeration would pass against the *unfixed* store. It was added after
    /// injection (i) demonstrated that hole.
    ClaimHold,
    /// Claim, then fail terminally.
    ClaimFail,
    /// Claim, then bind to a turn.
    ClaimDeliver,
}

const QUEUE_OPS: [QueueOp; 6] = [
    QueueOp::ReversePending,
    QueueOp::DeleteHead,
    QueueOp::ClaimRelease,
    QueueOp::ClaimHold,
    QueueOp::ClaimFail,
    QueueOp::ClaimDeliver,
];

const PROPERTY_ENTRIES: [&str; 3] = ["a", "b", "c"];

/// One `(id, ordinal, status)` row read back from the table.
type QueueRow = (String, i64, String);

/// The sequence that hit the pre-existing displacement collision, plus the rows
/// that triggered it.
type CollisionRecord = (String, Vec<QueueRow>);

/// The number of enumeration steps that hit the pre-existing displacement
/// collision, measured rather than guessed. See [`is_known_displacement_collision`].
const COLLISION_STEPS: usize = 4;

/// The first sequence in the enumeration to hit it, and the row state that does.
///
/// Minimal reproduction, in plain terms: queue three prompts, reorder them, let
/// one fail, reorder, reorder. The third reorder is refused with a storage
/// error. It is reachable from the queue box alone.
const FIRST_COLLISION_CONTEXT: &str =
    "[ReversePending, ClaimFail, ReversePending, ReversePending] @ ReversePending";
/// The `(id, ordinal, status)` rows as the failing step saw them, ascending.
///
/// Two pending rows in the dense band and one terminal row resting at band
/// height. The displacement lifts `a` 0 → 1 000 000 while `c` still occupies
/// that ordinal.
fn first_collision_rows() -> Vec<QueueRow> {
    vec![
        ("a".to_owned(), 0, "pending".to_owned()),
        ("b".to_owned(), 1, "pending".to_owned()),
        ("c".to_owned(), 1_000_000, "failed".to_owned()),
    ]
}

/// The live queue as `id → rank`, plus the ordered id list.
async fn live_snapshot(
    store: &UserPromptQueueStore,
    now_ms: i64,
) -> (Vec<String>, Vec<(String, UserPromptQueueStatus)>) {
    let live = store.list_live(SESSION_ID, now_ms).await.expect("live");
    let order: Vec<String> = live.iter().map(|e| e.id.clone()).collect();
    let statuses: Vec<(String, UserPromptQueueStatus)> =
        live.iter().map(|e| (e.id.clone(), e.status)).collect();
    (order, statuses)
}

/// The invariants that must hold at **every** step of **every** sequence.
async fn assert_step_invariants(
    store: &UserPromptQueueStore,
    pool: &SqlitePool,
    now_ms: i64,
    context: &str,
) {
    let live = store.list_live(SESSION_ID, now_ms).await.expect("live");

    // P1 — no live row is ever stranded above the displacement band. This is the
    // property the defect violated: before C001, any reorder that met a
    // `delivering` row pushed it above the band and nothing wrote it back, so a
    // released entry reappeared at ~1 000 000 and was *delivered* last.
    //
    // Stated as "below the band" rather than as "dense 0..n" on purpose. Density
    // is not a global invariant of this table and was never one: a row going
    // terminal leaves a hole that only the next reorder or delete closes, and
    // `add` appends over `MAX(ordinal)` across *all* rows, so after a reorder
    // that displaced a terminal row the next add legitimately lands above the
    // band too. Asserting density here would have been asserting something the
    // store has never guaranteed, and failing on `[ClaimFail]`'s `[1, 2]` is
    // that assertion being wrong rather than the store being wrong.
    let stranded: Vec<(&str, i64)> = live
        .iter()
        .filter(|e| e.ordinal >= USER_PROMPT_QUEUE_RENUMBER_BAND_OFFSET)
        .map(|e| (e.id.as_str(), e.ordinal))
        .collect();
    assert!(
        stranded.is_empty(),
        "{context}: a live row was stranded above the displacement band: {stranded:?}"
    );

    // P2 — `UNIQUE (session_id, ordinal)` across *every* row, terminal ones
    // included. The displacement step is the only thing that guarantees this,
    // so this is the assertion that catches its removal.
    let rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT id, ordinal FROM user_prompt_queue_entry WHERE session_id = ?")
            .bind(SESSION_ID)
            .fetch_all(pool)
            .await
            .expect("read every row");
    let mut all = rows.iter().map(|(_, o)| *o).collect::<Vec<i64>>();
    let total = all.len();
    all.sort_unstable();
    all.dedup();
    assert_eq!(
        all.len(),
        total,
        "{context}: two rows share an ordinal: {rows:?}"
    );
}

/// The `id → rank` pairs of every live row whose status matches `wanted`.
fn ranks_of(
    order: &[String],
    statuses: &[(String, UserPromptQueueStatus)],
    wanted: UserPromptQueueStatus,
) -> Vec<(String, usize)> {
    order
        .iter()
        .enumerate()
        .filter(|(_, id)| statuses.iter().any(|(s, st)| s == *id && *st == wanted))
        .map(|(rank, id)| (id.clone(), rank))
        .collect()
}

/// The ids of every live row whose status matches `wanted`, in queue order.
fn ids_with_status(
    order: &[String],
    statuses: &[(String, UserPromptQueueStatus)],
    wanted: UserPromptQueueStatus,
) -> Vec<String> {
    order
        .iter()
        .filter(|id| statuses.iter().any(|(s, st)| s == *id && *st == wanted))
        .cloned()
        .collect()
}

/// `true` for the `UNIQUE` failure raised by the displacement step itself.
///
/// This is a **pre-existing** defect that C001's property test found, not one
/// C001 introduced: the displacement `UPDATE` is byte-identical before and after
/// this milestone, and the collision happens inside that statement, before the
/// merge is ever consulted. A uniform offset is only collision-free while no row
/// already sits at or above the band, so once a terminal row has been displaced
/// into the band by an earlier renumber, the next one walks live rows straight
/// into it.
///
/// The first such step is pinned below with the exact row state that triggers it,
/// so the finding is reproducible from this test rather than from a hand-built
/// shape — the collision depends on the physical row order, and a synthetic
/// fixture that looks equivalent does **not** reproduce it.
fn is_known_displacement_collision(error: &UserPromptQueueError) -> bool {
    format!("{error} {error:?}").contains("UNIQUE constraint failed")
}

/// Position preservation, over every interleaving of reorder / delete / claim /
/// release on a three-entry queue.
///
/// The property is stated as an equality of orders rather than as "the entry is
/// not last", because that is the stronger and more falsifiable form: a released
/// entry resumes at its own position **and nothing else moves**. The specific
/// case M005 recorded — release lands at the back — is one point in this space.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_prompt_queue_a_released_entry_resumes_in_place_under_every_interleaving() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool.clone());

    let mut sequences_run = 0usize;
    let mut collisions = 0usize;
    let mut first_collision: Option<CollisionRecord> = None;

    /// The session's rows as `(id, ordinal, status)`, ascending — the state a
    /// displaced-terminal-row collision is read against.
    async fn rows_of(pool: &SqlitePool) -> Vec<QueueRow> {
        sqlx::query_as(
            "SELECT id, ordinal, status FROM user_prompt_queue_entry \
             WHERE session_id = ? ORDER BY ordinal ASC",
        )
        .bind(SESSION_ID)
        .fetch_all(pool)
        .await
        .expect("read rows")
    }
    for length in 1..=4usize {
        for mut code in 0usize..6usize.pow(length as u32) {
            // One pool, one migration, reset between sequences: the property is
            // about the store's transitions, not about re-running migrations.
            sqlx::query("DELETE FROM user_prompt_queue_entry")
                .execute(&pool)
                .await
                .expect("clear entries");
            sqlx::query("DELETE FROM user_prompt_queue_owner")
                .execute(&pool)
                .await
                .expect("clear owner");
            for id in PROPERTY_ENTRIES {
                store
                    .add(OWNER, SESSION_ID, id, id, None, T0)
                    .await
                    .expect("seed");
            }

            let mut sequence = Vec::with_capacity(length);
            for _ in 0..length {
                sequence.push(QUEUE_OPS[code % 6]);
                code /= 6;
            }
            let context = format!("{sequence:?}");

            for (step, op) in sequence.iter().enumerate() {
                let now = T0 + 10 + (step as i64 * 4) + (sequences_run as i64 % 4);
                assert_step_invariants(&store, &pool, now, &context).await;

                match op {
                    QueueOp::ReversePending => {
                        let (order, statuses) = live_snapshot(&store, now).await;
                        let mut reversed =
                            ids_with_status(&order, &statuses, UserPromptQueueStatus::Pending);
                        reversed.reverse();

                        // P4 — a `delivering` row's rank is invariant under a
                        // reorder. This is the merge rule stated as a property
                        // over the whole live set rather than as one example.
                        let before = ranks_of(&order, &statuses, UserPromptQueueStatus::Delivering);
                        match store.reorder(OWNER, SESSION_ID, &reversed, now).await {
                            Ok(_) => {
                                let (after_order, after_statuses) =
                                    live_snapshot(&store, now).await;
                                let after = ranks_of(
                                    &after_order,
                                    &after_statuses,
                                    UserPromptQueueStatus::Delivering,
                                );
                                assert_eq!(
                                    before, after,
                                    "{context}: a reorder moved a delivering row's rank"
                                );
                            }
                            // The pre-existing displacement collision, counted
                            // and pinned rather than hidden.
                            Err(error) if is_known_displacement_collision(&error) => {
                                collisions += 1;
                                if first_collision.is_none() {
                                    first_collision =
                                        Some((format!("{context} @ {op:?}"), rows_of(&pool).await));
                                }
                            }
                            Err(error) => panic!("{context}: reorder: {error}"),
                        }
                    }
                    QueueOp::DeleteHead => {
                        let (order, statuses) = live_snapshot(&store, now).await;
                        let pending =
                            ids_with_status(&order, &statuses, UserPromptQueueStatus::Pending);
                        if let Some(head) = pending.first() {
                            match store.delete(OWNER, SESSION_ID, head, now).await {
                                Ok(_) => {}
                                Err(error) if is_known_displacement_collision(&error) => {
                                    collisions += 1;
                                    if first_collision.is_none() {
                                        first_collision = Some((
                                            format!("{context} @ {op:?}"),
                                            rows_of(&pool).await,
                                        ));
                                    }
                                }
                                Err(error) => panic!("{context}: delete: {error}"),
                            }
                        }
                    }
                    QueueOp::ClaimRelease => {
                        let claim = store
                            .claim_next_for_delivery(SESSION_ID, now)
                            .await
                            .unwrap_or_else(|error| panic!("{context}: claim: {error}"));
                        let Some(claim) = claim else { continue };
                        let before = live_snapshot(&store, now).await.0;

                        assert!(
                            store
                                .release_delivery(&claim.entry.id, now + 1)
                                .await
                                .unwrap_or_else(|error| panic!("{context}: release: {error}")),
                            "{context}: the release must apply"
                        );

                        // P3 — the release is order-neutral. The entry was
                        // already inside the band, so restoring `pending` moves
                        // nothing: not the entry, and not the rows around it.
                        let after = live_snapshot(&store, now + 1).await.0;
                        assert_eq!(
                            before, after,
                            "{context}: releasing {} moved the queue; a released \
                             entry must resume at its own position and disturb \
                             nothing else",
                            claim.entry.id
                        );
                    }
                    QueueOp::ClaimHold => {
                        // Claim and leave the row `delivering`, so a later step
                        // can straddle it.
                        let _ = store
                            .claim_next_for_delivery(SESSION_ID, now)
                            .await
                            .unwrap_or_else(|error| panic!("{context}: claim: {error}"));
                    }
                    QueueOp::ClaimFail => {
                        if let Some(claim) = store
                            .claim_next_for_delivery(SESSION_ID, now)
                            .await
                            .unwrap_or_else(|error| panic!("{context}: claim: {error}"))
                        {
                            assert!(store
                                .mark_delivery_failed(&claim.entry.id, "transient_refusal", now + 1)
                                .await
                                .unwrap_or_else(|error| panic!("{context}: fail: {error}")));
                        }
                    }
                    QueueOp::ClaimDeliver => {
                        if let Some(claim) = store
                            .claim_next_for_delivery(SESSION_ID, now)
                            .await
                            .unwrap_or_else(|error| panic!("{context}: claim: {error}"))
                        {
                            assert!(store
                                .mark_delivered(&claim.entry.id, "turn-x", now + 1)
                                .await
                                .unwrap_or_else(|error| panic!("{context}: deliver: {error}")));
                        }
                    }
                }
            }

            assert_step_invariants(&store, &pool, T0 + 100, &context).await;
            sequences_run += 1;
        }
    }

    assert_eq!(
        sequences_run,
        6 + 36 + 216 + 1296,
        "every sequence of length 1..=4 over six operations must be covered"
    );

    // The measured blast radius of the pre-existing displacement collision. It
    // is asserted as an exact number rather than merely counted so that fixing
    // the collision is what turns this red — a fix that drops the count to zero
    // must be made deliberately, in the same edit that removes the skip above.
    assert_eq!(
        collisions, COLLISION_STEPS,
        "the measured number of steps that hit the pre-existing displacement \
         collision changed"
    );

    // And the first triggering state, pinned so the finding is reproducible
    // from this test rather than from a guess. C001 §14 stops the milestone on
    // exactly this: "a second displacement appears to be needed to keep
    // `UNIQUE`; stop".
    let (context, rows) = first_collision.expect(
        "the enumeration must still reach the pre-existing collision; if \
                 it does not, the count above changed first",
    );
    eprintln!("first displacement collision at {context}\n  rows: {rows:?}");
    assert_eq!(
        context, FIRST_COLLISION_CONTEXT,
        "the first colliding sequence changed"
    );
    assert_eq!(
        rows,
        first_collision_rows(),
        "the row state that triggers the displacement collision changed"
    );
}
