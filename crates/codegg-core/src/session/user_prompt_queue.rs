//! M002a (ADR-0035): durable, bounded, expiring ownership for queued user
//! prompts, plus the M002b (ADR-0037) mutation slice and the M003 (ADR-0038)
//! delivery slice layered on top of them.
//!
//! M002a shipped no producer and M002b shipped no user-visible surface. **M003
//! adds the producer**: it makes `delivering`, `delivered`, and `failed`
//! reachable, so all five v80 `status` values now have one, and it is the
//! milestone where a queued prompt actually runs.
//!
//! Entry text is user-authored prompt content with the same sensitivity as a
//! submitted turn. It never appears in an error message, an audit record, or a
//! `tracing` call; diagnostics reference entry identity only.
//!
//! Storage changes here are additive. No existing table is altered and no stored
//! data is rewritten.

use sqlx::{FromRow, SqlitePool, TransactionManager};
use thiserror::Error;

use crate::error::StorageError;

/// Bounds chosen for a *user prompt* rather than a derived shell projection.
/// See ADR-0035 for the ownership contract these encode.
pub const USER_PROMPT_QUEUE_MAX_ENTRY_BYTES: usize = 64 * 1024;
pub const USER_PROMPT_QUEUE_MAX_PENDING: i64 = 64;
pub const USER_PROMPT_QUEUE_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1000;
pub const USER_PROMPT_QUEUE_MAX_RETAINED_PER_SESSION: i64 = 256;

/// M003: the bound on how many stranded `delivering` rows one startup pass
/// will reconcile.
///
/// The restart twin is a bounded sweep, not a poller, for the same reason
/// `reconcile_idle_continuation_wakes` is: an unbounded repair loop is a new
/// background thread wearing a repair costume. Anything beyond this bound stays
/// `delivering` and is picked up by the next startup or the next post-terminal
/// wake for its session.
pub const USER_PROMPT_QUEUE_MAX_RECONCILE: i64 = 256;

/// M003: `delivering` is **not** a terminal state, so it must behave like a
/// live entry in exactly three places it did not have to before — the read
/// projection, the backlog bound, and the expiry sweep.
///
/// These three are one constant because they are one fact. They were separate
/// `status = 'pending'` literals under M002a/M002b, which was correct only
/// while `delivering` had no producer. Splitting them again is how a
/// `delivering` row becomes invisible, uncounted, and unexpired — the exact
/// defect M003 exists to close.
///
/// `expired` and `delivered` and `failed` are terminal and deliberately absent.
const LIVE_STATUS_PREDICATE: &str = "status IN ('pending', 'delivering')";

/// M003: the subset of [`LIVE_STATUS_PREDICATE`] a mutation may act on.
///
/// A `delivering` entry has an in-flight admission bound to it, so editing,
/// deleting, or reordering it would be a race against that admission. The
/// read projection includes it; the operable set does not.
const OPERABLE_STATUS_PREDICATE: &str = "status = 'pending'";

/// M002b: the displacement used to renumber a queue without colliding with
/// `UNIQUE (session_id, ordinal)`.
///
/// v80 added that constraint, so a renumber that walks straight to its target
/// can collide with a row it has not yet moved. Displacing every affected row
/// above the whole reachable ordinal band first, then writing the dense targets,
/// makes the renumber collision-free in one transaction.
///
/// A positive band offset — rather than negative sentinels — because v80's
/// `CHECK (ordinal >= 0)` forbids them. It sits far above every reachable
/// ordinal (`MAX_RETAINED_PER_SESSION` is 256) so the displaced band can never
/// overlap the dense targets, and it is uniform, so displaced rows stay distinct
/// from each other.
pub const USER_PROMPT_QUEUE_RENUMBER_BAND_OFFSET: i64 = 1_000_000;

/// Lifecycle of one queue entry.
///
/// M002a enumerated all five values because SQLite cannot `ALTER` a `CHECK`
/// constraint and narrowing it later would cost a full table rebuild. **M003
/// gave the three unproduced values their producer**, so every value v80 admits
/// is now reachable: `pending` is live and operable, `delivering` is live with an
/// admission in flight, and `delivered`, `failed`, `expired` are terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserPromptQueueStatus {
    Pending,
    Delivering,
    Delivered,
    Failed,
    Expired,
}

impl UserPromptQueueStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Delivering => "delivering",
            Self::Delivered => "delivered",
            Self::Failed => "failed",
            Self::Expired => "expired",
        }
    }

    fn parse(value: &str) -> Result<Self, UserPromptQueueError> {
        match value {
            "pending" => Ok(Self::Pending),
            "delivering" => Ok(Self::Delivering),
            "delivered" => Ok(Self::Delivered),
            "failed" => Ok(Self::Failed),
            "expired" => Ok(Self::Expired),
            _ => Err(UserPromptQueueError::Invalid {
                field: "status",
                message: "unknown stored status".to_owned(),
            }),
        }
    }

    /// M003: a live status must appear in the read projection, count against the
    /// backlog bound, and be swept by expiry. All three follow from here, so they
    /// cannot drift apart the way three independent `status = 'pending'`
    /// literals did.
    pub const fn is_live(self) -> bool {
        matches!(self, Self::Pending | Self::Delivering)
    }

    /// M003: whether a mutation may act on this status. A `delivering` entry is
    /// live but **not** operable — an admission is in flight against it, so
    /// editing, deleting, or reordering it would race that admission.
    pub const fn is_operable(self) -> bool {
        matches!(self, Self::Pending)
    }

    /// M003: whether an explicit retry may revive this terminal status.
    ///
    /// Only `failed`. `delivered` has already had its turn, and `expired` outlived
    /// the window in which the user expected it to run; reviving either would
    /// silently re-run or resurrect work the user has moved past.
    pub const fn is_retryable(self) -> bool {
        matches!(self, Self::Failed)
    }
}

/// One durable queue entry as the daemon owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserPromptQueueEntryRecord {
    pub id: String,
    pub session_id: String,
    pub ordinal: i64,
    pub text: String,
    pub text_bytes: usize,
    pub status: UserPromptQueueStatus,
    pub failure_reason: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub expires_at_ms: i64,
    pub delivered_turn_id: Option<String>,
    pub delivered_at_ms: Option<i64>,
    /// M007 (ADR-0039): when this entry becomes eligible for delivery.
    ///
    /// `None` is "no due time" and means the entry was eligible from the moment
    /// it was queued — the pre-M007 behaviour, and what every migrated row
    /// reads as. It is **not** a sentinel: see `migrate_v82`.
    pub run_at_ms: Option<i64>,
}

/// One live queue entry, as returned by the read projection.
///
/// **M003:** this is no longer a `pending`-only list. A `delivering` entry
/// appears here so a client can render an entry it can see but must not yet edit —
/// an entry that vanished from the list while a turn started for it would read as
/// data loss.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserPromptQueueLiveEntry {
    pub id: String,
    pub ordinal: i64,
    pub text: String,
    pub text_bytes: usize,
    pub status: UserPromptQueueStatus,
    pub created_at_ms: i64,
    pub expires_at_ms: i64,
    /// M007: the due time, or `None` for an entry that was always immediate.
    ///
    /// Carried on the read projection because a client cannot render "runs in
    /// 40m" — or distinguish a scheduled row from an ordinary one — without it.
    pub run_at_ms: Option<i64>,
}

/// A bounded resource refuses rather than truncates. Every bound below has a
/// typed variant here — the primary mechanism — and a matching `CHECK`
/// constraint in the schema — the backstop that survives a missed call site.
#[derive(Debug, Error)]
pub enum UserPromptQueueError {
    #[error("invalid user prompt queue entry {field}: {message}")]
    Invalid {
        field: &'static str,
        message: String,
    },
    #[error("user prompt queue already holds {limit} pending entries")]
    TooManyEntries { limit: i64 },
    #[error("user prompt queue entry exceeds {limit} bytes")]
    EntryTooLarge { limit: usize },
    #[error("user prompt queue entry was not found")]
    NotFound,
    #[error("user prompt queue entry has expired")]
    Expired,
    /// M002b (ADR-0037): the caller is not this session's queue owner.
    ///
    /// Deliberately carries **no owner identity**. Naming the owner to a refused
    /// caller would itself be the leak the refusal exists to prevent.
    #[error("caller is not the owner of this session's prompt queue")]
    NotQueueOwner,
    /// M002b (ADR-0037): the queue has entries but no owner row, so it has no
    /// authorized mutator at all. Only an explicit adoption repairs it.
    #[error("prompt queue has no recorded owner; explicit adoption is required")]
    QueueUnowned,
    /// M002b (ADR-0037): the ownership revision fence rejected a transfer. Two
    /// concurrent transfers converge on exactly one winner through this.
    #[error("prompt queue ownership revision conflict: expected {expected}, current {current}")]
    OwnerRevisionConflict { expected: u64, current: u64 },
    /// M002b: a reorder's entry set is not a permutation of the live queue.
    /// Refused rather than partially applied, so a stale client view cannot
    /// silently drop or resurrect entries.
    #[error("reorder entry list does not match the live prompt queue")]
    OrderMismatch,
    /// M003: an edit, delete, or reorder named an entry that is `delivering`.
    ///
    /// This is **not** [`Self::Expired`] and must not be reported as one. Before
    /// M003 gave `delivering` a producer, every non-`pending` row was terminal, so
    /// collapsing the two was correct; now a `delivering` row is live and will
    /// either become `delivered` or return to `pending`, and telling a client its
    /// entry expired because a turn started would be a lie it might act on.
    #[error("user prompt queue entry is being delivered")]
    Delivering,
    /// M007: the requested due time is at or beyond the entry's own expiry.
    ///
    /// Refused rather than clamped. Clamping would silently schedule the entry
    /// for its expiry, where it becomes due only once it has already expired —
    /// a scheduled prompt that can never run, with nothing on screen saying so.
    ///
    /// Carries both instants, which are entry state rather than user data, so a
    /// client can explain the conflict without being handed the prompt text.
    #[error(
        "user prompt queue entry is due at {run_at_ms}, at or after its expiry {expires_at_ms}"
    )]
    DueTimeAfterExpiry { run_at_ms: i64, expires_at_ms: i64 },
    #[error("user prompt queue storage error: {0}")]
    Storage(#[from] StorageError),
}

impl UserPromptQueueError {
    /// Stable wire code for `CoreResponse::Error`.
    ///
    /// Every variant is distinct, so a caller can tell a full backlog from a
    /// full retention window from a vanished entry. No variant here has no
    /// producer: `Invalid` from the field validators, `TooManyEntries` from
    /// `check_capacity`, `EntryTooLarge` from `validate_entry`, `NotFound`
    /// from `get`, `Expired` from `get_live`, `Storage` from any driver
    /// error, the four M002b variants from the mutation and ownership gates
    /// below, M003's `Delivering` from the operable-status gate, and M007's
    /// `DueTimeAfterExpiry` from `validate_run_at` on the add and edit paths.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid { .. } => "user_prompt_queue_invalid",
            Self::TooManyEntries { .. } => "user_prompt_queue_too_many_entries",
            Self::EntryTooLarge { .. } => "user_prompt_queue_entry_too_large",
            Self::NotFound => "user_prompt_queue_not_found",
            Self::Expired => "user_prompt_queue_expired",
            Self::NotQueueOwner => "user_prompt_queue_not_owner",
            Self::QueueUnowned => "user_prompt_queue_unowned",
            Self::OwnerRevisionConflict { .. } => "user_prompt_queue_owner_revision_conflict",
            Self::OrderMismatch => "user_prompt_queue_order_mismatch",
            Self::Delivering => "user_prompt_queue_entry_delivering",
            Self::DueTimeAfterExpiry { .. } => "user_prompt_queue_due_time_after_expiry",
            Self::Storage(_) => "user_prompt_queue_storage_error",
        }
    }
}

/// M002b (ADR-0037): the durable queue-owner binding for one session.
///
/// `revision` is the transfer fence, exactly as `session_turn_controller`'s is
/// for the turn lease. It is **not** the turn lease: see
/// [`UserPromptQueueOwnership`] for why the two ownership notions must stay
/// distinguishable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserPromptQueueOwnerRecord {
    pub session_id: String,
    pub owner_principal: String,
    pub revision: u64,
    pub adopted_at_ms: i64,
    pub updated_at_ms: i64,
}

/// M002b (ADR-0037): who may mutate this session's queue — a three-valued
/// fact whose values are **not** interchangeable.
///
/// | State | Meaning | Who may mutate |
/// |---|---|---|
/// | `Absent` | no owner row **and** no entries — the queue has never existed | the first `add`, which creates both in one transaction |
/// | `Owned` | an owner row names a principal | that principal |
/// | `Unowned` | entries exist but **no** owner row | nobody — an invalid state |
///
/// `Unowned` fails closed on **every** mutation including `add`, and is repaired
/// only by an explicit adoption. First-add must not silently adopt here, or an
/// invalidated queue would quietly become owned by whoever touched it next.
///
/// `Unowned` is unreachable through this milestone's own code paths, because the
/// only path that creates entries also creates the owner atomically. It is a
/// defensive state required by ADR-0037 rather than by an observed defect, and
/// the tests construct it directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserPromptQueueOwnership {
    Absent,
    Owned(UserPromptQueueOwnerRecord),
    Unowned,
}

impl UserPromptQueueOwnership {
    /// The recorded owner, if any. `Absent` and `Unowned` are both "no owner",
    /// which is why callers must not collapse the two: they carry different
    /// mutation rules.
    pub fn owner_principal(&self) -> Option<&str> {
        match self {
            Self::Owned(record) => Some(record.owner_principal.as_str()),
            Self::Absent | Self::Unowned => None,
        }
    }

    pub fn revision(&self) -> Option<u64> {
        match self {
            Self::Owned(record) => Some(record.revision),
            Self::Absent | Self::Unowned => None,
        }
    }
}

#[derive(Debug, FromRow)]
struct QueueEntryRow {
    id: String,
    session_id: String,
    ordinal: i64,
    text: String,
    text_bytes: i64,
    status: String,
    failure_reason: Option<String>,
    created_at_ms: i64,
    updated_at_ms: i64,
    expires_at_ms: i64,
    delivered_turn_id: Option<String>,
    delivered_at_ms: Option<i64>,
    run_at_ms: Option<i64>,
}

impl TryFrom<QueueEntryRow> for UserPromptQueueEntryRecord {
    type Error = UserPromptQueueError;

    fn try_from(row: QueueEntryRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            session_id: row.session_id,
            ordinal: row.ordinal,
            text: row.text,
            text_bytes: usize::try_from(row.text_bytes).map_err(|_| {
                UserPromptQueueError::Invalid {
                    field: "text_bytes",
                    message: "stored byte count is negative".to_owned(),
                }
            })?,
            status: UserPromptQueueStatus::parse(&row.status)?,
            failure_reason: row.failure_reason,
            created_at_ms: row.created_at_ms,
            updated_at_ms: row.updated_at_ms,
            expires_at_ms: row.expires_at_ms,
            delivered_turn_id: row.delivered_turn_id,
            delivered_at_ms: row.delivered_at_ms,
            run_at_ms: row.run_at_ms,
        })
    }
}

const ENTRY_COLUMNS: &str = "id, session_id, ordinal, text, text_bytes, status, failure_reason, \
                            created_at_ms, updated_at_ms, expires_at_ms, delivered_turn_id, \
                            delivered_at_ms, run_at_ms";

/// Session-owned store for the durable user prompt queue.
///
/// Reads never mutate user data; they do evaluate expiry lazily, which is the
/// one write this slice performs and the only thing that gives `expired` a
/// producer. There is no mutation API, by design.
#[derive(Debug, Clone)]
pub struct UserPromptQueueStore {
    pool: SqlitePool,
}

impl UserPromptQueueStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Read the live queue for one session in daemon-owned ordinal order.
    ///
    /// Expiry is evaluated here, lazily and deterministically: entries whose
    /// `expires_at_ms` is due are marked `expired` inside the same transaction
    /// that then selects the survivors, so a crash mid-sweep leaves either all
    /// or none of the due entries expired. An entry that is already `expired`
    /// matches no row, which makes repeated reads idempotent without a second
    /// write.
    ///
    /// **M003: the sweep and the selection both cover `delivering`.** A claim is
    /// made under the entry's TTL like any other live state, so a `delivering`
    /// row that never resolves — a daemon killed between claim and receipt —
    /// must still expire. Leaving it out of the sweep would let a stranded row
    /// outlive its TTL forever while still being counted as live backlog.
    pub async fn list_live(
        &self,
        session_id: &str,
        now_ms: i64,
    ) -> Result<Vec<UserPromptQueueLiveEntry>, UserPromptQueueError> {
        validate_session_id(session_id)?;
        let mut tx = self.pool.begin().await.map_err(storage_error)?;

        sqlx::query(
            "UPDATE user_prompt_queue_entry SET status = 'expired', updated_at_ms = ? \
             WHERE session_id = ? AND status IN ('pending', 'delivering') AND expires_at_ms <= ?",
        )
        .bind(now_ms)
        .bind(session_id)
        .bind(now_ms)
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;

        let rows: Vec<QueueEntryRow> = sqlx::query_as(&format!(
            "SELECT {ENTRY_COLUMNS} FROM user_prompt_queue_entry \
             WHERE session_id = ? AND {LIVE_STATUS_PREDICATE} AND expires_at_ms > ? \
             ORDER BY ordinal ASC LIMIT ?"
        ))
        .bind(session_id)
        .bind(now_ms)
        .bind(USER_PROMPT_QUEUE_MAX_RETAINED_PER_SESSION)
        .fetch_all(&mut *tx)
        .await
        .map_err(storage_error)?;

        tx.commit().await.map_err(storage_error)?;

        rows.into_iter()
            .map(|row| UserPromptQueueEntryRecord::try_from(row).map(|r| live_from(&r)))
            .collect()
    }

    /// Read one entry by identity regardless of status. This is the direct store
    /// read the expiry tests use to observe a terminal row; it performs no
    /// expiry sweep of its own, so it observes exactly what the last `list_live`
    /// committed.
    pub async fn get(
        &self,
        entry_id: &str,
    ) -> Result<UserPromptQueueEntryRecord, UserPromptQueueError> {
        validate_entry_id(entry_id)?;
        let row: Option<QueueEntryRow> = sqlx::query_as(&format!(
            "SELECT {ENTRY_COLUMNS} FROM user_prompt_queue_entry WHERE id = ?"
        ))
        .bind(entry_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(storage_error)?;
        let row = row.ok_or(UserPromptQueueError::NotFound)?;
        UserPromptQueueEntryRecord::try_from(row)
    }

    /// Read one entry only if it is still live at `now_ms`.
    ///
    /// Distinguishes [`UserPromptQueueError::NotFound`] (no such entry) from
    /// [`UserPromptQueueError::Expired`] (it exists but is due), which a caller
    /// needs in order to distinguish a gone queue from a swept one. It does not
    /// sweep: sweep status is owned by `list_live`.
    pub async fn get_live(
        &self,
        entry_id: &str,
        now_ms: i64,
    ) -> Result<UserPromptQueueEntryRecord, UserPromptQueueError> {
        let record = self.get(entry_id).await?;
        if record.status == UserPromptQueueStatus::Expired || record.expires_at_ms <= now_ms {
            return Err(UserPromptQueueError::Expired);
        }
        Ok(record)
    }

    /// Report whether one more entry may be added to this session's queue.
    ///
    /// The count bounds have no write path until M002b, so this is where they
    /// are enforced today: a read that refuses rather than truncates. It counts
    /// live entries and total retained rows independently, because the two
    /// bounds fail for different reasons and a caller must be able to tell a
    /// full backlog from a full retention window.
    ///
    /// **M003: `delivering` counts toward the backlog.** A claimed entry is still
    /// occupying a queue slot — its turn is running or about to be — so a user
    /// who can enqueue past it would let the backlog exceed its bound by however
    /// many deliveries are in flight.
    pub async fn check_capacity(
        &self,
        session_id: &str,
        now_ms: i64,
    ) -> Result<(), UserPromptQueueError> {
        validate_session_id(session_id)?;
        let pending: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM user_prompt_queue_entry \
             WHERE session_id = ? AND status IN ('pending', 'delivering') AND expires_at_ms > ?",
        )
        .bind(session_id)
        .bind(now_ms)
        .fetch_one(&self.pool)
        .await
        .map_err(storage_error)?;
        if pending >= USER_PROMPT_QUEUE_MAX_PENDING {
            return Err(UserPromptQueueError::TooManyEntries {
                limit: USER_PROMPT_QUEUE_MAX_PENDING,
            });
        }
        let retained: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM user_prompt_queue_entry WHERE session_id = ?")
                .bind(session_id)
                .fetch_one(&self.pool)
                .await
                .map_err(storage_error)?;
        if retained >= USER_PROMPT_QUEUE_MAX_RETAINED_PER_SESSION {
            return Err(UserPromptQueueError::TooManyEntries {
                limit: USER_PROMPT_QUEUE_MAX_RETAINED_PER_SESSION,
            });
        }
        Ok(())
    }

    /// Validate one candidate entry's bounded fields without persisting it.
    ///
    /// The per-entry length bound is a typed refusal, never a silent
    /// shortening: a truncated entry would be a different prompt from the one
    /// the user wrote. Returns the exact byte count to store, so the caller's
    /// `text_bytes` and `text` cannot disagree.
    pub fn validate_entry(
        entry_id: &str,
        session_id: &str,
        text: &str,
    ) -> Result<usize, UserPromptQueueError> {
        validate_entry_id(entry_id)?;
        validate_session_id(session_id)?;
        if text.trim().is_empty() {
            return Err(UserPromptQueueError::Invalid {
                field: "text",
                message: "queue entry text must not be empty".to_owned(),
            });
        }
        let text_bytes = text.len();
        if text_bytes > USER_PROMPT_QUEUE_MAX_ENTRY_BYTES {
            return Err(UserPromptQueueError::EntryTooLarge {
                limit: USER_PROMPT_QUEUE_MAX_ENTRY_BYTES,
            });
        }
        Ok(text_bytes)
    }

    /// Every entry a session owns, in ordinal order, including terminal rows.
    /// Test and diagnostic surface: the read projection deliberately exposes
    /// only live entries.
    pub async fn list_all_for_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<UserPromptQueueEntryRecord>, UserPromptQueueError> {
        validate_session_id(session_id)?;
        let rows: Vec<QueueEntryRow> = sqlx::query_as(&format!(
            "SELECT {ENTRY_COLUMNS} FROM user_prompt_queue_entry \
             WHERE session_id = ? ORDER BY ordinal ASC"
        ))
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(storage_error)?;
        rows.into_iter()
            .map(UserPromptQueueEntryRecord::try_from)
            .collect()
    }
}

/// M002b (ADR-0037): the mutation slice.
///
/// A separate `impl` block on purpose: the M002a read surface above is closed
/// and reviewed, and keeping this milestone's additions in their own block makes
/// the boundary readable rather than interleaved.
///
/// **Authorization is evaluated inside the transaction that writes.** Every
/// mutating method resolves [`UserPromptQueueOwnership`] on its own connection
/// before touching a row, so ownership and mutation are never separable by a
/// crash: a rejected mutation leaves no partial write, and a committed mutation
/// was authorized by the same owner row it acted under.
impl UserPromptQueueStore {
    /// Resolve who may mutate this session's queue.
    ///
    /// The three-valued read that [`UserPromptQueueOwnership`] documents. It is
    /// a read of two tables and deliberately does no write, so it cannot be the
    /// place where a decision is made for a mutation — the mutating methods
    /// resolve it again inside their own transaction.
    pub async fn ownership(
        &self,
        session_id: &str,
    ) -> Result<UserPromptQueueOwnership, UserPromptQueueError> {
        validate_session_id(session_id)?;
        let mut tx = self.pool.begin().await.map_err(storage_error)?;
        let ownership = resolve_ownership(&mut tx, session_id).await?;
        tx.commit().await.map_err(storage_error)?;
        Ok(ownership)
    }

    /// Append one entry, adopting ownership in the same transaction when the
    /// queue is `Absent`.
    ///
    /// The adoption policy ADR-0037 deferred to this milestone: **the calling
    /// principal becomes the owner.** Not the session creator, and not the local
    /// interactive owner — the caller, so the binding records who actually
    /// authored the queue. This is the audit decision ADR-0037's "Neutral or
    /// deferred" section required here.
    ///
    /// This is the only path that creates both the owner row and an entry, and
    /// it does so in one transaction, so a crash leaves neither or both — never
    /// an owner with no queue or a queue with no owner.
    ///
    /// `entry_id` is client-supplied and is the table's primary key, so a
    /// duplicate retry converges on the existing entry instead of creating a
    /// second one. That is at-most-once by identity and adds **no** receipt
    /// namespace, which ADR-0035 forbids.
    pub async fn add(
        &self,
        caller: &str,
        session_id: &str,
        entry_id: &str,
        text: &str,
        run_at_ms: Option<i64>,
        now_ms: i64,
    ) -> Result<UserPromptQueueLiveEntry, UserPromptQueueError> {
        validate_principal(caller)?;
        let text_bytes = Self::validate_entry(entry_id, session_id, text)?;
        // The expiry is derived here, so the due time is validated against the
        // exact value the row will carry rather than a recomputed guess. This
        // runs before the transaction: it is pure validation over two ints, and
        // refusing early keeps a bad due time from ever opening a write.
        let expires_at_ms = now_ms.saturating_add(USER_PROMPT_QUEUE_TTL_MS);
        validate_run_at(run_at_ms, expires_at_ms)?;
        let mut tx = begin_immediate(&self.pool).await?;

        let ownership = resolve_ownership(&mut tx, session_id).await?;
        match &ownership {
            UserPromptQueueOwnership::Owned(record) if record.owner_principal != caller => {
                // Fail before any write. `NotQueueOwner` names no owner.
                return Err(UserPromptQueueError::NotQueueOwner);
            }
            // An unowned queue is an invalid state with no authorized mutator.
            // First-add must NOT adopt here, or whoever happened to touch an
            // invalidated queue would silently become its owner — the exact
            // silent widening ADR-0037 exists to prevent.
            UserPromptQueueOwnership::Unowned => return Err(UserPromptQueueError::QueueUnowned),
            UserPromptQueueOwnership::Owned(_) | UserPromptQueueOwnership::Absent => {}
        }

        check_capacity_in(&mut tx, session_id, now_ms).await?;

        // `Absent` creates the owner row here, inside the same transaction as
        // the entry insert below.
        if matches!(ownership, UserPromptQueueOwnership::Absent) {
            sqlx::query(
                "INSERT INTO user_prompt_queue_owner \
                 (session_id, owner_principal, revision, adopted_at_ms, updated_at_ms) \
                 VALUES (?, ?, 0, ?, ?)",
            )
            .bind(session_id)
            .bind(caller)
            .bind(now_ms)
            .bind(now_ms)
            .execute(&mut **tx)
            .await
            .map_err(storage_error)?;
        }

        // `UNIQUE (session_id, ordinal)` spans every row, including terminal
        // ones, so the append ordinal is taken over all of them — not just the
        // live pending set, which would collide with a retained expired row.
        let next: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(ordinal), -1) + 1 FROM user_prompt_queue_entry WHERE session_id = ?",
        )
        .bind(session_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;

        let inserted = sqlx::query(
            "INSERT INTO user_prompt_queue_entry \
             (id, session_id, ordinal, text, text_bytes, status, failure_reason, \
              created_at_ms, updated_at_ms, expires_at_ms, delivered_turn_id, delivered_at_ms, \
              run_at_ms) \
             VALUES (?, ?, ?, ?, ?, 'pending', NULL, ?, ?, ?, NULL, NULL, ?) \
             ON CONFLICT(id) DO NOTHING",
        )
        .bind(entry_id)
        .bind(session_id)
        .bind(next)
        .bind(text)
        .bind(text_bytes as i64)
        .bind(now_ms)
        .bind(now_ms)
        .bind(expires_at_ms)
        .bind(run_at_ms)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;

        let entry = if inserted.rows_affected() == 0 {
            // Convergence path: the id already exists. It is this caller's
            // convergence only if it belongs to this session and is still live;
            // otherwise the collision is refused rather than silently resolved
            // in the caller's favour, and an expired row is never resurrected.
            let existing: Option<QueueEntryRow> = sqlx::query_as(&format!(
                "SELECT {ENTRY_COLUMNS} FROM user_prompt_queue_entry WHERE id = ?"
            ))
            .bind(entry_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(storage_error)?;
            let existing = existing.ok_or(UserPromptQueueError::NotFound)?;
            if existing.session_id != session_id {
                return Err(UserPromptQueueError::Invalid {
                    field: "id",
                    message: "entry identity is already used by another session".to_owned(),
                });
            }
            if existing.status == UserPromptQueueStatus::Pending.as_str()
                && existing.expires_at_ms > now_ms
            {
                existing
            } else {
                // M003: an id being reused mid-delivery belongs to this session's
                // own in-flight claim, so report that rather than claiming it
                // expired — the caller cannot act on either answer, but "expired"
                // would be false and could be shown to a user.
                return Err(
                    if existing.status == UserPromptQueueStatus::Delivering.as_str() {
                        UserPromptQueueError::Delivering
                    } else {
                        UserPromptQueueError::Expired
                    },
                );
            }
        } else {
            let row: QueueEntryRow = sqlx::query_as(&format!(
                "SELECT {ENTRY_COLUMNS} FROM user_prompt_queue_entry WHERE id = ?"
            ))
            .bind(entry_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(storage_error)?;
            row
        };

        let record = UserPromptQueueEntryRecord::try_from(entry)?;
        tx.commit().await?;
        Ok(live_from(&record))
    }

    /// Replace one entry's text and due time, leaving its identity and position
    /// untouched.
    ///
    /// Bounds are re-checked: an over-bound edit is refused, never truncated,
    /// because a shortened entry would be a different prompt from the one the
    /// user wrote. An edit against an expired entry is refused and cannot
    /// resurrect it — expiry is evaluated first, in this same transaction.
    ///
    /// M007: `run_at_ms` is a first-class editable field here, not an implicit
    /// re-add. Passing `None` clears the schedule and returns the entry to
    /// immediate-eligible, which is a distinct state from a future due time and
    /// not merely a due time of now.
    pub async fn edit(
        &self,
        caller: &str,
        session_id: &str,
        entry_id: &str,
        text: &str,
        run_at_ms: Option<i64>,
        now_ms: i64,
    ) -> Result<UserPromptQueueLiveEntry, UserPromptQueueError> {
        validate_principal(caller)?;
        let text_bytes = Self::validate_entry(entry_id, session_id, text)?;
        let mut tx = begin_immediate(&self.pool).await?;
        authorize_owned_queue(&mut tx, caller, session_id).await?;
        sweep_expired(&mut tx, session_id, now_ms).await?;

        // M007: the new due time is validated against the entry's **stored**
        // expiry, not a recomputed `now + TTL`. Recomputing would be wrong for
        // any entry queued earlier, and would let an edit stretch an entry past
        // the horizon it will actually be swept at.
        //
        // Validation is gated on the row being operable under the *same*
        // predicate the UPDATE below uses, so this introduces no new error
        // precedence: a missing, expired, or `delivering` entry still reports
        // through `refusal_for` exactly as it did before M007, and only an entry
        // that would really have been edited can fail the due-time rule.
        let target = load_entry(&mut tx, entry_id).await?;
        if let Some(row) = target.as_ref() {
            let operable = row.session_id == session_id
                && row.status == UserPromptQueueStatus::Pending.as_str()
                && row.expires_at_ms > now_ms;
            if operable {
                validate_run_at(run_at_ms, row.expires_at_ms)?;
            }
        }

        let updated = sqlx::query(
            "UPDATE user_prompt_queue_entry \
             SET text = ?, text_bytes = ?, run_at_ms = ?, updated_at_ms = ? \
             WHERE id = ? AND session_id = ? AND status = 'pending' AND expires_at_ms > ?",
        )
        .bind(text)
        .bind(text_bytes as i64)
        .bind(run_at_ms)
        .bind(now_ms)
        .bind(entry_id)
        .bind(session_id)
        .bind(now_ms)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;

        if updated.rows_affected() == 0 {
            // Distinguish "no such entry" from "expired"; the sweep above has
            // already made the second case terminal.
            let existing = load_entry(&mut tx, entry_id).await?;
            return Err(refusal_for(existing.as_ref(), session_id, now_ms));
        }

        let row = load_entry(&mut tx, entry_id)
            .await?
            .ok_or(UserPromptQueueError::NotFound)?;
        let record = UserPromptQueueEntryRecord::try_from(row)?;
        tx.commit().await?;
        Ok(live_from(&record))
    }

    /// Remove one entry and renumber the rest densely.
    ///
    /// The renumber keeps the queue a dense total order, which is what makes
    /// `max(ordinal) + 1` a correct append point and what makes a reorder's
    /// target ordinals well defined.
    ///
    /// **C001:** the survivor set passed to the renumber is the *operable*
    /// subset, so this path is where a returned-but-not-yet-reordered entry gets
    /// pulled back into the band. Before C001 a `delivering` row was displaced
    /// by this call and never written back, which is how an entry released by a
    /// later path could be stranded outside the dense band.
    pub async fn delete(
        &self,
        caller: &str,
        session_id: &str,
        entry_id: &str,
        now_ms: i64,
    ) -> Result<Vec<UserPromptQueueLiveEntry>, UserPromptQueueError> {
        validate_principal(caller)?;
        validate_entry_id(entry_id)?;
        validate_session_id(session_id)?;
        let mut tx = begin_immediate(&self.pool).await?;
        authorize_owned_queue(&mut tx, caller, session_id).await?;
        sweep_expired(&mut tx, session_id, now_ms).await?;

        let deleted = sqlx::query(
            "DELETE FROM user_prompt_queue_entry \
             WHERE id = ? AND session_id = ? AND status = 'pending'",
        )
        .bind(entry_id)
        .bind(session_id)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;

        if deleted.rows_affected() == 0 {
            let existing = load_entry(&mut tx, entry_id).await?;
            return Err(refusal_for(existing.as_ref(), session_id, now_ms));
        }

        // Close the gap the delete left, so the queue stays a dense total
        // order. Without this the next `add`'s `max(ordinal) + 1` would keep
        // growing past the survivors and a reorder's target ordinals would not
        // match the queue.
        let survivors = list_live_in(&mut tx, session_id, now_ms).await?;
        let survivor_ids: Vec<String> = survivors.iter().map(|e| e.id.clone()).collect();
        renumber_dense(&mut tx, session_id, now_ms, &survivor_ids).await?;

        let projection = list_projected_in(&mut tx, session_id, now_ms).await?;
        tx.commit().await?;
        Ok(projection)
    }

    /// Write a new daemon-owned total order for this queue.
    ///
    /// `entry_ids` is the **complete** desired order of the live entries, not a
    /// patch and not a move. It must be a permutation of the live set: a stale
    /// client view that dropped or invented an id is refused with
    /// [`UserPromptQueueError::OrderMismatch`] rather than partially applied,
    /// because a partial reorder is exactly the interleaving ADR-0037 forbids.
    ///
    /// Reorder is its own operation rather than an index argument smuggled
    /// through another request, which ADR-0035 forbids.
    ///
    /// **C001:** the client still sends the *operable* permutation only. A
    /// `delivering` row is not nameable and is not given a position here — it
    /// holds the rank it already had among live rows, which is what
    /// [`merge_operable_into_live`] decides. The client's contract is unchanged
    /// by C001; only the store's write set grew.
    pub async fn reorder(
        &self,
        caller: &str,
        session_id: &str,
        entry_ids: &[String],
        now_ms: i64,
    ) -> Result<Vec<UserPromptQueueLiveEntry>, UserPromptQueueError> {
        validate_principal(caller)?;
        validate_session_id(session_id)?;
        for entry_id in entry_ids {
            validate_entry_id(entry_id)?;
        }
        let mut tx = begin_immediate(&self.pool).await?;
        authorize_owned_queue(&mut tx, caller, session_id).await?;
        sweep_expired(&mut tx, session_id, now_ms).await?;

        let live = list_live_in(&mut tx, session_id, now_ms).await?;
        if !is_permutation(&live, entry_ids) {
            return Err(UserPromptQueueError::OrderMismatch);
        }

        renumber_dense(&mut tx, session_id, now_ms, entry_ids).await?;

        let projection = list_projected_in(&mut tx, session_id, now_ms).await?;
        tx.commit().await?;
        Ok(projection)
    }

    /// Explicitly claim ownership of this session's queue.
    ///
    /// This is the **only** operation that converts `Unowned` to `Owned`, which
    /// is what ADR-0037 requires: an unowned queue has no authorized mutator,
    /// and is not implicitly owned by the session creator or by the local
    /// interactive owner. It is a repair operation, so it is explicit and
    /// audited by construction.
    ///
    /// - `Absent` → creates the owner row. An explicit, deliberate act.
    /// - `Unowned` → creates the owner row. The repair path.
    /// - `Owned(caller)` → idempotent; the existing record is returned with its
    ///   revision unchanged, so a repeated adopt cannot churn the transfer
    ///   fence out from under a concurrent transfer.
    /// - `Owned(other)` → refused. Adoption never seizes an owned queue; that
    ///   is what transfer is for.
    pub async fn adopt(
        &self,
        caller: &str,
        session_id: &str,
        now_ms: i64,
    ) -> Result<UserPromptQueueOwnerRecord, UserPromptQueueError> {
        validate_principal(caller)?;
        validate_session_id(session_id)?;
        let mut tx = begin_immediate(&self.pool).await?;
        let record = match resolve_ownership(&mut tx, session_id).await? {
            UserPromptQueueOwnership::Owned(record) => {
                if record.owner_principal != caller {
                    return Err(UserPromptQueueError::NotQueueOwner);
                }
                record
            }
            UserPromptQueueOwnership::Absent | UserPromptQueueOwnership::Unowned => {
                sqlx::query(
                    "INSERT INTO user_prompt_queue_owner \
                     (session_id, owner_principal, revision, adopted_at_ms, updated_at_ms) \
                     VALUES (?, ?, 0, ?, ?)",
                )
                .bind(session_id)
                .bind(caller)
                .bind(now_ms)
                .bind(now_ms)
                .execute(&mut **tx)
                .await
                .map_err(storage_error)?;
                load_owner(&mut tx, session_id)
                    .await?
                    .ok_or(UserPromptQueueError::NotFound)?
            }
        };
        tx.commit().await?;
        Ok(record)
    }

    /// Move ownership to `recipient` under a compare-and-set revision fence.
    ///
    /// Mirrors [`crate::session_control::SessionControllerStore::transfer`],
    /// including its `expected_revision` fence: two concurrent transfers both
    /// read the same revision and exactly one `UPDATE` matches, so the loser is
    /// refused by name with
    /// [`UserPromptQueueError::OwnerRevisionConflict`] rather than silently
    /// overwriting. The fence is what keeps a queue from ever having two owners.
    ///
    /// Only the current owner may transfer, matching
    /// `session_control_transfer`'s controller-only rule.
    pub async fn transfer(
        &self,
        caller: &str,
        recipient: &str,
        expected_revision: u64,
        session_id: &str,
        now_ms: i64,
    ) -> Result<UserPromptQueueOwnerRecord, UserPromptQueueError> {
        validate_principal(caller)?;
        validate_principal(recipient)?;
        validate_session_id(session_id)?;
        let expected = i64::try_from(expected_revision).unwrap_or(i64::MAX);
        let mut tx = begin_immediate(&self.pool).await?;

        let current = match resolve_ownership(&mut tx, session_id).await? {
            UserPromptQueueOwnership::Owned(record) => record,
            // No queue to hand over. Deliberately the same refusal as an absent
            // queue elsewhere: adoption, not transfer, is how an ownerless queue
            // gets one.
            UserPromptQueueOwnership::Absent => return Err(UserPromptQueueError::NotFound),
            UserPromptQueueOwnership::Unowned => return Err(UserPromptQueueError::QueueUnowned),
        };
        if current.owner_principal != caller {
            return Err(UserPromptQueueError::NotQueueOwner);
        }

        let moved = sqlx::query(
            "UPDATE user_prompt_queue_owner \
             SET owner_principal = ?, revision = revision + 1, updated_at_ms = ? \
             WHERE session_id = ? AND owner_principal = ? AND revision = ?",
        )
        .bind(recipient)
        .bind(now_ms)
        .bind(session_id)
        .bind(caller)
        .bind(expected)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;

        let record = if moved.rows_affected() > 0 {
            load_owner(&mut tx, session_id)
                .await?
                .ok_or(UserPromptQueueError::NotFound)?
        } else {
            // Diagnose the stale write. The reread is inside the same
            // transaction, so it sees the winner's committed revision or, under
            // SQLite's single-writer model, the pre-transfer state.
            let observed = load_owner(&mut tx, session_id).await?;
            return Err(match observed {
                Some(record) if record.owner_principal != caller => {
                    UserPromptQueueError::NotQueueOwner
                }
                Some(record) => UserPromptQueueError::OwnerRevisionConflict {
                    expected: expected_revision,
                    current: record.revision,
                },
                None => UserPromptQueueError::NotFound,
            });
        };

        tx.commit().await?;
        Ok(record)
    }
}

/// Open a write transaction that takes SQLite's write lock **at `BEGIN`**.
///
/// Every read-then-write path in this store goes through here. See
/// [`ImmediateWrite`] for why that is not the same as `pool.begin()`.
///
/// This is a plain function rather than an associated one so the call sites read
/// `begin_immediate(&self.pool)` instead of naming the type twice.
async fn begin_immediate(pool: &SqlitePool) -> Result<ImmediateWrite, UserPromptQueueError> {
    ImmediateWrite::begin(pool).await
}

/// A write transaction that takes SQLite's write lock **at `BEGIN`**, rather than
/// upgrading a read lock at the first write.
///
/// # C002 — why this type exists
///
/// [`sqlx::SqlitePool::begin`] issues a **deferred** `BEGIN`, which takes no lock
/// at all. The first `SELECT` in that transaction then holds a shared read lock,
/// and the first `UPDATE` has to *upgrade* it to a write lock. SQLite refuses
/// that upgrade with `SQLITE_BUSY` — code 6, `database is deadlocked` — and does
/// **not** call the busy handler, because waiting could deadlock. `busy_timeout`
/// therefore never applies. Two connections that each read before either wrote
/// could **both** fail, rather than one winning and the other waiting its turn.
///
/// Measured on this store before the change: **204 of 256** concurrent claim
/// rounds returned that error to at least one caller
/// (`user_prompt_queue_concurrent_claims_never_deadlock_on_lock_upgrade`).
///
/// `BEGIN IMMEDIATE` takes the write lock **before** any read, and that
/// acquisition *does* honour `busy_timeout`. A second writer waits for the first
/// to commit and then proceeds — which is the behavior every path here already
/// assumed.
///
/// Holding the write lock for the whole transaction is the point, not a cost.
/// Each path reads a row, decides from what it read, and writes a decision that
/// must not be invalidated in between; the read-to-write window is precisely the
/// gap a concurrent writer would use. Serializing the path closes it.
///
/// # Reads are deliberately excluded
///
/// `list_live` sweeps expired rows inside its transaction, so it does write — but
/// its **first** statement is that sweep, which already takes the write lock at
/// that statement and gains nothing from taking it earlier. The same is true of
/// `mark_delivered`, `mark_delivery_failed`, `release_delivery`, and
/// `retry_failed`: each is a single guarded `UPDATE` with no prior read, so
/// there is no read lock to upgrade. Only the read-then-write paths need this.
///
/// # Reuse
///
/// The type lives here, next to the store that established the pattern, because
/// the C002 guard pins it to this file. Another store in this crate with the
/// same read-then-write shape opens its transaction with
/// [`ImmediateWrite::begin_with`] and maps the error into its own type, rather
/// than taking this module's error type.
pub(crate) struct ImmediateWrite {
    /// `Option` so [`Drop`] can take the connection out; it is `Some` for the
    /// whole time the transaction is usable.
    conn: Option<sqlx::pool::PoolConnection<sqlx::Sqlite>>,
    settled: bool,
}

/// The one call that has to name sqlx's transaction machinery fully, three times
/// per method, or once each here.
type SqliteTx = sqlx::sqlite::SqliteTransactionManager;

impl ImmediateWrite {
    async fn begin(pool: &SqlitePool) -> Result<Self, UserPromptQueueError> {
        Self::begin_with(pool, storage_error).await
    }

    /// Begin an immediate transaction, mapping the raw sqlx error with `map`.
    ///
    /// `BEGIN IMMEDIATE` takes SQLite's write lock **before** any read, and that
    /// acquisition honours `busy_timeout`. A deferred transaction takes a shared
    /// read lock on its first `SELECT` and then has to upgrade it, which SQLite
    /// refuses with `SQLITE_BUSY` without waiting — so two callers could both
    /// fail instead of one winning and the other waiting its turn.
    pub(crate) async fn begin_with<E>(
        pool: &SqlitePool,
        map: impl Fn(sqlx::Error) -> E,
    ) -> Result<Self, E> {
        let mut conn = pool.acquire().await.map_err(&map)?;
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
        .await
        .map_err(&map)?;
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
    async fn commit(self) -> Result<(), UserPromptQueueError> {
        self.commit_with(storage_error).await
    }

    /// Commit, mapping the raw sqlx error with `map`.
    pub(crate) async fn commit_with<E>(mut self, map: impl Fn(sqlx::Error) -> E) -> Result<(), E> {
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

/// Resolve the three-valued ownership state on an open transaction.
async fn resolve_ownership(
    conn: &mut sqlx::SqliteConnection,
    session_id: &str,
) -> Result<UserPromptQueueOwnership, UserPromptQueueError> {
    if let Some(record) = load_owner(conn, session_id).await? {
        return Ok(UserPromptQueueOwnership::Owned(record));
    }
    let has_entries: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM user_prompt_queue_entry WHERE session_id = ?)",
    )
    .bind(session_id)
    .fetch_one(&mut *conn)
    .await
    .map_err(storage_error)?;
    Ok(if has_entries == 1 {
        UserPromptQueueOwnership::Unowned
    } else {
        UserPromptQueueOwnership::Absent
    })
}

async fn load_owner(
    conn: &mut sqlx::SqliteConnection,
    session_id: &str,
) -> Result<Option<UserPromptQueueOwnerRecord>, UserPromptQueueError> {
    let row: Option<(String, String, i64, i64, i64)> = sqlx::query_as(
        "SELECT session_id, owner_principal, revision, adopted_at_ms, updated_at_ms \
         FROM user_prompt_queue_owner WHERE session_id = ?",
    )
    .bind(session_id)
    .fetch_optional(&mut *conn)
    .await
    .map_err(storage_error)?;
    row.map(
        |(session_id, owner_principal, revision, adopted_at_ms, updated_at_ms)| {
            Ok(UserPromptQueueOwnerRecord {
                session_id,
                owner_principal,
                revision: u64::try_from(revision).map_err(|_| UserPromptQueueError::Invalid {
                    field: "revision",
                    message: "stored ownership revision is negative".to_owned(),
                })?,
                adopted_at_ms,
                updated_at_ms,
            })
        },
    )
    .transpose()
}

async fn load_entry(
    conn: &mut sqlx::SqliteConnection,
    entry_id: &str,
) -> Result<Option<QueueEntryRow>, UserPromptQueueError> {
    sqlx::query_as::<_, QueueEntryRow>(&format!(
        "SELECT {ENTRY_COLUMNS} FROM user_prompt_queue_entry WHERE id = ?"
    ))
    .bind(entry_id)
    .fetch_optional(&mut *conn)
    .await
    .map_err(storage_error)
}

/// The ownership gate shared by edit, delete, and reorder.
///
/// `Absent` is `NotFound` (there is nothing to mutate), `Unowned` is
/// `QueueUnowned` (an invalid state with no authorized mutator), and an owner
/// that is not the caller is `NotQueueOwner`. `add` states its own version
/// because first-add is the one operation allowed on an `Absent` queue.
async fn authorize_owned_queue(
    conn: &mut sqlx::SqliteConnection,
    caller: &str,
    session_id: &str,
) -> Result<UserPromptQueueOwnership, UserPromptQueueError> {
    match resolve_ownership(conn, session_id).await? {
        UserPromptQueueOwnership::Owned(record) => {
            if record.owner_principal != caller {
                return Err(UserPromptQueueError::NotQueueOwner);
            }
            Ok(UserPromptQueueOwnership::Owned(record))
        }
        UserPromptQueueOwnership::Absent => Err(UserPromptQueueError::NotFound),
        UserPromptQueueOwnership::Unowned => Err(UserPromptQueueError::QueueUnowned),
    }
}

/// Mark due live entries `expired`, inside the caller's transaction.
///
/// Identical in effect to the sweep on the read path; a mutation must not be
/// able to act on a row the read projection would already have hidden.
///
/// **M003:** covers `delivering` as well as `pending`, for the same reason the
/// read-path sweep does — a claim made under the TTL must not outlive it.
async fn sweep_expired(
    conn: &mut sqlx::SqliteConnection,
    session_id: &str,
    now_ms: i64,
) -> Result<(), UserPromptQueueError> {
    sqlx::query(
        "UPDATE user_prompt_queue_entry SET status = 'expired', updated_at_ms = ? \
         WHERE session_id = ? AND status IN ('pending', 'delivering') AND expires_at_ms <= ?",
    )
    .bind(now_ms)
    .bind(session_id)
    .bind(now_ms)
    .execute(&mut *conn)
    .await
    .map_err(storage_error)?;
    Ok(())
}

async fn check_capacity_in(
    conn: &mut sqlx::SqliteConnection,
    session_id: &str,
    now_ms: i64,
) -> Result<(), UserPromptQueueError> {
    let pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_prompt_queue_entry \
         WHERE session_id = ? AND status IN ('pending', 'delivering') AND expires_at_ms > ?",
    )
    .bind(session_id)
    .bind(now_ms)
    .fetch_one(&mut *conn)
    .await
    .map_err(storage_error)?;
    if pending >= USER_PROMPT_QUEUE_MAX_PENDING {
        return Err(UserPromptQueueError::TooManyEntries {
            limit: USER_PROMPT_QUEUE_MAX_PENDING,
        });
    }
    let retained: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM user_prompt_queue_entry WHERE session_id = ?")
            .bind(session_id)
            .fetch_one(&mut *conn)
            .await
            .map_err(storage_error)?;
    if retained >= USER_PROMPT_QUEUE_MAX_RETAINED_PER_SESSION {
        return Err(UserPromptQueueError::TooManyEntries {
            limit: USER_PROMPT_QUEUE_MAX_RETAINED_PER_SESSION,
        });
    }
    Ok(())
}

/// The **operable** set: live entries a mutation may act on.
///
/// M003 split this from [`list_projected_in`] deliberately. `delete` renumbers
/// every survivor by displacing it into a high band and then writing dense
/// targets, and `reorder` requires the client's list to be a permutation of it.
/// Both need the entries that are *actually mutable*, and folding a `delivering`
/// row into that set would either renumber a row whose ordinal is bound to an
/// in-flight admission or force a client to submit an entry it cannot reorder.
async fn list_live_in(
    conn: &mut sqlx::SqliteConnection,
    session_id: &str,
    now_ms: i64,
) -> Result<Vec<UserPromptQueueLiveEntry>, UserPromptQueueError> {
    let rows: Vec<QueueEntryRow> = sqlx::query_as(&format!(
        "SELECT {ENTRY_COLUMNS} FROM user_prompt_queue_entry \
         WHERE session_id = ? AND {OPERABLE_STATUS_PREDICATE} AND expires_at_ms > ? \
         ORDER BY ordinal ASC LIMIT ?"
    ))
    .bind(session_id)
    .bind(now_ms)
    .bind(USER_PROMPT_QUEUE_MAX_RETAINED_PER_SESSION)
    .fetch_all(&mut *conn)
    .await
    .map_err(storage_error)?;
    rows.into_iter()
        .map(|row| UserPromptQueueEntryRecord::try_from(row).map(|r| live_from(&r)))
        .collect()
}

/// The **projected** set: every live entry, including one mid-delivery.
///
/// This is what a mutation *returns*, so the projection a client receives after
/// its own edit still contains a `delivering` entry it did not touch. Returning
/// the operable subset instead would make an untouched row vanish from the
/// client's list as a side effect of an unrelated edit.
async fn list_projected_in(
    conn: &mut sqlx::SqliteConnection,
    session_id: &str,
    now_ms: i64,
) -> Result<Vec<UserPromptQueueLiveEntry>, UserPromptQueueError> {
    let rows: Vec<QueueEntryRow> = sqlx::query_as(&format!(
        "SELECT {ENTRY_COLUMNS} FROM user_prompt_queue_entry \
         WHERE session_id = ? AND {LIVE_STATUS_PREDICATE} AND expires_at_ms > ? \
         ORDER BY ordinal ASC LIMIT ?"
    ))
    .bind(session_id)
    .bind(now_ms)
    .bind(USER_PROMPT_QUEUE_MAX_RETAINED_PER_SESSION)
    .fetch_all(&mut *conn)
    .await
    .map_err(storage_error)?;
    rows.into_iter()
        .map(|row| UserPromptQueueEntryRecord::try_from(row).map(|r| live_from(&r)))
        .collect()
}

/// Why a mutation refused to act on a named entry.
///
/// M003 added the `Delivering` arm. Before it, every non-`pending` row was
/// terminal and mapping all of them to `Expired` was accurate; a `delivering`
/// row is live and will either become `delivered` or return to `pending`, so
/// reporting it as expired would be a lie a client may render to a user.
fn refusal_for(
    existing: Option<&QueueEntryRow>,
    session_id: &str,
    now_ms: i64,
) -> UserPromptQueueError {
    match existing {
        Some(row)
            if row.session_id == session_id
                && row.status == UserPromptQueueStatus::Delivering.as_str() =>
        {
            UserPromptQueueError::Delivering
        }
        Some(row) if row.session_id == session_id && row.expires_at_ms <= now_ms => {
            UserPromptQueueError::Expired
        }
        Some(row)
            if row.session_id == session_id
                && row.status != UserPromptQueueStatus::Pending.as_str() =>
        {
            UserPromptQueueError::Expired
        }
        _ => UserPromptQueueError::NotFound,
    }
}

/// `true` when `target` lists exactly the operable entry ids, each once.
fn is_permutation(live: &[UserPromptQueueLiveEntry], target: &[String]) -> bool {
    if live.len() != target.len() {
        return false;
    }
    // `insert` returns true when the value was NEW, so requiring `true` is what
    // rejects a duplicate. Negating it would accept one.
    let mut seen = std::collections::HashSet::with_capacity(target.len());
    target
        .iter()
        .all(|id| seen.insert(id.as_str()) && live.iter().any(|entry| &entry.id == id))
}

/// C001: fold the caller's desired order for the **operable** set into the full
/// **live** order, holding every non-operable live row at the rank it already
/// holds.
///
/// `live` is the whole live set in ordinal order; `desired_operable` is the
/// permutation the caller validated against [`list_live_in`] — the `pending`
/// rows only, because a `delivering` row has an in-flight admission bound to it
/// and is not the caller's to place.
///
/// The rule is *preserve existing rank*, and it is decided rather than inferred:
/// a non-operable live row keeps the **slot** it occupied before the reorder. It
/// is never pinned to the head or to the tail. Pinning looks tidier and is a
/// one-line change here, but it would move a released entry to an end — which is
/// the very defect this milestone exists to remove.
///
/// Concretely, for `live = [p1(0), d1(1), p2(2)]` reordered to `[p2, p1]`, slot 0
/// is operable so it yields `p2`, slot 1 is not so it stays `d1`, slot 2 yields
/// `p1` — `[p2(0), d1(1), p1(2)]`. When `d1` is later released it resumes at
/// ordinal 1, between the two, which is where the user put it.
///
/// Refusing a length mismatch is a backstop, not the primary check: both callers
/// derive `desired_operable` from a read taken in this same transaction, under
/// the write lock `sweep_expired` already holds, so the two cannot disagree. It
/// exists so a future caller that forgets the contract gets a typed refusal
/// instead of a silently undensified row.
fn merge_operable_into_live(
    live: &[UserPromptQueueLiveEntry],
    desired_operable: &[String],
) -> Result<Vec<String>, UserPromptQueueError> {
    let mut desired = desired_operable.iter();
    let mut merged = Vec::with_capacity(live.len());
    for entry in live {
        if entry.status.is_operable() {
            let id = desired.next().ok_or(UserPromptQueueError::OrderMismatch)?;
            merged.push(id.clone());
        } else {
            merged.push(entry.id.clone());
        }
    }
    if desired.next().is_some() {
        return Err(UserPromptQueueError::OrderMismatch);
    }
    Ok(merged)
}

/// Renumber the whole **live** set densely, in the order [`merge_operable_into_live`]
/// produces.
///
/// C001 corrected the write set. It used to be the caller's operable ids alone,
/// while the displacement in front of it was **status-blind and session-wide** —
/// so a `delivering` row was pushed above the dense band and never re-densified.
/// Nothing wrote its ordinal back when delivery was released, retried, or
/// reconciled, so it returned to `pending` *outside* the band. Since
/// `claim_next_for_delivery` selects `ORDER BY ordinal ASC`, that is not a
/// rendering artefact: the entry was delivered last.
///
/// Writing the merged live order fixes **two** of the three return paths without
/// touching any of them, because a `delivering` row now *receives* a dense
/// ordinal and simply keeps it: `release_delivery` and
/// `resolve_stranded_delivery(Retry)`. The third, `retry_failed`, is **not**
/// reached — it revives a row that was already terminal, and a terminal row is
/// absent from the live set this function reads. See C001's closure record §6.1;
/// where such a row belongs is an open maintainer decision, not a detail to
/// settle here.
///
/// The displacement step below is unchanged, and it has to stay: terminal rows are
/// not in the live set but still hold ordinals under a session-wide `UNIQUE`, so
/// they must still be pushed clear of the band this function writes.
async fn renumber_dense(
    conn: &mut sqlx::SqliteConnection,
    session_id: &str,
    now_ms: i64,
    desired_operable: &[String],
) -> Result<(), UserPromptQueueError> {
    // Read before the displacement, so "the rank it already holds" is the rank
    // the caller saw rather than an artefact of the band offset.
    let live = list_projected_in(conn, session_id, now_ms).await?;
    let merged = merge_operable_into_live(&live, desired_operable)?;

    // Displace first, uniformly. Distinct ordinals stay distinct under a
    // uniform offset, and every displaced ordinal leaves the 0..n band the
    // dense targets occupy, so neither statement can violate
    // `UNIQUE (session_id, ordinal)`. This is the collision-free renumber; a
    // direct walk to the targets is not.
    sqlx::query("UPDATE user_prompt_queue_entry SET ordinal = ordinal + ? WHERE session_id = ?")
        .bind(USER_PROMPT_QUEUE_RENUMBER_BAND_OFFSET)
        .bind(session_id)
        .execute(&mut *conn)
        .await
        .map_err(storage_error)?;
    for (index, entry_id) in merged.iter().enumerate() {
        let ordinal = i64::try_from(index).map_err(|_| UserPromptQueueError::Invalid {
            field: "entry_ids",
            message: "queue length is not representable as an ordinal".to_owned(),
        })?;
        sqlx::query(
            "UPDATE user_prompt_queue_entry SET ordinal = ? WHERE id = ? AND session_id = ?",
        )
        .bind(ordinal)
        .bind(entry_id)
        .bind(session_id)
        .execute(&mut *conn)
        .await
        .map_err(storage_error)?;
    }
    Ok(())
}

/// Project a full record down to the live read shape.
///
/// One mapper, so `list_live`, the in-transaction list, `add`, and `edit` cannot
/// drift on which fields the projection carries.
fn live_from(record: &UserPromptQueueEntryRecord) -> UserPromptQueueLiveEntry {
    UserPromptQueueLiveEntry {
        id: record.id.clone(),
        ordinal: record.ordinal,
        text: record.text.clone(),
        text_bytes: record.text_bytes,
        // M003: the projection names the status instead of the caller inferring
        // it. A `delivering` entry is visible but not editable, and a client can
        // only grey that row if the status reaches it.
        status: record.status,
        created_at_ms: record.created_at_ms,
        expires_at_ms: record.expires_at_ms,
        run_at_ms: record.run_at_ms,
    }
}

/// M007: refuse a due time at or beyond the entry's own expiry.
///
/// This is the whole due-time validity rule, in one place, so `add` and `edit`
/// cannot drift on it — the same single-mapper discipline `live_from` follows.
///
/// A due time in the **past** is deliberately accepted: it is a time the user
/// has already passed, so the entry is eligible immediately. Refusing it would
/// be pedantry that turns a harmless slip into a refused add. That is also why
/// the comparison is `<` against the expiry and not a "must be in the future"
/// check — there are two independent bounds here, not one.
fn validate_run_at(run_at_ms: Option<i64>, expires_at_ms: i64) -> Result<(), UserPromptQueueError> {
    match run_at_ms {
        None => Ok(()),
        Some(run_at) if run_at < expires_at_ms => Ok(()),
        Some(run_at) => Err(UserPromptQueueError::DueTimeAfterExpiry {
            run_at_ms: run_at,
            expires_at_ms,
        }),
    }
}

fn validate_entry_id(entry_id: &str) -> Result<(), UserPromptQueueError> {
    if entry_id.is_empty() || entry_id.len() > 128 || entry_id.chars().any(char::is_control) {
        return Err(UserPromptQueueError::Invalid {
            field: "id",
            message: "entry identity is empty, too long, or contains control characters".to_owned(),
        });
    }
    Ok(())
}

/// Shape check for a principal recorded as a queue owner.
///
/// The daemon resolves a real `PrincipalId` before calling in; this keeps a
/// malformed value out of the owner row rather than trusting the caller to have
/// done it. Owner identity is not prompt text, so it may appear in errors and
/// audit records.
fn validate_principal(principal: &str) -> Result<(), UserPromptQueueError> {
    if principal.is_empty() || principal.len() > 128 || principal.chars().any(char::is_control) {
        return Err(UserPromptQueueError::Invalid {
            field: "principal",
            message: "principal is empty, too long, or contains control characters".to_owned(),
        });
    }
    Ok(())
}

/// M003 (ADR-0038): one entry claimed for delivery, with the attribution the
/// daemon must submit it under.
///
/// The store returns the claim, it does not perform the submission. Delivery
/// authority is a daemon-layer decision (ADR-0038), so the store's job stops at
/// "this row is now mine and here is who owns it".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserPromptQueueDeliveryClaim {
    pub entry: UserPromptQueueEntryRecord,
    /// The queue owner at claim time. The daemon re-authorizes this against
    /// **current** team state before submitting, rather than trusting this row.
    pub owner_principal: String,
    /// The durable ownership revision observed at claim time, so the daemon can
    /// detect that ownership moved between the claim and the submission.
    pub owner_revision: u64,
}

/// M003 (ADR-0038): how a stranded `delivering` row turned out.
///
/// Resolved from the **receipt**, not from the entry, because the receipt is the
/// only durable evidence of whether a turn actually started. Guessing from the
/// entry alone is what would risk running a prompt twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserPromptQueueDeliveryResolution {
    /// A turn was admitted; bind it and close the entry.
    Delivered { turn_id: String },
    /// No turn was admitted, so the claim never took effect. Return it to
    /// `pending` for a later attempt.
    Retry,
    /// The claim cannot be resolved and must not be retried automatically.
    Failed { reason: String },
}

/// M003 (ADR-0038): the delivery slice.
///
/// Delivery is **not** a mutation of the queue's contents and **not** a second
/// admission path. It is the narrow band between "an entry is live" and "a turn
/// exists", and every method here moves exactly one entry across exactly one
/// boundary of that band. Nothing here submits, authorizes, or starts a turn.
impl UserPromptQueueStore {
    /// The instant the advance tick should next wake, or `None` to park.
    ///
    /// This is what makes the M007 tick a *scheduler over its own work* rather
    /// than a fixed-interval poller: with nothing scheduled the answer is `None`
    /// and the task parks indefinitely, and with something scheduled it sleeps
    /// exactly until the due instant instead of waking to discover there is
    /// nothing to do.
    ///
    /// An already-due entry returns its past timestamp rather than `now_ms`.
    /// The caller clamps with `max`, and keeping the query free of "is it
    /// already past" logic means a due instant read once and a due instant read
    /// a millisecond later cannot disagree about *when* it fires — only about
    /// whether that moment has passed, which is the wall clock's job.
    ///
    /// Only `pending` rows count. A `delivering` entry is already past its gate,
    /// and a terminal one will never fire again.
    pub async fn next_scheduled_due_at_ms(&self) -> Result<Option<i64>, UserPromptQueueError> {
        let next: Option<i64> = sqlx::query_scalar(
            "SELECT MIN(run_at_ms) FROM user_prompt_queue_entry \
             WHERE status = 'pending' AND run_at_ms IS NOT NULL",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(next)
    }

    /// Sessions holding at least one entry that is due at or before `now_ms`.
    ///
    /// **This nominates; it does not deliver.** The result is a list of session
    /// ids for the caller to hand back through the ordinary per-session claim,
    /// which re-checks ownership and the at-most-once fence. Nothing here reads
    /// prompt text, resolves an owner, or submits a turn, so this query cannot
    /// become an alternate delivery authority.
    ///
    /// Bounded by `limit`, which is the guard against this being a way to walk
    /// every session in the catalogue. A tick that finds more due sessions than
    /// the bound takes this batch and comes back for the rest on its next pass —
    /// correctness does not depend on seeing them all at once, because each pass
    /// re-runs the query.
    ///
    /// `expires_at_ms > ?` mirrors the claim predicate: an entry that expired
    /// while it was scheduled is not deliverable, and naming it here would send
    /// the caller to a session whose claim will return nothing.
    pub async fn sessions_with_due_entries(
        &self,
        now_ms: i64,
        limit: i64,
    ) -> Result<Vec<String>, UserPromptQueueError> {
        if limit <= 0 {
            return Ok(Vec::new());
        }
        let sessions: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT session_id FROM user_prompt_queue_entry \
             WHERE status = 'pending' AND run_at_ms IS NOT NULL AND run_at_ms <= ? \
             AND expires_at_ms > ? \
             LIMIT ?",
        )
        .bind(now_ms)
        .bind(now_ms)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(sessions)
    }

    /// Claim the next due entry for delivery, in daemon-owned ordinal order.
    ///
    /// Returns `Ok(None)` when the queue has nothing to deliver — an empty
    /// queue, a queue whose entries are all `delivering` or terminal, or one
    /// whose next entry is past its TTL. `Ok(None)` is the ordinary idle answer,
    /// not a failure, so a trigger can call this on every wake without care.
    ///
    /// The claim and the transition are one statement guarded on the row still
    /// being `pending`. That guard is the at-most-once fence at this layer: two
    /// concurrent callers race to `rows_affected == 1` and at most one can win,
    /// because the loser no longer matches `status = 'pending'`.
    ///
    /// Ordering by ordinal rather than by age is what makes a queue a queue. A
    /// trigger that fires out of order must not reorder the user's work.
    ///
    /// **M007: a due time is a floor, not a hint.** The predicate gains
    /// `(run_at_ms IS NULL OR run_at_ms <= ?)` and nothing else — ordering stays
    /// `ORDER BY ordinal ASC`, so a reorder still only permutes entries that are
    /// *already* eligible. A not-yet-due entry is invisible to the ordering
    /// entirely, which is what makes "reorder cannot invert timer precedence"
    /// true by construction rather than by a check that could be forgotten.
    ///
    /// The comparison is `<=`, not `<`: at the exact due millisecond the entry
    /// is eligible. Invariant 9 is about two callers disagreeing about that same
    /// millisecond, and they cannot — the predicate is re-evaluated by the same
    /// SQL against the same `now_ms`, so a tick firing at the instant and a claim
    /// landing a millisecond later make the same decision about it.
    pub async fn claim_next_for_delivery(
        &self,
        session_id: &str,
        now_ms: i64,
    ) -> Result<Option<UserPromptQueueDeliveryClaim>, UserPromptQueueError> {
        validate_session_id(session_id)?;
        let mut conn = begin_immediate(&self.pool).await?;

        // Ownership is checked before the claim, inside the same transaction, so
        // a queue whose owner vanished is never claimed in the first place.
        let owner = load_owner(&mut conn, session_id).await?;
        let owner = match owner {
            Some(record) => record,
            // `Absent` means no queue has ever existed, and `Unowned` is the
            // ADR-0037 invalid state. Neither has an authorized deliverer.
            None => return Ok(None),
        };

        // The candidate is selected **by identity**, and the transition is a
        // separate statement guarded on that identity still being `pending`.
        //
        // Selecting the id and then re-reading "the lowest `delivering` row"
        // afterwards would be wrong: a row stranded by an earlier crash at a
        // lower ordinal would be returned instead of the row just claimed, and
        // the caller would submit the stranded entry's text while marking the
        // claimed one delivered. Identity, not position, is what this returns.
        let candidate: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM user_prompt_queue_entry \
             WHERE session_id = ? AND status = 'pending' AND expires_at_ms > ? \
             AND (run_at_ms IS NULL OR run_at_ms <= ?) \
             ORDER BY ordinal ASC LIMIT 1",
        )
        .bind(session_id)
        .bind(now_ms)
        .bind(now_ms)
        .fetch_optional(&mut **conn)
        .await
        .map_err(storage_error)?;

        let Some((candidate_id,)) = candidate else {
            conn.commit().await?;
            return Ok(None);
        };

        // `AND status = 'pending'` is the at-most-once fence: two concurrent
        // callers both read the same candidate, and only the one whose UPDATE
        // lands first reports `rows_affected == 1`.
        let claimed = sqlx::query(
            "UPDATE user_prompt_queue_entry SET status = 'delivering', updated_at_ms = ? \
             WHERE id = ? AND session_id = ? AND status = 'pending'",
        )
        .bind(now_ms)
        .bind(&candidate_id)
        .bind(session_id)
        .execute(&mut **conn)
        .await
        .map_err(storage_error)?;

        if claimed.rows_affected() == 0 {
            conn.commit().await?;
            return Ok(None);
        }

        let entry = load_entry(&mut conn, &candidate_id)
            .await?
            .map(UserPromptQueueEntryRecord::try_from)
            .transpose()?;
        let entry = match entry {
            Some(entry) => entry,
            None => {
                // Unreachable: the row was just claimed in this transaction.
                // Returning `None` rather than panicking keeps the store total.
                conn.commit().await?;
                return Ok(None);
            }
        };

        conn.commit().await?;
        Ok(Some(UserPromptQueueDeliveryClaim {
            entry,
            owner_principal: owner.owner_principal,
            owner_revision: owner.revision,
        }))
    }

    /// Bind a claimed entry to the turn that was admitted for it.
    ///
    /// Only a `delivering` row may be completed. A repeated call for an entry
    /// already `delivered` is **not** an error and not a re-bind: it returns
    /// `Ok(false)` so a retried submission — which re-derives the same receipt
    /// and is refused as `Duplicate` before reaching here — cannot silently
    /// overwrite a recorded `turn_id` with a different one.
    pub async fn mark_delivered(
        &self,
        entry_id: &str,
        turn_id: &str,
        now_ms: i64,
    ) -> Result<bool, UserPromptQueueError> {
        validate_entry_id(entry_id)?;
        validate_turn_id(turn_id)?;
        let updated = sqlx::query(
            "UPDATE user_prompt_queue_entry \
             SET status = 'delivered', delivered_turn_id = ?, delivered_at_ms = ?, \
                 updated_at_ms = ?, failure_reason = NULL \
             WHERE id = ? AND status = 'delivering'",
        )
        .bind(turn_id)
        .bind(now_ms)
        .bind(now_ms)
        .bind(entry_id)
        .execute(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(updated.rows_affected() == 1)
    }

    /// M006 (ADR-0036): settle a `pending` entry that a **steer** accepted.
    ///
    /// The second and last path into `delivered`, and the only one that does not
    /// begin at `delivering`. [`Self::mark_delivered`] completes a claim this
    /// daemon made for its own fresh turn; this one records that a prompt was
    /// accepted into a turn that already existed. The queue must not hand the
    /// same prompt out again afterwards — ADR-0036's own decision drivers forbid
    /// a delivery that was already delivered being run a second time.
    ///
    /// Guarded on `status = 'pending'` for the same at-most-once reason every
    /// other transition here is. A row M003 has already claimed is running as its
    /// **own** turn; settling it here as well would record one prompt delivered
    /// twice, and would hide the claim the user is waiting on.
    ///
    /// Ownership is resolved **inside this transaction**. That is settlement
    /// authority rather than steering authority — ADR-0037's exclusion of
    /// steering from the owner binding is about *reaching the turn*, and this
    /// write does not reach the turn at all; it changes the queue, so the
    /// question "may this principal author this session's queue?" applies
    /// unchanged. Without it a caller who steered someone else's turn could mark
    /// their entry delivered.
    ///
    /// Every non-winning case returns the same typed vocabulary as
    /// [`Self::delete`] rather than a second one, so a caller reports one reason
    /// set: `NotFound`, `Delivering`, `Expired`, `NotQueueOwner`,
    /// `QueueUnowned`.
    pub async fn mark_steer_delivered(
        &self,
        caller: &str,
        session_id: &str,
        entry_id: &str,
        turn_id: &str,
        now_ms: i64,
    ) -> Result<(), UserPromptQueueError> {
        validate_principal(caller)?;
        validate_entry_id(entry_id)?;
        validate_session_id(session_id)?;
        validate_turn_id(turn_id)?;
        let mut conn = begin_immediate(&self.pool).await?;
        authorize_owned_queue(&mut conn, caller, session_id).await?;
        // A settlement must not be able to write a row the read projection would
        // already have hidden, for the same reason `delete` sweeps first.
        sweep_expired(&mut conn, session_id, now_ms).await?;

        let settled = sqlx::query(
            "UPDATE user_prompt_queue_entry \
             SET status = 'delivered', delivered_turn_id = ?, delivered_at_ms = ?, \
                 updated_at_ms = ?, failure_reason = NULL \
             WHERE id = ? AND session_id = ? AND status = 'pending'",
        )
        .bind(turn_id)
        .bind(now_ms)
        .bind(now_ms)
        .bind(entry_id)
        .bind(session_id)
        .execute(&mut **conn)
        .await
        .map_err(storage_error)?;

        if settled.rows_affected() == 0 {
            // Returning without committing is the whole refusal: nothing was
            // written, so there is no partial state, and the caller learns which
            // condition it hit from the typed error rather than from a `false`.
            let existing = load_entry(&mut conn, entry_id).await?;
            return Err(refusal_for(existing.as_ref(), session_id, now_ms));
        }

        // Durable, not a frontend projection: a daemon restart cannot lose a
        // settlement the user was already told about.
        conn.commit().await?;
        Ok(())
    }

    /// Record a refused delivery. **Terminal** — see `release_delivery`.
    ///
    /// The reason is a short machine-readable code chosen by the daemon, never
    /// text copied from a refused submission: a provider error string can quote
    /// the prompt, and this column outlives the failure that filled it.
    pub async fn mark_delivery_failed(
        &self,
        entry_id: &str,
        reason: &str,
        now_ms: i64,
    ) -> Result<bool, UserPromptQueueError> {
        validate_entry_id(entry_id)?;
        let reason = bounded_reason(reason)?;
        let updated = sqlx::query(
            "UPDATE user_prompt_queue_entry \
             SET status = 'failed', failure_reason = ?, updated_at_ms = ? \
             WHERE id = ? AND status = 'delivering'",
        )
        .bind(reason)
        .bind(now_ms)
        .bind(entry_id)
        .execute(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(updated.rows_affected() == 1)
    }

    /// Return a claimed entry to `pending` without recording an outcome.
    ///
    /// This is the difference between "delivery did not happen" and "delivery
    /// failed". A submission that never reached admission — a transient storage
    /// error, a daemon shutting down mid-claim — did not consume the user's
    /// prompt, so the entry goes back and is delivered later. One that reached
    /// admission and was refused is a decision about the prompt itself, and is
    /// `failed`.
    pub async fn release_delivery(
        &self,
        entry_id: &str,
        now_ms: i64,
    ) -> Result<bool, UserPromptQueueError> {
        validate_entry_id(entry_id)?;
        let updated = sqlx::query(
            "UPDATE user_prompt_queue_entry \
             SET status = 'pending', updated_at_ms = ?, failure_reason = NULL \
             WHERE id = ? AND status = 'delivering'",
        )
        .bind(now_ms)
        .bind(entry_id)
        .execute(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(updated.rows_affected() == 1)
    }

    /// Revive a `failed` entry as `pending`.
    ///
    /// The daemon only calls this for a refusal it judged **transient**. A
    /// permanent refusal needs a human, and putting it back on the delivery path
    /// would spin the queue against a refusal that will never clear.
    pub async fn retry_failed(
        &self,
        entry_id: &str,
        now_ms: i64,
    ) -> Result<bool, UserPromptQueueError> {
        validate_entry_id(entry_id)?;
        let updated = sqlx::query(
            "UPDATE user_prompt_queue_entry \
             SET status = 'pending', failure_reason = NULL, updated_at_ms = ? \
             WHERE id = ? AND status = 'failed'",
        )
        .bind(now_ms)
        .bind(entry_id)
        .execute(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(updated.rows_affected() == 1)
    }

    /// List stranded `delivering` rows for startup reconciliation.
    ///
    /// Bounded by [`USER_PROMPT_QUEUE_MAX_RECONCILE`] and ordered by session then
    /// ordinal, so a repair pass is deterministic and cannot walk an unbounded
    /// backlog. `None` for `session_id` sweeps every session; that is the daemon
    /// singleton's restart case, and it is still bounded.
    pub async fn list_delivering_for_reconcile(
        &self,
        session_id: Option<&str>,
    ) -> Result<Vec<UserPromptQueueEntryRecord>, UserPromptQueueError> {
        if let Some(session_id) = session_id {
            validate_session_id(session_id)?;
        }
        let rows: Vec<QueueEntryRow> = sqlx::query_as(&format!(
            "SELECT {ENTRY_COLUMNS} FROM user_prompt_queue_entry \
             WHERE status = 'delivering' AND (? IS NULL OR session_id = ?) \
             ORDER BY session_id ASC, ordinal ASC LIMIT ?"
        ))
        .bind(session_id)
        .bind(session_id)
        .bind(USER_PROMPT_QUEUE_MAX_RECONCILE)
        .fetch_all(&self.pool)
        .await
        .map_err(storage_error)?;
        rows.into_iter()
            .map(UserPromptQueueEntryRecord::try_from)
            .collect()
    }

    /// Apply one reconciliation outcome to a stranded row.
    ///
    /// Guarded on `delivering` for the same reason the other transitions are: a
    /// row that a concurrent live delivery already resolved is left alone, so a
    /// startup sweep racing an in-progress delivery cannot undo it.
    pub async fn resolve_stranded_delivery(
        &self,
        entry_id: &str,
        resolution: &UserPromptQueueDeliveryResolution,
        now_ms: i64,
    ) -> Result<bool, UserPromptQueueError> {
        validate_entry_id(entry_id)?;
        let updated = match resolution {
            UserPromptQueueDeliveryResolution::Delivered { turn_id } => sqlx::query(
                "UPDATE user_prompt_queue_entry \
                     SET status = 'delivered', delivered_turn_id = ?, delivered_at_ms = ?, \
                         updated_at_ms = ?, failure_reason = NULL \
                     WHERE id = ? AND status = 'delivering'",
            )
            .bind(turn_id)
            .bind(now_ms)
            .bind(now_ms)
            .bind(entry_id),
            UserPromptQueueDeliveryResolution::Retry => sqlx::query(
                "UPDATE user_prompt_queue_entry \
                     SET status = 'pending', updated_at_ms = ?, failure_reason = NULL \
                     WHERE id = ? AND status = 'delivering'",
            )
            .bind(now_ms)
            .bind(entry_id),
            UserPromptQueueDeliveryResolution::Failed { reason } => sqlx::query(
                "UPDATE user_prompt_queue_entry \
                     SET status = 'failed', failure_reason = ?, updated_at_ms = ? \
                     WHERE id = ? AND status = 'delivering'",
            )
            .bind(bounded_reason(reason)?)
            .bind(now_ms)
            .bind(entry_id),
        };
        let updated = updated.execute(&self.pool).await.map_err(storage_error)?;
        Ok(updated.rows_affected() == 1)
    }
}

/// M003: the bound on a recorded failure reason.
///
/// Short on purpose. The column outlives the failure that filled it and is
/// readable by anything holding the queue, so a reason that could carry provider
/// or model text must not fit.
fn bounded_reason(reason: &str) -> Result<&str, UserPromptQueueError> {
    if reason.is_empty() {
        return Err(UserPromptQueueError::Invalid {
            field: "failure_reason",
            message: "delivery failure reason is empty".to_owned(),
        });
    }
    if reason.len() > 64 || reason.chars().any(char::is_control) {
        return Err(UserPromptQueueError::Invalid {
            field: "failure_reason",
            message: "delivery failure reason must be a short code, not prose".to_owned(),
        });
    }
    Ok(reason)
}

fn validate_session_id(session_id: &str) -> Result<(), UserPromptQueueError> {
    if session_id.is_empty() || session_id.len() > 128 || session_id.chars().any(char::is_control) {
        return Err(UserPromptQueueError::Invalid {
            field: "session_id",
            message: "session identity is empty, too long, or contains control characters"
                .to_owned(),
        });
    }
    Ok(())
}

/// Shape check for a turn id recorded as the delivery attribution.
///
/// Shared by `mark_delivered` and M006's `mark_steer_delivered` because both
/// write the same column under the same at-most-once rule; a second copy of
/// this rule could drift into accepting what the first refuses.
fn validate_turn_id(turn_id: &str) -> Result<(), UserPromptQueueError> {
    if turn_id.is_empty() || turn_id.len() > 128 || turn_id.chars().any(char::is_control) {
        return Err(UserPromptQueueError::Invalid {
            field: "turn_id",
            message: "turn identity is empty, too long, or contains control characters".to_owned(),
        });
    }
    Ok(())
}

fn storage_error(error: sqlx::Error) -> UserPromptQueueError {
    UserPromptQueueError::Storage(StorageError::Database(error.to_string()))
}
#[cfg(test)]
mod tests {
    use super::*;

    fn row(status: &str, text_bytes: i64) -> QueueEntryRow {
        QueueEntryRow {
            id: "entry-1".to_owned(),
            session_id: "session-1".to_owned(),
            ordinal: 0,
            text: "hi".to_owned(),
            text_bytes,
            status: status.to_owned(),
            failure_reason: None,
            created_at_ms: T0_MS,
            updated_at_ms: T0_MS,
            expires_at_ms: T0_MS + USER_PROMPT_QUEUE_TTL_MS,
            delivered_turn_id: None,
            delivered_at_ms: None,
            run_at_ms: None,
        }
    }

    const T0_MS: i64 = 1_700_000_000_000;

    /// Row mapping must surface a non-representable stored byte count as a
    /// typed `Invalid`, not a panic and not a silent zero. The `CHECK`
    /// constraint keeps a negative value out of the table, so this exercises
    /// the mapper on the row type directly — the same code path a corrupt or
    /// future-schema row would take.
    #[test]
    fn user_prompt_queue_row_mapping_rejects_negative_text_bytes() {
        let error = UserPromptQueueEntryRecord::try_from(row("pending", -1))
            .expect_err("negative byte count must refuse");
        match error {
            UserPromptQueueError::Invalid { field, message } => {
                assert_eq!(field, "text_bytes");
                assert_eq!(message, "stored byte count is negative");
            }
            other => panic!("expected Invalid(text_bytes), got {other:?}"),
        }
    }

    #[test]
    fn user_prompt_queue_row_mapping_accepts_a_representable_text_bytes() {
        let record = UserPromptQueueEntryRecord::try_from(row("pending", 2))
            .expect("representable byte count");
        assert_eq!(record.text_bytes, 2);
    }

    /// Every status the schema `CHECK` accepts must parse, including the three
    /// that are unreachable until M003. A parse that lagged the `CHECK` would
    /// turn a legitimately stored M003 row into a read error.
    #[test]
    fn user_prompt_queue_row_mapping_parses_every_schema_status() {
        for (text, expected) in [
            ("pending", UserPromptQueueStatus::Pending),
            ("delivering", UserPromptQueueStatus::Delivering),
            ("delivered", UserPromptQueueStatus::Delivered),
            ("failed", UserPromptQueueStatus::Failed),
            ("expired", UserPromptQueueStatus::Expired),
        ] {
            let record = UserPromptQueueEntryRecord::try_from(row(text, 2))
                .unwrap_or_else(|error| panic!("status {text} must parse: {error}"));
            assert_eq!(record.status, expected);
            assert_eq!(record.status.as_str(), text);
        }
    }

    #[test]
    fn user_prompt_queue_row_mapping_rejects_unknown_stored_status() {
        let error = UserPromptQueueEntryRecord::try_from(row("teleported", 2))
            .expect_err("unknown stored status must refuse");
        assert!(matches!(
            error,
            UserPromptQueueError::Invalid {
                field: "status",
                ..
            }
        ));
    }

    // -----------------------------------------------------------------------
    // C001: the merge rule, which is the whole decision this corrective pass
    // makes. It is a pure function on purpose — the ordering contract is worth
    // pinning without a database in the loop, because the defect it replaced was
    // invisible in every test that only looked at statuses.
    // -----------------------------------------------------------------------

    fn live_entry(id: &str, status: UserPromptQueueStatus) -> UserPromptQueueLiveEntry {
        UserPromptQueueLiveEntry {
            id: id.to_owned(),
            ordinal: 0,
            text: id.to_owned(),
            text_bytes: id.len(),
            status,
            created_at_ms: T0_MS,
            expires_at_ms: T0_MS + USER_PROMPT_QUEUE_TTL_MS,
            run_at_ms: None,
        }
    }

    fn pending(id: &str) -> UserPromptQueueLiveEntry {
        live_entry(id, UserPromptQueueStatus::Pending)
    }

    fn delivering(id: &str) -> UserPromptQueueLiveEntry {
        live_entry(id, UserPromptQueueStatus::Delivering)
    }

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_owned()).collect()
    }

    /// With nothing non-operable in the live set the merge is the identity on the
    /// desired order — which is what keeps every pre-C001 reorder result
    /// unchanged.
    #[test]
    fn user_prompt_queue_merge_with_no_non_operable_rows_is_the_desired_order() {
        let live = vec![pending("a"), pending("b"), pending("c")];
        let merged = merge_operable_into_live(&live, &ids(&["c", "a", "b"]))
            .expect("an all-pending live set merges");
        assert_eq!(merged, ids(&["c", "a", "b"]));
    }

    /// The plan's worked example, asserted verbatim: `d1` holds rank 1, so it is
    /// neither pinned to the head nor swept to the tail by the client's
    /// permutation.
    #[test]
    fn user_prompt_queue_merge_preserves_existing_rank_not_a_pin() {
        let live = vec![pending("p1"), delivering("d1"), pending("p2")];
        let merged = merge_operable_into_live(&live, &ids(&["p2", "p1"]))
            .expect("one non-operable row merges");
        assert_eq!(
            merged,
            ids(&["p2", "d1", "p1"]),
            "d1 keeps slot 1: pinning it to either end is the defect under repair"
        );
    }

    /// Head, middle, and tail are the three ranks a single non-operable row can
    /// hold. All three must survive, because which one it is depends on where
    /// the user's prompt sat when the turn started — not on this function.
    #[test]
    fn user_prompt_queue_merge_holds_a_delivering_row_at_head_middle_and_tail() {
        for (label, live, desired, expected) in [
            (
                "head",
                vec![delivering("d1"), pending("p1"), pending("p2")],
                ids(&["p2", "p1"]),
                vec!["d1", "p2", "p1"],
            ),
            (
                "middle",
                vec![pending("p1"), delivering("d1"), pending("p2")],
                ids(&["p2", "p1"]),
                vec!["p2", "d1", "p1"],
            ),
            (
                "tail",
                vec![pending("p1"), pending("p2"), delivering("d1")],
                ids(&["p2", "p1"]),
                vec!["p2", "p1", "d1"],
            ),
        ] {
            let merged = merge_operable_into_live(&live, &desired)
                .unwrap_or_else(|error| panic!("{label}: merge must succeed: {error}"));
            assert_eq!(merged, ids(&expected), "{label} rank is not preserved");
        }
    }

    /// Two non-operable rows interleave with the caller's permutation without
    /// either of them drifting: each lands on the slot it held, and the operable
    /// rows take the client's order in the remaining slots.
    #[test]
    fn user_prompt_queue_merge_interleaves_several_non_operable_rows() {
        let live = vec![
            pending("p1"),
            delivering("d1"),
            pending("p2"),
            delivering("d2"),
            pending("p3"),
        ];
        let merged = merge_operable_into_live(&live, &ids(&["p3", "p1", "p2"]))
            .expect("two non-operable rows merge");
        assert_eq!(
            merged,
            ids(&["p3", "d1", "p1", "d2", "p2"]),
            "each non-operable row keeps its slot and the operable rows follow \
             the client"
        );
    }

    /// An empty queue is not an error, and neither is a desired order with no
    /// non-operable rows to hold.
    #[test]
    fn user_prompt_queue_merge_of_an_empty_live_set_is_empty() {
        assert!(merge_operable_into_live(&[], &[])
            .expect("empty merge")
            .is_empty());
    }

    /// The backstop. A desired order that is not the operable permutation leaves
    /// operable rows unplaced; refusing is what stops a future caller from
    /// writing a dense order that silently strands a row above the band.
    #[test]
    fn user_prompt_queue_merge_refuses_a_desired_order_of_the_wrong_length() {
        let live = vec![pending("p1"), delivering("d1"), pending("p2")];

        for (label, desired) in [
            ("too short", ids(&["p1"])),
            ("too long", ids(&["p2", "p1", "p3"])),
        ] {
            let error = merge_operable_into_live(&live, &desired)
                .expect_err("a length mismatch must refuse");
            assert_eq!(
                error.code(),
                UserPromptQueueError::OrderMismatch.code(),
                "{label} must refuse with order_mismatch"
            );
        }
    }

    /// The merge never drops or invents a row: the result is a permutation of
    /// the live set, which is what makes the dense write a pure relabelling.
    #[test]
    fn user_prompt_queue_merge_output_is_a_permutation_of_the_live_set() {
        let live = vec![
            delivering("d1"),
            pending("p1"),
            delivering("d2"),
            pending("p2"),
            delivering("d3"),
            pending("p3"),
        ];
        let merged = merge_operable_into_live(&live, &ids(&["p2", "p3", "p1"]))
            .expect("merge of a fully interleaved set");
        assert_eq!(
            merged.len(),
            live.len(),
            "the merge must not resize the set"
        );
        let mut expected: Vec<String> = live.iter().map(|e| e.id.clone()).collect();
        let mut actual = merged.clone();
        expected.sort();
        actual.sort();
        assert_eq!(actual, expected, "the merge must not drop or invent a row");
    }
}
