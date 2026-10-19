//! M003 (ADR-0038, accepted — Option B): delivering a queued user prompt into an
//! ordinary turn admission.
//!
//! Delivery is **not** a second admission path. [`CoreDaemon::submit_queued_prompt_turn`]
//! is the single daemon-internal entry, and it does exactly what an ordinary
//! client submit does: it calls
//! [`handle_turns_request(CoreRequest::TurnSubmit { .. })`](super::daemon_turns::CoreDaemon::handle_turns_request).
//! There is no queue-specific submit, no relaxed receipt, and no replay of an
//! interrupted receipt.
//!
//! ## As whom
//!
//! ADR-0035 and ADR-0037 settled *that* a queued entry is delivered and *who may
//! mutate* the queue. Neither settled *as whom* the turn runs, and the answer
//! matters: a turn admitted under a synthetic daemon identity would hold the
//! controller lease as that identity, so the real owner's next mid-turn queue
//! mutation would fail `user_prompt_queue_controller_mismatch`. The queue would
//! lock itself out the moment it started working.
//!
//! So the delivered principal is **the queue owner**, reconstructed from durable
//! rows only:
//!
//! - `principal_id` — the durable `user_prompt_queue_owner` row.
//! - `kind` — the durable `TeamStore` record. `TeamError::PrincipalNotFound`
//!   means the owner was deleted, and delivery fails closed.
//! - `auth_method` / `transport_class` — `QueueDelivery` / `DaemonDelivery`, a
//!   **delivery classification** rather than a record of what authenticated the
//!   owner when they queued the prompt.
//! - `client_id` — `queue:<entry_id>`, a daemon-owned marker that is metadata
//!   and never authority.
//!
//! Authority is then re-authorized against **current** team state through the
//! canonical `AuthorizationService`, so a revoked, suspended, or
//! capability-narrowed owner fails delivery here rather than inheriting a
//! decision made when the prompt was queued.
//!
//! ## At-most-once
//!
//! The submission id is deterministic from the entry id, so a retry after a
//! restart re-derives the same `turn_admission_receipt` and receives
//! [`TurnAdmissionOutcome::Duplicate`](super::daemon_turns::TurnAdmissionOutcome)
//! instead of starting a second turn. At-most-once is **inherited from the
//! receipt**, not reimplemented here.

use codegg_core::session::user_prompt_queue::{
    UserPromptQueueDeliveryResolution, UserPromptQueueStore,
};
use sha2::Digest;

use crate::error::AppError;
use crate::protocol::core::{CoreRequest, CoreResponse};

use super::daemon::CoreDaemon;

/// Daemon-owned client marker recorded as audit/presence metadata for a
/// delivered turn. Never an authority: the authority is the queue owner.
pub fn queued_delivery_client_id(entry_id: &str) -> String {
    format!("queue:{entry_id}")
}

/// Deterministic delivery submission id.
///
/// Derived from the **entry id alone**, so the same entry always produces the
/// same submission identity. That is what makes at-most-once a property of the
/// existing receipt rather than of any queue bookkeeping: a claim that crashed
/// between submission and recording re-derives this id, the receipt reports
/// `Duplicate`, and no second turn exists.
pub fn queued_delivery_submission_id(entry_id: &str) -> String {
    format!(
        "queue-{:x}",
        sha2::Sha256::digest(format!("user-prompt-queue|{entry_id}").as_bytes())
    )
}

/// Canonical digest over the delivered turn content.
///
/// The receipt compares this, so a reused submission id carrying different
/// content or a different owner is a **conflict**, never a replay.
pub fn queued_delivery_request_digest(
    entry_id: &str,
    session_id: &str,
    owner_principal: &str,
    text: &str,
) -> String {
    format!(
        "{:x}",
        sha2::Sha256::digest(
            format!("user-prompt-queue|{entry_id}|{session_id}|{owner_principal}|{text}")
                .as_bytes()
        )
    )
}

/// What one delivery attempt did.
///
/// Every variant means the entry is no longer `delivering` — the claim is
/// always resolved one way or the other. An outcome that left the entry claimed
/// would be a stranded row waiting for the next restart sweep, which is a defect
/// rather than a state to model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueDeliveryOutcome {
    /// A turn was admitted and bound to the entry.
    Delivered { turn_id: String },
    /// The entry was already delivered and the receipt replayed. At-most-once
    /// holding after a restart.
    AlreadyDelivered { turn_id: String },
    /// The claim was taken back to `pending` because the attempt never reached
    /// admission. A later trigger retries.
    Released { reason: &'static str },
    /// The claim reached a terminal `failed` row with a recorded reason code.
    Failed { reason: &'static str },
    /// There was nothing to deliver: no owner row, no live entry, or the next
    /// entry is past its TTL.
    NothingToDo,
}

/// Refusal reasons, as short machine codes.
///
/// Short on purpose: `bounded_reason` in the store caps these at 64 bytes with no
/// control characters, because the column outlives the failure and is readable by
/// anything holding the queue. A provider error string can quote the prompt, so
/// nothing copied from one belongs here.
const REASON_OWNER_UNKNOWN: &str = "owner_unknown";
const REASON_SESSION_UNAVAILABLE: &str = "session_unavailable";
const REASON_AUTHORIZATION_REFUSED: &str = "authorization_refused";
const REASON_SESSION_BUSY: &str = "session_busy";
const REASON_COMPOSITION_REFUSED: &str = "composition_refused";
const REASON_TRANSIENT: &str = "transient";

/// M007: the shortest gap between two attempts at the **same** due instant.
///
/// Not a poll interval and not a delivery retry policy — it is a damper on one
/// specific failure shape. An entry whose due time has passed but which cannot
/// be delivered (its session is busy, its owner is gone, composition is
/// refused) remains `pending` and permanently due. Without a floor the advance
/// task would re-arm at zero delay and spin a core for as long as that entry
/// is stuck.
///
/// Everything that can actually resolve a stuck entry still resolves it
/// promptly: the post-terminal wake runs on the turn boundary, and the restart
/// twin runs on boot. This floor only bounds the retry of an entry the tick
/// cannot help.
const SAME_DUE_RETRY_FLOOR_MS: i64 = 30_000;

impl CoreDaemon {
    /// M003 (ADR-0038): claim the next due entry for `session_id` and deliver it
    /// as the queue owner.
    ///
    /// This is the single daemon-internal delivery entry. Exactly two callers are
    /// permitted — the post-terminal wake and the bounded restart twin — enforced
    /// by `scripts/check_queue_delivery_callsites.py`. Widening the guard is a
    /// recorded decision, never a quiet third call site.
    ///
    /// ## Ordering
    ///
    /// Exactly one entry is claimed per call. Entries are claimed in daemon-owned
    /// ordinal order, so a burst of N queued prompts runs in the order the user
    /// wrote them; the next one is delivered when the previous turn reaches a
    /// terminal state. A queue that dumped every entry at once would contend with
    /// itself for the controller lease and lose the ordering guarantee.
    ///
    /// ## Failure discipline
    ///
    /// Every refusal resolves the claim. One that never reached admission
    /// **releases** — the user's prompt was not consumed, so it is retried. One
    /// that reached admission and was refused is **terminal**, because a refusal
    /// is a decision about the prompt and retrying it in a loop would spin the
    /// queue against a condition that will not clear. The split is on the typed
    /// error class, never on error wording.
    pub async fn submit_queued_prompt_turn(&self, session_id: &str) -> QueueDeliveryOutcome {
        let Some(pool) = self.pool.clone() else {
            return QueueDeliveryOutcome::NothingToDo;
        };
        let store = UserPromptQueueStore::new(pool.clone());
        let now = chrono::Utc::now().timestamp_millis();

        let claim = match store.claim_next_for_delivery(session_id, now).await {
            Ok(Some(claim)) => claim,
            // `Ok(None)` is the ordinary idle answer: empty queue, no owner row,
            // everything terminal, or the next entry is past its TTL.
            Ok(None) => return QueueDeliveryOutcome::NothingToDo,
            Err(error) => {
                tracing::warn!(%session_id, %error, "queue delivery claim failed");
                return QueueDeliveryOutcome::NothingToDo;
            }
        };
        let entry_id = claim.entry.id.clone();
        let text = claim.entry.text.clone();

        // Attribution is resolved from durable rows, never synthesized. A queue
        // whose owner cannot be found in team state has no authorized deliverer,
        // so the entry is released rather than force-submitted — it goes back to
        // `pending` and is re-evaluated on the next trigger, which is where an
        // ownership repair would take effect.
        let owner_id = match codegg_core::identity::PrincipalId::parse(&claim.owner_principal) {
            Ok(id) => id,
            Err(_) => {
                // An unparseable owner id cannot name a team principal, so there
                // is nobody to re-authorize. Released, not failed: the entry may
                // become deliverable if ownership is repaired.
                return self
                    .resolve_delivery(&store, &entry_id, false, REASON_OWNER_UNKNOWN, now)
                    .await;
            }
        };
        let team_store = codegg_core::team::TeamStore::new(pool.clone());
        let kind = match team_store.get_principal(&owner_id).await {
            Ok(Some(record)) => record.kind,
            Ok(None) => {
                return self
                    .resolve_delivery(&store, &entry_id, false, REASON_OWNER_UNKNOWN, now)
                    .await
            }
            Err(_) => {
                return self
                    .resolve_delivery(&store, &entry_id, false, REASON_TRANSIENT, now)
                    .await
            }
        };

        let delivery_client = queued_delivery_client_id(&entry_id);
        let owner = codegg_core::transport_auth::AuthenticatedPrincipal::reconstructed(
            owner_id,
            kind,
            codegg_core::transport_auth::AuthMethod::QueueDelivery,
            codegg_core::transport_auth::TransportClass::DaemonDelivery,
            delivery_client.clone(),
        );

        // Owning session row, server-side: unknown, deleted, or archived sessions
        // fail closed before any admission work, exactly as the ADR-0031 path does.
        let session_store = codegg_core::session::SessionStore::new(pool.clone());
        match session_store.get(session_id).await {
            Ok(Some(session)) if session.time_archived.is_none() => {}
            _ => {
                return self
                    .resolve_delivery(&store, &entry_id, false, REASON_SESSION_UNAVAILABLE, now)
                    .await
            }
        }

        // Re-authorize against CURRENT team state. The owner row records who
        // queued the prompt, not whether they may still submit one: a revoked or
        // capability-narrowed principal must not inherit an old permission through
        // the queue.
        let project = self.session_project(&pool, session_id).await;
        let correlation = format!("user-prompt-queue:{entry_id}");
        let decision = {
            use codegg_core::authorization::{
                AuthorizationRequest, AuthorizationService, Capability, OperationDescriptor,
                ScopeKind,
            };
            let service = AuthorizationService::new(team_store);
            match service
                .authorize(&AuthorizationRequest::new(
                    owner.clone(),
                    OperationDescriptor::new(
                        "user_prompt_queue_deliver",
                        ScopeKind::ViaSession,
                        Some(Capability::AgentInvoke),
                    ),
                    project,
                    correlation.clone(),
                ))
                .await
            {
                Ok(decision) => decision,
                Err(_) => {
                    return self
                        .resolve_delivery(
                            &store,
                            &entry_id,
                            false,
                            REASON_AUTHORIZATION_REFUSED,
                            now,
                        )
                        .await
                }
            }
        };

        // Idleness is a precondition, not a claim. A session whose turn is still
        // running releases the entry so the post-terminal wake delivers it; a
        // claim that waited here would strand the entry if the turn never ended.
        if let Some(runtime) = self.sessions.get(session_id) {
            if runtime.active_turn.read().await.is_some() {
                return self
                    .resolve_delivery(&store, &entry_id, false, REASON_SESSION_BUSY, now)
                    .await;
            }
        }

        // Daemon-side composition through the same helper an ordinary session
        // submit uses. An unselected or unresolvable model fails closed here
        // rather than inventing a model.
        let (model, agents, messages) = match self
            .resolve_prompt_submit_composition(session_id, &text)
            .await
        {
            Ok(composed) => composed,
            Err(_) => {
                return self
                    .resolve_delivery(&store, &entry_id, false, REASON_COMPOSITION_REFUSED, now)
                    .await
            }
        };

        let submission_id = queued_delivery_submission_id(&entry_id);
        let digest =
            queued_delivery_request_digest(&entry_id, session_id, &claim.owner_principal, &text);
        let authority =
            codegg_core::transport_auth::RequestAuthorityContext::new(owner.clone(), correlation);

        // The generic pre-side-effect audit emit does not run for internal
        // recursion, so the delivered turn carries its own explicit record built
        // here from the revalidated decision.
        let delivery_audit = {
            let provenance = codegg_core::authorization::audit_provenance(&decision);
            let mut chain = codegg_core::audit_instrumentation::AuditChainContext::new();
            chain.project = decision.project_id.clone();
            chain.session_id = Some(session_id.to_owned());
            let bytes = text.len();
            (owner, provenance, chain, bytes)
        };

        let admission = Box::pin(self.handle_turns_request(
            CoreRequest::TurnSubmit {
                session_id: session_id.to_owned(),
                text,
                plan_mode: false,
                model,
                agents,
                current_agent_idx: 0,
                messages,
                reasoning: None,
                human_shell_promotion_ids: Vec::new(),
                max_model_tokens: None,
            },
            &submission_id,
            Some(digest),
            &delivery_client,
            authority,
            decision,
        ))
        .await;

        let outcome = match admission {
            Ok(CoreResponse::TurnAdmissionReceipt { receipt }) => {
                let bound = store
                    .mark_delivered(
                        &entry_id,
                        &receipt.turn_id,
                        chrono::Utc::now().timestamp_millis(),
                    )
                    .await
                    .unwrap_or(false);
                if bound {
                    QueueDeliveryOutcome::Delivered {
                        turn_id: receipt.turn_id,
                    }
                } else {
                    // The receipt exists but the binding did not land, so the
                    // entry is still `delivering` while a turn is genuinely
                    // running against it. Release is wrong here — it would let a
                    // later trigger deliver the same prompt a second time — and
                    // the startup sweep resolves it from this receipt. Leaving it
                    // stranded is the correct outcome, not a lost cause.
                    tracing::warn!(
                        %session_id,
                        %entry_id,
                        turn_id = %receipt.turn_id,
                        "queue delivery receipt admitted but entry binding did not land; \
                         startup reconciliation will resolve it from the receipt"
                    );
                    QueueDeliveryOutcome::AlreadyDelivered {
                        turn_id: receipt.turn_id,
                    }
                }
            }
            Ok(_) => {
                // A response that is not a receipt is not an admission. The turn
                // did not start, so the entry is released.
                self.resolve_delivery(&store, &entry_id, false, REASON_TRANSIENT, now)
                    .await
            }
            Err(error) => {
                // Typed classification, not wording: a storage or IO failure is
                // this daemon's problem and the prompt was never consumed, while a
                // policy/provider/agent refusal is a decision about the prompt.
                let transient = matches!(error, AppError::Storage(_) | AppError::Io(_));
                let reason = if transient {
                    REASON_TRANSIENT
                } else {
                    "submission_refused"
                };
                self.resolve_delivery(&store, &entry_id, !transient, reason, now)
                    .await
            }
        };

        // Delivery record: owner + entry + turn bound at admission. Byte count
        // only — never prompt text, never a digest of it.
        match &outcome {
            QueueDeliveryOutcome::Delivered { turn_id }
            | QueueDeliveryOutcome::AlreadyDelivered { turn_id } => {
                let (audit_owner, provenance, mut chain, bytes) = delivery_audit;
                chain.turn_id = Some(turn_id.clone());
                let builder = codegg_core::audit_instrumentation::user_prompt_queue_delivery_event(
                    &audit_owner,
                    &provenance,
                    &chain,
                    session_id,
                    &entry_id,
                    bytes,
                    "delivered",
                );
                self.append_audit_event(builder).await;
            }
            QueueDeliveryOutcome::Failed { reason } => {
                let (audit_owner, provenance, chain, bytes) = delivery_audit;
                let builder = codegg_core::audit_instrumentation::user_prompt_queue_delivery_event(
                    &audit_owner,
                    &provenance,
                    &chain,
                    session_id,
                    &entry_id,
                    bytes,
                    reason,
                );
                self.append_audit_event(builder).await;
            }
            // A released claim is not an outcome worth a durable audit record: no
            // authority was exercised and the prompt was not consumed. It is a
            // `tracing` line, and nothing more.
            QueueDeliveryOutcome::Released { .. } | QueueDeliveryOutcome::NothingToDo => {
                tracing::debug!(
                    %session_id,
                    %entry_id,
                    outcome = ?outcome,
                    "queue delivery attempt released the claim"
                );
            }
        }

        outcome
    }

    /// Resolve a claim one way or the other, then report the matching outcome.
    ///
    /// `terminal` chooses between `failed` and released-back-to-`pending`. A
    /// store failure here is itself reported as a release: the entry is still
    /// `delivering`, and the startup sweep — not this call — is what resolves a
    /// row this layer could not. Reporting that honestly is why the sweep exists.
    async fn resolve_delivery(
        &self,
        store: &UserPromptQueueStore,
        entry_id: &str,
        terminal: bool,
        reason: &'static str,
        now_ms: i64,
    ) -> QueueDeliveryOutcome {
        let applied = if terminal {
            store.mark_delivery_failed(entry_id, reason, now_ms).await
        } else {
            store.release_delivery(entry_id, now_ms).await
        };
        match applied {
            Ok(true) if terminal => QueueDeliveryOutcome::Failed { reason },
            // `Ok(false)` means the row was no longer `delivering` — a concurrent
            // delivery or sweep already resolved it — and `Ok(true)` non-terminal
            // means this call put it back. Both leave the claim resolved, which is
            // what `Released` reports.
            Ok(_) => QueueDeliveryOutcome::Released { reason },
            Err(error) => {
                tracing::warn!(%entry_id, %error, "queue delivery claim could not be resolved here");
                QueueDeliveryOutcome::Released {
                    reason: REASON_TRANSIENT,
                }
            }
        }
    }

    /// M003 (ADR-0038): the bounded startup sweep.
    ///
    /// Two passes, both bounded, both before the daemon accepts traffic:
    ///
    /// 1. **Reconcile.** Resolve every stranded `delivering` row against its
    ///    durable receipt. A row whose receipt records a turn becomes
    ///    `delivered`; one with no receipt returns to `pending`; one whose
    ///    receipt is present but unresolvable becomes `failed`. Resolution is
    ///    from the receipt and never from the entry, because the receipt is the
    ///    only evidence of whether a turn actually started.
    /// 2. **Deliver.** For each session touched, attempt one delivery through the
    ///    guarded entry.
    ///
    /// This is the restart twin, not a poller. It runs once, it is capped at
    /// [`USER_PROMPT_QUEUE_MAX_RECONCILE`](codegg_core::session::user_prompt_queue::USER_PROMPT_QUEUE_MAX_RECONCILE)
    /// rows, and it adds no background thread.
    pub async fn reconcile_stranded_queue_deliveries(&self) {
        let Some(pool) = self.pool.clone() else {
            return;
        };
        let store = UserPromptQueueStore::new(pool.clone());
        let now = chrono::Utc::now().timestamp_millis();

        let stranded = match store.list_delivering_for_reconcile(None).await {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(%error, "queue delivery reconciliation could not list stranded rows");
                return;
            }
        };
        if stranded.is_empty() {
            return;
        }

        let mut sessions: Vec<String> = Vec::new();
        for row in &stranded {
            let resolution = self.receipt_resolution(&row.id).await;
            match store
                .resolve_stranded_delivery(&row.id, &resolution, now)
                .await
            {
                // A `false` here means a concurrent live delivery already
                // resolved this row, which is the correct outcome for a sweep:
                // it must never undo work it did not do.
                Ok(true) => {}
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(
                        entry_id = %row.id,
                        %error,
                        "queue delivery reconciliation could not resolve a stranded row"
                    );
                }
            }
            if !sessions.contains(&row.session_id) {
                sessions.push(row.session_id.clone());
            }
        }

        for session_id in sessions {
            self.submit_queued_prompt_turn(&session_id).await;
        }
    }

    /// M007 (ADR-0039): permitted caller 3 of
    /// [`submit_queued_prompt_turn`](CoreDaemon::submit_queued_prompt_turn) — the
    /// bounded queue advance tick.
    ///
    /// Nominates every session holding an entry that has become due, and asks
    /// the ordinary per-session wake to deliver it. It changes **when** a due
    /// entry becomes eligible, not **as whom** it is submitted: ADR-0038 still
    /// governs the delivery itself, and nothing here reads prompt text, resolves
    /// an owner, or admits a turn.
    ///
    /// The nomination query is bounded by
    /// [`USER_PROMPT_QUEUE_MAX_RECONCILE`](codegg_core::session::user_prompt_queue::USER_PROMPT_QUEUE_MAX_RECONCILE),
    /// so this cannot become a way to walk every session in the catalogue. A
    /// backlog larger than the bound is drained over successive passes, and
    /// correctness does not depend on draining it at once because every pass
    /// re-runs the query.
    ///
    /// One entry per session per pass, inherited from
    /// [`Self::submit_queued_prompt_turn`]'s own ordering rule. A session with
    /// five due prompts gets one turn; that turn's terminal transition wakes the
    /// next, exactly as an unscheduled backlog does.
    pub async fn advance_due_queue_entries(&self) {
        let Some(pool) = self.pool.clone() else {
            return;
        };
        let store = UserPromptQueueStore::new(pool);
        let now_ms = chrono::Utc::now().timestamp_millis();
        let sessions = match store
            .sessions_with_due_entries(
                now_ms,
                codegg_core::session::user_prompt_queue::USER_PROMPT_QUEUE_MAX_RECONCILE,
            )
            .await
        {
            Ok(sessions) => sessions,
            // A failing advance must not wedge the queue. The next pass retries,
            // and a storage error here is not the entry's to carry.
            Err(error) => {
                tracing::warn!(%error, "queue advance could not list due sessions");
                return;
            }
        };
        for session_id in sessions {
            self.submit_queued_prompt_turn(&session_id).await;
        }
    }

    /// M007 (ADR-0039): the advance **loop** — a scheduler over its own work.
    ///
    /// Each pass asks one question: when is the next thing I have to do? If
    /// nothing is scheduled the answer is `None` and the task parks on
    /// [`Self::queue_schedule_changed`] with no timer at all. If something is
    /// scheduled, the task sleeps exactly until that instant. There is no fixed
    /// interval anywhere in this loop, which is the property M003 refused to
    /// give up and the one acceptance criterion 7 asks for.
    ///
    /// ## Why a notification, not a shorter poll
    ///
    /// Polling for "has anything been scheduled yet" would reintroduce exactly
    /// the unconditional wakeup this design exists to avoid — a daemon with an
    /// empty queue would still wake every few seconds forever. The write that
    /// creates a schedule is in the same process, so it can simply say so.
    ///
    /// ## Why the park is not a lost wakeup
    ///
    /// `Notify::notify_one` stores a permit when no one is waiting, so a
    /// schedule created *during* a pass is not dropped: the next `notified()`
    /// consumes the stored permit and recomputes immediately. The failure this
    /// rules out is a prompt scheduled for 09:00 sitting asleep because its
    /// `notify_one` landed while the task was mid-pass.
    ///
    /// Restart is handled by the same loop: `next_scheduled_due_at_ms` reads
    /// storage, so a due time that passed while the daemon was down is read as
    /// already-due on the first pass after boot. The due time is a floor, never
    /// a deadline the daemon could have met while absent.
    pub async fn run_queue_advance_task(&self) {
        // `(due_at, wall clock of the attempt)`. Only consulted when the next
        // due instant is one we have *already* tried, which is exactly the stuck
        // case the floor below exists for.
        let mut last_attempt: Option<(i64, i64)> = None;
        loop {
            let Some(pool) = self.pool.clone() else {
                // No storage means nothing can ever be scheduled. Park forever
                // rather than spin on a query that cannot succeed.
                std::future::pending::<()>().await;
                return;
            };
            let store = UserPromptQueueStore::new(pool);
            let next_due = match store.next_scheduled_due_at_ms().await {
                Ok(next) => next,
                Err(error) => {
                    // Back off rather than spin. A storage failure is not a
                    // reason to retry at full speed.
                    tracing::warn!(%error, "queue advance could not read the next due time");
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    continue;
                }
            };

            match next_due {
                // Park. No timer is armed, so an idle daemon does no work at
                // all until a write tells it there is something to do.
                None => {
                    last_attempt = None;
                    self.queue_schedule_changed.notified().await;
                }
                Some(due_at_ms) => {
                    let now_ms = chrono::Utc::now().timestamp_millis();
                    // How long ago this task last tried *this* due instant.
                    let since_same_due = last_attempt
                        .filter(|(attempted, _)| *attempted == due_at_ms)
                        .map(|(_, at)| now_ms.saturating_sub(at));
                    let wait = if due_at_ms > now_ms {
                        due_at_ms - now_ms
                    } else if since_same_due.is_none() {
                        // Due, and not one we have just tried — fire now. This
                        // is the restart case: a due time that passed while the
                        // daemon was down must not wait out a damper.
                        0
                    } else {
                        // Due, and we already tried it. An entry that cannot be
                        // delivered — session busy, owner unknown, composition
                        // refused — stays `pending` and due forever, so without
                        // this floor the loop re-arms at `wait = 0` and spins a
                        // core for as long as the entry is stuck.
                        //
                        // A damper, not a poll: the post-terminal wake still
                        // retries immediately when a turn ends.
                        SAME_DUE_RETRY_FLOOR_MS.saturating_sub(since_same_due.unwrap_or(0))
                    };
                    tokio::select! {
                        // The schedule changed underneath us — an edit moved an
                        // entry later, or cleared the last one. Recompute.
                        _ = self.queue_schedule_changed.notified() => { last_attempt = None; }
                        _ = tokio::time::sleep(std::time::Duration::from_millis(wait as u64)) => {
                            self.advance_due_queue_entries().await;
                            last_attempt = Some((
                                due_at_ms,
                                chrono::Utc::now().timestamp_millis(),
                            ));
                        }
                    }
                }
            }
        }
    }

    /// Decide a stranded row's fate from its durable receipt.
    ///
    /// The submission id is re-derived from the entry id, so the lookup key is
    /// known without reconstructing any authority — the row itself supplies the
    /// identity that produced it.
    async fn receipt_resolution(&self, entry_id: &str) -> UserPromptQueueDeliveryResolution {
        let Some(pool) = self.pool.as_ref() else {
            return UserPromptQueueDeliveryResolution::Failed {
                reason: "storage_unavailable".to_owned(),
            };
        };
        let submission_id = queued_delivery_submission_id(entry_id);
        let turn_id: Option<String> = sqlx::query_scalar(
            "SELECT turn_id FROM turn_admission_receipt WHERE submission_id = ?",
        )
        .bind(&submission_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();

        match turn_id {
            // The receipt exists, so a turn really was admitted. Binding it is
            // safe: at-most-once already holds, and nothing can admit a second
            // turn under this submission id.
            Some(turn_id) => UserPromptQueueDeliveryResolution::Delivered { turn_id },
            // No receipt, so no turn was started under this id. The claim never
            // took effect and the entry is safe to deliver again — the
            // deterministic submission id is what makes that safe rather than a
            // duplicate.
            None => UserPromptQueueDeliveryResolution::Retry,
        }
    }
}
