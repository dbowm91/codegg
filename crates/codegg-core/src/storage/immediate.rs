//! A write transaction that takes SQLite's write lock **at `BEGIN`**, rather than
//! upgrading a read lock at the first write.
//!
//! # Why this type exists
//!
//! [`sqlx::SqlitePool::begin`] issues a **deferred** `BEGIN`, which takes no lock
//! at all. The first `SELECT` in such a transaction then holds a shared read lock,
//! and the first `UPDATE` has to *upgrade* it to a write lock. SQLite refuses
//! that upgrade with `SQLITE_BUSY` — code 6, `database is deadlocked` — and does
//! **not** call the busy handler, because waiting could deadlock. `busy_timeout`
//! therefore never applies. Two connections that each read before either wrote
//! could **both** fail, rather than one winning and the other waiting its turn.
//!
//! Measured on the user-prompt queue store before the change: **204 of 256**
//! concurrent claim rounds returned that error to at least one caller
//! (`user_prompt_queue_concurrent_claims_never_deadlock_on_lock_upgrade`).
//!
//! `BEGIN IMMEDIATE` takes the write lock **before** any read, and that
//! acquisition *does* honour `busy_timeout`. A second writer waits for the first
//! to commit and then proceeds — which is the behavior every read-then-write path
//! already assumed.
//!
//! Holding the write lock for the whole transaction is the point, not a cost.
//! Each path reads a row, decides from what it read, and writes a decision that
//! must not be invalidated in between; the read-to-write window is precisely the
//! gap a concurrent writer would use. Serializing the path closes it.
//!
//! # Scope
//!
//! This is a **storage primitive**, not an admission mechanism. It serializes
//! writers *to one SQLite database file*; it says nothing about which logical
//! resource a transaction is protecting. A store that protects a shared logical
//! resource still needs its own compare-and-swap predicate — the write lock only
//! makes the read-to-write window atomic.
//!
//! # Reads are deliberately excluded
//!
//! A read-only path, or a path whose **first** statement is already a write, has
//! no read lock to upgrade and gains nothing by taking the write lock earlier.
//! Taking it earlier would serialize every read in the daemon against every
//! write, which is a real cost for no gain. Use [`sqlx::SqlitePool::begin`] for
//! those.
//!
//! # Rollback contract
//!
//! An unsettled transaction queues a `ROLLBACK` on its connection in [`Drop`]
//! and never closes it. Closing would force the rollback, and that is
//! destructive for a shared-cache in-memory database, where dropping the last
//! connection destroys the database.

use sqlx::{Sqlite, SqlitePool, TransactionManager};
use std::ops::{Deref, DerefMut};

/// The one call that has to name sqlx's transaction machinery fully, three times
/// per method, or once each here.
type SqliteTx = sqlx::sqlite::SqliteTransactionManager;

/// A write transaction that holds SQLite's write lock from `BEGIN` to commit.
pub struct ImmediateWrite {
    /// `Option` so [`Drop`] can take the connection out; it is `Some` for the
    /// whole time the transaction is usable.
    conn: Option<sqlx::pool::PoolConnection<Sqlite>>,
    settled: bool,
}

impl ImmediateWrite {
    /// Open an immediate write transaction against `pool`.
    ///
    /// The caller is responsible for committing or letting the transaction roll
    /// back; see [`ImmediateWrite::commit`].
    pub async fn begin(pool: &SqlitePool) -> Result<Self, sqlx::Error> {
        let mut conn = pool.acquire().await?;
        // `TransactionManager::begin` takes the statement to run. Handing it
        // `BEGIN IMMEDIATE` is what makes this transaction immediate — and going
        // through the manager rather than issuing raw SQL is what keeps sqlx's
        // own transaction **depth** counter correct. A raw `BEGIN` leaves that
        // counter at zero, so the queued rollback below became a no-op and the
        // connection went back to the pool still holding its write lock.
        SqliteTx::begin(
            &mut conn,
            Some(std::borrow::Cow::Borrowed("BEGIN IMMEDIATE")),
        )
        .await?;
        Ok(Self {
            conn: Some(conn),
            settled: false,
        })
    }

    /// Commit, then hand the connection back to the pool.
    ///
    /// `settled` is set **after** the commit returns, not before. A failed commit
    /// therefore leaves the connection unsettled, so [`Drop`] rolls it back —
    /// rather than returning a connection that may still hold a write lock to
    /// the next borrower.
    pub async fn commit(mut self) -> Result<(), sqlx::Error> {
        SqliteTx::commit(self.conn.as_mut().expect("live while committing")).await?;
        self.settled = true;
        Ok(())
    }
}

impl Deref for ImmediateWrite {
    type Target = sqlx::pool::PoolConnection<Sqlite>;

    fn deref(&self) -> &Self::Target {
        self.conn
            .as_ref()
            .expect("the connection is only taken in Drop")
    }
}

impl DerefMut for ImmediateWrite {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.conn
            .as_mut()
            .expect("the connection is only taken in Drop")
    }
}

impl Drop for ImmediateWrite {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        // Every refusal below is a `return Err(..)` with nothing committed, and it
        // arrives here as a drop. `Drop` cannot `await` a `ROLLBACK`, so this
        // queues one on the connection's worker thread — the same thing
        // `sqlx::Transaction` does on its own drop, and for the same reason: a
        // future dropped at an arbitrary `.await` must not leave an open
        // transaction for the next borrower of this connection.
        //
        // It deliberately does **not** close the connection to force the
        // rollback. Closing works for a file-backed database and is destructive
        // for a shared-cache in-memory one, where dropping the last connection
        // destroys the database: the mutation suite caught exactly that, as an
        // unowned-queue refusal turning into `no such table` two operations
        // later.
        if let Some(mut conn) = self.conn.take() {
            SqliteTx::start_rollback(&mut conn);
        }
    }
}
