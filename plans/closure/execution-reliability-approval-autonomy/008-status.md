# Execution Reliability, Approval, and Autonomy M008 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/execution-reliability-approval-autonomy/008-fault-injection-and-reliability-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md#7-milestones`

Repository baseline reviewed: `f9e37930`

Implementation commits or pull requests:

- `f9e37930` — execution-reliability M008: fault-injection and reliability qualification

## 1. Executive finding

M008 is complete. The integrated M001-M007 retry, approval, sandbox,
reviewer, model-preference, and user-mode architecture was qualified under
deterministic injected failures with no production-code change required.
`tests/reliability_qualification_m008.rs` (34 M008 scenarios + 5 shared
secret-scan, 39/39 pass) proves failures remain bounded, externally visible
state stays coherent, no representative non-idempotent side effect is
duplicated, no approval path fails open, no constrained sandbox silently
degrades, and restart restores the intended model/policy state. All plan
§11 verification passes; static guards pass; `verify.sh quick` passes.
The workstream closes with M008. No corrective pass required.

## 2. Requirement-to-evidence matrix

| Requirement (plan §5/§10) | Evidence | Result | Notes |
|---|---|---|---|
| Provider failure before first token/event (class 1) | `m008_transient_faults_are_retryable_within_shared_bound` (RateLimit/Timeout/Transport transient, `RetryContext::for_operation` bound, provider child capped at 3) + existing `agent_loop_harness` M001 pre-stream tests (green, 52/52) | pass | M008 asserts taxonomy + bound; harness asserts loop behavior |
| Provider failure after text/reasoning delta (class 2) | `m008_visible_attempt_generations_cannot_silently_merge` (chain stable, distinct operations distinct chains, `stream_interrupted` class) + harness `m001_midstream_text_failure_does_not_replay` (green) | pass | No silent merge; supersession owned by turn layer |
| Provider failure after tool-call announcement (class 3) | Same chain-identity test + harness `m001_midstream_tool_start_is_explicit_and_not_executed` (green) | pass | Abandoned tool-start never executed |
| 429+Retry-After, 5xx, timeout, connect/DNS/TLS (class 4) | `m008_transient_faults_are_retryable_within_shared_bound` + `m008_retry_after_hint_is_bounded_and_clamped` (parse/clamp to `MAX_RETRY_AFTER_HINT`) + `codegg-providers` 144/144 | pass | Far-future hints clamped, never block |
| Permanent bad auth/invalid/missing model (class 5) | `m008_permanent_faults_stop_promptly_without_churn` (Auth/ModelNotFound/400/401/404/422 permanent) + `m008_secret_bearing_transport_stays_redacted_and_permanent` | pass | Auth permanent; invalid never churns |
| Circuit open/half-open + cancel during backoff (class 6) | `m008_circuit_open_is_conditional_and_half_open_admits_single_probe` (Conditional, never blind-retry; breaker Open observed) + `m008_cancel_during_backoff_stops_retry` (Cancelled, 0 attempts) | pass | Unified `Conditional→AuthRefreshable`, recovery-gated only |
| Nested retry budget exhaustion (class 7) | `m008_nested_retry_budget_cannot_multiply_or_replenish` (derive caps, DTO tamper clamp, expiry stops) | pass | Chain ≤8; provider+tool compose under operation bound |
| Ambiguous external/non-idempotent ack (class 8) | `m008_uncertain_non_idempotent_is_surfaced_exactly_once` (1 attempt, `UncertainSideEffect`, secret-safe, programmatic err) + `m008_fake_mutation_backend_reconciles_without_replay` (1 commit, reconciled) + `m008_git_push_uncertain_requires_reconciliation` + retry-budget suite 15/15 | pass | No blind replay; reconciliation over canonical state |
| Preference/permission write/read/corruption (class 9) | `m008_preference_store_failures_are_typed_not_silent` (empty→Validation, stale→Conflict, bogus→Storage fail-closed) + `m008_corrupt_permission_store_fails_conservatively` (Ask, never Allow) | pass | All failures typed, never silent allow |
| Daemon restart between selection/mode updates and next turn (class 10) | `m008_preference_write_read_restart_round_trip` (file reopen, mode/profile/model/revision preserved) + `m008_production_permission_decisions_survive_restart` + `m008_persisted_sandbox_reresolves_after_restart` (re-resolved enforcement) | pass | Restart restores intent; enforcement recomputed |
| Sandbox helper unavailable/setup/unsupported (class 11) | `m008_sandbox_helper_outcome_fixtures_fail_closed` (Enforced/Unavailable/SetupError round-trip; truncated/empty/oversized/duplicate fail closed) + `m008_workspace_write_failure_cannot_fall_through_to_full_host` | pass | Existing status-frame API; no new backend |
| Reviewer Allow/Deny/Defer/malformed/timeout/unavailable/forbidden/injection (class 12) | `m008_reviewer_allow_deny_defer_matrix` + `m008_reviewer_adversarial_never_becomes_allow` (malformed/unavailable→defer; headless→deny; prose ALLOW never parses; forbidden tools denied) + reviewer suite 22/22 | pass | Never fail-open; prompt content is untrusted data |
| Mode/sandbox change races (class 13) | `m008_mode_sandbox_change_races_keep_captured_snapshot` + approval-router contention tests (17/17) | pass | Captured snapshot wins; CAS on writes |
| Yolo+WorkspaceWrite and Yolo+FullHost (class 14) | `m008_full_host_is_always_explicit_and_auditable` (WW Caution/1, FullHost Strong/2) + policy_surface suite 14/14 + harness Yolo tests | pass | Orthogonal dimensions; FullHost explicit |
| Subagent parent-ceiling inheritance (class 15) | `m008_child_effective_authority_never_exceeds_parent` (mode+sandbox narrow/broaden matrix, `narrow_for_child`/`resolve_child_sandbox`) | pass | Child never exceeds parent |
| Stale catalog/disabled remembered connection (class 16) | `m008_stale_catalog_does_not_silently_reroute` (`StaleCatalog`) + `m008_disabled_remembered_connection_leaves_session_unselected` (`ConnectionNotSelectable`) + session_selection 21/21 + model_preference 20/20 | pass | No silent reroute/fallback |
| Restart/recovery mandatory (preference, permission, model, uncertain job, sandbox re-resolution) | Round-trip + permission-restart + sandbox-reresolve tests above; durable-job reconcile covered by retry-budget `durable_submission_reconciles_after_lost_ack`/`restart_reconciles_from_durable_store` (green) | pass | No pending non-idempotent replay on restart |
| Contention/cancel mandatory (backoff cancel, concurrent preference/model, mode/pending race, half-open, child ceiling) | Cancel test + CAS-conflict test + captured-snapshot test + circuit test + child-ceiling test + scheduler_contention 14/14 + concurrent model-preference conflict test (green) | pass | All races fail closed |
| Security/negative mandatory (injection/forbidden, Yolo deny, no-fallback, redaction, FullHost explicit) | Adversarial reviewer + `m008_deterministic_deny_survives_every_approval_mode` + no-fallback + `m008_diagnostics_are_secret_safe_and_useful` + FullHost explicitness | pass | Yolo cannot override hard deny |
| Migration/compat mandatory (§9 six items) | `m008_legacy_compat_matrix` (legacy snapshot JSON, `resolve_cli_policy` legacy/compat mapping, `danger_full_access`→FullHost, unsupported-host never-FullHost) + `m008_legacy_permission_file_and_preference_db_compat` (legacy broad file readable, fresh DB defaults) | pass | Additive only; legacy stays readable |

## 3. Production implementation evidence

No production-code change. Per plan §6 (closure-oriented, production code
only for exposed defects), qualification exposed no bounded defect
requiring a canonical-owner fix, and no finding changed
architecture/authority (no corrective plan/ADR per §14 stop conditions).

Landed ownership (test + docs only):

```text
Tests (NEW, tests/reliability_qualification_m008.rs, 34 scenarios)
  WP-A provider/retry: transient/permanent/conditional taxonomy,
    Retry-After clamp, secret-safe transport, circuit conditional,
    nested-budget caps/DTO clamp/expiry, chain identity, backoff cancel
  WP-B reconciliation: idempotent same-key recovery, uncertain exactly-once,
    fake mutation backend (commit-once + ack-loss + reconcile lookup),
    git push unreconciled vs log not-applicable, bounded detail
  WP-C approval/reviewer/sandbox: Deny×3 modes, mode/sandbox orthogonality,
    reviewer Allow/Deny/Defer, adversarial never-Allow, reviewer≠sandbox
    mutation, no WorkspaceWrite→FullHost fall-through, FullHost explicit
    (Caution/Strong, 1/2 confirmations), helper status fixtures, race
    snapshot-wins
  WP-D persistence/restart/selection: file-backed preference round-trip,
    typed store failures, permission restart + corrupt-conservative,
    stale-catalog + disabled-connection, sandbox re-resolution
  WP-E child/diagnostics/compat: mode+sandbox ceilings, secret-safe
    diagnostics, legacy snapshot/permission/DB/exec/sandbox compat,
    no-missing-event-as-success oracle

Docs (finalize per §12)
  architecture/retry.md — M008 qualification note
  architecture/permission.md — M008 item
  architecture/security.md — M008 FullHost/sandbox/diagnostics note
  architecture/session.md — M008 selection/restart note
  architecture/tool.md — M008 reconciliation note
```

Approval × sandbox behavior matrix (qualified):

```text
Allow x {Interactive,Automatic,Yolo} → Allow (mode-independent)
Deny x {Interactive,Automatic,Yolo} → Deny (M008 asserts all three)
Escalate x Interactive → human wait (router preserved)
Escalate x Yolo → Allow(yolo) within ceiling, sandbox unchanged
Escalate x Automatic + reviewer verdict → Allow/Deny/Defer (M008 matrix)
Escalate x Automatic, no reviewer → human fallback (defer preserved)
Pending x concurrent mode toggle → captured snapshot wins
Yolo+WorkspaceWrite → Caution, 1 confirmation, containment enforced
Yolo+FullHost → Strong, 2 confirmations, explicit host authority
WorkspaceWrite failure → Unavailable (never FullHost)
FullHost → no SandboxConfig by construction, is_full_host explicit
```

## 4. Verification executed

### Commands run (local, this environment)

```bash
cargo test --test reliability_qualification_m008
cargo test -p codegg-providers
cargo test -p codegg-core -- runtime_preference
cargo test --test permission
cargo test --test session_selection
cargo test --test agent_loop_harness
cargo test --test command_routing_execution_ownership
cargo test --test scheduler_contention
cargo test --test retry_budget_reconciliation
cargo test --test approval_router
cargo test --test approval_reviewer
cargo test --test sandbox_policy_wiring
cargo test --test model_preference_convergence
cargo test --test policy_surface_m007
cargo test --test sandbox_landlock
python3 scripts/check_sandbox_contract.py
python3 scripts/check_execution_ownership.py
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
./scripts/verify.sh quick
```

Plan §11 names `python3 scripts/check_core_boundary.py`. The canonical
guard in this repo is `bash scripts/check-core-boundary.sh` (same
invariant, executed; same deviation as M006 §4/M007 §4). All other plan
commands map 1:1.

### Results

- `cargo test --test reliability_qualification_m008`: 39/39 pass (34 M008 + 5 shared secret-scan).
- `cargo test -p codegg-providers`: 144/144 pass (taxonomy, circuit, fallback, auth, eggpool).
- `cargo test -p codegg-core -- runtime_preference`: 7/7 pass.
- `cargo test --test permission`: 44/44 pass.
- `cargo test --test session_selection`: 21/21 pass.
- `cargo test --test agent_loop_harness`: 52/52 pass (incl. M001 midstream/permanent/transient + M006/M007 approval).
- `cargo test --test command_routing_execution_ownership`: 21/21 pass.
- `cargo test --test scheduler_contention`: 14/14 pass.
- `cargo test --test retry_budget_reconciliation`: 15/15 pass.
- `cargo test --test approval_router`: 17/17 pass.
- `cargo test --test approval_reviewer`: 22/22 pass.
- `cargo test --test sandbox_policy_wiring`: 20/20 pass.
- `cargo test --test model_preference_convergence`: 20/20 pass.
- `cargo test --test policy_surface_m007`: 14/14 pass.
- `cargo test --test sandbox_landlock`: 1/1 pass (unsupported-host path on this darwin host; see §10).
- Guards: `check_sandbox_contract.py` pass; `check_execution_ownership.py` pass; `check-core-boundary.sh` pass.
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass.
- `./scripts/verify.sh quick`: pass (fmt, agent schema, core-boundary, sandbox, execution-ownership, tui-authority, cargo check workspace).

Attempt/side-effect count evidence (representative): idempotent write
recovers in exactly 2 attempts with the same key; uncertain
non-idempotent surfaces after exactly 1 attempt with secret-free
diagnostics; fake mutation backend commits exactly once across ack-loss +
reconcile + redispatch; backoff-cancel performs 0 attempts with
`Cancelled`; nested chain of 4 with 2 consumed leaves exactly 2 for the
child; tampered DTO claiming 250 remaining is clamped to the total.

Supported-Linux Landlock: no suitable host was available in this
environment (darwin). The suite exercises the unsupported-host typed path
(`Unavailable`, never `FullHost`) and the status-frame fixtures; the
supported-host `Enforced` path remains covered by the existing
`sandbox_landlock` suite convention. Recorded as the named operational gap
per plan §11 (no live-provider matrix added).

## 5. Invariant review

| Plan invariant (§4) | Evidence |
|---|---|
| One logical operation has a bounded retry chain | `for_operation` ≤8; provider child capped at 3; DTO tamper clamp; expiry stops |
| Visible generations cannot be silently merged | Chain stable per operation, distinct across operations; midstream `stream_interrupted`; harness supersession tests green |
| Permanent auth/invalid/policy errors do not churn | Auth/invalid/model-missing permanent; secret-bearing transport permanent + redacted |
| Ambiguous non-idempotent dispatch is reconciled/surfaced, never replayed | Exactly-once uncertain; fake-backend reconcile; git unreconciled; programmatic err |
| Deterministic Deny survives every approval mode | M008 Deny×3 + Yolo-deny harness |
| Reviewer malformed/timeout/unavailable/injected never becomes Allow | Adversarial matrix (defer interactive, deny headless); prose ALLOW never parses |
| Reviewer mode cannot mutate sandbox profile | Mismatched-sandbox review denied; snapshot unchanged |
| WorkspaceWrite failure cannot fall through to FullHost | Unavailable enforcement; FullHost has no config by construction |
| FullHost always explicit/auditable | Warning matrix (Strong/2 for Yolo+FullHost); enforcement `is_full_host`; CLI/TUI confirmations preserved |
| Preferences restore through restart with explicit/session/project ceilings winning | File-reopen round-trip; explicit-selection-wins (model suite green); ceilings narrow (child tests) |
| Production permission decisions survive restart | File-backed always-allow round-trip |
| Child effective authority never exceeds parent | Mode+sandbox ceiling matrix + `narrow_for_child`/`resolve_child_sandbox` |
| No credential/hidden reasoning leaks | Canonical constructors secret-safe; uncertain/reviewer diagnostics bounded; enforcement describe secret-free |

## 6. Failure and recovery review

- Transient 429/5xx/timeout/transport retries within the shared bound with clamped `Retry-After`; permanent auth/invalid stops promptly.
- Circuit-open never blind-retries (Conditional); half-open probe is recovery-gated; backoff cancel yields explicit `Cancelled` with zero attempts.
- Budget exhaustion returns the last typed failure; expired chains stop all layers.
- Uncertain side effects surface `UncertainSideEffect` (bounded, secret-safe) and reconcile against canonical state; no blind replay.
- Preference write failures are typed (`Validation`/`Conflict`/`Storage`); corrupt permission JSON degrades to `Ask`; bogus preference strings are rejected by the CHECK constraint.
- Restart reloads mode/profile/model identity and re-resolves enforcement/ceilings; pending non-idempotent work is never replayed.
- Mode/sandbox races keep the captured snapshot; concurrent writes conflict via CAS for reload-and-retry.
- Stale catalog/disabled connection yield typed `StaleCatalog`/`ConnectionNotSelectable`, never silent reroute.
- Sandbox helper malformed/duplicate/truncated streams fail closed with explicit errors.

## 7. Migration and compatibility review

- Legacy `PermissionConfig`/permissions file: legacy broad JSON (no `scope`) stays readable and still matches; scoped rows are additive with HMAC-signed scope.
- Pre-`RuntimePreference` DB: fresh migrate opens cleanly with `None` reads and `Interactive`/`WorkspaceWrite` defaults.
- Old TUI manifest selected-model hint: stays display-only (M007 `reconcile_tab_model_with_daemon` preserved; restore toast labels daemon identity).
- Legacy `ModelSelect` request: stale/disabled/unknown paths stay typed (session_selection + model_preference suites green); no silent fallback.
- Legacy exec permissive mode/flag mapping: `resolve_cli_policy(None,None,false)` keeps the documented permissive default; explicit flags select the new contract; conflicts rejected.
- Unsupported-host sandbox behavior: constrained profiles report `Unavailable` (never `FullHost`); `danger_full_access` compat maps explicitly to `FullHost`.
- Protocol evolution additive: legacy snapshot JSON without new fields decodes; new DTO fields are `#[serde(default)]`.

## 8. Security review

- Yolo/Automatic never imply FullHost (orthogonal store/protocol/CLI/selector; matrix asserted).
- Deterministic Deny precedes all routing including Yolo and reviewer paths; reviewer never sees Allow/Deny inputs.
- Reviewer palette stays read-only (`read/glob/grep/list/diff/git_read`); forbidden tools denied without execution; injection content treated as untrusted data.
- Scope tamper resistance preserved (HMAC-signed; M007 suites green).
- No secrets in new coverage: preference/policy DTOs carry identifiers + mode/profile + revision only; `Auth` canonical paths redacted; uncertain/reviewer diagnostics bounded and argument-free.
- FullHost explicitness preserved end to end (warnings, confirmations, enforcement truth, audit revision).
- Child authority unchanged and asserted for both dimensions.

## 9. Documentation and operations

Updated:

- `architecture/retry.md` — M008 qualification note (suite + contract held).
- `architecture/permission.md` — M008 item (Deny matrix, reviewer adversarial, races, restart, ceilings).
- `architecture/security.md` — M008 FullHost/sandbox/diagnostics note + darwin unsupported-host qualification.
- `architecture/session.md` — M008 selection/restart note.
- `architecture/tool.md` — M008 reconciliation note.

Operator notes: no new operational surface. Qualification is
deterministic and in-process; do not add a permanent chaos service,
live-provider matrix, new sandbox backend, or network-containment claims
(no backend change). On supported Linux, existing Landlock fixtures remain
the enforcement evidence; on other hosts, expect explicit `Unavailable`
(not `FullHost`) for constrained profiles.

No new CI lane: the suite is a standard integration target run locally
(plan §6 allowance for focused fixtures; verification-policy prohibition
on chaos/benchmark gates respected).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Low | Supported-Linux Landlock `Enforced` observation was not reproduced on this darwin host; the suite asserts the unsupported-host `Unavailable` path and status-frame fixtures | Hosted enforcement claim still rests on the existing `sandbox_landlock` supported-host convention, not on a new M008 hosted run | Future supported-Linux run may attach `Enforced` evidence; no correctness impact (constrained failure stays `Unavailable`, never silent `FullHost`) |
| Low | Free-form `ProviderError::Auth` strings preserve caller text verbatim (like other error variants); only canonical transport/HTTP constructors guarantee redaction | A caller passing a raw secret to `Auth` would surface it | Callers must pass redacted text (documented in the M008 test); no code change (same contract as pre-M008 error types) |
| — | No other open items | — | — |

No stop condition triggered (no provider-identity/selection, scheduler-authority, router-ownership, sandbox-backend, or principal-authorization redesign was needed).

## 11. Roadmap disposition

Milestone closed with workstream closure:

- M008 (fault-injection and reliability qualification): hard dependencies were M001+M002+M003+M004+M005+M006+M007. All closed and consumed as designed. **Close.**
- Execution-reliability workstream: M001+M002+M003+M004+M005+M006+M007+M008 all closed. **Close the subsystem roadmap** (no M009; expansion belongs in deferred product work only if prioritized).
- No corrective pass required; no deferred product work registered.
- Downstream unblock audit: no registered `blocked`/`proposed` plan lists M008 (or the execution-reliability workstream) as a hard/interface dependency. Long-horizon M002-M005 remain ordered on long-horizon M001; dependency-security M005 remains blocked on the external updater interface; architecture-convergence M009 and runtime-safety C002 remain conditionally closed on their named operational evidence. **Nothing newly unblocked; nothing newly blocked.**

## 12. Registry updates

- `plans/registry.md`: subsystem row `active; M001+M002+M003+M004+M005+M006+M007 closed, M008 ready` → `closed; M001+M002+M003+M004+M005+M006+M007+M008 closed`; dependency-ready table M008 row `ready` → `closed` with closure link and implementation `f9e37930`; execution-order item 2 rewritten (workstream closed); M008 appended to recently-closed work.
- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md`: header `Status: active` → `Status: closed`; M008 section `ready` → `closed` with closure link; status table updated.
- `plans/implementation/execution-reliability-approval-autonomy/008-fault-injection-and-reliability-qualification.md`: `Status: ready for handoff` → `Status: implemented`.
