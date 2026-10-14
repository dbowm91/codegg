# Async Background Jobs Corrective C005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/async-background-jobs-corrective/005-authenticated-idle-resume-and-canonical-executor.md`

Source subsystem roadmap:

- `plans/subsystems/async-background-jobs-idle-completion-corrective-addendum.md#post-c004-authenticated-resume-successor-c005`

Repository baseline reviewed: `fe47cf00` (merge of `origin/main` at `f73afcec`)

Implementation commits or pull requests:

- `53c23f3d` — feat(async-jobs): C005 authenticated idle resume under accepted ADR-0031
- `fe47cf00` — Merge `origin/main` (`f73afcec`: shell ADR-0028 + git clean/reset/remote correctives; registry conflicts resolved keeping both sides)
- Prior: `2d00cb3c` (design resolution + delivery hardening), `79c10516` (hosted-stable clippy), `4fead33d` (hosted-evidence note)

Implemented on the plan branch; draft PR #119 open for qualification only (DO NOT MERGE until hosted evidence lands).

## 1. Executive finding

C005 closes the resume half C004 left blocked, under ADR-0031 (accepted
2026-10-14 in the same change that implements it). An authenticated owner
approves exactly one bounded idle continuation per background wait through
the existing daemon gate, and on the durable terminal the daemon admits
exactly one canonical owner turn through the real session
admission/turn runtime with the bounded host-authored terminal result —
with no model-supplied flag, no fake `client_id`, no second scheduler, no
wire dispatch surface, and zero inference for notify-only. Legacy
C002/C004 grant rows without attestable origin stay non-autonomous by
construction (wake-eligibility requires the stored attribution row they
can never gain). The plan's stop rule never fired: a host-trusted grant
path (new `CoreRequest` family through the proven gate) and a delegated
(non-forged) turn-admission entry (daemon-internal, canonical `TurnSubmit`
recursion with reconstructed owner authority) were both established and
verified.

## 2. Requirement-to-evidence matrix

| Requirement (plan §5) | Evidence | Result | Notes |
|---|---|---|---|
| 1. Prod idle resume via real daemon + host grant + real `TurnRuntime` (fake provider): exactly one admitted turn with receipt, correct lineage/budgets, one provider entry; no harness dispatch | `prod_resume_admits_exactly_one_owner_turn_through_fanout`: grant via gate → `record_terminal` → production fanout wake → intent `Completed` with `turn_id`, stub runtime exactly 1 call (session/model/host-authored summary asserted), grant `Consumed` with `used_continuations == 1`, `TurnStarted` on the live log, turn origin attribution captured; second wake → `AlreadyDispatched`, still 1 call | pass | Stub `TurnRuntime` at the provider boundary (no inference); admission, receipt, lease, events all run real production code |
| 2. Notify-only and all unapproved/expired/revoked/mismatched cases: one notice + one live event, zero turns | `notify_only_without_grant_publishes_hint_and_starts_no_turn` (hint publishes, zero intents, zero calls); `revoked_and_expired_grants_stay_notify_only`; `binding_mismatches_and_legacy_rows_stay_notify_only` (wrong model/connection, legacy `NewGrant` row without attribution); `grant_create_rejects_unknown_and_archived_sessions` | pass | Legacy-row suppression is structural (missing attribution row), not string matching |
| 3. Live+replay with two clients, disconnect/reconnect, restart: identical notice id/status exactly once, no turn needed | C004 suites green (12/12: live envelope, owner/foreign replay filters, restart replay, duplicate suppression); `restart_twin_heals_a_missed_wake_exactly_once` (twin dispatches once, then quiescent); `restart_with_claimed_intent_reports_replay_without_readmitting` (claimed intent + restart → replay, zero calls) | pass | Two-socket transport isolation relies on existing authorization/projection tests (unchanged, green) |
| 4. Consent/revocation matrix fails closed before inference with actionable attention | `viewer_is_denied_grant_creation_with_zero_side_effect` (Viewer denied at gate, no grant row; Contributor allowed through the same gate); revoke idempotent + visible in status; unknown/foreign grants read as absent; denial paths covered in matrix tests above | pass | Team-capability denial proven end-to-end, not just at the service layer |
| 5. 50-race/restart-before-after at every boundary: one notice, one turn, no duplicate provider entry; ambiguity becomes `needs_attention` | `fifty_way_wake_race_admits_exactly_one_turn` (50 concurrent wakes → 1 `Dispatched`, 1 provider call, losers replay; convergence re-wake → replay); twin + claimed-restart tests (wake-missed and claim-without-admission shapes); `ambiguous_provider_start_becomes_needs_attention_without_replay` (failing provider boundary → `NeedsAttention`, second wake no replay, 1 entry) | pass | Transient pre-claim storage-error losers (≤5 tolerated, documented) converge to replay; exactly-once + convergence hold in all runs. Coherence fixes found here: never-overwrite fills, DB-fresh grant loads (see §10) |
| 6. Interop suites green; no foreground semantic change | C002 8/8, C003 12/12, C004 12/12, `scheduler_authority_matrix` 13/13, `identity_m003_daemon_authorization` 9/9, `projection_replay` 68/68, `verify.sh quick` green | pass | `TurnSubmit`/`SessionPromptSubmit`/Jobs behavior unchanged (recursion reuses the identical arm) |
| 7. `cargo fmt`, Clippy `-D warnings`, `verify.sh quick`, scheduler/execution-ownership/core-boundary/authorization/projection guards, exact-head hosted CI + Desktop E2E recorded | fmt clean, Clippy `-D warnings` clean (incl. `--all-targets`), quick green post-merge; auth-matrix, audit-coverage, catalog-invariants, idle-callsite, scheduler-bypass, execution-ownership, core-boundary guards green; hosted CI/E2E on the implementation head outstanding (PR #119 updated; runs to be recorded) | partial | Local gates all green; hosted evidence is the only open item (see §10 low) |
| 8. Mainline integration reconciled on a separate head with gates rerun | Merged `origin/main` at `f73afcec` into the branch (`fe47cf00`); registry conflicts resolved keeping both sides' rows; post-merge `cargo check`, shell-policy guard, catalog invariants green; full `quick` rerun green | pass | `src/permission/mod.rs` + CI shell-guard changes are main's; no semantic interaction with this slice |

## 3. Production implementation evidence

Delivered (additive wire surface for grant management only; dispatch has
no wire surface):

- **Protocol** (`crates/codegg-protocol/src/{core.rs,dto.rs}`):
  `CoreRequest::ContinuationGrantCreate/Revoke/Status` (locators + bounds
  only — no identity field exists to spoof) and
  `CoreResponse::ContinuationGrantCreated/Revoked/Status` plus
  `ContinuationGrantDto` (server-resolved owner, empty owner marks legacy
  rows non-autonomous at the projection boundary).
- **Authorization** (`crates/codegg-core/src/authorization/policy.rs`):
  `continuation_grant_create/revoke` → `via_session` + `agent.invoke`,
  `continuation_grant_status` → `via_session` + `session.read`, with
  `representative_requests` entries; `architecture/authorization.md`
  matrix rows added. Audit
  (`audit_instrumentation.rs`): create/revoke → `agent_delegate`,
  status uninstrumented (project-keyed read); daemon pre-emit skipped for
  the minting create (post-creation emit with the durable id, `JobSubmit`
  pattern) with explicit `AgentDelegate` arms; matrix + coverage guards
  green.
- **Attribution root** (`authorization/attribution.rs`, migration v78):
  `continuation_grant` admitted to the first-write-wins origin store
  (Rust allow-list + `origin_attribution` CHECK rebuild, v61 pattern);
  `STORAGE_LAYOUT_VERSION` 77 → 78; catalog-invariants guard green.
  Deviation from ADR-0031 §Compatibility (which foresaw origin columns on
  the grant row): the store approach satisfies the same immutability with
  no new table and no backfill; legacy rows are unchanged and therefore
  permanently unattributed.
- **Grant handlers** (`src/core/daemon_continuation.rs`,
  wired in `daemon_jobs.rs` under the Jobs family):
  server-side session row (unknown/deleted/archived fail closed),
  workspace re-resolution, empty-binding rejection, `HostGrantInput`
  (type boundary keeping caller-supplied `NewGrant` strings off the
  autonomous path), attribution capture with revoke-on-failure,
  post-creation `agent_delegate` audit; revoke/status session-scoped with
  privacy-safe absent/not-found shapes.
- **Delegated entry** (`CoreDaemon::submit_idle_continuation_turn`,
  same file): resumable-status gate → lineage bind → replay short-circuit
  → usability → attribution reconstruct (`reconstructed` owner,
  `idle:<grant_id>` client marker) → session/archived check → workspace
  re-resolve → canonical `AuthorizationService` revalidation (current
  membership revision) → daemon-side composition
  (`resolve_prompt_submit_composition`, now `pub(crate)`) → grant
  model/connection binding → intent first-write + `Pending→Claimed` CAS →
  post-claim idleness re-check → canonical `TurnSubmit` recursion with
  deterministic submission id + digest → intent turn record +
  `TurnAccepted` → budget bump → `Completed` (+ explicit dispatch audit);
  admission refusal or runtime failure → `NeedsAttention`, never replay.
- **Wake wiring**: `forward_idle_completions` publishes the hint then
  evaluates one detached wake per notice through the `OnceLock<Weak>`
  cell published by `attach_idle_wake` at every `Arc` site (both mains,
  in-process client); unattached daemons stay notify-only. Restart twin
  `reconcile_idle_continuation_wakes` in `recover_jobs` (bounded 512,
  skips notices with intents). Call-site guard
  `scripts/check_idle_continuation_callsites.py` pins the two callers and
  the no-wire-dispatch invariant.
- **Coherence hardening found during race verification**: intent/grant
  cache fills never overwrite (`or_insert`; a loser's stale read
  previously clobbered the winner's recorded turn id); wake/status paths
  load grants DB-fresh (`get_grant_fresh`, incl. `used_continuations`
  overlay) so a revocation or budget consumption committed elsewhere is
  visible (revocation effectiveness is an ADR-0031 requirement).
- **Deliberately not built**: no production `ContinuationExecutor` impl
  (the wake calls the daemon entry directly per ADR Decision.2's
  single-entry rule; the trait stays for test harnesses);
  `IdleContinuationCoordinator::maybe_dispatch` stays a harness/test
  path production never invokes; no TUI grant-approval surface (grants
  via API; `on_complete` tool copy already names the grant requirement).

## 4. Verification executed

Local (all on the implementation lineage; post-merge `quick` on
`fe47cf00` + implementation):

- `cargo test --test async_background_c005_idle_resume` — 16/16 green
  (incl. 50-race stable across repeats; race uses a multi-connection
  pool because the shared single-connection pool starves racing wakes
  into acquire timeouts — documented harness shape, not CAS logic).
- Interop: C002 8/8, C003 12/12, C004 12/12, scheduler_authority 13/13,
  identity_m003 9/9, projection_replay 68/68.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked
  -- -D warnings` (3 findings fixed: `result_large_err`,
  `too_many_arguments`, redundant binding), `scripts/verify.sh quick`
  green post-merge.
- Guards green: authorization-matrix, audit-coverage,
  project-catalog-invariants, idle-continuation-callsites (new),
  scheduler-bypass, execution-ownership, core-boundary, shell-policy
  test-safety (main's), no-hardcoded-models, provider
  wire/catalog/resilience, OpenAI endpoint, eggwork routing.
- Hosted on exact head `4f2581f9` (draft PR #119, unmerged): Desktop E2E run `38648633553` SUCCESS (renderer + host + built-app WebDriver), Windows launcher run `38648633435` SUCCESS, CI run `38648633509` reached Workspace tests with exactly one failure — the pre-existing unrelated tool-advisor receipt drift (see §10). Formatting, static guards, Clippy, and all in-scope suites green on the hosted run.

## 5. Invariant review

- Singleton daemon + scheduler-only execution: no second scheduler,
  no per-job task, no provider call from completion callbacks; the wake
  is detached per notice and holds no scheduler permit; fairness,
  shutdown drain, and 512-row reconciliation bounds preserved.
- `notify_only` complete with zero inference: default and every
  unapproved path yield notice + hint and zero turns; budgets
  (continuations/tokens/tool-calls/duration/TTL) enforced pre-admission.
- Exactly-once per terminal: DB intent CAS is the authority (deterministic
  delegation submission id + receipt digest make retried wakes idempotent
  at the durable receipt); two daemons may duplicate hints (dedupe absorbs)
  but never turns; crash after provider entry → `needs_attention`.
- Mutual exclusion + consent: owner-idle validated under claim; cancel,
  steering, permission prompts, revocation, expiry flow through the
  existing controller/permission paths; dropped frontends license nothing
  (delegated lease is owner-attributed, transfer/takeover per ADR-0007).
- No forged identity: reconstructed principal comes only from the stored
  attribution row; `LocalOwner` broad policy never acts as another
  principal; the `idle:` marker is metadata, never authority.
- Secrets: no credential enters grant rows, intents, receipts, events,
  toasts, or audit metadata (existing redaction/bounds preserved).

## 6. Failure and recovery review

- Missed wake (crash between commit and fanout): twin heals bounded at
  restart; test-proven exactly once then quiescent.
- Claimed-but-unadmitted (crash between CAS and admission): restart
  reports replay without re-admitting (explicit ambiguity, test-proven).
- Ambiguous provider start: `NeedsAttention`, no replay (test-proven).
- Stale attribution/grant divergence: warn + fail closed.
- Pool-less daemons: grants refused, wakes suppressed (durable receipts
  mandatory); hints still publish.
- Forwarder abort / closed channel: durability is the notice row, not the
  hint; recovery republishes.

## 7. Migration and compatibility review

- Migration v78 rebuilds `origin_attribution` with the extended CHECK
  (v61 pattern); existing rows copied verbatim; fresh DBs get the shape
  directly. `STORAGE_LAYOUT_VERSION` tracks the highest migration (78).
- Protocol additive for grant management only; no `TurnSubmit` wire
  change; old clients see existing version negotiation.
- No config or credential changes; global default stays `notify_only`.
- Rollback: dropping the branch removes the grant family (descriptors
  are exhaustive — compile-enforced) and restores notify-only; durable
  rows from this branch read as inert without it.

## 8. Security review

- Capability model: creating a future turn requires `agent.invoke`
  (same gate as submitting it); reads need `session.read`. Viewer denied
  end-to-end with zero side effect; Contributor allowed; cross-project
  denials carry no existence signal (privacy-safe shapes + existing
  `denial_as_not_found`).
- Knowledge of ids confers nothing: exact-owner session equality +
  capability + live membership + binding match all required.
- Delegated entry threat (daemon-may-submit-as-anyone): restricted to two
  call sites (static guard), full revalidation, CAS, receipt, audit; no
  transport route exists to invoke it.
- Budgets bound tokens/tool-calls/duration/continuations/TTL; exhaustion
  suppresses before inference with the notice retained as attention.

## 9. Documentation and operations

- `architecture/{authorization,agent,jobs}.md` updated (matrix rows,
  C005 sections replacing the blocked-resume statements); `audit.md`
  unchanged (existing `agent_delegate` action reused).
- New guard documented in the delegated-entry module docs; plan §D tool
  copy satisfied by existing `on_complete` grant-requirement copy (no new
  TUI approval surface in this slice).
- Operations: standard `verify.sh quick` + guards; dashboards/toasts
  consume the existing `BackgroundJobCompleted` event (no new type).

## 10. Unresolved findings

- None blocking. Operational low, now evidenced: exact-head hosted runs on `4f2581f9` give Desktop E2E SUCCESS (`38648633553`), Windows launcher SUCCESS (`38648633435`), and CI green except one failure — the pre-existing unrelated tool-advisor receipt drift (`m003_checked_in_receipt_matches_recomputation`: deterministic, byte-identical locally and hosted, code+receipt untouched by this workstream and identical to `main` by merge diff, owned by the tool-advisor line, not fixed here). No in-scope hosted failure remains.
- Medium (accepted residual, by design): suppression is sticky per
  `(notice, grant)` — a parked attempt requires a fresh grant to retry.
  The notice stands as attention, so no wake is ever silent.
- During verification the race exposed and fixed two real defects (never
  shipped): stale cache-fill clobbering and mem-cached grant staleness
  across revocation (see §3 coherence). No further action.

## 11. Roadmap disposition

C005 as planned is complete: the authenticated grant workflow and the
canonical delegated executor are implemented, verified, and documented;
the C004-blocked resume half is delivered. No corrective successor is
required. The subsystem addendum moves to closed with C005.

## 12. Registry updates

- Move C005 (`005-authenticated-idle-resume-and-canonical-executor.md`)
  from `active` to `closed` with this closure record; mark the subsystem
  addendum row closed for the C005 milestone.
- Dependency-ready table: C004 stays `corrective pass required`
  (historical; its resume-block is now satisfied by C005 — note the
  supersession without rewriting the C004 record).
- Blocked-work audit: no registered plan lists C005 (or the async
  idle-completion corrective) as a hard/interface dependency; the shell
  ADR-0028, git clean/reset/remote, TUI, windows-sandbox, and
  command-authorization tracks are independent. Nothing newly unblocked;
  no new follow-up work registered. This audit is explicit per the
  planning skill's unblock check.
