# Tool-Selection Advisor Causal Frontier Experiment M005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-causal-frontier-experiment/005-active-disclosure-trajectory-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-causal-frontier-experiment-roadmap.md#m005--bounded-active-disclosure-and-trajectory-qualification`

Repository baseline reviewed: `7d8a63fb`

Implementation commits or pull requests:

- `019bf326` — causal frontier M005: bounded active disclosure + structural qualification

Disposition: **B — structurally sound, live evidence unavailable/incomplete**

## 1. Executive finding

M005 is structurally complete and closes with disposition **B**. The M002
frontier (M003 never closed positively) now runs in real request
preparation in an opt-in bounded active mode: at most two causally
admissible deferred tools promote from deferred to immediate per
preparation within a 16 KiB promoted-schema budget, selected by greedy
canonical-name order over the resolution-time deferred universe.
Promotion-only semantics hold everywhere: required/core/contextual tools
remain as today, contracted inadmissible tools stay discoverable via
`tool_search`, uncontracted tools are never promoted, and no tool becomes
hidden, callable, or authorized because of this layer. All §4 structural
gates pass on a fresh 284-scenario post-M004-freeze holdout graded
exact-match against an independently transcribed oracle (284/284), on the
frozen 56-case M001 qualification partition through the bound, and on a
live production-path abstention check: current-step preservation 1.00,
uncontracted discovery 1.00, 0 authority violations, 0 premature
promotions, median promotion 2 against a frozen bound of 2, p95 ~1.0 ms
against a 5 ms budget, every promotion within budget. Live model
trajectories (§5) were unavailable — no operator provider credentials
exist in this environment and no operator configuration is present — so
per plan §5 the milestone closes structurally positive but does **not**
unblock the historical live-primary-model trajectory work, which still
requires disposition A with live evidence. Active mode remains opt-in
research (default off).

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Promotion-only objective (§1) | `evaluate_active` returns a promotion decision only; production unions ≤2 names into the existing advisor promotion set; required/core/contextual paths untouched | pass | 284-scenario exact oracle match incl. required-bypass and core-immediacy pins |
| Frozen active contract (§2) | `assets/tool-advisor/causal-frontier-m005-freeze.json` written at intake before any qualification; freeze-integrity test pins live constants/catalog/palette/benchmark bytes/prereg split | pass | No threshold touched after the freeze; selection rule frozen as greedy canonical + skip + stop-at-2 |
| Promotion limits (§3) | `CAUSAL_ACTIVE_MAX_PROMOTIONS=2`, `CAUSAL_ACTIVE_SCHEMA_BUDGET_BYTES=16384`; per-case asserts in all three arms | pass | Max observed 2 tools / 1000 bytes; bound-bites pin promotes exactly 2 of 4 admissibles; budget-skip pin skips 20 KiB and continues |
| Structural qualification (§4) | Holdout arm + M001 qual arm + live abstention arm in `tests/causal_active_m005.rs` | pass | preservation 1.00, discovery 1.00, violations 0, premature 0, median 2, p95 1.001 ms |
| Downstream trajectories (§5) | Attempted and unavailable: env has no provider credential names; no operator codegg configuration; recorded in the result receipt, not skipped silently | B | 0 models run; historical live study stays blocked |
| Fresh holdout (§6) | `assets/tool-advisor/causal-frontier-m005-holdout.json`: 284 scenarios (28 pinned + 256 expanded), generator `scripts/generate_causal_m005_holdout.py`, coverage self-check over all §6 dimensions | pass | Retrieval v4 holdout untouched; every gold cites fact+contract |
| Safety cases (§7) | All ten cases pinned (denied×2, ceiling×2, stale preview, no plan, completed, cancelled, failed→verify×2, commit±evidence×2, commit+error, uncontracted, lookalikes, drift pair, disabled) | pass | Exact-match pins; drift pair proves turn-to-turn promotion change |
| Disposition (§8) | B per §8: structural gates pass, required live gates unavailable | B | Only A could unblock historical live qualification; not claimed |
| Trust boundary (§9) | Only the frozen native static catalog is consumed (`native_causal_contract`); no learned/external contracts anywhere in the path | pass | Catalog fp pinned live == frozen `0ac06de8…` |
| Verification (§10) | Commands below, all local green | pass | Hosted CI runs on push (recorded in registry when green) |
| Acceptance (§11) | Explicit disposition B + machine-readable receipt `assets/tool-advisor/causal-frontier-m005-result.json` | pass | Receipt pins holdout sha256, metrics, and the unavailable-trajectory record |

## 3. Production implementation evidence

- `src/tool_advisor/causal_active.rs` (new): `evaluate_active`
  (borrowed surface + host inputs + resolved deferred byte map →
  `CausalActiveOutcome` with promoted set/bytes/applied flag/no-change
  reason/latency), `bound_active_promotion` (pure frozen bound), in-module
  unit tests for the bound (canonical order, skip-and-continue, missing
  bytes, cumulative budget, empty) and for `evaluate_active`
  (promote/abstain/budget-exhausted paths).
- `src/tool_advisor/causal_observe.rs`: `CausalFrontierMode::Active`
  (`"active"`; unknown still fails closed to off) + doc updates.
- `crates/codegg-config/src/schema.rs`: config comment documents the
  opt-in active mode and its bounds.
- `src/agent/request_preparation.rs`: active branch after the existing
  advisor promotions — builds the resolved deferred byte map from
  `candidate_deferred` (canonicalized, max on wire collision), evaluates,
  unions the promotion into `promoted_names`, debug-logs, and records the
  inner observe outcome so broker-side classification keeps working.
  Empty promotion leaves definitions byte-identical.
- `src/tool_advisor/causal_frontier.rs` + `causal_observe.rs` doc
  refinement: the palette-based `deferred_promotion` is the offline
  proxy; active disclosure intersects the admissible set with the
  resolution-time universe. Found by the oracle disagreeing with the
  first implementation on a deferred palette-core `grep` (pinned-015);
  hand-adjudicated against plan §3 in favor of the resolution-time
  universe.
- `scripts/generate_causal_m005_holdout.py` (new): deterministic
  generator with an independent transcription of fact derivation,
  contract preconditions, structured signal, and the frozen bound, plus a
  coverage self-check over every §6 dimension and §7 safety family.
- `tests/causal_active_m005.rs` (new, 4 tests): freeze integrity
  (constants, catalog/palette/benchmark/prereg, receipt holdout-sha
  linkage), 284-scenario holdout qualification, 56-case M001 qual arm,
  live active-without-state abstention through the real
  `AgentLoop::build_tool_definitions` path.

## 4. Verification executed (commands + results; label local vs CI truthfully)

Local (this environment), all green:

- `cargo test --test causal_active_m005` — 4 pass (284/284 oracle-exact;
  holdout median promotion 2, p95 1.001 ms; qual arm median 2, max 2)
- `cargo test --lib -- tool_advisor::causal` — 73 pass incl. 8 new
  active unit tests and the M002 receipt recomputation (1 pre-existing
  ignore)
- `cargo test --test causal_observe_replay` — 13 pass (M004 invariants
  unregressed)
- `cargo fmt --all -- --check` — clean; `git diff --check` — clean
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — clean
- `./scripts/verify.sh quick` — passed

Not run locally: hosted canonical CI (runs on push; run ID recorded in
`plans/registry.md` when green). `lsp-real-server-tests` never in scope.

Load note: this shared machine sat at load ~55–80 during the session.
The M004 `replay_p95_within_budget` test flaked three times under load
(6–27 ms samples against its 5 ms budget) on a code path this milestone
provably does not touch (docs + enum variant only), then passed cleanly
as load dipped; all 13 replay tests are green in the final run. The M005
p95 gate itself passed at ~1.0 ms with headroom. Timing marginality
under external load is recorded here, not hidden.

## 5. Invariant review

- `ResolvedToolSurface` remains the only per-turn capability ceiling: hold
  — promotion only exempts resolved-deferred definitions from deferral;
  withheld tools never enter any causal set (28 pinned + expanded
  omission coverage, 0 violations).
- Causal filtering changes visibility only: hold — promotion moves
  deferred→immediate; nothing is hidden, removed, or authorized.
- Required/never-reduce tools remain visible: hold — bypass sets never
  promoted, asserted per scenario (pinned-018/019).
- Uncontracted tools never removed from discovery: hold — never
  promoted, 1.00 discovery (pinned-014/015 + T3 expansion).
- Hidden/denied/disabled/parent-ceiling tools never restored: hold —
  omissions excluded before evaluation; denied-commit-with-matching-
  contract pins prove it.
- No remote telemetry: hold — `tracing::debug!` process-local only.
- No model weights or new inference runtime: hold — pure deterministic
  evaluation + greedy bound.
- No chain-of-thought parsing/storing: hold — snapshot holds
  booleans/counts/ids only.
- State projection host-owned and bounded: hold — same builders as M004
  plus documented absent signals.
- Contracts versioned/deterministic/fingerprinted: hold — live catalog,
  palette, ontology, and benchmark bytes all pinned to the freeze.
- No inferred contracts: hold — static native catalog only (§9).

## 6. Failure and recovery review

- Insufficient structured signal: abstain, `abstained_insufficient_state`,
  no definition change (12 holdout scenarios + live path).
- No admissible contracted tool in the resolved deferred universe
  (uncontracted-only or inadmissible-only sets): no change,
  `no_admissible_deferred_in_universe`.
- Byte budget exhausted with candidates present: no change,
  `schema_budget_exhausted` (unit-covered; no natural holdout case hits
  full exhaustion since real schemas are small — the skip path is covered
  by pinned-020).
- Frontier construction error: fail closed to the full eligible fallback
  universe with `frontier_error:<detail>`, no promotion (observe-level
  guarantee reused).
- Unknown config value: `Off` (current behavior).
- Missing outcome at broker time: existing observe no-op path unchanged.

## 7. Migration and compatibility review

No migration. Config is additive (`"active"` accepted alongside
`"off"`/`"observe"`; default and unknown remain off). Omission is
behaviorally identical to main (proven live: active-without-state is
byte-identical to off). No protocol, storage, or provider-contract
change. `ToolDefinition` wire shape untouched.

## 8. Security review

No new authority: active disclosure reads the resolved surface and host
stores and promotes at most two already-callable deferred tools to
immediate visibility. No prompts, arguments, outputs, file contents,
secrets, or credentials are recorded or logged. No network I/O, no
background tasks, no filesystem writes. The selection rule is
deterministic and data-independent (canonical order + budget), so no
input can steer promotion beyond contract admissibility.

## 9. Documentation and operations

- Implementation plan status → implemented (this commit).
- Subsystem roadmap M005 section → closed (disposition B); workstream
  stays active only via the independent optional M003 (which
  subsequently closed negative — D, `003-status.md`).
- `plans/registry.md` causal-frontier rows (this commit).
- `architecture/tool-advisor.md` gains the M005 bounded-active-
  disclosure section (implementation commit `019bf326`).
- Operators: `mode = "active"` remains opt-in research. Set
  `[tool_advisor.causal_frontier] mode = "active"` only to evaluate
  bounded causal promotion; `off` (or omission) restores exact main
  behavior. Diagnostics surface via `tracing` debug logs under scopes
  `causal_frontier_active` / `causal_frontier_observe`.

## 10. Unresolved findings

None blocking. Residual notes (not blockers):

- Full budget exhaustion (`schema_budget_exhausted` with non-empty
  candidates) is unit-covered but has no natural holdout case — real
  native schemas are two orders of magnitude below the budget. If a
  future provider ships multi-KiB schemas, add a holdout case then.
- `check_tool_broker_boundary.py` remains red on pre-existing HEAD lines
  (unchanged by M005; owned by whichever workstream touches those files).
- Timing tests on this workstream are load-sensitive (see §4 load note);
  hosted CI is the quiet-room arbiter.

## 11. Roadmap disposition

M005 closes with disposition **B**. Per the roadmap graph and plan §11,
no disposition other than A unblocks historical live advisor
qualification — and none is claimed. Consequences:

- The historical live-primary-model trajectory work (registry blocked-work
  rows conditioned on "causal-frontier M005 records A") **stays blocked**.
  The original operator/provider/trajectory/resource prerequisites still
  apply on top for any future attempt.
- A future disposition-A attempt needs its own plan with live
  model/provider access; it may reuse the frozen contract, holdout, and
  harness (all pinned by hash), but it must re-freeze the downstream
  model/provider set per plan §2 — no threshold tuning after trajectories
  begin.
- The optional M003 effect-path experiment subsequently closed negative
  with disposition D (`003-status.md`; factual correction — it was still
  `active` when this record was written). No corrective pass is required:
  structural qualification is positive with zero correctness findings.

## 12. Registry updates

- Dependency-ready table: M005 row `active` → `closed` (implementation
  `019bf326`, this record, disposition B).
- Active roadmaps table: causal-frontier row current milestone →
  "M001 closed (positive); M002 closed (A); M003 active (optional); M004
  closed (positive); M005 closed (B — opt-in research, live study stays
  blocked)" (as of this commit; since advanced — M003 closed D in its
  own record and the roadmap row now reads all-closed).
- Blocked-work audit: no registered plan is unblocked by this closure.
  The two historical live-trajectory rows gated on "M005 records A" remain
  `blocked` (condition not met — B ≠ A). The optional M003 row is
  unaffected (independent). No new follow-up work is registered: there
  is no corrective (zero findings) and the deferred live-trajectory
  attempt is intentionally left unregistered until operator/provider
  prerequisites exist, per the planning rule against registering
  non-dependency-ready product work.
