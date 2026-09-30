# Eggplan Assessment Integration M002 Closure — Staged Production Assessment Adoption

Source plan: `plans/implementation/eggplan-assessment-integration/003-staged-production-assessment-adoption.md`
Subsystem roadmap: `plans/subsystems/eggplan-assessment-integration-roadmap.md`
Coordinated Eggplan plan: `eggstack/eggplan: plans/implementation/codegg-integration/002-staged-eggplan-assessment-adoption.md` (status: ready, cross-repo record pending — see below)
CodeGG implementation commits: `3e992291` (facade, migration, differential, docs) + `3c7438c7` (unrelated test-hermeticity fix found during qualification; part of the qualified tree)

## Disposition

M002 is closed positively. All 13 acceptance criteria are met with the
evidence below. No stop condition fired.

## Dependency pin and graph

- Eggplan repository: `https://github.com/eggstack/eggplan.git`
- Exact immutable revision: `0d4a6af7adc6f80f975aca1bfe9bae04e2eb27d8`
  (the reviewed head from the plan; recorded in root `Cargo.toml` with
  `rev =`, resolved identically in `Cargo.lock`).
- Consumed production packages: `eggplan-core`, `eggplan-codegg-compat`
  only. `eggplan-repo`/`cli`/`projection`/`markdown`/`integrations` are
  absent from CodeGG's production graph (`cargo tree` shows only the
  two pure crates; `eggplan-repo` enters `Cargo.lock` solely as a
  dev-dependency of the compat crate's own test suite).
- `codegg-core` remains Eggplan-free (core-boundary guard passes; the
  facade lives at `src/work_plan_eggplan.rs`, application layer only).
- Static guards (in-module tests, run everywhere): exact-rev
  manifest+lock assertion, production-graph boundary assertion,
  production call-site assertion (legacy assessor reachable only via
  the facade or the explicit legacy arbiter wrappers).
- No MSRV/toolchain policy change.

## Implementation summary (WP1–WP4, WP6)

- WP1: pin + guards above; no duplicate direct dependency (compat
  re-exports core through the path dependency).
- WP2: `CodeggVerificationSpecV1` + `verification_digest_for_job`
  (variant, canonical argv or content digests, normalized cwd, scope,
  mode, effective timeout, target class; prompt/policy hashed, display
  and transient data excluded). Golden behaviors: determinism,
  argv/cwd/target/timeout sensitivity, display-text exclusion,
  policy-reorder stability, raw-prompt exclusion,
  Shell-without-argv / hash-less-Python / legacy-Subagent /
  AgentTurn / Research unavailable.
- WP3: `HostEvidenceResolver` over `assemble_resolved` + native
  job/attempt records — kind/payload match, Stable subjects only
  (incomplete materialization rejected), historical subject projected
  with `sha256:`-normalized dirty digests, matching verification
  binding, deterministic `epe_`-prefixed ids, native timestamps
  (terminal for terminal evidence, start/creation for InProgress
  display), fixed `epp_codegg_host` provider (Test/Command/
  DelegatedRun).
- WP4: `assess_work_plan_with_eggplan` (Active/Blocked only; terminal
  plans rejected as history), `assess_with_engine` selection,
  `snapshot_for_assessment` mapping (ids preserved, owners
  provenance-only, Satisfied mapped faithfully for the bridge to
  record as loss), family-to-DTO projection from source state,
  `complete_plan_with_subject_revalidation` (S2 == S1 on revision,
  dirty state, and dirty digest or `subject_changed_before_completion`).
- WP6: agent-loop terminal check + turn-end close (S2-guarded for
  EggplanGit, legacy close otherwise), goal gates/feedback (3 sites,
  session-root resolution), work-plan tool reads (4 sites). The legacy
  core assessor stays the compatibility API and the explicit legacy
  engines.

## Engine-selection matrix

| Condition | Engine | Evidence |
|---|---|---|
| Git-backed root, supported kinds, Active/Blocked | EggplanGit | facade + 28-case differential |
| No resolvable root, or no `.git` entry | LegacyNonGit | explicit, deterministic |
| `Artifact`/`Commit` ref present | LegacyUnsupportedEvidence | whole-assessment legacy |
| Git capture failure with `.git` present | adapter error (fail-closed) | capture-failure differential case |
| Terminal plan | rejected (`plan_not_active`); callers keep history semantics | terminal differential case |

## Verification-spec / digest matrix

Covered kinds: TestJob→`test`/`test`; SchedulerJob→`managed_argv`/
`shell` (argv required)/`python` (source digest)/`git`;
DelegatedRun+AgentRun→`subagent_run` (prompt/policy digests +
durable identities). Unavailable: argv-less Shell, hash-less inline
Python, legacy `Subagent`, `AgentTurn`, `Research`, `ToolProgram`,
kind/payload mismatches, missing terminal timestamps. Unit tests pin
determinism, semantic sensitivity, display exclusion, reorder
stability, and content secrecy.

## Subject S1/S2 race evidence

- `completion_revalidation_refuses_changed_subject`: assessment
  allows at S1; source mutated before CAS → `Err(
  subject_changed_before_completion)`; plan row stays Active;
  evidence untouched.
- `completion_revalidation_accepts_stable_subject`: stable S1/S2 →
  `Ok(true)`; plan row Completed.

## Resolved-evidence adapter matrix

Terminal Passed/Failed with Stable exact subjects and bindings →
observations; InProgress + Stable → InProgress display observations;
Unavailable/missing/dangling refs, non-Stable dispositions,
incomplete materialization, kind mismatches, and missing digests →
resolver errors (whole assessment fail-closed, plan preserved).
Observation ids/timestamps deterministic across repeated reads
(same native inputs → same outputs; asserted by construction and the
passing differential rerun stability).

## Differential parity table (28 cases, zero permissive deltas)

Parity (same allow/deny): no-evidence, passing-TestJob (both
Complete), failed-TestJob, in-flight TestJob (both wait),
blocked item, dependency gate, human-judgment-only,
delegated run, linked AgentRun, scheduler variants
(ManagedArgv/Shell/Python-path/Git), multi-item dependencies,
non-Git/unsupported engines, terminal plans.

Intentional stricter deltas (all Eggplan-non-permissive, each
asserted): missing/dangling refs → fail-closed error (legacy
Actionable); stale/drifted subjects → error (legacy
Complete/Actionable); forged serialized Satisfied → non-completion
(legacy Complete — the bridge records Satisfied as loss, never
evidence); verification-unavailable → error; completed-without-proof
→ Actionable in both. No case allows completion under Eggplan while
legacy forbids it (hard-stop assertion in every Git case).

## Production call-site inventory

Before: `src/agent/loop.rs` (2), `src/tool/goal.rs` (3),
`src/tool/work_plan.rs` (4) called the legacy assessor (plus
assembled snapshots) directly. After: all go through
`assess_with_engine` / `assess_*_plan_with_eggplan` with resolved
session workspace roots; completion through the S2-guarded path for
EggplanGit. Remaining direct `assess_work_plan(` calls: the two
legacy arbiter wrappers, the facade's legacy engines, core, and
tests — enforced by the source guard test.

## Ownership and static-guard outputs

- `bash scripts/check-core-boundary.sh`: passed.
- `python3 scripts/check_execution_ownership.py`: passed (no new
  spawn sites; git capture stays inside `egggit`).
- `python3 scripts/check_scheduler_bypass.py`,
  `check_eggwork_target_routing.py`: passed.
- In-module guards: pin/boundary/call-site assertions green.

## Local verification

- `cargo test -p codegg --lib -- work_plan_eggplan`: 14 passed.
- `cargo test --test work_plan_eggplan_differential`: 28 passed.
- `work_plan_projection_arbiter` (14), `work_plan_resolved_evidence`
  (9), `long_horizon_trajectory_qualification` (27),
  `scheduler_authority_matrix` (13), lib `work_plan` (19),
  `tool::goal`/`agent` (351): all passed.
- `scripts/verify.sh quick`: passed. Workspace Clippy `-D warnings`,
  `cargo fmt --check`, `git diff --check`: clean.

## Hosted qualification (operational gate satisfied)

Post-C002 stable baseline (C002 closed at `d9dd0249` with runs
`36748429660` attempts 1–3 green). Exact-head canonical run
`36760308368` on final tree `3c7438c7`: **success** — 11,830 passed,
5 skipped, 0 failed, live Eggwork included (main push), incl. the
live derived-reuse, end-to-end, lease, and restart suites. This run
postdates C002 closure and covers the exact implementation tree.

## Cross-repo reconciliation

CodeGG side records the exact Eggplan pin
(`0d4a6af7adc6f80f975aca1bfe9bae04e2eb27d8`, packages
`eggplan-core` + `eggplan-codegg-compat`) and this closure SHA.
The Eggplan-repository update (their plan/registry reconciliation
SHA for coordinated M002) is pending the Eggplan repo's own workflow
— no push access was available from this session; the pin and the
bridge-compatibility expectations (`SOURCE_CODEGG_SHA`-era API used
as reviewed) are stated here for that handoff.

## Residual findings

- During qualification, hosted run `36758088400` exposed a
  pre-existing test-isolation defect: plugin dir validators
  canonicalize the real HOME-derived plugins dir, which only exists
  as a side effect of other tests' installs (four prior runs were
  scheduling-lucky). Fixed minimally in `3c7438c7` (ensure-dir in
  the three affected tests, matching install-test precedent);
  the canonical run above proves it green. Filed as observed evidence
  for future corrective triage, not as an M002 finding (no M002 code
  involved).
- The single-occurrence `eggpool` cancellation flakes from the C002
  window did not reproduce in any subsequent run (M003, 3× C002,
  Eggplan canonical).

## M003 disposition

M002 closure lifts the Eggplan gate: CodeGG M003 (repository Plan
binding) may now be planned. No M003 implementation was started here;
`architecture/work_plan.md` documents the staged boundary that M003
must preserve.
