# Frontend-Neutral Session Projections Milestone 002 — Closure Status

Status: **blocked — implementation never landed**

Source implementation plan:

- `plans/implementation/session-projections/002-scoped-subscriptions-durable-replay.md`

Source subsystem roadmap:

- `plans/subsystems/session-projections-roadmap.md#milestone-2--scoped-subscriptions-and-durable-replay`

Repository baseline reviewed: `1c37787afc6b2afd437f1d3f21a6fe26226a73d7`
(`main`; subsequent planning-only commits do not alter production behavior)

Current HEAD at review time: `eab77c10962ad001a99f516d54c5457d2f60a4fd`

Implementation commits or pull requests:

- None. The plan was registered and the implementation plan was authored,
  but no production-code commits were produced for this milestone. The
  implementation plan status was left at `ready for handoff` and was
  never transitioned to `active`.

## 1. Executive finding

Milestone 002 has **not been implemented**. Every work package defined in
the source plan is at zero percent completion. The plan document,
subsystem-roadmap entry, and registry entry all describe intended work,
not landed work.

The closure record is being filed because the planning process requires
one when the registry review asks for formal status; this record
documents the actual production state and explicitly removes the
milestone from the `dependency-ready` table so the registry no longer
advertises capability that does not exist.

The milestone is classified as **blocked — implementation never
landed**. This is not a `corrective pass required` disposition (there
is no original implementation to correct), nor a `conditionally
closed` disposition (no production work landed). It is the truthful
acknowledgement that the milestone was registered but never entered
implementation, and the closure record is the gate that records this.

Downstream consequences:

- Session Projections Milestone 003 (visibility, redaction, and
  artifact handles) **remains blocked**. It cannot proceed.
- Session Projections Milestone 004 (frontend adoption) **remains
  blocked**. It cannot proceed.
- The existing raw `CoreEvent` / `EventLog` / remote-TUI replay path
  remains unchanged and is not replaced. No new projection replay
  authority exists. Frontends cannot subscribe to canonical
  session/project projection streams, cannot resume from a projection
  cursor, and cannot survive daemon restart through durable projection
  replay. There is no M2 capability to consume.

## 2. Requirement-to-evidence matrix

The matrix covers the requirements enumerated across
`#6 Target architecture` and `#7 Ordered work packages` of the source
plan.

| Requirement (plan section) | Evidence | Result | Notes |
|---|---|---|---|
| Replay module under `codegg_protocol::projection` (e.g. `projection/replay.rs`) | `crates/codegg-protocol/src/projection/` contains only the M1 files: `caps.rs`, `limits.rs`, `dto.rs`, `event.rs`, `snapshot.rs`, `reducer.rs`, `adapters.rs`, `fixtures.rs`. No `replay.rs`. | not run | The M2 protocol module is absent. |
| `ProjectionStreamKind`, `ProjectionStreamId`, `ProjectionStreamDescriptor`, `ProjectionCursor`, `ProjectionSubscriptionId`, `ProjectionSubscriptionRequest`, `ProjectionSnapshotBundle`, `ProjectionReplayBatch`, `ProjectionResyncReason`, `ProjectionAck` DTOs (§6.1) | Search of the workspace for any of these identifiers: zero matches outside the plan document. | not run | None of the required DTOs exist. |
| Additive `CoreRequest` variants: `ProjectionCapabilities`, `ProjectionSubscribe`, `ProjectionResume`, `ProjectionAck`, `ProjectionUnsubscribe`, `ProjectionSnapshotGet`, `ProjectionSubscriptionStatus` (§6.2) | `crates/codegg-protocol/src/core.rs` exposes pre-existing `CoreRequest::Subscribe` / `CoreRequest::Resume` only. No `Projection`-prefixed variants. | not run | No new protocol operations exist. |
| Additive live event shape carrying `subscription_id`, stream ID, cursor/event sequence, `ProjectionEnvelope` (§6.2) | None found in `CoreEvent` / `CoreResponse` enums. | not run | No new live event shape exists. |
| Canonical stream identity: opaque `ProjectionStreamId` per canonical session / canonical project; no path-derived IDs; uniqueness constraints in storage (§6.3) | No projection storage tables exist; no stream resolution code exists. | not run | Stream identity not implemented. |
| Routing rules: source event → canonical binding → session stream + project stream; all-or-nothing fan-out (§6.3) | No routing code exists; `event_log.publish` has the single call site at `src/core/daemon.rs:824` with no projection replay hook. | not run | Routing not implemented. |
| Additive storage tables `projection_stream`, `projection_event`, `projection_checkpoint` (§6.4) | None of these tables exist in `crates/codegg-core/src/storage/`. `STORAGE_LAYOUT_VERSION = 31` (`crates/codegg-core/src/storage/mod.rs:39`); no migration bump for M2. | not run | No schema migration landed. |
| Daemon-owned `ProjectionReplayService` with per-stream sequence allocation, transactional publication, checkpoint scheduling, retention pruning, live subscription delivery (§6.5) | No such service, coordinator, or trait exists in the workspace. | not run | Replay authority not implemented. |
| Persist-before-broadcast publication ordering (§6.5) | The existing `event_log.publish` is best-effort at `src/core/event_log.rs` and broadcasts before confirming persistence; no projection replay seam replaces it. | not run | Publication ordering invariant is unestablished. |
| Inventory / centralization of direct `event_log.publish` call sites (§6.6) | One call site at `src/core/daemon.rs:824`; no centralization hook exists. | not run | Centralization not implemented. |
| `EventLog` restart high-water hydration correctness; new raw events do not collide with persisted `core_event_log` rows (§6.6) | No changes to `src/core/event_log.rs` since baseline. | not run | Restart hydration test missing. |
| Safe-publication gate (`Public` / `ClientLocal` / `Internal` / `Sensitive`) before M3 (§6.7) | `VisibilityClass` exists as a typed enum on projection DTOs (M1 deliverable), but no replay-publication classification, no `Internal` / `Sensitive` exclusion from durable rows, and no oversized-event downgrade exists. | not run | Safe-publication gate absent. |
| Snapshot bundle builder; no-gap initial-subscribe boundary; incremental checkpoints every 256 events / 1 MiB / 5 minutes (§6.8) | No snapshot bundle builder, no checkpoint machinery. | not run | Snapshot/checkpoint not implemented. |
| Retention policy: session 20k events / 7d / 64 MiB; project 50k events / 7d / 128 MiB; 64 KiB hard cap per event; bounded prune batches; sequence numbers never reused (§6.9) | No retention constants, no pruning code. | not run | Retention not implemented. |
| Subscription registry with per-client caps (32), daemon-wide cap (256), per-subscription queue cap (512), `SubscriberLagged` resync transition (§6.10) | No subscription registry exists; `TuiTaskRegistry` exists for TUI task tracking but is unrelated. | not run | Subscription registry absent. |
| Cursor validation distinguishing `HistoryExpired` / `HistoryGap` / `CursorAhead` / `StreamMismatch` / `ScopeMismatch` / `VersionMismatch` / `SnapshotUnavailable` / `SubscriberLagged` (§6.11) | `ResyncRequired` exists as a reducer `ProjectionEvent` variant (M1); the typed `ProjectionResyncReason` enum required by M2 does not exist. | not run | Cursor validation not implemented. |
| Restart hydration from durable stream rows; corruption quarantine; no eager rebuild; lazy-load on subscribe (§6.12) | No projection restart hydration; existing `EventLog` hydration was not modified. | not run | Restart behavior not implemented. |
| Work package A — Replay protocol contracts | Zero DTOs, zero variants, zero tests added. | not run | Not started. |
| Work package B — Additive storage and sequence authority | Zero schema migration, zero stores. | not run | Not started. |
| Work package C — Projection publication integration | Zero centralized seam, zero routing. | not run | Not started. |
| Work package D — Snapshot/checkpoint and retention | Zero bundle builder, zero checkpoint, zero prune. | not run | Not started. |
| Work package E — Subscription/ack/live delivery | Zero subscription registry, zero transport filtering. | not run | Not started. |
| Work package F — Resume/resync/restart | Zero cursor validation, zero failpoint tests. | not run | Not started. |
| Work package G — Observability, docs, and broad verification | Zero new metrics, zero updated architecture docs. | not run | Not started. |
| Focused tests: protocol compatibility, storage, publication, subscription isolation, replay/resync, retention, failpoint, security/negative (§9) | No test files or test functions match `projection_replay`, `projection_subscribe`, `projection_resume`, `projection_isolation`, `subscription_isolation`, `replay_storage`, `resync_required`. | not run | Test suite absent. |
| Verification commands from the plan (`cargo fmt --all -- --check`, `cargo check --workspace --all-targets --all-features`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, focused projection / session / event-log / daemon / consumer / lifecycle / storage_migrations / single_daemon tests, static guards, `git diff --check`) | Plan never entered implementation, so these commands are not applicable to M2 work. Baseline status remains `cargo fmt -- --check` clean; pre-existing tests remain green. | not run | Verification never executed for M2. |
| Architecture docs updated: `architecture/projection.md`, `architecture/protocol.md`, `architecture/storage.md`, `architecture/server.md`, `architecture/client.md`, `architecture/session.md`, operational troubleshooting (§11) | `architecture/projection.md` continues to list durable replay, subscription registry, and retention as **out of scope for M1** and **owned by M2**. No M2 sections added. | not run | Architecture docs not updated. |
| Observability metrics: stream count, high-water/retention-floor distance, retained bytes, checkpoint age, accepted/omitted visibility class counts, subscription lag, resync reasons, prune rows/bytes/duration, corrupt/quarantined count (§12) | None of these metrics exist. | not run | Observability not implemented. |
| Acceptance criteria (§13): subscribe to one session or one project; project isolation; opaque stable IDs; persist-before-deliver; atomic fan-out; reconnect replays exactly missing events; daemon restart preserves retained cursors without sequence reuse; resync reasons are explicit; monotonic acks; slow-subscriber overflow → resync; bounded count/time/byte retention; `Internal` / original `Sensitive` never persist; raw `CoreEvent` compatibility remains; `core_event_log` not misrepresented as projection replay; M3 can extend policy without replacing ownership | None of these acceptance criteria are evidenced. The capability does not exist. | not run | Acceptance criteria unmet. |

## 3. Production implementation evidence

There is no M2 production implementation evidence. The implementation
plan was authored in `94e86dc`, registered in `0c342c8`, and surfaced
in the active batch in `eab77c1`. None of those commits introduce
production code; they introduce plan documents.

`git diff --stat 1c37787..HEAD` shows:

```text
.../002-scoped-subscriptions-durable-replay.md     | 851 +++++++++++++++++++++
.../002-project-picker-tab-navigation.md           | 588 ++++++++++++++
plans/registry.md                                  |  21 +-
plans/subsystems/session-projections-roadmap.md    |  14 +-
plans/subsystems/tui-project-sessions-roadmap.md   |   8 +-
 5 files changed, 1465 insertions(+), 17 deletions(-)
```

No file under `crates/` or `src/` was touched by this milestone.

Production state relevant to the plan (still at M1):

- `codegg_protocol::projection` — M1 DTOs, capability negotiation,
  deterministic `ProjectionReducer`, adapters from `CoreResponse` and
  `CoreEvent`, golden fixtures, second-consumer equivalence test.
- `src/core/event_log.rs` — global in-memory ring + best-effort
  SQLite persistence for a selected subset of `CoreEvent` values;
  remains a compatibility / recovery source, not a projection replay
  authority.
- `CoreRequest::Subscribe` / `CoreRequest::Resume` — session/global
  raw-event subscribe and resume; no projection-scoped variants.
- `STORAGE_LAYOUT_VERSION = 31` at `crates/codegg-core/src/storage/mod.rs:39`.

## 4. Verification executed

### Commands run

```bash
# Confirm baseline
git rev-parse HEAD                 # eab77c10962ad001a99f516d54c5457d2f60a4fd
git diff --stat 1c37787..HEAD      # plan-only diff (no production code)

# Confirm M2 protocol DTOs do not exist
git grep -nE 'ProjectionStreamId|ProjectionCursor|ProjectionReplayService|ProjectionResyncReason'
git grep -nE 'projection_stream|projection_event|projection_checkpoint'

# Confirm storage tables do not exist
ls crates/codegg-core/src/storage/

# Confirm no M2 tests
git grep --name-only -E 'projection_replay|projection_subscribe|projection_resume|projection_isolation|subscription_isolation|replay_storage|resync_required' -- '*.rs'

# Confirm architecture doc has not been updated for M2
grep -n 'Milestone 2\|durable replay\|subscription registry\|retention' \
  architecture/projection.md
```

### Results

- `git diff --stat 1c37787..HEAD` — only plan documents changed;
  zero production-code changes attributable to M2.
- `git grep` for M2 identifiers — zero matches outside
  `plans/implementation/session-projections/002-scoped-subscriptions-durable-replay.md`
  (the plan itself) and the existing M1 closure record.
- `ls crates/codegg-core/src/storage/` — `mod.rs`, `paths.rs`,
  `preferences.rs` only; no projection storage module.
- `git grep` for M2 test names — zero matches across the workspace.
- `grep` on `architecture/projection.md` — durable replay,
  subscription registry, and retention are explicitly described as
  out of scope for M1 and as belonging to M2; no M2 sections added.

No M2-targeted verification commands were executed because no M2
implementation exists to verify. The M1 baseline verification from
`plans/closure/session-projections/001-status.md` is unaffected and
remains authoritative for the M1 contract.

## 5. Invariant review

The source plan enumerates invariants in `#4 Invariants that must not
regress` and `#6 Target architecture`. Because M2 implementation has
not landed, every M2-introducing invariant is **unestablished**, not
**regressed**:

- **One daemon-owned service assigns sequence numbers for each
  projection stream.** Unestablished — no such service exists.
- **Cursors are scoped to one immutable stream ID.** Unestablished —
  no cursor type exists.
- **Project streams contain only sessions canonically bound to that
  project; session streams contain only the canonical session.**
  Unestablished — no stream isolation exists; the existing
  `Subscribe` / `Resume` requests broadcast globally.
- **Path / directory / label / tab ID never defines a stream.**
  Trivially true today because no streams exist; will become
  enforceable once the typed `ProjectionStreamId` is introduced.
- **Projection events are persisted transactionally before live
  delivery.** Unestablished — `event_log.publish` broadcasts before
  persistence confirms.
- **Acknowledgement never advances beyond the committed stream
  high-water.** Unestablished — no ack type exists.
- **Duplicate delivery is reducer-idempotent.** Already true at the
  M1 reducer level; the M2 replay transport adds nothing here yet.
- **Retention is bounded by event count, age, and serialized bytes.**
  Unestablished — no retention code exists.
- **Subscriber queues and subscription counts are bounded.**
  Unestablished — no subscription registry exists.
- **Lag / queue overflow / expired history / sequence gaps / future
  cursors / scope mismatch / version mismatch produce explicit
  resync.** Unestablished — `ResyncRequired` exists as a M1 reducer
  signal variant, but the typed M2 reasons and replay paths do not.
- **Existing `CoreEvent` subscribers and raw core replay remain
  backward compatible.** Trivially preserved — nothing M2-shaped was
  added.
- **Replay storage is distinct from chat / message storage and final
  audit retention.** Unestablished — no replay storage exists yet;
  cannot regress.
- **Raw render frames, full file bodies, unrestricted logs,
  provider-private hidden reasoning, credentials, and secret-bearing
  provider configuration do not enter durable projection replay.**
  Unestablished — no durable projection replay exists.
- **Until M3 lands full policy / redaction, only explicitly accepted
  safe publication classes enter shared durable replay.** Unestablished
  — no publication classification exists on the replay path.
- **Restart hydration never reuses a committed sequence number.**
  Unestablished — no projection sequence authority exists.
- **No network or filesystem operation is performed by replay
  reducers or cursor validation.** Trivially true — neither exists.

The M1 invariants from the prior closure record remain unaffected.

## 6. Failure and recovery review

The plan calls out failure modes that exercise durability, replay,
and resync. None of these have been tested because none of the
machinery exists:

- Duplicate transport delivery at the projection-replay boundary —
  no replay boundary exists.
- Cancellation races between snapshot build and live subscription —
  no subscription registration exists.
- Daemon restart after a partial projection-event insert — no
  projection-event insert exists; the existing `event_log`
  best-effort path is unchanged.
- Queue overflow on a slow projection subscriber — no subscription
  queue exists.
- Persist-before-broadcast failure paths — no replay seam exists.
- Corruption of stream / checkpoint metadata — no stream / checkpoint
  metadata exists.
- Cursor ahead / below retention floor / scope mismatch — no cursor
  type exists.

The closure does not introduce regressions in any of these failure
modes because the pre-M2 state already lacks durable projection
replay.

## 7. Migration and compatibility review

No M2 migration landed. `STORAGE_LAYOUT_VERSION` remains `31`. No
SQLite migration file was added under `crates/codegg-core/src/storage/`.

Protocol compatibility: zero additive `CoreRequest` / `CoreResponse` /
`CoreEvent` variants landed. Existing clients and the pre-existing
`CoreRequest::Subscribe` / `CoreRequest::Resume` paths are unaffected.

Configuration: no new configuration keys were introduced. The M2
plan's retention / checkpoint / per-client-cap defaults are
undocumented because they are not implemented.

Rollback: not applicable — no M2 code shipped.

Legacy path status: `src/core/event_log.rs` remains the sole in-memory
ring + best-effort SQLite path for selected `CoreEvent` values. It is
not misrepresented as projection replay; it never has been. The
`SessionProjectionSnapshot` reducer contract from M1 continues to be
fed by `projection_events_from_core` adapters, but no replay service
durably stores or replays those envelopes.

## 8. Security review

Because no M2 code shipped, there are no new credentials, paths,
permissions, secrets, render frames, or unbounded payloads
introduced. The pre-M2 security posture is unchanged.

The M2 plan's required publication classification
(`Public` / `ClientLocal` / `Internal` / `Sensitive`) is not
enforced anywhere. The pre-existing `VisibilityClass` enum on M1 DTOs
remains the only classification surface. M3 cannot begin until M2's
classification gate exists.

## 9. Documentation and operations

No M2 documentation was produced. The following documents still
describe M2 work as outstanding:

- `architecture/projection.md` — durable replay, subscription
  registry, retention are listed as M2-deliverable and out-of-scope
  for M1; no M2 sections added.
- `architecture/protocol.md` — no M2 projection replay module,
  capability negotiation, or cursor semantics documented.
- `architecture/storage.md` — no `projection_stream` /
  `projection_event` / `projection_checkpoint` tables documented.
- `architecture/server.md`, `architecture/client.md` — no
  filtered-delivery or reconnect-flow M2 narrative.
- `architecture/session.md` — no canonical binding-to-stream routing
  documented.

No operator diagnostics, metrics, or troubleshooting entries for M2
were added.

Static guards: the existing `scripts/check-core-boundary.sh`,
`scripts/check_daemon_cwd_usage.py`, `scripts/check_git_forbidden_patterns.py`,
and `scripts/check_execution_ownership.py` remain applicable but
cannot enforce what does not exist. Their pass status is unchanged.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| critical | The capability described in the source plan does not exist. Frontends cannot subscribe to canonical session/project projection streams, cannot resume from a projection cursor, and cannot survive daemon restart through durable projection replay. | M2 capability is not delivered. Downstream M3 (visibility/redaction/artifact handles) and M4 (frontend adoption) cannot proceed. | Schedule and execute a new implementation effort against the existing `plans/implementation/session-projections/002-scoped-subscriptions-durable-replay.md` plan. Treat this closure record as the formal handoff point: the plan must transition to `active` when work begins, and a new closure record will be required when production work lands. |
| high | The registry currently advertises this plan as `ready` in the `dependency-ready implementation plans` table, implying that downstream work can rely on the projection replay capability. | Future planning that consumes the registry as ground truth may assume M2 capability that does not exist. | Update `plans/registry.md` to remove this plan from the `dependency-ready` table. The plan must be re-registered as `ready` only when an implementation effort is scheduled. |
| high | The source subsystem roadmap (`plans/subsystems/session-projections-roadmap.md#milestone-2--scoped-subscriptions-and-durable-replay`) currently lists M2 status as `ready`. | Same misleading advertisement as the registry. | Update the subsystem roadmap so M2 status reflects this closure disposition: the milestone has not been closed; the plan remains the canonical specification but no implementation has landed. |
| medium | The existing `event_log.publish` best-effort path remains the only durable publication seam for selected raw `CoreEvent` values. | The M2 plan calls for replacing this as the projection replay authority; that has not happened. The boundary between `EventLog` (raw-core recovery) and projection replay (M2-deliverable) remains unestablished. | When M2 implementation begins, the chosen persistence seam and the boundary with `EventLog` must be made explicit in the next closure record. |
| medium | No new schema migration, no new tests, and no new architecture docs were produced for M2. | Plan completeness gap is total: seven work packages, all required tests, all required docs. | Implementation effort must produce all of: schema migration (next `STORAGE_LAYOUT_VERSION`), the seven work-package deliverables, the §9 test taxonomy, the §11 documentation updates, and the §12 observability metrics before another closure is filed. |

No low-severity findings are reported — there is no M2 work to
characterize at low severity.

## 11. Roadmap disposition

Milestone is **not closed**. The formal disposition is:

- **blocked — implementation never landed**: the source plan is
  well-specified and dependency-ready, but no production-code work
  entered the implementation phase. No `corrective implementation
  plan` is required (there is no closure to correct); no `subsystem
  roadmap revision` is required (the roadmap correctly identifies M2
  as the durable-replay milestone and the source plan still
  describes the intended work).

Implications for downstream milestones:

- Session Projections Milestone 003 (visibility, redaction, and
  artifact handles) **remains blocked**. Its hard dependencies on
  Milestones 1–2 are not satisfied. The M3 entry in
  `plans/registry.md` under `Blocked work` should be retained
  verbatim and its blocker phrase updated to refer to this closure
  record instead of the open plan.
- Session Projections Milestone 004 (frontend adoption and closure)
  **remains blocked**. Its hard dependencies on Milestones 1–3 are
  not satisfied.
- No other subsystem roadmap is affected. No implementation plan in
  `plans/implementation/` outside this milestone has a hard
  dependency on M2 capability.

When a future implementation effort begins, the source plan remains
authoritative; only the registry and roadmap disposition need to
transition `ready` → `active` → `closed`.

## 12. Registry updates

Required updates after this record is accepted:

- `plans/subsystems/session-projections-roadmap.md` — Milestone 2
  status row should be updated from `ready` to reflect this closure:
  the plan exists and is authoritative, but no implementation has
  landed; pointer to this closure record should replace the pointer
  to the implementation plan in the `Closure record` column for
  traceability.
- `plans/registry.md` — `dependency-ready implementation plans` table
  row for `Frontend-neutral session projections / 002 — scoped
  subscriptions and durable replay` should be removed. The
  `Blocked work` row for `Session Projections 003` should be updated
  to reference this closure record rather than the open plan.
- The plan file
  `plans/implementation/session-projections/002-scoped-subscriptions-durable-replay.md`
  is the authoritative specification for the next implementation
  effort and **must not be modified**, archived, or rewritten — it
  remains correct for the work it describes.
- No other documents require updates. The `active closure work`
  table does not list this milestone; it does not need to be added
  there. The `recently closed work` table does **not** receive a row
  for M2 because M2 was not closed.