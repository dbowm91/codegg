# Tool-Selection Advisor Causal Frontier Experiment M004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-causal-frontier-experiment/004-observe-mode-runtime-integration.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-causal-frontier-experiment-roadmap.md#m004--observe-mode-request-preparation-integration`

Repository baseline reviewed: `b5b3b14e`

Implementation commits or pull requests:

- `635213bc` — causal frontier M004: observe-mode runtime integration

## 1. Executive finding

M004 is complete and closes **positive**. The selected frontier is M002
(M003 has not closed positively, so the plan's M002 fallback applies): the
qualified disposition-A admissibility frontier now runs inside real request
preparation in observe-only mode. Evaluation happens after final
`ResolvedToolSurface` resolution and before provider definitions are
finalized, takes `&surface`, and returns diagnostics only — provider
definitions and `defer_loading` bits are byte-for-byte identical with
observe disabled, proven by a live off-vs-on run through the real
`AgentLoop::build_tool_definitions` path. Authority is unchanged
(`ResolvedToolSurface` stays the only ceiling; withheld tools never appear;
uncontracted tools stay discoverable), observation is bounded local-only
diagnostics plus a session-local window with a never-blocking broker-side
classification hook, and all §7 runtime gates hold (zero delta, zero
violations, 100% fallback preservation, p95 well under 5 ms, no network
I/O, no new background service, default-feature CI green via
`verify.sh quick`, M002 gates reproduced against the qualified catalog).
Positive M004 unblocks M005 to ready.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Objective/selected frontier (§1) | M002 disposition A (`002-status.md`); M003 closed negative with disposition D (`003-status.md`, merged concurrently — factual correction to this row), so M004 evaluates the M002 frontier | pass | Replay test pins receipt `disposition == "A"` and live catalog fingerprint equality |
| Integration seam (§2) | Hook in `AgentLoop::build_tool_definitions` after `surface.definitions()` (request_preparation.rs), before deferral partitioning; `evaluate_observe(&surface, …)` borrows | pass | Live delta test proves byte-identical definitions |
| Configuration (§3) | `ToolAdvisorCausalFrontierConfig { mode }` (`off` default), `[tool_advisor.causal_frontier]`; no active/promote mode; omission ≡ main | pass | `frontier_mode_parses_default_off` (+ unknown → off); off session records nothing in the live test |
| Runtime diagnostics (§4) | `CausalObserveOutcome`: three fingerprints, four counts, canonical-name sets, latency, fallback reason; `tracing::debug!` only; promotion names only under debug | pass | No prompts/args/outputs/contents/secrets in the struct by construction |
| Actual-call observation (§5) | Session-local bounded window (`record_observe_outcome` / `latest_observe_outcome`); `classify_observed_call` (5 classes); broker hook `observe_tool_call` after schema validation, debug-log only | pass | Inadmissible never blocks; no-op without a recorded outcome |
| Replay qualification (§6) | `tests/causal_observe_replay.rs`: 10 scenarios (no-state, goal, workplan, artifacts, errors, failed tests, preview, uncontracted, denied/disabled/ceiling, aliases) + live delta + catalog identity + p95 | pass | 13 tests; per-scenario off/on identity, fingerprint identity, canonical stability, discoverability, authority checks |
| Runtime gates (§7) | Zero delta (live JSON equality), zero violations (withheld ∩ sets = ∅ everywhere), 100% fallback (every uncontracted eligible ∈ fallback), p95 ≤ 5 ms, no I/O/service, CI green, M002 gates reproduced | pass | p95 measured 201-sample; catalog fp == M002 receipt fp |
| Verification (§8) | Commands below, all local green | pass | Hosted CI runs on push (recorded in registry when green) |
| Acceptance (§9) | Safe production-shaped integration without behavior change | pass | M005 → ready |

## 3. Production implementation evidence

- `src/tool_advisor/causal_observe.rs` (new, 469 lines): `CausalFrontierMode`
  (`Off` default, `Observe`; unknown → `Off`), `ObservedCallClass` (5
  variants), `CausalObserveOutcome` (fingerprints/counts/name sets/latency/
  fallback reason; `deferred_promotion()` diagnostics-only helper),
  `evaluate_observe` (pure, deterministic: snapshot → catalog → frontier →
  outcome; frontier errors fail closed to the full fallback universe),
  `classify_observed_call` (wire→canonical first; total function),
  bounded static window (8 outcomes × 256 sessions, eviction), `record_/
  latest_observe_outcome`, `observe_tool_call` (debug-log only). In-module
  unit tests: mode parsing, no-mutation/no-suppression, structured-state
  classification.
- `crates/codegg-config/src/schema.rs`: `ToolAdvisorCausalFrontierConfig`
  with `mode: Option<String>` (default `"off"`), wired as
  `ToolAdvisorConfig::causal_frontier` (default `None` ≡ off).
- `src/agent/request_preparation.rs`: `causal_frontier_mode()` (config
  read), `causal_observe_inputs()` (goal via `GoalStore::active_for_session`
  + `apply_goal`; work plan via `WorkPlanStore::active_for_session` +
  `apply_work_plan`; ledger via `apply_ledger`; failed-tests false and
  LSP-preview absent documented as unavailable-at-preparation signals;
  security count from `recent_findings`; `context_read` from registry), and
  the observe hook (evaluate → `tracing::debug!` → record). Cached
  preparations return before any surface exists and reuse the last recorded
  outcome for the same cache identity (documented at the call site).
- `src/tool/broker.rs`: `observe_tool_call(session_id, tool_name)` after
  input-schema validation; session-keyed, no-op without an outcome, never
  blocks. No new broker-boundary violation (`check_tool_broker_boundary.py`
  flags only pre-existing `git_read`/`verify`/`extension`/`reviewer` lines
  present at HEAD).
- `tests/causal_observe_replay.rs` (new, 13 tests): see §6 matrix above.

## 4. Verification executed (commands + results; label local vs CI truthfully)

Local (this environment), all green:

- `cargo test --test causal_observe_replay` — 13 pass (10 replay
  scenarios + live off/on delta + M002 catalog identity + p95 budget)
- `cargo test --lib -- tool_advisor::causal_observe request_preparation` —
  19 pass (3 new observe unit tests + 16 existing preparation/advisor tests)
- `cargo test --lib -- tool::broker` — 7 pass
- `cargo test --lib -- tool_advisor::causal` — 81 pass incl. M002 receipt
  recomputation (1 pre-existing ignore)
- `cargo fmt --all -- --check` — clean; `git diff --check` — clean
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — clean
- `./scripts/verify.sh quick` — passed (fmt, agent schema, core-boundary,
  sandbox, execution-ownership, tui-authority, http-route-disposition,
  audit-coverage, scheduler-bypass, cargo check workspace)
- Focused dirty-E2E suites from the C001 closure re-run green after the
  M004 merge-base (no shared code touched, but the workspace check covers it)

Not run locally: hosted canonical CI (runs on push; run ID recorded in
`plans/registry.md` when green). `lsp-real-server-tests` never in scope.
`check_tool_broker_boundary.py` fails on pre-existing HEAD lines only
(verified via `git show HEAD`); no new violation from M004.

## 5. Invariant review

- `ResolvedToolSurface` remains the only per-turn capability ceiling: hold
  — observe reads `&surface`; withheld sets flow into `FrontierInputs` and
  any leak would `Err` (fail closed to full fallback).
- Causal filtering changes visibility only: hold — M004 changes no
  visibility at all (observe records; nothing is promoted/suppressed).
- Required/never-reduce tools remain visible: hold — bypass sets; live
  delta proves identical definitions.
- Uncontracted tools never removed from discovery: hold — 100% fallback
  preservation asserted per scenario.
- Hidden/denied/disabled/parent-ceiling tools never restored: hold —
  omissions never enter any causal set (asserted across denied, disabled,
  and all-false-ceiling fixtures).
- No remote telemetry: hold — `tracing::debug!` process-local only.
- No model weights or new inference runtime: hold — pure deterministic
  evaluation over static contracts.
- No chain-of-thought parsing/storing: hold — snapshot holds
  booleans/counts/ids only; failed-test prose never parsed.
- State projection host-owned and bounded: hold — same builders as M002
  plus documented absent signals.
- Contracts versioned/deterministic/fingerprinted: hold — live catalog fp
  equals the M002 receipt fp.
- No inferred contracts: hold — `causal_catalog()` static table only.

## 6. Failure and recovery review

- Frontier evaluation error (withheld leak, missing required): fail closed
  to the full eligible fallback universe with `frontier_error:<detail>`;
  definitions unchanged; nothing suppressed.
- Insufficient state: abstain (`abstained_insufficient_state`), empty
  promotion, full fallback.
- Unknown config value: `Off` (current behavior).
- Missing outcome at broker time (observe off, cache path, other
  entry points): no-op.
- Registry lock poisoned: record skipped; read returns `None`.
- Session eviction past caps: oldest session dropped; bounded memory.

## 7. Migration and compatibility review

No migration. Config is additive (`Option`, default `None` ≡ off);
omission is behaviorally identical to main (proven live). No protocol,
storage, or provider-contract change. `ToolDefinition` wire shape
untouched.

## 8. Security review

No new authority: observe reads the resolved surface and host stores and
writes only its own bounded diagnostics. No prompts, arguments, outputs,
file contents, secrets, or credentials are recorded or logged. No network
I/O (no sockets, no telemetry endpoint), no background tasks, no filesystem
writes. The broker hook cannot alter execution outcome (debug-log then fall
through). Config surface is a two-state mode flag.

## 9. Documentation and operations

- Implementation plan status → implemented (this commit).
- Subsystem roadmap M004 section → closed; M005 → ready (this commit).
- `plans/registry.md` causal-frontier row + M004/M005 milestone rows (this
  commit).
- `architecture/tool-advisor.md` gains the M004 observe-mode section
  (implementation commit `635213bc`).
- Operators: set `[tool_advisor.causal_frontier] mode = "observe"` to
  collect bounded local diagnostics; `off` (or omission) restores exact
  main behavior. Diagnostics surface via `tracing` debug logs under scopes
  `causal_frontier_observe`.

## 10. Unresolved findings

None. Residual notes (not blockers):

- Two pilot facts (structured failed-test status, turn-local LSP preview
  availability) are unavailable at preparation time and stay absent; M005
  may wire closer signals if its qualification needs them.
- Cached preparations reuse the last recorded outcome for the same cache
  identity instead of re-evaluating; the replay suite (not the live
  window) is the M004 evidence, so this best-effort staleness is accepted.
- `check_tool_broker_boundary.py` remains red on pre-existing HEAD lines
  (`permission/reviewer`, `tool/extension`, `tool/git_read`,
  `tool/verify`); owned by whichever workstream touches those files, not
  by M004.

## 11. Roadmap disposition

M004 closes positive. Per the roadmap graph, M005 (bounded active
disclosure + trajectory qualification) moves from `blocked` (requires
positive M004) to `ready` in the same commit: only a disposition-A
successor can satisfy its historical positive-successor dependency, and
this closure is that successor. The optional M003 effect-path experiment
closed negative with disposition D (`003-status.md`, merged concurrently —
factual correction: it was still `active` when this record was written),
confirming M002 as the M005 candidate per the roadmap. No corrective pass
is required.

## 12. Registry updates

- Dependency-ready table: M004 row `active` → `closed` (implementation
  `635213bc`, this record); M005 row `blocked` → `ready` (positive M004
  is its only hard dependency; interface contracts — `CausalFrontier`,
  `ResolvedToolSurface`, config surface — are stable).
- Active roadmaps table: causal-frontier row current milestone → "M001
  closed (positive); M002 closed (A); M003 active (optional); M004 closed
  (positive); M005 ready" (as of this commit; since advanced — M003
  closed D and M005 closed B in their own records, and the roadmap now
  reads all-closed).
- Blocked-work audit: M005 was the only plan gated on M004; it is now
  ready. Order-invariance M005, retrieval-signal M004/M005, and
  late-interaction M002–M005 stay blocked/terminal on their own
  dispositions (unaffected — separate workstreams, no shared dependency).
