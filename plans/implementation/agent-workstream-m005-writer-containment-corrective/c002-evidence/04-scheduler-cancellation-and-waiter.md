# C002 evidence 04 — Scheduler admission, cancellation and waiter patterns

Read-only source audit for M005 writer-containment corrective, milestone C002
("durable writer permit and quarantine"). Scope audited:
`crates/codegg-core/src/jobs/mod.rs`, `src/scheduler/**`,
`scripts/check_scheduler_bypass.py`, `src/core/runtime_deps.rs` (scheduler/job
wiring parts). Other files are cited as corroborating evidence only.

Worktree: `/home/sugarwookie/projects/codegg/.worktrees/m005-writer-audit`
Base commit: `37a82843 docs(plans): accept ADR-0047 option B and unblock C002`

> Naming collision warning for later readers: `AGENTS.md:108-116` uses the label
> **"C002"** for the M003 *session user-prompt-queue* write-transaction-shape
> fix (`scripts/check_queue_write_transaction_shape.py`). That is a **different**
> C002 from this M005 writer-containment C002. Both constraints are listed in the
> checklist below because both bind a new durable write path.

---

## 1. What is the `JobScheduler`? Where is it defined, how is it constructed, what is its public API?

### Definition

`JobScheduler` is the single global admission-control scheduler. It is defined
at `src/scheduler/scheduler.rs:243`:

```rust
pub struct JobScheduler {
    store: Arc<dyn JobStore>,
    workspaces: Arc<WorkspaceServiceRegistry>,
    executors: Arc<AsyncMutex<crate::scheduler::executor::ExecutorRegistry>>,
    admission: Arc<AdmissionController>,
    queue: Arc<AsyncMutex<FairJobQueue>>,
    dispatch: AsyncMutex<()>,
    ...
}
```
(`src/scheduler/scheduler.rs:243-310`)

It is re-exported from the scheduler module at `src/scheduler/mod.rs:69`:
`pub use scheduler::{JobScheduler, SchedulerShutdownMode, SchedulerWake};`

The scheduler module surface is `src/scheduler/mod.rs:31-50` (`admission`,
`ci_watch`, `ci_watch_idle`, `config`, `eggwork`, `events`, `executor`,
`executors`, `fair_queue`, `idle_continuation`, `job_complete_audit`,
`job_notifications`, `permit`, `scheduler`, `snapshot`, `submission`,
`tool_program_executor`, `tool_program_notifications`, `types`).

### Construction

```rust
pub fn new(
    store: Arc<dyn JobStore>,
    workspaces: Arc<WorkspaceServiceRegistry>,
    config: ResolvedSchedulerConfig,
    daemon_generation: DaemonGeneration,
) -> Arc<Self>
```
(`src/scheduler/scheduler.rs:316-321`)

The body builds the `AdmissionController` (`src/scheduler/scheduler.rs:322`),
the `FairJobQueue` (`:323`), the completion cache (`:342`), the per-job
completion-signal registry (`:344-346`), the coalescing `Notify` (`:356`), and
the scheduler's own `shutdown` `CancellationToken` (`:357`).

Production construction is in the daemon, not in `runtime_deps.rs`:
`src/core/daemon_construct.rs:287-292` (enabled path) and
`src/core/daemon_construct.rs:343-348` (disabled placeholder). `runtime_deps.rs`
only *carries* the handle: field `scheduler: Option<Arc<crate::scheduler::JobScheduler>>`
at `src/core/runtime_deps.rs:101`, with the builder setter
`with_scheduler` at `src/core/runtime_deps.rs:421-429`. The `Clone` impl
propagates the handle at `src/core/runtime_deps.rs:156`.

### Public API surface (admission-relevant subset)

| Operation | Signature | Location |
|---|---|---|
| construct | `pub fn new(store, workspaces, config, daemon_generation) -> Arc<Self>` | `src/scheduler/scheduler.rs:316` |
| submit (raw) | `pub async fn submit(&self, spec: codegg_core::jobs::NewJob) -> Result<JobRecord, JobSchedulerError>` | `src/scheduler/scheduler.rs:743-746` |
| enqueue existing | `pub async fn enqueue_existing(&self, job: JobRecord) -> Result<(), JobSchedulerError>` | `src/scheduler/scheduler.rs:759` |
| wait | `pub async fn wait_for_completion(&self, job_id: &JobId, wait_timeout: Duration) -> Result<ExecutorCompletion, JobSchedulerError>` | `src/scheduler/scheduler.rs:776-780` |
| cancel | `pub async fn request_cancel(&self, job_id: &JobId, reason: &str) -> Result<codegg_core::jobs::CancelResult, JobSchedulerError>` | `src/scheduler/scheduler.rs:1972-1976` |
| status | `pub async fn snapshot(&self) -> SchedulerSnapshot` | `src/scheduler/scheduler.rs:1693` |
| reconcile | `pub async fn reconcile(&self) -> Result<ReconcileReport, JobSchedulerError>` | `src/scheduler/scheduler.rs:872` |
| dispatch batch | `pub async fn admit_and_dispatch_batch(self: Arc<Self>) -> usize` | `src/scheduler/scheduler.rs:1122` |
| main loop | `pub async fn run(self: Arc<Self>)` | `src/scheduler/scheduler.rs:1098` |
| spawn loop | `pub fn spawn_run(self: &Arc<Self>) -> tokio::task::JoinHandle<()>` | `src/scheduler/scheduler.rs:1019` |
| restart recovery | `pub async fn recover_at_startup(&self, policy: &codegg_core::jobs::RecoveryPolicy) -> Result<codegg_core::jobs::RecoveryReport, JobSchedulerError>` | `src/scheduler/scheduler.rs:2076-2079` |
| shutdown | `pub async fn shutdown(&self, mode: SchedulerShutdownMode)` | `src/scheduler/scheduler.rs:1783` |
| wake | `pub fn wake(&self, reason: WokeReason)` | `src/scheduler/scheduler.rs:687` |
| accessors | `config()`, `admission()`, `store()`, `workspaces()`, `daemon_generation()`, `is_enabled()`, `is_mandatory()` | `src/scheduler/scheduler.rs:473-502` |

**`submit` is explicitly NOT the admission boundary** (verbatim doc comment,
`src/scheduler/scheduler.rs:737-742`):

> WARNING: this is NOT the admission boundary. It skips every
> `JobSubmissionService` check (payload validation, resource policy, leases,
> idempotency, size caps) and exists for scheduler subsystem internals and test
> fixtures. Production callers must submit through `JobSubmissionService::submit`
> (`scripts/check_scheduler_bypass.py` enforces this).

The production submission boundary is
`pub async fn submit(&self, key: Option<SubmissionKey>, mut spec: NewJob) -> Result<SubmittedJob, JobSubmissionError>`
at `src/scheduler/submission.rs:171-175`.

`JobSchedulerError` variants: `Store`, `Registry`, `Workspace(String)`,
`SchedulerDisabled`, `Internal(String)` (`src/scheduler/scheduler.rs:2158-2169`).

---

## 2. How is job CANCELLATION represented and propagated? What is the cancellation token type? How does a cancelled job wake a blocked waiter?

### The token type

The token is `tokio_util::sync::CancellationToken`. It appears in
`JobExecutionContext` as `pub cancellation: CancellationToken`
(`src/scheduler/executor.rs:161`), and in the scheduler's internal running-attempt
record as `pub cancellation: CancellationToken` (`src/scheduler/scheduler.rs:115`).
The scheduler's own shutdown token is `shutdown: CancellationToken`
(`src/scheduler/scheduler.rs:288`).

### The durable cancellation types (`crates/codegg-core/src/jobs/mod.rs`)

```rust
/// Reason attached to a cancellation request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelReason {
    pub requested_by: String,
    pub reason: String,
}
```
(`crates/codegg-core/src/jobs/mod.rs:1583-1586`; constructor at `:1589`)

```rust
/// Result of [`JobStore::request_cancel`].
pub struct CancelResult {
    pub job_id: JobId,
    pub state: CancelOutcome,
    pub terminal: bool,
}
```
(`crates/codegg-core/src/jobs/mod.rs:1599-1603`)

```rust
pub enum CancelOutcome {
    /// Cancellation was applied immediately because no attempt had
    /// started yet.
    Cancelled,
    /// Cancellation was marked as requested; the executor will be
    /// notified.
    Requested,
    /// The job was already terminal; cancellation was rejected.
    AlreadyTerminal,
}
```
(`crates/codegg-core/src/jobs/mod.rs:1609-1618`)

The store-side API (`crates/codegg-core/src/jobs/mod.rs:1832-1839`):

```rust
/// Persist a cancellation request. Honours the deterministic
/// precedence rules: queued → cancelled; running → request recorded
/// and reported to caller; terminal → rejected.
async fn request_cancel(
    &self,
    id: &JobId,
    reason: CancelReason,
) -> Result<CancelResult, JobStoreError>;
```

Durable job states (`crates/codegg-core/src/jobs/mod.rs:895-906`):
`Scheduled, Queued, Running, Completed, Failed, Cancelled, TimedOut, Interrupted,
Blocked, Expired`.

### Propagation path (scheduler-owned)

`JobScheduler::request_cancel` (`src/scheduler/scheduler.rs:1972-2070`):

1. `let cancel = CancelReason::new("scheduler", reason);` (`:1977`) then
   `self.store.request_cancel(job_id, cancel).await?` (`:1979`).
2. If the outcome is `CancelOutcome::Cancelled` (i.e. the job was still queued),
   the job terminalizes without ever running an executor. The scheduler then
   explicitly wakes waiters (`:2038-2040`):
   ```rust
   if matches!(result.state, codegg_core::jobs::CancelOutcome::Cancelled) {
       self.signal_job(job_id).await;
   }
   ```
   with the comment at `:2034-2037`: *"A queued job reaches its terminal state
   here without any executor ever running, so it never publishes a completion.
   Wake its waiters explicitly, otherwise they only learn about the cancel from
   the durable-state backstop tick."*
3. The job is removed from the in-memory fair queue:
   `q.remove(job_id, QueueRemovalReason::Cancelled)` (`:2042-2044`).
4. If the job is running, the executor's `CancellationToken` is cancelled
   (`:2046-2051`):
   ```rust
   let running = self.running.lock().await;
   for ra in running.values() {
       if ra.job_id == *job_id {
           ra.cancellation.cancel();
       }
   }
   ```
5. Descendants are cancelled independently of the executor future
   (`:2057-2067`, `self.store.cancel_descendants(job_id, ...)`), so children are
   cleaned up even if the executor future already exited.

### How a cancelled job wakes a blocked waiter

`wait_for_completion` (`src/scheduler/scheduler.rs:776-867`) parks on a
**per-job broadcast `watch` signal**, never on the scheduler's shared
`Notify`:

- The waiter subscribes *before* its first completion-cache read
  (`:786-789`), which is what makes the wait race-free.
- The park is (`:845-859`):
  ```rust
  let tick = tokio::time::sleep(remaining.min(COMPLETION_RECHECK_INTERVAL));
  tokio::pin!(tick);
  if signal_open {
      tokio::select! {
          changed = completion_signal.changed() => {
              if changed.is_err() {
                  // Sender evicted; fall back to the tick below.
                  signal_open = false;
              }
          }
          _ = &mut tick => {}
      }
  } else {
      (&mut tick).await;
  }
  ```
- The signal is advanced by `signal_job` (`:726-730`):
  ```rust
  async fn signal_job(&self, job_id: &JobId) {
      let mut signals = self.completion_signals.lock().await;
      signals.advance(job_id);
      signals.evict_over_capacity();
  }
  ```
- The registry is `CompletionSignals` (`:166-200`): a `HashMap<JobId, watch::Sender<u64>>`
  with insertion-order eviction and `capacity` (`:166-171`, `:184-195`);
  `COMPLETION_SIGNAL_CAPACITY = 1024` (`:73`).
- The `watch` receiver replays the "value changed" fact to a later subscriber,
  which is the documented reason the ordering in `wait_for_completion` is safe
  (`:162-165`).
- The durable-state backstop is `COMPLETION_RECHECK_INTERVAL = 250ms`
  (`:68`). It is explicitly *not* the delivery path; it covers terminal
  transitions written by startup recovery or an external `JobStore` writer
  (`:59-67`, `:842-844`).
- A waiter that observes a terminal durable job state synthesises the
  completion from that state (`:805-823`), mapping `Cancelled` →
  `ExecutorStatus::Cancelled` (`:809`).

The whole path is thus: `request_cancel` → durable `request_cancel` → outcome
`Cancelled` → `signal_job` → `watch` advance → blocked `wait_for_completion`
wakes → reads durable terminal state → returns
`ExecutorCompletion { status: Cancelled, ... }`.

**A second, independent wake path** exists for the case where cancellation
lands *after* the attempt was created but *before* the executor task is
spawned (`src/scheduler/scheduler.rs:1355-1363`):

```rust
if let Some(current) = self.store.get_job(&job.job_id).await? {
    if current.cancel_requested_at.is_some() {
        cancel_token.cancel();
    }
}
```

### The parent-cancels-child pattern (closest existing "waiter" composition)

`src/scheduler/tool_program_executor.rs:808-847` is the repo's existing
"parent waits for a child job and propagates cancellation" pattern:

```rust
let completion = loop {
    if self.cancellation.as_ref()
        .is_some_and(tokio_util::sync::CancellationToken::is_cancelled)
    {
        let _ = submission.scheduler()
            .request_cancel(&submitted.job_id, "parent_tool_program_cancelled")
            .await;
        return Err(InterpreterError::Cancelled);
    }
    match submission.scheduler()
        .wait_for_completion(&submitted.job_id, std::time::Duration::from_millis(100))
        .await
    { ... }
};
```

i.e. a bounded, repeatedly-polled `wait_for_completion` whose timeout is
decremented from an overall `wait_timeout` budget (`:806`, `:826-838`), and
which cancels the child on parent cancellation *and* on timeout.

---

## 3. How does the scheduler's ADMISSION control work? What are "resource keys" and "exclusivity keys"? What does an empty exclusivity-key set mean in practice?

### The request shape

```rust
/// Resource request metadata persisted before Phase 5 admission exists.
pub struct ResourceRequest {
    pub cpu_weight: u32,
    pub memory_mb_hint: u64,
    pub process_slots: u16,
    pub io_weight: u32,
    pub network_slots: u16,
    pub exclusivity_keys: Vec<String>,
}
```
(`crates/codegg-core/src/jobs/mod.rs:686-693`)

```rust
pub struct PermitDimensions {
    pub cpu_weight: u32,
    pub memory_mb_hint: u64,
    pub process_slots: u16,
    pub io_weight: u32,
    pub network_slots: u16,
    pub exclusivity_keys: Vec<String>,
}
```
(`src/scheduler/permit.rs:19-26`)

The mapping from a durable job to admission dimensions is
`build_permit_dimensions` (`src/scheduler/scheduler.rs:2197-2206`).

### The decision

```rust
pub enum AdmissionDecision {
    Admitted(ResourcePermitGuard),
    TemporarilyBlocked(BlockReason),
    Impossible(UnschedulableReason),
}
```
(`src/scheduler/admission.rs:29-38`)

`BlockReason` variants (`src/scheduler/admission.rs:44-52`):
`InsufficientProcessSlots`, `InsufficientCpuWeight`, `InsufficientMemory`,
`InsufficientIoWeight`, `InsufficientNetworkSlots`, `KeyContended { key: String }`,
`QueueFull`.

`UnschedulableReason` variants (`src/scheduler/admission.rs:57-63`).

The admission entry points are `try_admit_arc` (`src/scheduler/admission.rs:139`)
and `try_admit` (`src/scheduler/admission.rs:152`). Both are **non-blocking**.

### "Resource keys" vs "exclusivity keys" — the decisive rule

There is no separate "resource key" concept. The only key mechanism is
`exclusivity_keys`, and **a key only conflicts if its name starts with
`exclusive:`**. Verbatim (`src/scheduler/admission.rs:199-209`):

```rust
// Exclusivity keys: a key is "conflicting" if its name starts
// with `exclusive:`. All other keys are read-only/no-conflict.
for key in &dims.exclusivity_keys {
    if key.starts_with("exclusive:") {
        let bare = key.trim_start_matches("exclusive:").to_string();
        if g.held_keys.get(&bare).copied().unwrap_or(0) > 0 {
            g.rejected += 1;
            return AdmissionDecision::TemporarilyBlocked(BlockReason::KeyContended {
                key: bare,
            });
        }
    }
}
```

Reservation mirrors the same rule (`src/scheduler/admission.rs:220-225`) and
release mirrors it again (`src/scheduler/admission.rs:279-287`).

A key without the `exclusive:` prefix is **stored nowhere and blocks nothing** —
it is not reserved, not counted, and not released. The only defence against
that is the submission-boundary normaliser, which rewrites legacy unscoped
names (`src/scheduler/submission.rs:467-478`):

```rust
fn apply_resource_policy(spec: &mut NewJob) {
    if spec.resource_request == ResourceRequest::default() {
        spec.resource_request = ResourceRequest::for_kind(spec.kind);
    }
    // Legacy callers may carry the old unscoped names. Normalize them at the
    // single submission boundary so admission sees the intended keys.
    for key in &mut spec.resource_request.exclusivity_keys {
        if !key.starts_with("exclusive:") {
            *key = format!("exclusive:{key}");
        }
    }
}
```

### What an empty exclusivity-key set means in practice — C001 hole #9 CONFIRMED

`ResourceRequest::for_kind` (`crates/codegg-core/src/jobs/mod.rs:713-804`) is
the single place per-kind profiles are set. The exact empty-set kinds:

| JobKind | `exclusivity_keys` | Line |
|---|---|---|
| `JobKind::AgentTurn \| Subagent \| Research` | `Vec::new()` | `crates/codegg-core/src/jobs/mod.rs:715-722` |
| `JobKind::Build` | `vec!["exclusive:workspace-mutation"]` | `:723-730` |
| `JobKind::Lint` | `Vec::new()` | `:731-738` |
| `JobKind::Format` | `vec!["exclusive:workspace-mutation"]` | `:739-746` |
| `JobKind::Test` | `Vec::new()` | `:747-754` |
| `JobKind::Shell \| JobKind::ManagedProcess` | `Vec::new()` | `:755-762` |
| `JobKind::Python` | `Vec::new()` | `:763-770` |
| `JobKind::GitRead` | `Vec::new()` | `:771-778` |
| `JobKind::GitMutation` | `vec!["exclusive:worktree-mutation"]` | `:779-786` |
| `JobKind::Maintenance` | `Vec::new()` | `:787-794` |
| `JobKind::ToolProgram` | `Vec::new()` | `:795-802` |
| `JobKind::Unsupported` | `ResourceRequest::default()` (empty) | `:803`; default at `:695-709` |

**C001 hole #9 is CONFIRMED exactly as stated**: `JobKind::Shell`,
`ManagedProcess`, `Test`, `Lint`, `Python`, and `ToolProgram` all have
**empty exclusivity-key sets**.

**Practical meaning**: an empty set means the job is subject to **no key-based
exclusion at all**. It still consumes scalar capacity (cpu/memory/process/io/
network — every per-kind profile is non-zero; the invariant is asserted at
`src/scheduler/submission.rs:614-628`), but two such jobs may run
**concurrently** in the same workspace, and a `Shell` job contends with nothing
on keys. Only `Build`/`Format` (`exclusive:workspace-mutation`) and
`GitMutation` (`exclusive:worktree-mutation`) exclude each other.

This is exactly why C002 must NOT be implemented as a scheduler exclusivity key:
a `Shell` job (the actual writer) carries no key at all.

### The dispatch-side consequence

`try_dispatch_next` (`src/scheduler/scheduler.rs:1153-1689`):
`build_permit_dimensions` (`:1171`) → `try_admit_arc` (`:1174`) → on
`TemporarilyBlocked` it emits `SchedulerEvent::AdmissionBlocked` and
**re-inserts the entry** to retry on the next tick (`:1188-1201`); on
`Impossible` it marks the job unschedulable (`:1203-1209`); then it acquires
the workspace services lease (`:1213-1236`) and **drops the permit and
requeues** if the lease fails (`:1216-1217`).

`ResourcePermitGuard` is RAII (`src/scheduler/permit.rs:41-43`, `Drop` at
`:114-122`):

```rust
impl Drop for ResourcePermitGuard {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            if let Some(c) = inner.controller {
                c.release(inner.dimensions);
            }
        }
    }
}
```

The permit is moved into the executor task and released only when the
executor fully stops (`src/scheduler/executor.rs:138-144`).

---

## 4. Is there an existing BOUNDED WAIT / timeout / park mechanism a permit waiter should reuse? Is there a queue-depth bound? A fairness mechanism?

### The bounded wait to reuse

`JobScheduler::wait_for_completion` (`src/scheduler/scheduler.rs:776-867`) is
the repo's canonical bounded async waiter. Its contract is documented at
`:773-775`:

> Wait for a scheduler-owned completion without executing the job in a
> caller task. The timeout is a client wait bound, not the job's own
> process timeout.

Its structure, in order:

1. `let deadline = Instant::now() + wait_timeout;` (`:781`).
2. Subscribe to the per-job broadcast signal **before** the first
   completion-cache read (`:786-789`).
3. Loop: check the completion cache (`:794-804`); check durable terminal state
   (`:805-829`); check the deadline (`:830-835`); park on the per-job signal
   with a durable-state backstop tick (`:845-859`); re-check the deadline
   (`:860-865`).
4. On deadline expiry it returns
   `Err(JobSchedulerError::Internal(format!("timed out waiting for job {}", job_id)))`
   (`:830-835`, `:860-865`).

**This is the model a C002 permit waiter should mirror**: subscribe-before-read
ordering, a per-resource broadcast signal (not a shared `Notify`), an explicit
client-side deadline, and a durable-state backstop tick that covers
transitions the waker did not publish.

**Important constraint for C002**: this mechanism is *per-job* and lives inside
`JobScheduler`. It is `JobId`-keyed (`CompletionSignals.entries:
HashMap<JobId, watch::Sender<u64>>`, `src/scheduler/scheduler.rs:167`) and the
completion cache is likewise `HashMap<JobId, (u64, ExecutorCompletion)>`
(`:119`). A permit waiter keyed by *worktree* cannot reuse the existing
registry without either (a) generalising `CompletionSignals` to a second key
space, or (b) introducing its own per-worktree broadcast registry. It **must
not** park on the scheduler's shared `notify` (`src/scheduler/scheduler.rs:287`,
`:1108`) — the source explicitly forbids this: *"Completion delivery uses this
map, never the shared `notify`, so a terminal outcome for one job cannot
consume another job's wakeup"* (`:260-263`) and *"`wait_for_completion`
deliberately does NOT park on this — it uses `completion_signals` instead"*
(`:283-286`).

The other bounded-wait primitive available to a caller is the
**executor execution timeout** (`src/scheduler/scheduler.rs:1320-1323`,
`:1428-1453`): `job.deadline` → `(deadline - Utc::now())` else `job.timeout`,
applied via `tokio::time::timeout`, followed by a
`EXECUTOR_CLEANUP_GRACE = 5s` grace (`:57`, `:1434-1451`).

### Queue-depth bound

Yes, for the **job** queue (`QueueCaps`, `src/scheduler/config.rs:73-78`):

```rust
pub struct QueueCaps {
    pub max_total: usize,
    pub max_per_workspace: usize,
    pub max_interactive_per_session: usize,
    pub claim_batch: usize,
}
```

Defaults (`src/scheduler/config.rs:103-108`): `max_total: 256`,
`max_per_workspace: 64`, `max_interactive_per_session: 8`, `claim_batch: 32`.

Enforcement is in `FairJobQueue::insert` (declared at `src/scheduler/fair_queue.rs:245`; the bound check at `:276-278`):

```rust
// Enforce bounds. Bounded queue: never silently drop existing
// queued jobs. New jobs are rejected if bounds are exceeded.
if self.total_count >= self.cfg.queue.max_total {
    return Err(QueueInsertError::Overflow);
}
```
plus a per-workspace cap at `:284-286`. `QueueInsertError::Overflow` is a
typed error (`src/scheduler/types.rs:118-121`), and the queue overflow is
surfaced as `BlockReason::QueueFull` (`src/scheduler/admission.rs:51`).

**There is no queue-depth bound for a permit waiter.** C002 must introduce one
itself (C002 work package B requires "no second resource admission/queue", so
the bound must be a *waiter* bound, not a second job queue).

### Fairness mechanism

Yes, for the job queue:

- `FairnessWeights` (`src/scheduler/config.rs:81-88`): `interactive_weight`,
  `normal_weight`, `background_weight`, `maintenance_weight`,
  `max_high_priority_burst`, `aging_secs`. Defaults at `:109-116`:
  `8 / 4 / 2 / 1`, `max_high_priority_burst: 8`, `aging_secs: 300`.
- Anti-starvation: `FairJobQueue::select_next` — *"after
  `max_high_priority_burst` consecutive high-priority admissions, force a
  non-high-priority admission if any eligible entry exists"*
  (`src/scheduler/fair_queue.rs:389-393`).
- Round-robin across workspaces: `select_next` picks a non-empty lane whose id
  differs from the cursor (`src/scheduler/fair_queue.rs:128-145`).
- Aging: `reconcile` applies aging (`src/scheduler/scheduler.rs:870-872`).
- The dispatch loop examines a bounded candidate window rather than treating a
  blocked head job as global back-pressure, so an unrelated workspace can make
  progress (`src/scheduler/scheduler.rs:1128-1133`).

**No fairness mechanism exists for a permit waiter.** C002's "waiters cannot
starve other groups indefinitely" requirement (plan WP-B) has no existing
scheduler primitive to reuse; `FairJobQueue` is job-lane-specific and keyed by
`WorkspaceId`/priority, not by worktree.

---

## 5. What exactly does `scripts/check_scheduler_bypass.py` enforce? Would a new "permit waiter" that does its own parking/queueing trip it? What are the permitted call sites?

### What it is

A **static text lint** over `.rs` files (`scripts/check_scheduler_bypass.py:158-171`,
`:174-182`). It walks `src/` and `tests/` (`:235-252`), excluding
`src/scheduler/**` and `src/test_runner/**` from the `src/` walk
(`:238-243`) — note this is a *whole-directory* exclusion applied by filtering
the file list, so `src/scheduler/**` is never scanned at all.

For each file it applies six regex rules (`scripts/check_scheduler_bypass.py:56-115`):

| # | Pattern | Message | Exemptions |
|---|---|---|---|
| 1 | `\btest_runner::runner::resolve_and_run_test\b` | must route through the scheduler | `src/scheduler/**`, `tests/**`, `src/test_runner/runner.rs` (`:58-65`) |
| 2 | `\bdispatch_to_test_runner\b` | must submit a job to the scheduler | `tests/**`, `docs/**`, `architecture/**` (`:67-74`) |
| 3 | `\bSubAgentJobDispatcher\b` | legacy bridge | `src/job_dispatcher.rs`, `src/agent/worker.rs`, `src/scheduler/**`, `tests/**` (`:76-84`) |
| 4 | `\.spawner\(\)\.send(?:_async)?\(` | direct subagent pool sends | `src/scheduler/**`, `src/tool/task.rs`, `src/agent/task.rs`, `src/job_dispatcher.rs`, `tests/**` (`:86-95`) |
| 5 | `\.spawn_loop\(` | background scheduler loops | `src/agent/task.rs`, `src/main.rs`, `tests/**` (`:97-104`) |
| 6 | `\bscheduler\s*\.\s*submit\s*\(` | `JobScheduler::submit` bypasses `JobSubmissionService` admission checks | `src/scheduler/**`, `tests/**` (`:106-114`) |

Escape hatch: an inline `// scheduler-audit: <reason>` annotation, on the same
line or **exactly one line above** the call site
(`scripts/check_scheduler_bypass.py:119-121`, `:138-155`). Recognized reasons:
`scheduler-owned`, `standalone-compat`, `definition-site`, `test-only`
(`:26-31`, `:120`).

The guard carries its own `--self-test` (`:185-229`, `:233-234`) that proves
the annotation window is tight.

### Would a new "permit waiter" that does its own parking/queueing trip it?

**No — not on the grounds of "doing its own parking/queueing."** The guard has
no rule about parking, queueing, semaphores, waiters, or permits. It is a
regex lint over six narrow call-shape patterns. A permit waiter that uses
`tokio::sync::Notify`, `watch`, `Semaphore`, `CancellationToken`, or its own
queue would not match any pattern.

**But two of the six rules bind a permit waiter in practice:**

- **Rule 6** (`scheduler . submit (`): if the permit waiter causes a job to be
  submitted, it must go through `JobSubmissionService::submit`, not
  `JobScheduler::submit`. The rule's own message says so (`:111-113`):
  > `JobScheduler::submit` bypasses JobSubmissionService admission checks
  > (payload validation, resource policy, leases, idempotency). Submit through
  > `JobSubmissionService::submit` instead.

  Note the regex matches `self.scheduler.submit(` too, because `.` is a
  non-word character and `\b` therefore matches before `s`.

- **Rule 4** (`.spawner().send(`): a permit waiter must not dispatch work to
  the subagent pool directly; it must go through the scheduler.

Additionally, if the permit waiter lives **inside** `src/scheduler/**`, the
guard never scans it at all (`scripts/check_scheduler_bypass.py:238-243`), so
rules 1-4 are moot there. If it lives **outside** `src/scheduler/**`, rules 1-6
apply to it with the exemptions listed above.

### Permitted call sites (summary)

- Anything under `src/scheduler/**` (excluded from the scan entirely).
- Anything under `tests/**`.
- `src/test_runner/runner.rs` (definition site of `resolve_and_run_test`).
- `src/job_dispatcher.rs`, `src/agent/worker.rs` (legacy `SubAgentJobDispatcher`).
- `src/tool/task.rs`, `src/agent/task.rs` (standalone compatibility).
- `src/main.rs` (standalone `spawn_loop` wiring).
- `docs/**`, `architecture/**` (documentation references).
- **Any** site (outside the above) carrying an adjacent `// scheduler-audit: <reason>`.

---

## 6. What is `JobExecutionContext`? What fields does it have? Does the argv executor ignore `workspace_root`?

### The struct

```rust
pub struct JobExecutionContext {
    pub job: JobRecord,
    pub attempt_id: AttemptId,
    pub daemon_generation: DaemonGeneration,
    pub workspace_id: WorkspaceId,
    /// Canonical workspace root captured from the scheduler-owned lease.
    /// Executors must use this instead of consulting process-global CWD.
    pub workspace_root: std::path::PathBuf,
    /// Workspace-scoped artifact store captured by the scheduler lease.
    /// `None` is retained for lightweight executor unit tests; production
    /// scheduler dispatch always supplies it.
    pub run_store: Option<Arc<dyn codegg_core::run_store::RunStore>>,
    /// Durable attempt store and scheduler-captured source identity. Executors
    /// may seal only at a backend-specific immutable-input boundary.
    pub subject_store: Option<Arc<dyn codegg_core::jobs::JobStore>>,
    pub source_subject_started: Option<codegg_core::jobs::ExecutionSubjectProvenance>,
    pub cancellation: CancellationToken,
    pub progress: Arc<dyn JobProgressSink>,
    pub resources: ResourcePermitGuard,
}
```
(`src/scheduler/executor.rs:145-164`)

The `workspace_root` doc comment is normative: *"Executors must use this
instead of consulting process-global CWD"* (`src/scheduler/executor.rs:150-152`).

The context is built by the scheduler at dispatch time
(`src/scheduler/scheduler.rs:1275-1290`); `workspace_root` comes from the
**scheduler-owned lease**, not from the process CWD:
`workspace_root: lease.path_policy().canonical_root.clone()`
(`src/scheduler/scheduler.rs:1280`).

### C001 hole #10 — the argv executor ignores it — CONFIRMED

`ManagedArgvExecutor` is declared at `src/scheduler/executors.rs:230-238`:

```rust
pub struct ManagedArgvExecutor {
    label: &'static str,
}
```

Its `execute` derives the working directory **only** from the durable job
payload (`src/scheduler/executors.rs:321-340`):

```rust
let cwd = match &ctx.job.payload {
    JobPayload::ManagedArgv { cwd, .. } | JobPayload::Shell { cwd, .. } => {
        cwd.clone().map(PathBuf::from)
    }
    _ => None,
};

let Some(cwd) = cwd else {
    // production path: fall back to workspace root via
    // ExecutionContext was used by callers submitting; here we
    // best-effort to the job's session's recorded workspace
    // dir if available. For Phase 5 the bash tool wires cwd
    // explicitly into the payload, so missing cwd is a
    // configuration error.
    return failure_completion(
        started,
        ExecutorStatus::Failed,
        "managed argv missing cwd".into(),
    );
};
```

**`ctx.workspace_root` is never read in this path.** The comment at `:329-331`
claims a "fall back to workspace root", but the code does not do it — it fails
the job instead. The comment is stale relative to the code.

Contrast: the `TestJobExecutor` and `PythonJobExecutor` *do* consume
`ctx.workspace_root` (`src/scheduler/executors.rs:127-142` and `:1613-1625`).
The C001 hole is therefore specific to the managed-argv/shell path — which is
the path a writer would take.

**C001 hole #10 is CONFIRMED.**

---

## 7. Is there an existing pattern for a component that (a) waits for a resource, (b) must not hold another resource while waiting, and (c) must not leak a token on cancellation?

No single component does all three, but the repo contains three concrete,
citable patterns that together constitute the precedent. The closest single
analogue is the **`WorktreeService` lease** (durable, generation-fenced,
single-owner) composed with **`JobScheduler::wait_for_completion`** (bounded
wait) and **`ResourcePermitGuard` / `WorkspaceServicesLease`** (RAII release).

### (a) Waiting for a resource

**`JobScheduler::wait_for_completion`** — `src/scheduler/scheduler.rs:776-867`
(described in §4). Bounded, per-resource broadcast signal, durable-state
backstop.

**`WorkspaceServiceRegistry::acquire`** — `crates/codegg-core/src/workspace_services.rs:495-505`:

```rust
pub async fn acquire(
    &self,
    workspace_id: &WorkspaceId,
) -> Result<WorkspaceServicesLease, WorkspaceServiceError> {
    let services = self.activate(workspace_id).await?;
    services.increment_leases();
    Ok(WorkspaceServicesLease { ... })
}
```

`activate` (`:511-525`) is the single-flight pattern: *"Uses a per-workspace
async mutex so concurrent first activations produce exactly one bundle. The
lock is `Send`-safe across awaits."* (`:507-510`).

**`WorkspaceLockTable::acquire_repository`** —
`crates/codegg-core/src/workspace_services.rs:216-219`: a per-canonical-root
`Arc<AsyncMutex<()>>` so that native Git tool calls and Bash-translated Git
operations contend on the *same* lock.

### (b) Must not hold another resource while waiting

**This is the sharpest precedent in the repo, and it is a direct analogue of
C002's "no permit held during provider inference, approval or submission of a
nested job that needs the same permit" rule.**

`JobSubmissionService::submit` — `src/scheduler/submission.rs:187-195`:

```rust
// Acquiring and immediately dropping a lease validates registration
// and canonical workspace identity without pinning a service bundle
// for the lifetime of the job.
let lease = self
    .workspaces
    .acquire(&spec.workspace_id)
    .await
    .map_err(|e| JobSubmissionError::Workspace(e.to_string()))?;
drop(lease);
```

The lease is taken **only** to validate, then dropped before the durable write.
The repo's idiom for "I need to consult a resource but must not hold it" is
*acquire → validate → drop*.

A second instance: `JobScheduler::try_dispatch_next` explicitly drops the
admission permit when a later acquisition fails, so a partially-dispatched
job does not strand capacity
(`src/scheduler/scheduler.rs:1216-1217`, `:1246-1247`, `:1253-1254`,
`:1260-1261`).

A third: `JobSubmissionService::submit` reads the idempotency index
"without holding its lock across durable reads"
(`src/scheduler/submission.rs:201-202`) and "the potentially large durable
scans above happen without this mutex held" (`:260-262`).

### (c) Must not leak a token on cancellation

**`ResourcePermitGuard`** — RAII release on drop
(`src/scheduler/permit.rs:114-122`, quoted in §3), plus an explicit
`detach` that takes the dimensions without releasing
(`src/scheduler/permit.rs:85-96`).

**`WorkspaceServicesLease`** — `crates/codegg-core/src/workspace_services.rs:344-351`
and `:383-389`:

```rust
impl Drop for WorkspaceServicesLease {
    fn drop(&mut self) {
        if !self.released.swap(true, Ordering::AcqRel) {
            self.services.decrement_leases();
        }
    }
}
```

The `released: AtomicBool` guard is the **idempotent-release** idiom: a lease
cannot be double-decremented.

**The `WorktreeService` lease** is the closest durable, generation-fenced,
single-owner analogue to C002's permit state machine
(`Available → Held(owner, generation)`). Its API and typed refusals:

```rust
pub struct WorktreeLease {
    pub worktree_id: WorktreeId,
    pub owner_run_id: AgentRunId,
    pub generation: u64,
    pub acquired_at: i64,
    pub renewed_at: i64,
    pub released_at: Option<i64>,
}
```
(`crates/codegg-core/src/worktree_service.rs:138-145`)

```rust
pub enum WorktreeServiceError {
    ...
    #[error("worktree lease conflict: {0}")]
    LeaseConflict(String),
    #[error("stale worktree lease generation: expected {expected}, actual {actual}")]
    StaleGeneration { expected: u64, actual: u64 },
    ...
}
```
(`crates/codegg-core/src/worktree_service.rs:189-207`; `LeaseConflict` at
`:192-193`, `StaleGeneration` at `:194-195`)

The generation is bumped on every acquire
(`crates/codegg-core/src/worktree_service.rs:415`, and the SQL equivalent at
`:642-648`), and every renew/release re-checks it and refuses a stale holder
(`:441-446`, `:475-480`, `:674-679`). The durable acquire is a single
conditional UPDATE with a `rows_affected() != 1` conflict check
(`:644-647`) — i.e. a **single-owner transactional CAS**, which is precisely
what C002 WP-A asks for ("single-owner transactional CAS with monotonic
revision").

**The closest analogue to the restart-reconciliation seam** is
`WorktreeOwnerResolver` (`crates/codegg-core/src/worktree_service.rs:299-309`):

```rust
/// Narrow restart-reconciliation seam. The core worktree domain does not
/// depend on the agent runtime; the daemon supplies this adapter from its
/// durable run store.
#[async_trait]
pub trait WorktreeOwnerResolver: Send + Sync {
    async fn is_terminal(&self, owner: &AgentRunId) -> Result<bool, String>;
}
```

and `reconcile_repository` / `reconcile_all`
(`crates/codegg-core/src/worktree_service.rs:1152-1156`, `:1233-1241`).

### Summary of the closest analogue

**`WorktreeService` lease + `JobScheduler::wait_for_completion` +
RAII-release guards** is the composite precedent:
durable generation-fenced single-owner token (worktree lease), bounded
per-resource wait (scheduler completion signal), acquire→validate→drop
discipline (submission service), and idempotent RAII release
(`ResourcePermitGuard`, `WorkspaceServicesLease`).

---

## 8. How does the daemon wire the scheduler into the runtime? What is the construction order, and where would a permit service be added? Are there ordering constraints?

### `src/core/runtime_deps.rs` — the carrier

`CoreRuntimeDeps` (`src/core/runtime_deps.rs:54`) holds the scheduler-related
handles:

| Field | Line |
|---|---|
| `job_store: Arc<dyn JobStore>` | `src/core/runtime_deps.rs:88` |
| `schedule_store: Arc<dyn ScheduleStore>` | `:90` |
| `recovery_policy: RecoveryPolicy` | `:92` |
| `daemon_generation: DaemonGeneration` | `:94` |
| `scheduler: Option<Arc<crate::scheduler::JobScheduler>>` | `:101` |
| `submission: Option<Arc<crate::scheduler::JobSubmissionService>>` | `:104` |
| `job_notification_service: Option<Arc<...JobNotificationService>>` | `:109-110` |
| `ci_watch_service: Option<Arc<...CiWatchService>>` | `:114` |
| `continuation_service: Option<Arc<...ContinuationService>>` | `:122` |
| `ci_watch_driver: Option<Arc<...CiWatchIdleDriver>>` | `:126` |
| `scheduler_config: crate::scheduler::config::ResolvedSchedulerConfig` | `:130` |

Builder setters: `with_scheduler` (`:421-429`), `with_submission` (`:431-437`).

The `Clone` impl propagates all of them (`:139-165`; scheduler at `:156`).

**Daemon-scoped singleton pattern.** `CoreRuntimeDeps` already hosts the
"create once per daemon lifetime, share the same `Arc`" idiom, with pointer
identity as the singleton proof:

```rust
/// C001 shared daemon-scoped notifier, creating it once on first use.
/// The same `Arc` is returned on every call (pointer identity is the
/// singleton proof); per-turn construction must not replace it.
pub fn shared_job_notification_service(
    &mut self,
) -> Arc<crate::scheduler::job_notifications::JobNotificationService> {
```
(`src/core/runtime_deps.rs:169-186`; also `shared_ci_watch_service` at
`:190-209`, `shared_continuation_service` at `:214-226`)

**This is where a permit service belongs**: a new
`permit_service: Option<Arc<WriterPermitService>>` field on `CoreRuntimeDeps`
plus a `shared_writer_permit_service(&mut self) -> Arc<...>` accessor following
the `shared_job_notification_service` shape exactly.

### Construction order in the daemon

The scheduler is built during `CoreDaemon` construction in
`src/core/daemon_construct.rs`:

1. `scheduler_config` resolved from config (`:275-283`).
2. `JobScheduler::new(deps.job_store.clone(), workspace_services.clone(),
   scheduler_config.clone(), deps.daemon_generation.clone())` (`:287-292`).
3. Default executors registered **synchronously**
   (`register_default_executors_sync_with_agent_runs_and_control_and_worktree`,
   `:294-300`).
4. `deps.scheduler = Some(scheduler.clone())` (`:352`).
5. `run_control.set_scheduler_sync(scheduler.clone())` (`:356`) — *"The
   scheduler is allowed to cancel the owned job after the durable mailbox
   intent has been committed."*
6. `scheduler.configure_agent_run_store_sync(...)` (`:358`).
7. `JobSubmissionService::new_with_goal_store(deps.job_store.clone(),
   scheduler.clone(), workspace_services.clone(), deps.daemon_generation.clone(), ...)`
   (`:365-371`) or `new(...)` (`:372-377`); `deps.submission = Some(...)` (`:379`).
8. Daemon-scoped singletons: `deps.shared_job_notification_service()` (`:385`),
   `deps.shared_ci_watch_service()` (`:386`).
9. Tool-program executor registered with the submission service (`:463-472`).

**Ordering constraints relevant to a permit service:**

- The scheduler needs `deps.job_store`, `workspace_services` and
  `deps.daemon_generation` — all available before construction.
- The permit service, per C002 WP-E ("Separate permit from scheduling
  capacity"), should be constructed **independently of** the scheduler, and
  must not be a second admission queue. It needs `deps.job_store` (or its own
  store), `deps.daemon_generation`, and — for WP-E's "root daemon adapter
  supplies authorization" — the daemon-boundary identity, **not** the
  scheduler.
- If the permit service needs to *cancel* a job (WP-B: "integrated with the
  existing scheduler cancellation semantics"), the dependency is one-way:
  permit → scheduler (`JobScheduler::request_cancel`,
  `src/scheduler/scheduler.rs:1972`). The scheduler must **not** depend on the
  permit service, otherwise a permit held by a job that the scheduler is
  dispatching would create a cycle.
- The permit service must be created **before** the scheduler's run loop is
  spawned, so no executor can run before the permit authority exists. The
  scheduler loop is spawned via `spawn_run` (`src/scheduler/scheduler.rs:1019`)
  after construction.
- The permit service must be reachable from the **daemon boundary** (the
  adapter that resolves authenticated actor/AgentRunId), not from
  `codegg-core` (WP-E).

---

## 9. How are durable job states recovered after a daemon restart? What is the reconciliation entry point?

### The durable recovery API

```rust
/// Reconcile a daemon-generation restart: any attempt whose stored
/// `daemon_generation` does not match `stale` and is in a
/// non-terminal state is marked `Interrupted` and the parent job
/// either requeues or transitions to `Failed` according to policy.
async fn recover_generation(
    &self,
    stale: &DaemonGeneration,
    policy: &RecoveryPolicy,
) -> Result<RecoveryReport, JobStoreError>;
```
(`crates/codegg-core/src/jobs/mod.rs:1853-1861`)

`RecoveryPolicy` (`crates/codegg-core/src/jobs/mod.rs:1622-1628`):
`requeue_read_only`, `requeue_safe_repeat`, `requeue_conditional`,
`requeue_non_idempotent`, `requeue_destructive` — all `bool`.

`RecoveryReport` (`crates/codegg-core/src/jobs/mod.rs:1644-1649`):
`interrupted_attempts`, `requeued_jobs`, `terminal_jobs`, `schedules_reconciled`.

The in-memory implementation is at `crates/codegg-core/src/jobs/store.rs:915`
and the SQLite implementation at `crates/codegg-core/src/jobs/store.rs:2263`
(the latter opens a transaction with `.pool.begin()` at `:2268-2272` — see the
`begin_immediate` note in the checklist).

### The reconciliation entry point

`JobScheduler::recover_at_startup` (`src/scheduler/scheduler.rs:2076-2090`):

```rust
pub async fn recover_at_startup(
    &self,
    policy: &codegg_core::jobs::RecoveryPolicy,
) -> Result<codegg_core::jobs::RecoveryReport, JobSchedulerError> {
    // The `recover_generation` API expects the *new* generation
    // and interrupts any non-terminal attempt whose generation
    // does not match.
    let new_gen = self.daemon_generation.clone();
    let report = self.store.recover_generation(&new_gen, policy).await?;
    self.reconcile_agent_runs_after_recovery().await?;
    // Wake the scheduler so the requeued work is considered
    // during the next reconcile pass.
    self.wake(crate::scheduler::events::WokeReason::Reconciled);
    Ok(report)
}
```

Called from the daemon bootstrap: `CoreDaemon::recover_jobs`
(`src/core/daemon_bootstrap.rs:365-...`), specifically
`src/core/daemon_bootstrap.rs:382-385`:

```rust
let recovery = if let Some(scheduler) = self.deps.scheduler.as_ref() {
    match scheduler
        .recover_at_startup(&self.deps.recovery_policy)
        .await
```

The doc comment states the ordering rule (`:355-359`): *"Must run at startup
before the scheduler admits queued work so interrupted attempts do not silently
consume capacity."*

`recover_jobs` is invoked from the daemon start paths at
`src/main.rs:4008-4013`, `src/main.rs:4281-4286`, and
`src/core/daemon_bootstrap.rs:722-725`.

### The parallel "operator trigger" path

`CoreRequest::JobRecoveryReport` (`crates/codegg-protocol/src/core.rs:2846`)
triggers a recovery pass on demand; the handler is at
`src/core/daemon_jobs.rs:500-520` and it calls
`self.deps.job_store.recover_generation(&current_gen, &policy)` directly
(`:507-513`).

### The analogous C002 restart reconciliation

C002's permit needs the same shape. The repo precedent for a **narrow
restart-reconciliation seam** is `WorktreeOwnerResolver`
(`crates/codegg-core/src/worktree_service.rs:299-309`) and the
`reconcile_repository` / `reconcile_all` pair
(`:1152-1156`, `:1233-1241`).

C002's WP-D requires reconciliation over: *persisted orphan owner/attempt,
active scheduler job state, worktree health and lease generations*. The
scheduler-job-state input is available via `JobStore` (the same store the
scheduler owns) and the lease-generation input via `WorktreeService`.

---

## 10. Is there an existing "operator" or "admin" surface (CLI subcommand, daemon method) that could host a typed inspect/release for a quarantined permit? Where do daemon CLI commands live?

### CLI

**Daemon CLI commands live in `src/main.rs`**, in the `DaemonCommand` enum
(`src/main.rs:618-665`):

```rust
#[derive(Subcommand, Clone, Debug)]
enum DaemonCommand {
    Start { endpoint: Option<String>, force_take_lock: bool },
    Stop { endpoint: Option<String> },
    Status { endpoint: Option<String> },
    Logs { file: Option<String>, lines: usize },
    Attach { endpoint: Option<String>, session: Option<String>, new: bool },
}
```

(`src/main.rs:618-665`)

The top-level `Commands` enum is at `src/main.rs:262-263`; the `Cli` struct is
at `src/main.rs:175`. The daemon-attach implementation helper is
`cmd_daemon_attach` (`src/main.rs:675-692`).

A typed `codegg daemon permit inspect|release` would be a new `DaemonCommand`
variant at `src/main.rs:618-665`.

**However**: the CLI is the *presentation* layer only. A typed permit
inspect/release needs a **daemon method** to call, and those live behind the
`CoreRequest`/`CoreResponse` protocol.

### The typed daemon request surface

`CoreRequest` is at `crates/codegg-protocol/src/core.rs:2076`. The
durable-jobs group starts at `:2782` and includes:

```rust
JobSubmit { spec: JobSubmitDto },          // :2784-2786
SchedulerSnapshot,                          // :2788
JobWait { job_id: String, timeout_ms: Option<u64> },  // :2791-2795
JobGet { job_id: String },                  // :2797-2799
JobList { query: JobQueryDto },             // :2801-2803
...
JobRecoveryReport,                         // :2846
```

The responses live in the same file; e.g. `CoreResponse::JobRecoveryReport`
(`crates/codegg-protocol/src/core.rs:1455-1458`).

Daemon-side dispatch: `src/core/daemon_jobs.rs` (the `JobRecoveryReport` arm
at `:500-520`), and the request→family routing at `src/core/daemon_family.rs`
(e.g. `CoreRequest::JobRecoveryReport => Self::Jobs` at `:175`).

### The authorization constraint on a new operator request

**Every `CoreRequest` variant must be classified in the centralized
authorization service.** `scripts/check_authorization_matrix.py:41-62`
(`check_matrix_covers_every_request`) requires an exhaustive
`operation_descriptor` match with **no wildcard arm** (`:55-57`) and a
`R::<Variant>` arm for every variant (`:58`).

The descriptor for the closest analogue is
`crates/codegg-core/src/authorization/policy.rs:774-778`:

```rust
R::JobRecoveryReport => OperationDescriptor::new(
    "job_recovery_report",
    ScopeKind::Opaque,
    Some(Capability::AuditRead),
),
```

The available capabilities are enumerated in
`crates/codegg-core/src/team.rs:248-291` — among them `job.submit`
(`:271-272`), `job.cancel` (`:273-274`), `worktree.create` (`:279-280`),
`worktree.remove` (`:281-282`), `audit.read` (`:287-288`).

`OperationDescriptor` and the matrix are in
`crates/codegg-core/src/authorization/policy.rs`; the `operation_descriptor`
function is located by the guard at
`scripts/check_authorization_matrix.py:47`.

A new `CoreRequest::WriterPermitInspect` / `WriterPermitRelease` would
therefore need: (1) a protocol variant, (2) an `operation_descriptor` arm, (3)
a capability-matrix entry, (4) a daemon handler, (5) a family routing entry,
and (6) the `architecture/authorization.md` doc must exist and mention the
required strings (`scripts/check_authorization_matrix.py:108-118`).

**No existing admin/operator surface currently exposes worktree-lease or
permit state.** `WorktreeService::reconcile_*` is a library API with no CLI or
protocol surface. `UNPROVEN` that an operator-facing permit inspect/release
exists today — it does not; the closest existing operator surfaces are
`codegg daemon status`, `codegg daemon stop`, and the typed
`CoreRequest::JobRecoveryReport` / `CoreRequest::SchedulerSnapshot`.

---

## 11. Does the repo have an existing bounded "event/projection with redaction" pattern that a permit state change event would mirror?

Yes — three, in increasing order of directness.

### (i) The scheduler's own bounded event enum

`src/scheduler/events.rs:21-64`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SchedulerEvent {
    AdmissionBlocked { job_id: String, reason: BlockReason },
    JobAdmitted { job_id: String, attempt_id: AttemptId, run_id: Option<RunId> },
    JobResourceReleased { job_id: String, attempt_id: AttemptId },
    SchedulerOverloaded { queued: usize, cap: usize },
    SchedulerQueueChanged { ready_window: usize, durable_queued: usize },
    ExecutorUnavailable { executor: ExecutorKind, reason: String },
    SchedulerQueueReconciled { durable_queued: usize, ready_window: usize },
    SchedulerWoke { reason: WokeReason },
    Progress { job_id: String, message: String },
}
```

The module header states the bounding rule (`:17-20`):

> A scheduler-related event. Constructed by the scheduler and surfaced via the
> daemon's event bus. The variants are kept narrow and bounded; a
> `SchedulerSnapshot` is the only way to get full state.

The event sink is installed with `set_event_sink` /
`set_event_sink_blocking` (`src/scheduler/scheduler.rs:509-511`, `:519-521`).

### (ii) The scheduler's redacted snapshot label — **the closest direct mirror**

`src/scheduler/snapshot.rs:13-30`:

```rust
/// Stable redacted label for an exclusivity key. The snapshot must
/// never echo raw paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExclusivityKeyLabel(pub String);

impl ExclusivityKeyLabel {
    pub fn from_key(key: &str) -> Self {
        // Strip path prefixes by hashing the canonical absolute
        // path component. We only stable-hash; the original key is
        // never reconstructed.
        let mut h: u64 = 1469598103934665603;
        for b in key.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        Self(format!("key:{:016x}", h))
    }
}
```

This is the repo's canonical **redaction idiom for a resource key**: FNV-1a
stable-hash, the raw key is never reconstructed, and the emitted label is
`key:<16 hex>`. A permit state change event that must not leak a checkout path
would mirror exactly this.

`ResourceSummary` (`src/scheduler/snapshot.rs:66-78`) carries
`held_exclusivity_keys: Vec<ExclusivityKeyLabel>`, populated by
`ResourceSummary::from_admission` (`:81-98`). The full externally-visible
surface is `SchedulerSnapshot` (`src/scheduler/snapshot.rs:141`), returned by
`JobScheduler::snapshot()` (`src/scheduler/scheduler.rs:1693`).

### (iii) The projection-target redaction rule

`src/shell/projector.rs:44-53` defines the four projection targets
(`ModelContext`, `TuiTranscript`, `TuiDetail`, `ToolExpansion`), and
`:60-65` states the redaction rule:

```rust
pub fn requires_redaction(self) -> bool {
    matches!(
        self,
        ProjectionTarget::ModelContext | ProjectionTarget::ToolExpansion
    )
}
```

with the doc (`:56-59`): *"Local TUI views may show unredacted raw bytes; model
context and tool-expansion paths must always be redacted."* The module header
also states that model context is stricter about *token budget* than the local
TUI detail view (`:39-42`).

### (iv) The bounded durable-notice pattern

`JobNotificationService` (`src/scheduler/job_notifications.rs:300-306`) is the
repo's bounded, durable, session-indexed notice store. Its bounded-scan
contract is explicit (`:1034-1038`):

> Scan the durable job store for subscribed terminal jobs without notices.
> Bounded (256 rows) and newest-first so a fixed oldest-N scan cannot starve
> new notices: pending notices are always served from the session index, while
> this scan only backfills missing terminals.

and (`:1089-1094`): *"Returns pool rows merged by notice id when pooled (bounded,
newest behavior matches `recover_from_store`'s 512-row ceiling) … The rows are
the authority; callers republish only the *hint* for notices missing from
`core_event_log` — never a second notice, never a turn."*

**Recommendation for C002 (as a pattern to mirror, not a design):** a permit
state-change event should follow (i) a narrow, tagged, bounded `…Event` enum
carrying identifiers only, (ii) the `ExclusivityKeyLabel` redaction idiom for
any path-bearing key, and (iii) a full-state accessor separate from the event
stream, exactly as `SchedulerSnapshot` is separate from `SchedulerEvent`.

---

## Checklist a bounded permit waiter must satisfy

Numbered, concrete repo rules that bind C002's waiter. Each is grounded in a
cited source.

1. **Do not add a second admission queue or reuse the scheduler's exclusivity
   keys for the permit.** C002 WP-B requires "no second resource admission/queue"
   (`plans/implementation/agent-workstream-m005-writer-containment-corrective/002-durable-writer-permit-and-quarantine.md:22`).
   The scheduler's key mechanism only fires for `exclusive:`-prefixed keys
   (`src/scheduler/admission.rs:199-209`) and the writer-bearing job kinds
   carry **no** keys at all — `Shell`/`ManagedProcess`
   (`crates/codegg-core/src/jobs/mod.rs:755-762`), `Test` (`:747-754`),
   `Lint` (`:731-738`), `Python` (`:763-770`), `ToolProgram` (`:795-802`).
   A key-based permit would be a no-op for the writers it must protect.

2. **Never park on the scheduler's shared `Notify`.** The scheduler's own
   waiter deliberately does not: *"Completion delivery uses this map, never the
   shared `notify`, so a terminal outcome for one job cannot consume another
   job's wakeup"* (`src/scheduler/scheduler.rs:260-263`, `:283-286`). Use a
   per-resource broadcast signal of the `CompletionSignals` shape
   (`src/scheduler/scheduler.rs:166-200`).

3. **Subscribe before the first state read, or the wait is racy.** The
   documented ordering is: *"Subscribe BEFORE the first completion-cache read.
   A publish that lands after the read is delivered by `changed()`; a publish
   that landed before the read is caught by the cache check itself. This
   ordering is what makes the wait race-free without a timeout-based fallback."*
   (`src/scheduler/scheduler.rs:782-785`)

4. **The waiter must have an explicit client-side deadline and a durable-state
   backstop tick.** The pattern is `deadline = Instant::now() + wait_timeout`
   (`:781`) plus a `tick` that re-reads durable state
   (`remaining.min(COMPLETION_RECHECK_INTERVAL)`, `:845`; interval 250 ms,
   `:68`), so transitions that bypass the waker (startup recovery, external
   `JobStore` writers) still wake the waiter (`:59-67`, `:842-844`). A waiter
   with only an in-memory signal will hang across those transitions.

5. **No permit may be held while waiting for provider inference, approval, or
   submission of a nested job that needs the same permit** (C002 WP-B, plan
   `:22`). The repo idiom for "consult a resource without holding it" is
   *acquire → validate → drop*, used at
   `src/scheduler/submission.rs:187-195`. The scheduler's own dispatch drops
   the permit whenever a later acquisition fails
   (`src/scheduler/scheduler.rs:1216-1217`, `:1246-1247`, `:1253-1254`,
   `:1260-1261`).

6. **Cancellation while waiting must not leak a writer token.** Release must be
   idempotent and RAII: `ResourcePermitGuard::drop` →
   `AdmissionController::release` (`src/scheduler/permit.rs:114-122`) and
   `WorkspaceServicesLease::drop` with its `released: AtomicBool` idempotence
   guard (`crates/codegg-core/src/workspace_services.rs:344-351`, `:383-389`).
   A waiter that is cancelled must drop its pending-claim state, not merely
   stop waiting.

7. **Cancellation must be integrated with the scheduler's existing semantics,
   not a parallel one.** The token type is `tokio_util::sync::CancellationToken`
   (`src/scheduler/executor.rs:161`, `src/scheduler/scheduler.rs:115`), the
   cancel API is `JobScheduler::request_cancel`
   (`src/scheduler/scheduler.rs:1972-1976`), and the durable outcome type is
   `CancelOutcome { Cancelled, Requested, AlreadyTerminal }`
   (`crates/codegg-core/src/jobs/mod.rs:1609-1618`). A permit waiter that
   cancels a job must go through `request_cancel` so the durable state, the
   in-memory queue removal, the token cancel, and the descendant cancel all
   happen (`src/scheduler/scheduler.rs:1979`, `:2042-2044`, `:2046-2051`,
   `:2057-2067`).

8. **A cancelled job must wake blocked waiters through the signal, not only
   through the backstop tick.** The queued-cancel path calls `signal_job`
   explicitly for exactly this reason (`src/scheduler/scheduler.rs:2034-2040`);
   the running-cancel path reaches waiters through the executor's terminal
   completion. A permit waiter must likewise publish on both paths.

9. **Do not call `JobScheduler::submit` from outside `src/scheduler/**`.**
   `scripts/check_scheduler_bypass.py:106-114` forbids
   `\bscheduler\s*\.\s*submit\s*\(` outside the scheduler subsystem and
   `tests/**`. Use `JobSubmissionService::submit`
   (`src/scheduler/submission.rs:171-175`), which performs payload validation
   (`:180`), the payload size cap (`:181-185`), the workspace identity check
   (`:187-195`), the resource policy normalisation (`:197`),
   the idempotency/conflict check (`:203-257`), and the create+enqueue
   (`:283-285`).

10. **Do not dispatch to the subagent pool directly.**
    `scripts/check_scheduler_bypass.py:86-95` forbids
    `\.spawner\(\)\.send(?:_async)?\(` outside the listed exemptions, and
    `SubAgentJobDispatcher` is likewise restricted (`:76-84`).

11. **A permit waiter needs its own bound and its own fairness; the scheduler's
    do not transfer.** The job queue is bounded by `QueueCaps`
    (`src/scheduler/config.rs:73-78`; defaults `:103-108`; enforced at
    `src/scheduler/fair_queue.rs:276-286`) and fair via
    `FairnessWeights` + `max_high_priority_burst` anti-starvation
    (`src/scheduler/config.rs:81-88`, `:109-116`;
    `src/scheduler/fair_queue.rs:389-393`). Neither is keyed by worktree, so
    C002 must provide its own bound and its own anti-starvation rule for
    "waiters cannot starve other groups indefinitely" (plan WP-B, `:22`).

12. **Durable read-then-write paths must open the transaction with
    `BEGIN IMMEDIATE`, not `pool.begin()`.** A deferred transaction takes no
    lock at `BEGIN`, so the first `SELECT` holds a shared read lock and the
    first `UPDATE` must upgrade it, which SQLite refuses **without waiting**
    (`AGENTS.md:108-116`; `architecture/session.md:251-259`). The guard
    `scripts/check_queue_write_transaction_shape.py` currently targets only
    `crates/codegg-core/src/session/user_prompt_queue.rs` (`:39`), so a new
    permit store would **not** be caught by it — the rule still binds. The
    in-repo helper to reuse is `ImmediateWrite` /
    `ImmediateWrite::begin` (`crates/codegg-core/src/storage/immediate.rs:60-89`;
    the `begin_immediate` free-function call site is
    `crates/codegg-core/src/session/user_prompt_queue.rs:1082-1084`).
    **Note on volatility**: `crates/codegg-core/src/storage/immediate.rs` was
    **untracked** at the time of this audit — a parallel worker was extracting
    `ImmediateWrite` out of `user_prompt_queue.rs` into a shared storage
    module. Re-verify the module path before C002 depends on it.
    Note also the existing job store's `recover_generation` uses
    `.pool.begin()` (`crates/codegg-core/src/jobs/store.rs:2268-2272`) and is
    **not** covered by that guard.

12a. **`BEGIN IMMEDIATE` is a storage primitive, not an admission mechanism —
    C002's permit store still needs its own compare-and-swap predicate.** The
    new shared helper states this in its own scope note
    (`crates/codegg-core/src/storage/immediate.rs:30-34`):

    > This is a **storage primitive**, not an admission mechanism. It serializes
    > writers *to one SQLite database file*; it says nothing about which logical
    > resource a transaction is protecting. A store that protects a shared
    > logical resource still needs its own compare-and-swap predicate — the
    > write lock only makes the read-to-write window atomic.

    It also documents two boundaries C002 must respect: **reads are
    deliberately excluded** — a read-only path, or one whose *first* statement
    is already a write, should use `sqlx::SqlitePool::begin` because taking
    the write lock earlier would serialize every read in the daemon against
    every write (`crates/codegg-core/src/storage/immediate.rs:36-42`) — and the
    **rollback contract**: an unsettled transaction queues a `ROLLBACK` on its
    connection in `Drop` and never closes it, because closing is destructive
    for a shared-cache in-memory database (`:44-49`, `:122-143`).

13. **The permit must be generation-fenced and single-owner, with typed
    refusals.** Follow `WorktreeService`: a monotonically bumped
    `lease_generation` (`crates/codegg-core/src/worktree_service.rs:415`,
    `:642-648`), a conditional UPDATE whose `rows_affected() != 1` is a
    conflict (`:644-647`), and typed errors
    `WorktreeServiceError::LeaseConflict` / `StaleGeneration`
    (`:192-195`). A stale generation or unknown id must be **rejected**, not
    repaired (plan WP-A, `:20`).

14. **Restart reconciliation must mirror the scheduler's, and must not replay
    a mutation.** The scheduler's entry point is
    `JobScheduler::recover_at_startup` (`src/scheduler/scheduler.rs:2076-2090`),
    called from `CoreDaemon::recover_jobs` (`src/core/daemon_bootstrap.rs:365`,
    `:382-385`) **before** the scheduler admits queued work
    (`:355-359`). The narrow-seam precedent is `WorktreeOwnerResolver`
    (`crates/codegg-core/src/worktree_service.rs:299-309`) and
    `reconcile_repository` / `reconcile_all` (`:1152-1156`, `:1233-1241`).
    C002 WP-D forbids replaying a non-idempotent mutation to repair metadata
    (plan `:26`).

15. **A new `CoreRequest` variant must be classified in the centralized
    authorization service, with no wildcard arm.**
    `scripts/check_authorization_matrix.py:41-62` requires an exhaustive
    `operation_descriptor`; the closest analogue is
    `R::JobRecoveryReport => OperationDescriptor::new("job_recovery_report",
    ScopeKind::Opaque, Some(Capability::AuditRead))`
    (`crates/codegg-core/src/authorization/policy.rs:774-778`). Available
    capabilities are listed in `crates/codegg-core/src/team.rs:248-291`. The
    daemon must ask for capabilities, not match on roles
    (`scripts/check_authorization_matrix.py:65-75`).

16. **Any permit state exposed in an event or projection must be redacted.**
    Mirror `ExclusivityKeyLabel::from_key`
    (`src/scheduler/snapshot.rs:19-29`): stable-hash, never reconstruct the raw
    key, emit `key:<16 hex>`. The scheduler's own rule is *"The variants are
    kept narrow and bounded; a `SchedulerSnapshot` is the only way to get full
    state"* (`src/scheduler/events.rs:17-20`), and model-context projections
    must always be redacted (`src/shell/projector.rs:60-65`).

17. **The permit service must be a daemon-scoped singleton held by
    `CoreRuntimeDeps`, following the existing shape.** See
    `CoreRuntimeDeps::shared_job_notification_service`
    (`src/core/runtime_deps.rs:169-186`), which states the singleton proof
    explicitly: *"The same `Arc` is returned on every call (pointer identity is
    the singleton proof); per-turn construction must not replace it."* The
    `Clone` impl (`src/core/runtime_deps.rs:139-165`) must propagate it.

18. **The permit service must not create a dependency cycle with the
    scheduler.** The scheduler is constructed first
    (`src/core/daemon_construct.rs:287-292`), then the submission service
    (`:365-379`), then the daemon-scoped singletons (`:385-386`). If the
    permit service needs to cancel a job, the dependency is one-way
    (permit → `JobScheduler::request_cancel`,
    `src/scheduler/scheduler.rs:1972`); the scheduler must not depend on the
    permit service. C002 WP-E additionally requires the permit to live in
    `codegg-core` with the daemon supplying authorization, and no
    `codegg-core` dependency on TUI/server/tool implementations (plan `:28`).

19. **The permit's own durable identity must be resolvable from daemon stores
    and unforgeable by models.** C002 WP-A: *"Only the daemon creates an
    unforgeable operation context"* (plan `:16`); the repo's equivalent rule
    is that the principal/workspace come from the daemon boundary
    (`ToolExecutionContext`), never from model input
    (`src/scheduler/job_notifications.rs:410-412`, `:441-444`). C002's stop
    conditions forbid exposing a bearer token to models (plan `:38`).

20. **A permit waiter must not be a tool surface.** C002 WP-A states the
    permit is *"NOT a model tool, admission scheduler, project-level global
    mutex"* (plan `:12`), and WP-E requires the permit to be *separate* from
    native tool authorization and from scheduling capacity (plan `:28`).

---

## Appendix — files read in this audit

In scope (audited directly):
`crates/codegg-core/src/jobs/mod.rs`, `crates/codegg-core/src/jobs/store.rs`,
`src/scheduler/mod.rs`, `src/scheduler/scheduler.rs`, `src/scheduler/admission.rs`,
`src/scheduler/permit.rs`, `src/scheduler/config.rs`, `src/scheduler/fair_queue.rs`,
`src/scheduler/types.rs`, `src/scheduler/events.rs`, `src/scheduler/snapshot.rs`,
`src/scheduler/executor.rs`, `src/scheduler/executors.rs`,
`src/scheduler/submission.rs`, `src/scheduler/job_notifications.rs`,
`src/scheduler/tool_program_executor.rs`, `src/scheduler/idle_continuation.rs`,
`scripts/check_scheduler_bypass.py`.

Cited as corroborating evidence (outside scope, read-only):
`src/core/runtime_deps.rs` (in scope for the scheduler/job wiring parts),
`src/core/daemon_construct.rs`, `src/core/daemon_bootstrap.rs`,
`src/core/daemon_jobs.rs`, `src/core/daemon_family.rs`, `src/main.rs`,
`crates/codegg-core/src/worktree_service.rs`,
`crates/codegg-core/src/workspace_services.rs`,
`crates/codegg-core/src/session/user_prompt_queue.rs`,
`crates/codegg-core/src/team.rs`, `crates/codegg-core/src/authorization/policy.rs`,
`crates/codegg-protocol/src/core.rs`, `src/shell/projector.rs`,
`scripts/check_authorization_matrix.py`, `scripts/check_queue_write_transaction_shape.py`,
`AGENTS.md`, `architecture/session.md`,
`plans/implementation/agent-workstream-m005-writer-containment-corrective/002-durable-writer-permit-and-quarantine.md`.
