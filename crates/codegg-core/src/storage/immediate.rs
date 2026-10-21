//! A write transaction that takes SQLite's write lock **at `BEGIN`**.
//!
//! # C002 — why this exists
//!
//! A deferred `BEGIN` takes no lock at all. The first `SELECT` in such a
//! transaction holds a shared read lock, and the first `UPDATE` has to
//! *upgrade* it to a write lock. SQLite refuses that upgrade with
//! `SQLITE_BUSY` — code 6, `database is deadlocked` — and does **not** call
//! the busy handler, because waiting could deadlock. `busy_timeout`
//! therefore never applies. Two connections that each read before either
//! wrote could **both** fail, rather than one winning and the other waiting
//! its turn.
//!
//! Measured on the user prompt queue before the change: **204 of 256**
//! concurrent claim rounds returned that error to at least one caller.
//!
//! `BEGIN IMMEDIATE` takes the write lock **before** any read, and that
//! acquisition *does* honour `busy_timeout`. A second writer waits for the
//! first to commit and then proceeds — which is the behaviour every
//! read-then-write path already assumed.
//!
//! Holding the write lock for the whole transaction is the point, not a cost.
//! Each path reads a row, decides from what it read, and writes a decision
//! that must not be invalidated in between; the read-to-write window is
//! precisely the gap a concurrent writer would use. Serializing the path
//! closes it.
//!
//! # Reads are deliberately excluded
//!
//! A path whose **first** statement is already a write gains nothing from
//! taking the write lock earlier: that statement takes the lock anyway. Only
//! the read-then-write paths need this.
//!
//! # Reuse
//!
//! This module is the shared home of the pattern. The user prompt queue
//! store established it and `scripts/check_queue_write_transaction_shape.py`
//! pins the shape there; the daemon's scratch retention sweep uses the same
//! helper, because it reads a session's activity and liveness before it
//! writes its reclaim decision. A second copy of this transaction shape is
//! how the deadlock comes back.

use sqlx::{SqlitePool, TransactionManager};

/// Open a write transaction that takes SQLite's write lock **at `BEGIN`**.
///
/// Every read-then-write path goes through here. See [`ImmediateWrite`] for
/// why that is not the same as a deferred transaction.
///
/// This is a plain function rather than an associated one so the call sites
/// read `begin_immediate(&pool)` instead of naming the type twice.
pub async fn begin_immediate(pool: &SqlitePool) -> Result<ImmediateWrite, sqlx::Error> {
    ImmediateWrite::begin(pool).await
}

/// A write transaction that takes SQLite's write lock **at `BEGIN`**, rather
/// than upgrading a read lock at the first write.
pub struct ImmediateWrite {
    /// `Option` so [`Drop`] can take the connection out; it is `Some` for the
    /// whole time the transaction is usable.
    conn: Option<sqlx::pool::PoolConnection<sqlx::Sqlite>>,
    settled: bool,
}

/// The one call that has to name sqlx's transaction machinery fully, three
/// times per method, or once each here.
type SqliteTx = sqlx::sqlite::SqliteTransactionManager;

impl ImmediateWrite {
    /// Begin an immediate transaction.
    pub async fn begin(pool: &SqlitePool) -> Result<Self, sqlx::Error> {
        Self::begin_with(pool, |error| error).await
    }

    /// Begin an immediate transaction, mapping the raw sqlx error with `map`.
    ///
    /// `BEGIN IMMEDIATE` takes SQLite's write lock **before** any read, and
    /// that acquisition honours `busy_timeout`. A deferred transaction takes a
    /// shared read lock on its first `SELECT` and then has to upgrade it,
    /// which SQLite refuses with `SQLITE_BUSY` without waiting — so two
    /// callers could both fail instead of one winning and the other waiting
    /// its turn.
    pub async fn begin_with<E>(
        pool: &SqlitePool,
        map: impl Fn(sqlx::Error) -> E,
    ) -> Result<Self, E> {
        let mut conn = pool.acquire().await.map_err(&map)?;
        // `TransactionManager::begin` takes the statement to run. Handing it
        // `BEGIN IMMEDIATE` is what makes this transaction immediate — and
        // going through the manager rather than issuing raw SQL is what keeps
        // sqlx's own transaction **depth** counter correct. A raw `BEGIN`
        // leaves that counter at zero, so the queued rollback below became a
        // no-op and the connection went back to the pool still holding its
        // write lock.
        SqliteTx::begin(
            &mut conn,
            Some(std::borrow::Cow::Borrowed("BEGIN IMMEDIATE")),
        )
        .await
        .map_err(&map)?;
        Ok(Self {
            conn: Some(conn),
            settled: false,
        })
    }

    /// Commit, handing the raw sqlx error back unchanged.
    pub async fn commit(self) -> Result<(), sqlx::Error> {
        self.commit_with(|error| error).await
    }

    /// Commit, mapping the raw sqlx error with `map`.
    ///
    /// `settled` is set **after** the commit returns, not before. A failed
    /// commit therefore leaves the connection unsettled, so [`Drop`] rolls it
    /// back — rather than returning a connection that may still hold a write
    /// lock to the next borrower.
    pub async fn commit_with<E>(mut self, map: impl Fn(sqlx::Error) -> E) -> Result<(), E> {
        SqliteTx::commit(self.conn.as_mut().expect("live while committing"))
            .await
            .map_err(&map)?;
        self.settled = true;
        Ok(())
    }
}

impl std::ops::Deref for ImmediateWrite {
    type Target = sqlx::pool::PoolConnection<sqlx::Sqlite>;

    fn deref(&self) -> &Self::Target {
        self.conn
            .as_ref()
            .expect("the connection is only taken in Drop")
    }
}

impl std::ops::DerefMut for ImmediateWrite {
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
        // Every refusal below is a `return Err(..)` with nothing committed, and
        // it arrives here as a drop. `Drop` cannot `await` a `ROLLBACK`, so
        // this queues one on the connection's worker thread — the same thing
        // `sqlx::Transaction` does on its own drop, and for the same reason: a
        // future dropped at an arbitrary `.await` must not leave an open
        // transaction for the next borrower of this connection.
        //
        // It deliberately does **not** close the connection to force the
        // rollback. Closing works for a file-backed database and is
        // destructive for a shared-cache in-memory one, where dropping the
        // last connection destroys the database: the queue store's mutation
        // suite caught exactly that, as an unowned-queue refusal turning into
        // `no such table` two operations later.
        if let Some(mut conn) = self.conn.take() {
            SqliteTx::start_rollback(&mut conn);
        }
    }
}
