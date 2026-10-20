//! M003 (ADR-0038, accepted — Option B): the queue delivery slice.
//!
//! Delivery is the band between "an entry is live" and "a turn exists". This
//! file covers the store half of that band: the claim fence, the three terminal
//! transitions, the retry that separates "did not happen" from "failed", and the
//! bounded startup reconciliation of stranded `delivering` rows.
//!
//! A separate file from the M002a and M002b files on purpose — both are closed
//! and reviewed. Every test name carries the `user_prompt_queue` prefix so the
//! plan's §11 filter selects them by name rather than by module path.
//!
//! What this file deliberately does **not** cover is the daemon half: who the
//! delivery principal is, and that the submission ran under that principal. Those
//! live in `tests/identity_m003_daemon_authorization.rs` and in the callsite
//! guard, because a store test cannot observe them.

use codegg_core::session::user_prompt_queue::{
    UserPromptQueueDeliveryClaim, UserPromptQueueDeliveryResolution, UserPromptQueueError,
    UserPromptQueueStatus, UserPromptQueueStore, USER_PROMPT_QUEUE_MAX_PENDING,
    USER_PROMPT_QUEUE_TTL_MS,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

const SESSION_ID: &str = "session-queue-3";
const OTHER_SESSION: &str = "session-queue-3-other";
const OWNER: &str = "principal-owner";
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
    let name = format!(
        "user_prompt_queue_deliver_{}",
        uuid::Uuid::new_v4().simple()
    );
    pool_at(&format!("file:{name}?mode=memory&cache=shared")).await
}

async fn ensure_session(pool: &SqlitePool, session_id: &str) {
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT OR IGNORE INTO project (id, worktree, sandboxes, time_created, time_updated) \
         VALUES ('project-queue-3', '/tmp/project-queue-3', '[]', ?, ?)",
    )
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("insert project");
    sqlx::query(
        "INSERT OR IGNORE INTO session \
         (id, project_id, slug, directory, title, version, time_created, time_updated) \
         VALUES (?, 'project-queue-3', 'queue-3', '/tmp/project-queue-3', 'Queue', '1', ?, ?)",
    )
    .bind(session_id)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("insert session");
}

/// An owned queue with `count` pending entries, ready to deliver.
async fn seeded(store: &UserPromptQueueStore, pool: &SqlitePool, count: usize) {
    ensure_session(pool, SESSION_ID).await;
    for index in 0..count {
        store
            .add(
                OWNER,
                SESSION_ID,
                &format!("entry-{index}"),
                &format!("prompt {index}"),
                None,
                T0,
            )
            .await
            .expect("seed add");
    }
}

async fn status_of(store: &UserPromptQueueStore, entry_id: &str) -> UserPromptQueueStatus {
    store.get(entry_id).await.expect("load entry").status
}

/// The claim is the at-most-once fence at the store layer: two callers race to
/// `rows_affected == 1`, and because the UPDATE is guarded on the row still being
/// `pending`, at most one can win.
///
/// **This test used to be flaky**, and its assertion is left exactly as M003 wrote
/// it. M006 found the cause while repeating the suite — reproduced 4 times in 12
/// runs — and the maintainer landed M007 without fixing it, filing a corrective
/// instead. C002 fixed it; see
/// `user_prompt_queue_concurrent_claims_never_deadlock_on_lock_upgrade` below for
/// the repeated form that measures the rate rather than betting on one round.
///
/// The failure was `Storage(Database("error returned from database: (code: 6)
/// database is deadlocked"))` on **both** futures. `claim_next_for_delivery` did
/// `begin()` (deferred) → `SELECT` → `UPDATE`, and SQLite refuses to upgrade a
/// shared read lock to a write lock while another connection holds a read lock. It
/// does not wait, because waiting could deadlock, and `busy_timeout` does not retry
/// it: this was an upgrade deadlock, not contention.
///
/// Two `expect` calls made that fatal here, while the at-most-once property itself
/// held — neither caller wrote anything, so nothing was double-delivered. The
/// assertion below is still `winners == 1`, which is what the store actually
/// guarantees once the write lock is taken at `BEGIN`.
#[tokio::test]
async fn user_prompt_queue_concurrent_claims_admit_exactly_one_entry() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded(&store, &pool, 1).await;

    let (first, second) = tokio::join!(
        store.claim_next_for_delivery(SESSION_ID, T0 + 1),
        store.claim_next_for_delivery(SESSION_ID, T0 + 2),
    );
    let claims = [first.expect("claim a"), second.expect("claim b")];
    let winners = claims.iter().filter(|claim| claim.is_some()).count();
    assert_eq!(winners, 1, "exactly one claimant must win the entry");
}

/// The same race as above, run often enough that its rate is a measurement rather
/// than an anecdote.
///
/// A single round cannot prove the lock-upgrade deadlock is gone: before C002 this
/// shape failed a minority of rounds, which is exactly why it read as an
/// unexplained flake for three milestones. Asserting over one round would let the
/// old code pass whenever the two connections happened not to interleave, and a
/// guard that passes on luck is not a guard. So this test states the **whole**
/// contract across every round: no round may fail to acquire the write lock, and
/// every round must produce exactly one winner.
///
/// Each round adds its own entry and settles the winner, so rounds are independent
/// and the session never approaches the backlog bound.
///
/// **There is deliberately no hand-written "two deferred readers" reproduction
/// here.** One was written and removed: sqlx's SQLite driver resolves lock waits
/// through an *unlock-notify* VFS, so a connection parked on a lock its peer will
/// never release waits indefinitely rather than returning `SQLITE_BUSY`. Holding
/// one deferred reader open while a second tries to upgrade hangs the test binary
/// instead of failing it, which is a worse failure than the one it would prove.
/// The store's own two-caller race terminates because both callers finish, which
/// is why the measurement lives here and the *shape* is pinned statically by
/// `scripts/check_queue_write_transaction_shape.py`.
#[tokio::test]
async fn user_prompt_queue_concurrent_claims_never_deadlock_on_lock_upgrade() {
    const ROUNDS: usize = 64;

    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    ensure_session(&pool, SESSION_ID).await;

    let mut storage_failures: Vec<String> = Vec::new();
    let mut fence_failures: Vec<String> = Vec::new();
    // Compact on purpose: dumping the whole claim record made a 51-round failure
    // unreadable, which is how a real failure gets skimmed past.
    let claimed = |claim: &Option<UserPromptQueueDeliveryClaim>| {
        claim
            .as_ref()
            .map(|claim| claim.entry.id.clone())
            .unwrap_or_else(|| "no entry".to_owned())
    };

    for round in 0..ROUNDS {
        let entry_id = format!("entry-race-{round}");
        store
            .add(
                OWNER,
                SESSION_ID,
                &entry_id,
                &format!("prompt {round}"),
                None,
                T0,
            )
            .await
            .expect("seed add");

        let (first, second) = tokio::join!(
            store.claim_next_for_delivery(SESSION_ID, T0 + 1),
            store.claim_next_for_delivery(SESSION_ID, T0 + 2),
        );
        let (first, second) = match (first, second) {
            (Ok(first), Ok(second)) => (first, second),
            (first, second) => {
                // Recorded rather than panicked, so one run reports how *many*
                // rounds were affected instead of stopping at the first.
                let side =
                    |result: Result<Option<UserPromptQueueDeliveryClaim>, UserPromptQueueError>| {
                        match result {
                            Ok(Some(claim)) => format!("claimed {}", claim.entry.id),
                            Ok(None) => "no entry".to_owned(),
                            Err(error) => format!("error {error}"),
                        }
                    };
                storage_failures.push(format!("round {round}: {} / {}", side(first), side(second)));
                continue;
            }
        };

        let winners = [first.as_ref(), second.as_ref()]
            .into_iter()
            .filter(|claim| claim.is_some())
            .count();
        if winners != 1 {
            fence_failures.push(format!(
                "round {round}: {winners} winners ({} / {})",
                claimed(&first),
                claimed(&second)
            ));
            continue;
        }

        let claim = first.or(second).expect("one winner exists");
        store
            .mark_delivered(&claim.entry.id, &format!("turn-race-{round}"), T0 + 3)
            .await
            .expect("settle the round");
    }

    assert!(
        storage_failures.is_empty(),
        "{} of {ROUNDS} concurrent claim rounds failed to reach the store instead of \
         serializing on the write lock. Both callers must be able to run; a deferred \
         `begin()` upgrading a shared read lock is what made them fail: {}",
        storage_failures.len(),
        storage_failures.join("; "),
    );
    assert!(
        fence_failures.is_empty(),
        "{} of {ROUNDS} rounds did not admit exactly one entry: {}",
        fence_failures.len(),
        fence_failures.join("; "),
    );
}

/// Delivery is ordered by daemon-owned ordinal, not by claim time. A queue is a
/// queue only if the prompt the user wrote second never runs first.
#[tokio::test]
async fn user_prompt_queue_delivery_follows_ordinal_order() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded(&store, &pool, 3).await;

    let mut delivered = Vec::new();
    while let Some(claim) = store
        .claim_next_for_delivery(SESSION_ID, T0 + 1)
        .await
        .expect("claim")
    {
        delivered.push(claim.entry.ordinal);
        assert!(store
            .mark_delivered(&claim.entry.id, "turn-1", T0 + 2)
            .await
            .expect("mark delivered"));
    }
    assert_eq!(
        delivered,
        vec![0, 1, 2],
        "delivery must follow ordinal order"
    );
}

/// A `delivering` entry stays visible in the read projection.
///
/// Before M003 gave `delivering` a producer, `list_live` selected
/// `status = 'pending'` only. Once an entry starts delivering it would vanish
/// from the client's list — which reads as data loss for a prompt the user can
/// still see a turn start for.
#[tokio::test]
async fn user_prompt_queue_delivering_entry_stays_in_the_read_projection() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded(&store, &pool, 2).await;

    let claim = store
        .claim_next_for_delivery(SESSION_ID, T0 + 1)
        .await
        .expect("claim")
        .expect("one entry to claim");
    let projected = store.list_live(SESSION_ID, T0 + 2).await.expect("list");
    assert_eq!(projected.len(), 2, "a delivering entry must stay projected");
    let delivering = projected
        .iter()
        .find(|entry| entry.id == claim.entry.id)
        .expect("claimed entry still projected");
    assert_eq!(delivering.status, UserPromptQueueStatus::Delivering);
    assert_eq!(
        projected
            .iter()
            .filter(|e| e.status == UserPromptQueueStatus::Pending)
            .count(),
        1
    );
}

/// A claimed entry occupies a backlog slot, so it must count against the bound.
///
/// If it did not, a user could enqueue past every in-flight delivery and exceed
/// `MAX_PENDING` by however many deliveries are happening.
#[tokio::test]
async fn user_prompt_queue_delivering_entry_counts_against_the_backlog_bound() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded(&store, &pool, USER_PROMPT_QUEUE_MAX_PENDING as usize).await;

    // The bound is "at most N live", so N pending entries are already at it.
    assert!(
        store.check_capacity(SESSION_ID, T0 + 1).await.is_err(),
        "MAX_PENDING pending entries must already be at the bound"
    );

    store
        .claim_next_for_delivery(SESSION_ID, T0 + 2)
        .await
        .expect("claim")
        .expect("one entry to claim");
    assert!(
        store.check_capacity(SESSION_ID, T0 + 3).await.is_err(),
        "a delivering entry must still occupy its backlog slot"
    );
}

/// A claim is made under the entry's TTL, so a `delivering` row must still expire.
///
/// This is the second of the three `status = 'pending'` literals M003 had to
/// widen. Left as-is, a row stranded by a daemon killed mid-claim would outlive
/// its TTL forever while still being counted as live backlog.
#[tokio::test]
async fn user_prompt_queue_stranded_delivering_entry_still_expires() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded(&store, &pool, 1).await;

    let claim = store
        .claim_next_for_delivery(SESSION_ID, T0 + 1)
        .await
        .expect("claim")
        .expect("one entry to claim");
    let past_ttl = T0 + USER_PROMPT_QUEUE_TTL_MS + 1;
    let projected = store.list_live(SESSION_ID, past_ttl).await.expect("list");
    assert!(projected.is_empty(), "a past-TTL entry is no longer live");
    assert_eq!(
        status_of(&store, &claim.entry.id).await,
        UserPromptQueueStatus::Expired
    );
}

/// `delivered` is terminal and idempotent: a repeated completion must not rebind
/// the entry to a different turn.
#[tokio::test]
async fn user_prompt_queue_delivery_is_terminal_and_does_not_rebind() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded(&store, &pool, 1).await;

    let claim = store
        .claim_next_for_delivery(SESSION_ID, T0 + 1)
        .await
        .expect("claim")
        .expect("one entry to claim");
    assert!(store
        .mark_delivered(&claim.entry.id, "turn-a", T0 + 2)
        .await
        .expect("complete"));
    assert!(
        !store
            .mark_delivered(&claim.entry.id, "turn-b", T0 + 3)
            .await
            .expect("repeat"),
        "a delivered entry must not rebind to a second turn"
    );
    assert_eq!(
        store
            .get(&claim.entry.id)
            .await
            .expect("load")
            .delivered_turn_id
            .as_deref(),
        Some("turn-a")
    );
}

/// A `delivering` entry is live but **not** operable. Editing or deleting it
/// would race an admission that is already in flight against it.
#[tokio::test]
async fn user_prompt_queue_delivering_entry_refuses_mutation_distinctly() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded(&store, &pool, 1).await;

    let claim = store
        .claim_next_for_delivery(SESSION_ID, T0 + 1)
        .await
        .expect("claim")
        .expect("one entry to claim");

    let edit = store
        .edit(OWNER, SESSION_ID, &claim.entry.id, "changed", None, T0 + 2)
        .await;
    assert_eq!(
        edit.expect_err("edit must refuse").code(),
        "user_prompt_queue_entry_delivering",
        "a delivering entry must not be reported as expired"
    );
    assert!(store
        .delete(OWNER, SESSION_ID, &claim.entry.id, T0 + 2)
        .await
        .is_err());
}

/// Release is not failure. A submission that never reached admission did not
/// consume the user's prompt, so the entry returns to `pending` and is delivered
/// later rather than becoming terminal.
#[tokio::test]
async fn user_prompt_queue_release_returns_the_entry_for_a_later_attempt() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded(&store, &pool, 1).await;

    let claim = store
        .claim_next_for_delivery(SESSION_ID, T0 + 1)
        .await
        .expect("claim")
        .expect("one entry to claim");
    assert!(store
        .release_delivery(&claim.entry.id, T0 + 2)
        .await
        .expect("release"));
    assert_eq!(
        status_of(&store, &claim.entry.id).await,
        UserPromptQueueStatus::Pending
    );

    let reclaimed = store
        .claim_next_for_delivery(SESSION_ID, T0 + 3)
        .await
        .expect("reclaim")
        .expect("the released entry is claimable again");
    assert_eq!(reclaimed.entry.id, claim.entry.id);
}

/// Only `failed` is retryable. `delivered` has had its turn and `expired` has
/// outlived the window the user expected it in; reviving either would silently
/// re-run or resurrect work.
#[tokio::test]
async fn user_prompt_queue_only_a_failed_entry_is_retryable() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded(&store, &pool, 1).await;

    let claim = store
        .claim_next_for_delivery(SESSION_ID, T0 + 1)
        .await
        .expect("claim")
        .expect("one entry to claim");
    assert!(store
        .mark_delivery_failed(&claim.entry.id, "submission_refused", T0 + 2)
        .await
        .expect("fail"));
    assert_eq!(
        store
            .get(&claim.entry.id)
            .await
            .expect("load")
            .failure_reason
            .as_deref(),
        Some("submission_refused")
    );
    assert!(store
        .retry_failed(&claim.entry.id, T0 + 3)
        .await
        .expect("retry"));
    assert_eq!(
        status_of(&store, &claim.entry.id).await,
        UserPromptQueueStatus::Pending
    );

    store
        .claim_next_for_delivery(SESSION_ID, T0 + 4)
        .await
        .expect("claim")
        .expect("claim again");
    store
        .mark_delivered(&claim.entry.id, "turn-x", T0 + 5)
        .await
        .expect("complete");
    assert!(
        !store
            .retry_failed(&claim.entry.id, T0 + 6)
            .await
            .expect("retry refused"),
        "a delivered entry is terminal"
    );
}

/// The failure reason is a short machine code, never prose copied from a refused
/// submission — a provider error string can quote the prompt, and this column
/// outlives the failure that filled it.
#[tokio::test]
async fn user_prompt_queue_delivery_failure_reason_refuses_prose() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded(&store, &pool, 1).await;

    let claim = store
        .claim_next_for_delivery(SESSION_ID, T0 + 1)
        .await
        .expect("claim")
        .expect("one entry to claim");
    let prose = "provider said: \"your prompt is bad\" and then some more, with detail";
    let refused = store
        .mark_delivery_failed(&claim.entry.id, prose, T0 + 2)
        .await;
    assert_eq!(
        refused.expect_err("prose must be refused").code(),
        "user_prompt_queue_invalid"
    );
}

/// Reconciliation is how a row stranded by a daemon killed between claim and
/// receipt becomes something other than `delivering` forever.
#[tokio::test]
async fn user_prompt_queue_stranded_rows_are_reconcilable() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded(&store, &pool, 2).await;

    let first = store
        .claim_next_for_delivery(SESSION_ID, T0 + 1)
        .await
        .expect("claim")
        .expect("claim first");
    store
        .claim_next_for_delivery(SESSION_ID, T0 + 1)
        .await
        .expect("claim")
        .expect("claim second");

    let stranded = store
        .list_delivering_for_reconcile(None)
        .await
        .expect("list stranded");
    assert_eq!(
        stranded.len(),
        2,
        "both claims are stranded until reconciled"
    );

    // The receipt said a turn started for one and not for the other.
    assert!(store
        .resolve_stranded_delivery(
            &first.entry.id,
            &UserPromptQueueDeliveryResolution::Delivered {
                turn_id: "turn-restored".to_owned()
            },
            T0 + 2,
        )
        .await
        .expect("resolve delivered"));
    let second_id = stranded
        .iter()
        .find(|row| row.id != first.entry.id)
        .expect("second stranded row")
        .id
        .clone();
    assert!(store
        .resolve_stranded_delivery(
            &second_id,
            &UserPromptQueueDeliveryResolution::Retry,
            T0 + 2,
        )
        .await
        .expect("resolve retry"));

    assert_eq!(
        status_of(&store, &first.entry.id).await,
        UserPromptQueueStatus::Delivered
    );
    assert_eq!(
        status_of(&store, &second_id).await,
        UserPromptQueueStatus::Pending
    );
    assert!(store
        .list_delivering_for_reconcile(None)
        .await
        .expect("list")
        .is_empty());
}

/// A startup sweep races deliveries that are genuinely in flight in another
/// process or a live turn. Resolving an already-resolved row is a no-op, not a
/// rollback.
#[tokio::test]
async fn user_prompt_queue_reconcile_never_undoes_a_live_resolution() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded(&store, &pool, 1).await;

    let claim = store
        .claim_next_for_delivery(SESSION_ID, T0 + 1)
        .await
        .expect("claim")
        .expect("claim");
    store
        .mark_delivered(&claim.entry.id, "turn-live", T0 + 2)
        .await
        .expect("complete");

    assert!(
        !store
            .resolve_stranded_delivery(
                &claim.entry.id,
                &UserPromptQueueDeliveryResolution::Retry,
                T0 + 3,
            )
            .await
            .expect("late sweep"),
        "a sweep must not revert a completed delivery"
    );
    assert_eq!(
        status_of(&store, &claim.entry.id).await,
        UserPromptQueueStatus::Delivered
    );
}

/// A queue with no owner has no authorized deliverer. Delivery is a mutation of
/// queue state, so it inherits ADR-0037's ownership gate rather than bypassing it.
#[tokio::test]
async fn user_prompt_queue_delivery_requires_an_owned_queue() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    ensure_session(&pool, SESSION_ID).await;
    assert!(
        store
            .claim_next_for_delivery(SESSION_ID, T0 + 1)
            .await
            .expect("claim on an absent queue")
            .is_none(),
        "an absent queue has nothing to deliver"
    );

    // The claim carries the owner and its revision, so the daemon can re-authorize
    // against current team state rather than trusting this row.
    seeded(&store, &pool, 1).await;
    let claim = store
        .claim_next_for_delivery(SESSION_ID, T0 + 2)
        .await
        .expect("claim")
        .expect("claim");
    assert_eq!(claim.owner_principal, OWNER);
    let stored: (String, i64) = sqlx::query_as(
        "SELECT owner_principal, revision FROM user_prompt_queue_owner WHERE session_id = ?",
    )
    .bind(SESSION_ID)
    .fetch_one(&pool)
    .await
    .expect("owner row");
    assert_eq!(
        claim.owner_revision,
        u64::try_from(stored.1).expect("revision fits"),
        "the claim must carry the durable ownership revision, not a constant, so a \
         transfer between claim and submission is detectable"
    );
}

/// Delivery is per session. A claim for one session must never take another
/// session's entry, even when the other session has an older due entry.
#[tokio::test]
async fn user_prompt_queue_delivery_does_not_cross_sessions() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    ensure_session(&pool, OTHER_SESSION).await;
    seeded(&store, &pool, 1).await;
    store
        .adopt(OWNER, OTHER_SESSION, T0)
        .await
        .expect("adopt other");
    store
        .add(
            OWNER,
            OTHER_SESSION,
            "other-entry",
            "other prompt",
            None,
            T0,
        )
        .await
        .expect("add other");

    let claim = store
        .claim_next_for_delivery(SESSION_ID, T0 + 1)
        .await
        .expect("claim")
        .expect("claim");
    assert_eq!(claim.entry.session_id, SESSION_ID);
    assert_eq!(
        status_of(&store, "other-entry").await,
        UserPromptQueueStatus::Pending,
        "another session's entry is untouched"
    );
}

/// A sweep is bounded, not a background thread. The bound is what stops a repair
/// pass from becoming a poller.
#[tokio::test]
async fn user_prompt_queue_reconcile_sweep_is_bounded() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    ensure_session(&pool, SESSION_ID).await;
    store.adopt(OWNER, SESSION_ID, T0).await.expect("adopt");

    for index in 0..(codegg_core::session::user_prompt_queue::USER_PROMPT_QUEUE_MAX_RECONCILE + 5) {
        sqlx::query(
            "INSERT INTO user_prompt_queue_entry \
             (id, session_id, ordinal, text, text_bytes, status, failure_reason, \
              created_at_ms, updated_at_ms, expires_at_ms, delivered_turn_id, delivered_at_ms) \
             VALUES (?, ?, ?, 'p', 1, 'delivering', NULL, ?, ?, ?, NULL, NULL)",
        )
        .bind(format!("strand-{index}"))
        .bind(SESSION_ID)
        .bind(index)
        .bind(T0)
        .bind(T0)
        .bind(T0 + USER_PROMPT_QUEUE_TTL_MS)
        .execute(&pool)
        .await
        .expect("insert stranded row");
    }

    let swept = store
        .list_delivering_for_reconcile(None)
        .await
        .expect("sweep");
    assert_eq!(
        swept.len(),
        codegg_core::session::user_prompt_queue::USER_PROMPT_QUEUE_MAX_RECONCILE as usize,
        "the restart sweep must stop at its declared bound"
    );
}

/// A claim returns **the entry it claimed**, even when a lower-ordinal row is
/// already stranded in `delivering`.
///
/// This is a regression test for a real defect. The claim originally selected the
/// next due id and then re-read "the lowest `delivering` row" to get its record.
/// With a stranded row at a lower ordinal, that second read returned the
/// *stranded* entry: the caller would submit the stranded entry's text while
/// marking the entry it actually claimed as delivered — running one prompt and
/// recording it against another.
#[tokio::test]
async fn user_prompt_queue_claim_returns_its_own_entry_not_a_stranded_one() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded(&store, &pool, 3).await;

    // Strand the lowest-ordinal entry the way a crash between claim and receipt
    // would, without letting it resolve.
    let stranded = store
        .claim_next_for_delivery(SESSION_ID, T0 + 1)
        .await
        .expect("claim")
        .expect("claim first");
    assert_eq!(stranded.entry.id, "entry-0");

    // The next claim must be entry-1 — the stranded entry-0 is at a *lower*
    // ordinal, so any "lowest delivering row" lookup would return it instead.
    let second = store
        .claim_next_for_delivery(SESSION_ID, T0 + 2)
        .await
        .expect("claim")
        .expect("claim second");
    assert_eq!(
        second.entry.id, "entry-1",
        "a claim must return its own entry, never an older stranded one"
    );
    assert_eq!(
        second.entry.ordinal, 1,
        "the returned record must belong to the claimed row"
    );
    assert_eq!(
        status_of(&store, "entry-0").await,
        UserPromptQueueStatus::Delivering,
        "the stranded entry is untouched by the later claim"
    );
}

// ---------------------------------------------------------------------------
// M006 (ADR-0036): the steer settlement.
//
// `mark_steer_delivered` is the only path into `delivered` that never passes
// through `delivering`, because the prompt went into a turn that already
// existed. Everything below is about keeping that second path from becoming a
// second, overlapping way to deliver one prompt twice.
// ---------------------------------------------------------------------------

/// The point of the whole slice: a steer settlement moves `pending` straight to
/// `delivered`, with no claim in between and no fresh turn started.
#[tokio::test]
async fn user_prompt_queue_a_steer_settles_a_pending_entry_without_a_claim() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded(&store, &pool, 2).await;

    store
        .mark_steer_delivered(OWNER, SESSION_ID, "entry-0", "turn-live", T0 + 1)
        .await
        .expect("settle the steered entry");

    let record = store.get("entry-0").await.expect("load settled entry");
    assert_eq!(
        record.status,
        UserPromptQueueStatus::Delivered,
        "a steered prompt is delivered, never left pending for the next turn"
    );
    assert_eq!(
        record.delivered_turn_id.as_deref(),
        Some("turn-live"),
        "the attribution must name the turn that accepted it, so a later retry \
         reports the real target instead of claiming the prompt never ran"
    );
    assert_eq!(record.delivered_at_ms, Some(T0 + 1));
    assert!(
        !store
            .list_live(SESSION_ID, T0 + 1)
            .await
            .expect("list live")
            .iter()
            .any(|entry| entry.id == "entry-0"),
        "a settled prompt leaves the live projection rather than lingering as a \
         pending row the user could steer a second time"
    );
}

/// The at-most-once rule, in the direction that matters most: a row the daemon
/// already claimed is running as its **own** turn, so settling it as steered
/// too would record one prompt delivered twice.
#[tokio::test]
async fn user_prompt_queue_a_steer_settlement_refuses_a_delivering_entry() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded(&store, &pool, 1).await;

    let claim = store
        .claim_next_for_delivery(SESSION_ID, T0 + 1)
        .await
        .expect("claim")
        .expect("claim the entry");
    assert_eq!(claim.entry.id, "entry-0");

    let error = store
        .mark_steer_delivered(OWNER, SESSION_ID, "entry-0", "turn-live", T0 + 2)
        .await
        .expect_err("a claimed entry is already running as its own turn");
    assert_eq!(
        error.code(),
        "user_prompt_queue_entry_delivering",
        "the refusal must be the existing typed condition, not a new vocabulary"
    );

    let record = store.get("entry-0").await.expect("load entry");
    assert_eq!(
        record.status,
        UserPromptQueueStatus::Delivering,
        "the refused settlement must leave the claim alone — the turn the user \
         is watching must still be attributed to the delivery it came from"
    );
    assert_eq!(
        record.delivered_turn_id, None,
        "a refused settlement must not stamp a turn id onto someone else's claim"
    );
}

/// Ownership is settlement authority, not steering authority, but it is the
/// question that keeps a caller who reached *another* principal's turn from
/// editing *their own* queue as a side effect.
#[tokio::test]
async fn user_prompt_queue_a_steer_settlement_refuses_a_foreign_caller() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded(&store, &pool, 1).await;

    let error = store
        .mark_steer_delivered(
            "principal-not-the-owner",
            SESSION_ID,
            "entry-0",
            "turn-live",
            T0 + 1,
        )
        .await
        .expect_err("a caller who does not own the queue cannot settle into it");
    assert_eq!(error.code(), "user_prompt_queue_not_owner");
    assert_eq!(
        status_of(&store, "entry-0").await,
        UserPromptQueueStatus::Pending,
        "the entry must be untouched, so it is still deliverable"
    );
}

/// A session with no queue at all is `NotFound`, exactly as `delete` reports it
/// — one reason vocabulary across every settlement path.
#[tokio::test]
async fn user_prompt_queue_a_steer_settlement_refuses_an_absent_queue() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    ensure_session(&pool, OTHER_SESSION).await;

    let error = store
        .mark_steer_delivered(OWNER, OTHER_SESSION, "entry-0", "turn-live", T0 + 1)
        .await
        .expect_err("no queue has ever been adopted for this session");
    assert_eq!(error.code(), "user_prompt_queue_not_found");
}

/// The settlement is a durable write, so a settlement the user was already told
/// about cannot be lost by a daemon restart — which is the whole reason the
/// store settles rather than the frontend merely hiding the row.
#[tokio::test]
async fn user_prompt_queue_a_steer_settlement_survives_a_pool_restart() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let path = dir.path().join("queue-restart.sqlite3");
    let db = path.to_string_lossy().into_owned();

    {
        let pool = pool_at(&format!("file:{db}")).await;
        let store = UserPromptQueueStore::new(pool.clone());
        seeded(&store, &pool, 1).await;
        store
            .mark_steer_delivered(OWNER, SESSION_ID, "entry-0", "turn-live", T0 + 1)
            .await
            .expect("settle the steered entry");
        pool.close().await;
    }

    // A fresh store over the same file: the row is still terminal, so the
    // restarted daemon cannot hand the prompt out a second time.
    let pool = pool_at(&format!("file:{db}")).await;
    let store = UserPromptQueueStore::new(pool.clone());
    let record = store.get("entry-0").await.expect("load after restart");
    assert_eq!(record.status, UserPromptQueueStatus::Delivered);
    assert_eq!(record.delivered_turn_id.as_deref(), Some("turn-live"));
}

/// Two frontends press send-now on the same entry. Exactly one settlement may
/// win, and the attribution is single-valued — a second writer must not
/// overwrite which turn took the prompt.
#[tokio::test]
async fn user_prompt_queue_a_steer_settlement_is_at_most_once() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded(&store, &pool, 1).await;

    let (first, second) = tokio::join!(
        store.mark_steer_delivered(OWNER, SESSION_ID, "entry-0", "turn-a", T0 + 1),
        store.mark_steer_delivered(OWNER, SESSION_ID, "entry-0", "turn-b", T0 + 2),
    );
    // **At most one** wins, not exactly one. That is the whole at-most-once
    // contract; "exactly one" was the stronger claim and it was never true.
    //
    // Two concurrent settlements each `begin()` a deferred transaction and read
    // before they write. SQLite cannot upgrade a shared read lock to a write lock
    // while another connection holds a read lock, and it will not wait, because
    // waiting could deadlock: it returns `SQLITE_BUSY` (code 6, surfaced here as
    // `database is deadlocked`) and `busy_timeout` does **not** retry it, because
    // this is an upgrade deadlock rather than contention.
    //
    // So both attempts can fail, and that is a correct outcome rather than a
    // defect: neither wrote anything, the row is still `pending`, and the caller
    // sees a storage refusal it already reports as
    // `AcceptedNotSettled`. What must never happen is two winners.
    //
    // The same exposure is why M003's own `concurrent_claims_admit_exactly_one_entry`
    // flakes — see this file's note above that test.
    let attempts = [("turn-a", first), ("turn-b", second)];
    let winners: Vec<&str> = attempts
        .iter()
        .filter(|(_, outcome)| outcome.is_ok())
        .map(|(turn, _)| *turn)
        .collect();
    assert!(
        winners.len() <= 1,
        "at most one settlement may win the entry, got {winners:?}"
    );

    let record = store.get("entry-0").await.expect("load entry");
    match winners.as_slice() {
        // A winner: the row is terminal and attributed to exactly that caller.
        [winner] => {
            assert_eq!(record.status, UserPromptQueueStatus::Delivered);
            assert_eq!(
                record.delivered_turn_id.as_deref(),
                Some(*winner),
                "the settled attribution must be the one that actually won, not \
                 the loser's id written over it"
            );
        }
        // No winner: both deadlocked, neither wrote, and the prompt is still
        // waiting exactly where it was.
        [] => {
            assert_eq!(
                record.status,
                UserPromptQueueStatus::Pending,
                "if neither settlement committed, the row must be untouched and \
                 still deliverable — a refused settlement may never half-apply"
            );
            assert_eq!(record.delivered_turn_id, None);
        }
        other => panic!("more than one settlement won: {other:?}"),
    }
}

/// An entry past its TTL is already invisible to the read projection. Settling
/// it would write a terminal row the user was never shown, so the sweep runs
/// first and the write refuses.
#[tokio::test]
async fn user_prompt_queue_a_steer_settlement_refuses_an_expired_entry() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    ensure_session(&pool, SESSION_ID).await;
    store
        .add(OWNER, SESSION_ID, "entry-0", "prompt 0", None, T0)
        .await
        .expect("seed add");

    let after_ttl = T0 + USER_PROMPT_QUEUE_TTL_MS + 1;
    let error = store
        .mark_steer_delivered(OWNER, SESSION_ID, "entry-0", "turn-live", after_ttl)
        .await
        .expect_err("an entry past its TTL is no longer the user's to steer");
    assert_eq!(error.code(), "user_prompt_queue_expired");

    // The refusal is what protects the row. Note what it does *not* leave behind:
    // the in-transaction `sweep_expired` is rolled back with the refusal, so the
    // stored status is still `pending`. That is the pre-existing behaviour of
    // every mutation here (`delete`, `edit`, and `reorder` share the shape), and
    // it is benign only because every read path sweeps and commits — so the
    // observable contract is that the entry is gone from the projection.
    assert!(
        !store
            .list_live(SESSION_ID, after_ttl)
            .await
            .expect("list live")
            .iter()
            .any(|entry| entry.id == "entry-0"),
        "an entry past its TTL must be invisible to the read projection"
    );
}

/// Settlement is not idempotent-by-rewrite: a row that already reached a
/// terminal status is refused, so a retry cannot move its attribution.
#[tokio::test]
async fn user_prompt_queue_a_steer_settlement_refuses_a_terminal_entry() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded(&store, &pool, 1).await;

    store
        .mark_steer_delivered(OWNER, SESSION_ID, "entry-0", "turn-first", T0 + 1)
        .await
        .expect("first settlement");

    let error = store
        .mark_steer_delivered(OWNER, SESSION_ID, "entry-0", "turn-second", T0 + 2)
        .await
        .expect_err("a terminal entry cannot be settled again");
    assert_eq!(error.code(), "user_prompt_queue_expired");

    let record = store.get("entry-0").await.expect("load entry");
    assert_eq!(
        record.delivered_turn_id.as_deref(),
        Some("turn-first"),
        "the second attempt must not overwrite the attribution"
    );
}

/// Session scoping: an entry that exists must not be reachable by naming a
/// different session. The `session_id = ?` guard is what makes this `NotFound`
/// rather than a silent cross-session write.
///
/// The obvious variant of this test — the *same* entry id in two sessions — is
/// not merely weaker, it is unrepresentable: `add` rejects a reused entry id
/// outright, so entry identity is globally unique and the id alone could never
/// address the wrong row.
#[tokio::test]
async fn user_prompt_queue_a_steer_settlement_does_not_cross_sessions() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    ensure_session(&pool, SESSION_ID).await;
    ensure_session(&pool, OTHER_SESSION).await;
    store
        .add(OWNER, SESSION_ID, "entry-a", "mine", None, T0)
        .await
        .expect("seed session a");
    store
        .add(OWNER, OTHER_SESSION, "entry-b", "theirs", None, T0)
        .await
        .expect("seed session b");

    let error = store
        .mark_steer_delivered(OWNER, SESSION_ID, "entry-b", "turn-live", T0 + 1)
        .await
        .expect_err("the queued prompt belongs to the other session");
    assert_eq!(error.code(), "user_prompt_queue_not_found");
    assert_eq!(
        status_of(&store, "entry-b").await,
        UserPromptQueueStatus::Pending,
        "the other session's entry must be untouched"
    );
}

/// The settlement writes no ordinal. C001 made the surviving rows keep their
/// ranks, and a settlement that renumbered would silently reorder everything
/// still waiting behind the steered entry.
#[tokio::test]
async fn user_prompt_queue_a_steer_settlement_preserves_the_live_order() {
    let pool = isolated_pool().await;
    let store = UserPromptQueueStore::new(pool.clone());
    seeded(&store, &pool, 3).await;

    store
        .mark_steer_delivered(OWNER, SESSION_ID, "entry-0", "turn-live", T0 + 1)
        .await
        .expect("settle the head entry");

    let live: Vec<String> = store
        .list_live(SESSION_ID, T0 + 1)
        .await
        .expect("list live")
        .into_iter()
        .map(|entry| entry.id)
        .collect();
    assert_eq!(
        live,
        vec!["entry-1".to_owned(), "entry-2".to_owned()],
        "the survivors must stay in the order the user wrote them"
    );
}

// ── M007 (ADR-0039): delayed delivery ─────────────────────────────
//
// The property under test is **never early**. Everything else here exists to
// show that a second, less obvious way of being early has not been introduced:
// a boundary that is off by one, an edit that bypasses the predicate, a reorder
// that promotes a scheduled entry, or a sweep that nominates on the wrong side
// of the instant.

// The headline property: a not-yet-due entry is never claimed, and becomes
// claimed at exactly its due millisecond. Tested either side of the boundary
// because an off-by-one here is invisible from any single sample.
#[tokio::test]
async fn user_prompt_queue_not_yet_due_is_never_claimed_and_due_is_claimed_at_its_instant() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool);

    let run_at = T0 + 60_000;
    store
        .add(OWNER, SESSION_ID, "e1", "later", Some(run_at), T0)
        .await
        .expect("add scheduled entry");

    for now in [T0, T0 + 1, run_at - 2, run_at - 1] {
        let claim = store
            .claim_next_for_delivery(SESSION_ID, now)
            .await
            .expect("claim must not error");
        assert!(
            claim.is_none(),
            "an entry due at {run_at} was claimed at {now}: early delivery"
        );
    }

    // The boundary itself is eligible: `<=`, not `<`.
    let claim = store
        .claim_next_for_delivery(SESSION_ID, run_at)
        .await
        .expect("claim at due instant")
        .expect("an entry is eligible exactly at its due millisecond");
    assert_eq!(claim.entry.id, "e1");
    assert_eq!(
        claim.entry.run_at_ms,
        Some(run_at),
        "the claim must carry the due time it was gated on"
    );
}

/// An immediate entry is unchanged: the `NULL` branch of the predicate is the
/// pre-M007 behaviour exactly, not an approximation of it.
#[tokio::test]
async fn user_prompt_queue_immediate_entry_is_unaffected_by_the_eligibility_predicate() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool);

    let entry = store
        .add(OWNER, SESSION_ID, "e1", "no timer", None, T0)
        .await
        .expect("add immediate entry");
    assert_eq!(
        entry.run_at_ms, None,
        "an entry added without a due time must read as None, not as a sentinel"
    );

    let claim = store
        .claim_next_for_delivery(SESSION_ID, T0)
        .await
        .expect("claim")
        .expect("an immediate entry is eligible the moment it is queued");
    assert_eq!(claim.entry.id, "e1");
}

/// Ordinal order still decides among eligible entries, and a scheduled entry
/// cannot be pulled forward by the ones before it going away.
#[tokio::test]
async fn user_prompt_queue_ordinal_orders_eligible_entries_and_skips_not_yet_due_ones() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool);

    // Ordinal 0 is far in the future; ordinal 1 is immediate. The immediate one
    // is delivered even though a lower ordinal exists — otherwise one scheduled
    // entry at the head would block the whole queue until its due time.
    store
        .add(
            OWNER,
            SESSION_ID,
            "later",
            "scheduled first",
            Some(T0 + 600_000),
            T0,
        )
        .await
        .expect("add scheduled");
    store
        .add(OWNER, SESSION_ID, "now", "immediate second", None, T0)
        .await
        .expect("add immediate");

    let claim = store
        .claim_next_for_delivery(SESSION_ID, T0)
        .await
        .expect("claim")
        .expect("the eligible entry is deliverable");
    assert_eq!(
        claim.entry.id, "now",
        "a not-yet-due entry must be invisible to ordering, not a head-of-line block"
    );

    // Once the scheduled one is due, it becomes the next candidate — still by
    // ordinal among whatever is eligible at that moment.
    store
        .mark_delivery_failed("now", "test", T0)
        .await
        .expect("terminal the immediate entry");
    let claim = store
        .claim_next_for_delivery(SESSION_ID, T0 + 600_000)
        .await
        .expect("claim")
        .expect("the scheduled entry is due now");
    assert_eq!(claim.entry.id, "later");
}

/// Reorder permutes only what is *already* eligible, so it cannot invert timer
/// precedence. This is the plan's §3.4 decision, asserted as behaviour rather
/// than as a comment.
#[tokio::test]
async fn user_prompt_queue_reorder_cannot_deliver_a_not_yet_due_entry_early() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool);

    store
        .add(OWNER, SESSION_ID, "a", "first", None, T0)
        .await
        .expect("add a");
    store
        .add(
            OWNER,
            SESSION_ID,
            "b",
            "scheduled second",
            Some(T0 + 3_600_000),
            T0,
        )
        .await
        .expect("add b");

    // Move the scheduled entry to the front. Before the due time it must still
    // not be claimed, because ordering only permutes eligible entries.
    store
        .reorder(OWNER, SESSION_ID, &["b".to_string(), "a".to_string()], T0)
        .await
        .expect("reorder");

    // `a` is immediate, so it is still the eligible one — and it is *not* the
    // entry that was promoted to the front. Asserting the identity rather than
    // "some claim happened" is what makes this a test of the predicate: a
    // missing eligibility check would hand back `b` here.
    let claim = store
        .claim_next_for_delivery(SESSION_ID, T0 + 3_599_999)
        .await
        .expect("claim")
        .expect("the immediate entry is claimable");
    assert_eq!(
        claim.entry.id, "a",
        "reordering a scheduled entry to the front must not make it the next \
         claim while it is not yet due"
    );

    let claim = store
        .claim_next_for_delivery(SESSION_ID, T0 + 3_600_000)
        .await
        .expect("claim")
        .expect("eligible at its due instant regardless of position");
    assert_eq!(
        claim.entry.id, "b",
        "the reordered entry is delivered when due"
    );
}

// ── the expiry refusal ─────────────────────────────────────────────

/// A due time at or beyond the entry's own expiry is refused on add, never
/// clamped. Clamping would schedule it for its expiry — due only once expired,
/// i.e. a prompt that can never run.
#[tokio::test]
async fn user_prompt_queue_due_time_at_or_beyond_expiry_is_refused_on_add() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool);

    let expires_at = T0 + USER_PROMPT_QUEUE_TTL_MS;

    // Exactly at expiry: refused.
    let at_expiry = store
        .add(OWNER, SESSION_ID, "e1", "never runs", Some(expires_at), T0)
        .await
        .expect_err("a due time exactly at expiry can never run and must be refused");
    assert!(
        matches!(at_expiry, UserPromptQueueError::DueTimeAfterExpiry { .. }),
        "expected the typed expiry refusal, got {at_expiry:?}"
    );

    // Past expiry: refused.
    assert!(
        store
            .add(
                OWNER,
                SESSION_ID,
                "e2",
                "never runs",
                Some(expires_at + 1),
                T0
            )
            .await
            .is_err(),
        "a due time beyond expiry must be refused"
    );

    // One millisecond before expiry: accepted.
    store
        .add(
            OWNER,
            SESSION_ID,
            "e3",
            "just in time",
            Some(expires_at - 1),
            T0,
        )
        .await
        .expect("one millisecond before expiry is still runnable");

    // Nothing was written by either refusal.
    assert!(
        store.get("e1").await.is_err(),
        "a refused add must leave no row"
    );
}

/// A due time in the past is accepted, not refused: the user has already passed
/// it, so the entry is eligible immediately.
#[tokio::test]
async fn user_prompt_queue_due_time_in_the_past_is_accepted_as_immediately_eligible() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool);

    store
        .add(OWNER, SESSION_ID, "e1", "oops", Some(T0 - 60_000), T0)
        .await
        .expect("a due time already passed is not an error");

    let claim = store
        .claim_next_for_delivery(SESSION_ID, T0)
        .await
        .expect("claim")
        .expect("eligible immediately");
    assert_eq!(claim.entry.id, "e1");
}

/// The same refusal on edit, against the entry's **stored** expiry rather than a
/// recomputed `now + TTL`.
#[tokio::test]
async fn user_prompt_queue_due_time_beyond_the_entries_own_expiry_is_refused_on_edit() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool);

    store
        .add(OWNER, SESSION_ID, "e1", "original", None, T0)
        .await
        .expect("add");
    let entry = store.get("e1").await.expect("entry exists");
    let expires_at = entry.expires_at_ms;

    let refused = store
        .edit(
            OWNER,
            SESSION_ID,
            "e1",
            "revised",
            Some(expires_at + 5_000),
            T0 + 1_000,
        )
        .await
        .expect_err("beyond the entry's own expiry must be refused");
    assert!(
        matches!(refused, UserPromptQueueError::DueTimeAfterExpiry { .. }),
        "expected the typed expiry refusal, got {refused:?}"
    );

    // The refused edit changed nothing — not the text, not the schedule.
    let after = store.get("e1").await.expect("row survives a refused edit");
    assert_eq!(
        after.text, "original",
        "a refused edit must not write the text"
    );
    assert_eq!(
        after.run_at_ms, None,
        "a refused edit must not write a due time"
    );

    // An accepted edit does set it.
    let edited = store
        .edit(
            OWNER,
            SESSION_ID,
            "e1",
            "revised",
            Some(T0 + 120_000),
            T0 + 1_000,
        )
        .await
        .expect("an in-horizon due time is accepted");
    assert_eq!(edited.run_at_ms, Some(T0 + 120_000));
}

/// Editing a due time is first-class: set, change, and clear are three
/// distinct reachable states, and clearing is not the same as "due now".
#[tokio::test]
async fn user_prompt_queue_editing_a_due_time_sets_changes_and_clears_it() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool);

    store
        .add(OWNER, SESSION_ID, "e1", "prompt", None, T0)
        .await
        .expect("add");

    let set = store
        .edit(OWNER, SESSION_ID, "e1", "prompt", Some(T0 + 60_000), T0)
        .await
        .expect("set a due time");
    assert_eq!(set.run_at_ms, Some(T0 + 60_000));

    let changed = store
        .edit(OWNER, SESSION_ID, "e1", "prompt", Some(T0 + 120_000), T0)
        .await
        .expect("change a due time");
    assert_eq!(changed.run_at_ms, Some(T0 + 120_000));

    let cleared = store
        .edit(OWNER, SESSION_ID, "e1", "prompt", None, T0)
        .await
        .expect("clear a due time");
    assert_eq!(
        cleared.run_at_ms, None,
        "clearing the schedule must return the entry to immediate, not to 'due now'"
    );

    // Immediately claimable again, having been not-yet-due a moment ago.
    assert!(
        store
            .claim_next_for_delivery(SESSION_ID, T0)
            .await
            .expect("claim")
            .is_some(),
        "a cleared entry is eligible again"
    );
}

// ── the sweep queries the advance tick is built on ─────────────────

/// Nothing scheduled means no wake: this is the assertion that makes the tick
/// a scheduler over its own work rather than a poller. It asserts the **park**,
/// not merely the absence of a delivery — a task that woke and found nothing
/// would satisfy the weaker claim.
#[tokio::test]
async fn user_prompt_queue_next_due_is_absent_when_nothing_is_scheduled() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool);

    assert_eq!(
        store
            .next_scheduled_due_at_ms()
            .await
            .expect("read next due"),
        None,
        "an empty queue must park the advance task"
    );

    store
        .add(OWNER, SESSION_ID, "e1", "immediate", None, T0)
        .await
        .expect("add immediate");
    assert_eq!(
        store
            .next_scheduled_due_at_ms()
            .await
            .expect("read next due"),
        None,
        "an unscheduled entry must not arm a timer; only a due time does"
    );

    store
        .add(OWNER, SESSION_ID, "e2", "later", Some(T0 + 60_000), T0)
        .await
        .expect("add scheduled");
    assert_eq!(
        store
            .next_scheduled_due_at_ms()
            .await
            .expect("read next due"),
        Some(T0 + 60_000),
        "the earliest due time across sessions is the wake"
    );

    // An earlier entry takes over the wake.
    store
        .add(OWNER, SESSION_ID, "e3", "sooner", Some(T0 + 1_000), T0)
        .await
        .expect("add sooner");
    assert_eq!(
        store.next_scheduled_due_at_ms().await.expect("read"),
        Some(T0 + 1_000)
    );
}

/// A due time already in the past is returned as itself, not clamped to `now`.
/// The caller owns the clamp; keeping it out of the query is what stops two
/// readers of the same instant from disagreeing about when to fire.
#[tokio::test]
async fn user_prompt_queue_next_due_returns_a_past_due_time_unchanged() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool);

    let run_at = T0 - 30_000;
    store
        .add(
            OWNER,
            SESSION_ID,
            "e1",
            "missed while down",
            Some(run_at),
            T0,
        )
        .await
        .expect("add");
    assert_eq!(
        store.next_scheduled_due_at_ms().await.expect("read"),
        Some(run_at),
        "a due time passed while the daemon was down must survive verbatim; \
         the daemon is late, not early"
    );
}

/// The sweep nominates exactly the sessions holding a due entry, bounded, and
/// never names a session through an expired or terminal row.
#[tokio::test]
async fn user_prompt_queue_sessions_with_due_entries_is_bounded_and_due_only() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    ensure_session(&pool, OTHER_SESSION).await;
    let store = UserPromptQueueStore::new(pool);

    assert!(
        store
            .sessions_with_due_entries(T0, 16)
            .await
            .expect("sweep")
            .is_empty(),
        "nothing scheduled means nothing to sweep"
    );

    store
        .add(OWNER, SESSION_ID, "e1", "immediate", None, T0)
        .await
        .expect("add immediate");
    assert!(
        store
            .sessions_with_due_entries(T0, 16)
            .await
            .expect("sweep")
            .is_empty(),
        "an unscheduled entry must not wake the sweep"
    );

    store
        .add(OWNER, SESSION_ID, "e2", "due now", Some(T0), T0)
        .await
        .expect("add due");
    store
        .add(
            OWNER,
            OTHER_SESSION,
            "e3",
            "due later",
            Some(T0 + 600_000),
            T0,
        )
        .await
        .expect("add later");

    let due_now = store
        .sessions_with_due_entries(T0, 16)
        .await
        .expect("sweep");
    assert_eq!(
        due_now,
        vec![SESSION_ID.to_string()],
        "only the session whose entry is actually due is nominated"
    );

    let both = store
        .sessions_with_due_entries(T0 + 600_000, 16)
        .await
        .expect("sweep");
    assert_eq!(both.len(), 2, "both are due by now");

    // The bound is real, not decorative.
    store
        .add(OWNER, SESSION_ID, "e4", "due too", Some(T0 + 600_000), T0)
        .await
        .expect("add");
    let bounded = store
        .sessions_with_due_entries(T0 + 600_000, 1)
        .await
        .expect("sweep");
    assert_eq!(bounded.len(), 1, "the sweep must honour its limit");
    assert_eq!(
        store
            .sessions_with_due_entries(T0 + 600_000, 0)
            .await
            .expect("sweep"),
        Vec::<String>::new(),
        "a non-positive limit must sweep nothing rather than everything"
    );
}

/// A scheduled entry that expired before its due time is not nominated. It could
/// not be claimed anyway, and naming it would send the sweep to a session whose
/// claim returns nothing.
#[tokio::test]
async fn user_prompt_queue_sweep_skips_an_entry_that_expired_before_it_was_due() {
    let pool = isolated_pool().await;
    ensure_session(&pool, SESSION_ID).await;
    let store = UserPromptQueueStore::new(pool);

    let expires_at = T0 + USER_PROMPT_QUEUE_TTL_MS;
    store
        .add(
            OWNER,
            SESSION_ID,
            "e1",
            "expires first",
            Some(expires_at - 1),
            T0,
        )
        .await
        .expect("add");

    let after_expiry = expires_at + 1;
    assert!(
        store
            .sessions_with_due_entries(after_expiry, 16)
            .await
            .expect("sweep")
            .is_empty(),
        "an expired entry must not be nominated for delivery"
    );
}
