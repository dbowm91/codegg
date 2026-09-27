# Eggplan Assessment Integration M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/eggplan-assessment-integration/003-staged-production-assessment-adoption.md`
  (status `implemented`)

Source subsystem roadmap:

- `plans/subsystems/eggplan-assessment-integration-roadmap.md#m002--eggplan-backed-workplan-assessment-adoption`

Hard predecessors (immutable):

- M001 durable execution-subject provenance:
  `plans/closure/eggplan-assessment-integration/001-status.md`
  (implementation `418fdc85`; hosted CI run `36106606574` green)
- C002 AgentRun-link/bridge-fixture corrective:
  `plans/closure/eggplan-assessment-integration/002-status.md`
  (implementation `91b2bc7b`; hosted CI run `36136964995` success)

Repository baseline reviewed: CodeGG `79bae03425470a0ec898003b5b39bd75b5b21d8c`
(implementation `85058541`, plus a one-line test-only 1.98-lint fix
`81a914df` and one added differential variant case `79bae034`; merge
`361d5788` reconciled the `origin/main` handoff registration `ce088e91`
with the local CI-corrective line).

Implementation commits:

- `85058541` — M002 staged production assessment adoption (WP1-WP7:
  dependency pin + boundary guard, verification spec, evidence adapter,
  assessment facade, differential qualification, production migration with
  S2 revalidation, docs). Includes a two-line pre-existing clippy repair
  in `src/tool_advisor/retrieval_signal.rs` (see §10).
- `81a914df` — test-only fix for the CI-only `useless_vec` lint (1.98
  toolchain; local MSRV is 1.89): index-only `Vec` → array in the
  binding-mismatch test. No behavior change.
- `79bae034` — one added differential variant case (scheduled
  job/actionable item) with its closure-table entry. No production change.

External baseline pinned and rechecked:

- Eggplan repository: `https://github.com/eggstack/eggplan`
- Pinned immutable head: `0d4a6af7adc6f80f975aca1bfe9bae04e2eb27d8`
  (verified present; docs-only delta over the pure bridge below)
- Pure compatibility bridge: `088968bd58680ae2b3741e2f1feb0614e0ff81a0`
  (verified ancestor of the pin; one comment-only delta in
  `eggplan-codegg-compat/src/lib.rs` between the two)
- Consumed packages: `eggplan-core` + `eggplan-codegg-compat` only, same
  exact revision (boundary guard enforces; see §3). Transitive delta is
  zero new crates (`serde`, `serde_json`, `sha2`, `thiserror`, `uuid`
  already in the graph). `eggplan-repo/-cli/-projection/-markdown/
  -integrations` appear in neither manifests nor lockfile (guard-enforced).
- Coordinated Eggplan plan:
  `plans/implementation/codegg-integration/002-staged-eggplan-assessment-adoption.md`
  (status `ready`; CodeGG-side reconciliation recorded in §12).

## 1. Executive finding

M002 is closed. CodeGG's existing WorkPlan completion families for the
supported Git-backed evidence subset are now produced through Eggplan's
generic assessment (`assess_codegg_snapshot` over `normalize_snapshot`)
behind a CodeGG application-layer facade, with no permissive regression:
36 differential cases prove Eggplan allows completion only where the legacy
assessor also allows it, and the git-backed completion race proves S2
revalidation blocks a drifted close. CodeGG retains WorkPlan/Job/AgentRun
persistence, scheduler authority, Goal/Todo/checkpoint state, worktree
leases, and agent-loop policy; no Eggplan repository state is created and
no repository Plan binding is attempted (M003 scope, still gated).

## 2. Requirement-to-evidence matrix

| Plan requirement | Evidence | Result |
|---|---|---|
| §3 one immutable Eggplan revision, pure crates only | `Cargo.toml` pins both crates to `0d4a6af7` (git+rev, no branch); `scripts/check_eggplan_assessment_boundary.py` → ok; `cargo tree` shows zero new transitive crates and no forbidden `eggplan-*` in manifests or lockfile | pass |
| §3 no `eggplan-repo` production dependency; M002 builds Eggplan values from durable records | `to_eggplan_fields` bridge only (C002); adapter constructs `SubjectRevision`/observations from `ExecutionSubjectRevision`/job rows | pass |
| §2/§11 assessment lives above `codegg-core`; core stays Eggplan-free | `src/work_plan_eggplan.rs` (root package); `check-core-boundary.sh` → pass; boundary guard scans `crates/` for `eggplan_*` use → ok | pass |
| §4 current subject is governed capture, never HEAD-only; S2 revalidation before completion CAS | `src/scheduler/assessment_subject.rs` (scheduler-owned entry; M001 ownership guard still green); `resolve_assessment_workspace` (catalog → session directory; never CWD); `revalidate_subject_for_completion` + `maybe_complete_plan_on_turn_end` S2 gate with `subject_changed_before_completion` | pass |
| §5 supported subset TestJob/SchedulerJob/DelegatedRun/AgentRun; Artifact/Commit stay legacy; no mixed decisions | facade routes Artifact/Commit plans to `LegacyUnsupportedEvidence` whole-assessment; `differential_*` + `engine_unsupported_evidence_kind` | pass |
| §6 explicit engine selection, deterministic and observable | `AssessmentEngine` 5 variants + `engine_detail`; `engine_*` tests (terminal/no-context/unsupported/non-git/git) | pass |
| §7 canonical verification spec from authoritative native execution data | `verification_digest_for_{test,scheduler_job,delegated_run,evidence}` + 16 lib tests (golden pin `sha256:8e6db4aa…`, sensitivity, ID/label/timeout/node independence, fail-closed matrix) | pass |
| §8 adapter: exact subject + verification binding, fixed provider, deterministic IDs/timestamps | `bind_resolved_evidence`/`SnapshotEvidenceResolver`/`observation_base_id`; `codegg_host_providers` (`epp_codegg_host` ⊂ {Test, Command, DelegatedRun}); differential matrix | pass |
| §9 snapshot mapping preserves M001 compatibility rules; terminal history untouched | `build_plan_snapshot` (+N1/N2 normalizations, manifest keeps source dispositions); facade terminal short-circuit; `engine_terminal_history_passthrough` | pass |
| §10 facade returns the existing DTO + bounded diagnostics; no Eggplan handles escape | `EggplanBackedAssessment`; `project_bridge_assessment` populates all detail from CodeGG state; `decide_from_assessment` unchanged | pass |
| §11 production call-site migration + static guard | arbiter (both assess paths), `tool/work_plan.rs` (3 read/mutation assessments), `agent/loop.rs`, `tool/goal.rs` go through the facade; boundary guard rule 5 → ok; item-level gate and todo sync unchanged (status-only) | pass |
| §12 differential qualification, no permissive delta | `tests/work_plan_eggplan_differential.rs` 35/35 + bridge mismatch-rejection test; every stricter delta recorded below | pass |
| §13 completion race both directions | `facade_git_backed_complete_and_stable_close` + `facade_source_drift_blocks_completion_cas` (real git repos: stable closes, drift blocks CAS, evidence untouched, re-assessment stale) | pass |
| §14 provider trust is host code, not evidence text | `codegg_host_providers` only; LLM `ProviderRegistry` never consulted (different domain); `provider_identity_is_fixed` | pass |
| §15 no new storage migration | v67 untouched; no schema change (`git diff` shows no `schema.rs`/migration delta) | pass |
| §16 fail-closed semantics | `facade_capture_failure_fails_closed` (empty-repo capture → `CurrentCaptureFailed`, never legacy); drifted/stale/unbound → Eggplan-unavailable, never satisfied | pass |
| §17 hosted qualification after CI stability | CI corrective C001+C002+C003 closed (separate workstream); exact-head canonical run recorded in §4 | pass |

Acceptance criteria disposition (§23): 1-11 hold by the matrix above;
12 holds via the §4 hosted run; 13 holds CodeGG-side in this record with
the Eggplan-side reconciliation committed as recorded in §12 (their M002
plan stays `ready` until their own closure pass consumes these SHAs).

Two bounded deviations from plan letter (both safer-or-equal, recorded):

- D1 — engine enum has five variants, not three: `TerminalHistory`
  (terminal records are never live-re-assessed, §9) and
  `LegacyNoWorkspaceContext` (no workspace identity ⇒ no trustworthy
  current subject; legacy result, zero delta). Both are deterministic from
  structured state, observed in `engine_detail`, and covered by tests.
- D2 — snapshot/bounds rejections (empty/oversized item set, overlong
  source IDs, unmappable current subject) route to
  `LegacyUnsupportedEvidence` with a `*_rejected` diagnostic instead of a
  hard error. The legacy result is byte-identical to pre-M002 behavior
  (zero permissiveness delta); the direction preserves availability for
  plans the pinned bridge contract cannot represent.
- D3 — `Shell` without explicit argv fails closed per plan letter even
  though the executor would run `sh -c <command>`: display text is not
  canonical verification identity. Strict-only effect, pinned by test.
- D4 — `Cancelled` items keep the plan open (Eggplan has no bound
  completion evidence for scope removal) where legacy completed. Strict-only,
  recorded in the differential table.

## 3. Production implementation evidence

- `src/work_plan_eggplan.rs` (~2000 lines with docs/tests): engine enum,
  `AssessmentAdapterError`, `EggplanBackedAssessment`,
  `CodeggVerificationSpecV1` digests, workspace resolution, current
  capture wrapper, snapshot builder with N1/N2, resolver with suffixed
  deterministic observation IDs, host provider registry, subject
  conversion with dirty-digest namespace translation, adapter runner,
  family→DTO projection, facade, S2 revalidation helper.
- `src/scheduler/assessment_subject.rs`: scheduler-owned current-capture
  entry (`codegg-workspace:{id}` identity identical to M001 scheduler
  construction, so current and historical subjects compare exactly).
- `src/work_plan_arbiter.rs`: both assess paths return the backed
  assessment through the facade; `maybe_complete_plan_on_turn_end` takes
  the backed assessment and enforces S2 == S1 for `EggplanGit` before the
  `Completed` CAS (legacy engines keep the direct CAS path).
- `src/tool/work_plan.rs`, `src/agent/loop.rs`, `src/tool/goal.rs`:
  read/mutation assessments through the facade; response shapes and
  `decide_from_assessment` unchanged.
- `scripts/check_eggplan_assessment_boundary.py`: six-rule static guard
  (pin, forbidden surface, layer confinement, legacy retention,
  call-site routing, scheduler-owned capture).

Engine-selection matrix (all covered by `work_plan_eggplan_engines`):

| Engine | Trigger | DTO source |
|---|---|---|
| `EggplanGit` | Git root, S1 captured, all refs supported | Eggplan family + CodeGG detail |
| `LegacyNonGit` | capture positively `NotGit` | legacy verbatim |
| `LegacyUnsupportedEvidence` | Artifact/Commit cited, or bridge shape rejection | legacy verbatim |
| `LegacyNoWorkspaceContext` | no session/workspace identity | legacy verbatim |
| `TerminalHistory` | Completed/Cancelled plan | legacy verbatim |

Verification-spec/digest matrix (lib tests, all green):

- deterministic Test digest, golden-pinned
  `sha256:8e6db4aa76179d6b629f74a0ef2908d80c757604dc048d191564ee0811616de7`;
- argv/cwd/scope/timeout/target changes alter the digest; display
  `command`, job/session/attempt IDs, labels, and timestamps do not;
- `EggworkNode` target digests as `eggwork` independent of node ID;
- ManagedArgv/Shell/Python/Git specs complete for supported shapes;
- Shell-without-argv, Python-without-source-identity, legacy Subagent,
  and non-executable payloads fail closed; ref/job ID alone proves nothing;
- delegated policy order canonicalized; prompt participates only as a
  digest; Python `source_hash` ≡ content digest equivalence.

Subject S1/S2 race evidence (`work_plan_eggplan_engines`, real git repos):

- stable: S1 captured at assessment equals S2 at close → CAS proceeds,
  plan `Completed`, no active plan remains;
- drift: file added after assessment → close returns `Ok(false)` with
  `subject_changed_before_completion`, plan stays `Active`, attempt
  evidence rows untouched (`Passed` retained), re-assessment is
  `EggplanGit` + `ActionableWorkRemaining` (`allows_completion == false`).

Resolved-evidence adapter matrix (differential suite): stable exact
Test/Scheduler(Python/Shell/ManagedArgv/Git)/DelegatedRun/linked-AgentRun
complete; failed/in-flight/blocked/dependency/human-only/missing behave
identically across engines; forged `Satisfied`, stale/drifted/incomplete/
unsealed subjects, unbindable specs, unlinked AgentRun rows, cancelled
items, and distinct-identity failed co-evidence are recorded stricter
deltas (legacy completes, Eggplan does not — subject/verification
authority); actionable+blocked mixes and owner-only in-flight are recorded
variant-ordering deltas (both refuse completion).

Production call-site inventory before/after: `assess_work_plan` direct
production callers were `work_plan_arbiter.rs` (2) + `tool/work_plan.rs`
(4); after, all six go through `assess_work_plan_with_eggplan` (boundary
guard rule 5 enforces; `work_plan_eggplan.rs` itself and tests retain
oracle/fallback use). `decide_from_assessment`, response shapes, and the
item-level host-evidence gate are unchanged.

Ownership/static-guard outputs: `check_eggplan_assessment_boundary.py`
ok; `check_execution_subject_ownership.py` ok (M001 capture confinement
intact — the facade captures only via `src/scheduler/`); `check-core-
boundary.sh` ok; `check_execution_ownership.py` ok (via `verify.sh
quick`); `cargo fmt --check` clean; `git diff --check` clean.

## 4. Verification executed

Local (all green; see §3 for counts):

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p codegg --lib work_plan_eggplan                        # 16 passed
cargo test --test work_plan_eggplan_differential                    # 36 passed
cargo test --test work_plan_eggplan_engines                         # 12 passed
cargo test --test work_plan_resolved_evidence                       # 14 passed
cargo test --test work_plan_projection_arbiter                      # 9 passed
cargo test --test long_horizon_trajectory_qualification             # 27 passed
cargo test --test scheduler_authority_matrix                        # 13 passed
cargo test --test eggwork_remote_execution                          # 25 passed
cargo test -p codegg-core --lib -- work_plan                        # 58 passed
cargo test -p codegg-core --test work_plan_foundation               # 10 passed
cargo test -p codegg-core migration                                 # 7 lib + store suites green
cargo test -p codegg --lib tool_advisor                             # 60 passed (clippy-repair cover)
python3 scripts/check_eggplan_assessment_boundary.py
python3 scripts/check_execution_subject_ownership.py
bash scripts/check-core-boundary.sh
./scripts/verify.sh quick
git diff --check
```

Hosted (canonical, exact-head, after the §17 gate was satisfied by the
separately closed CI corrective C001+C002+C003):

- PR `dbowm91/codegg#80` (branch `ci-throughput-c001-measurements`)
- canonical `CI / verify` run `36336450431`: **success** on exact
  implementation head `79bae03425470a0ec898003b5b39bd75b5b21d8c`.
- Superseded non-passing evidence (retained for traceability, not cited
  as qualification): run `36335738826` on `85058541` failed workspace
  Clippy on one `useless_vec` instance that only the CI toolchain
  (1.98) lints — the repo MSRV toolchain (1.89) is clean (fixed
  test-only in `81a914df`); run `36336303269` on `81a914df` was
  superseded by the `79bae034` push under the workflow concurrency
  policy (cancel-in-progress). The run above is the governing
  qualification.
- The closure commit adds only `plans/` records, which CI
  `paths-ignore` excludes from code signal.

## 5. Invariant review

- Attempt-scoped provenance authority unchanged (`JobAttempt.source_
  subject`; no `JobRecord`/label provenance; guard rule 4 of the M001
  guard still passes).
- Historical resolution never captures the worktree (resolver loads
  durable attempt/agent_run rows only; M001 guard rule 2 passes on the
  untouched `work_plan_evidence.rs`).
- Stable exact-subject evidence still requires S1 == S2 at the governed
  seal (local `LiveExecutionEnd`, remote `SnapshotMaterialized` with
  complete manifest); incomplete materialization stays unavailable.
- Current assessment subject is governed capture at the catalog-resolved
  root, never HEAD-only when dirty (dirty state carried as the canonical
  digest through the namespace translation).
- Completion authority unchanged: the arbiter DTO families, CAS
  discipline, Goal-verifier final authority, and budget-expiry
  preservation are untouched; S2 revalidation only ever refuses the CAS,
  never forces it.
- No assessor ownership transfer: Eggplan never sees SQLite/JobStore/Git/
  scheduler authority; `codegg-core` never sees Eggplan.

## 6. Failure and recovery review

- Current capture failure on a Git-backed root is a fail-closed error
  (no completion, no legacy route); on a positively non-Git root it is
  the explicit legacy engine.
- Bridge/mapping rejection is either a compatibility fallback (shape
  outside the pinned contract, zero delta, warn-logged) or a fail-closed
  error (store failures, invariant violations).
- Crash between assessment and close cannot complete: the CAS requires a
  fresh S2 equal to the assessed S1; a restarted world re-resolves and
  re-captures rather than trusting the backed struct.
- Retry attempts remain independent (M001 attempt scoping untouched);
  S1/S2 is bounded revalidation, not a filesystem transaction (documented
  in code and in `architecture/work_plan.md`).
- Concurrent seal attempts remain governed by M001 terminal immutability;
  the facade adds no writes.

## 7. Migration and compatibility review

No storage migration (layout stays v67; no `schema.rs` delta). No
`JobStore`/`WorkPlanStore` trait changes. `RunManifest`/agent_run rows
untouched. `assess_active_plan`/`assess_goal_plan` return the backed
assessment (callers in `agent/loop.rs`, `tool/goal.rs`, and the two
integration suites updated mechanically); `maybe_complete_plan_on_turn_
end` takes the backed assessment (same mechanical updates).
`decide_from_assessment`, tool response shapes, and event publication are
unchanged. Legacy `assemble`/`assess_work_plan` (including all core unit
tests) are untouched and remain the compatibility API + oracle.

## 8. Security review

Persisted and transmitted shapes contain only bounded schema/disposition
metadata, OIDs/digests, namespaced stable IDs, and manifest digests.
Prompts enter digests only as SHA-256; file contents, paths beyond
bounded locators, remotes, credentials, environment, branch descriptions,
and model text never enter subjects, digests, observations, or metadata.
Provider trust is fixed host configuration (`epp_codegg_host` ⊂
{Test, Command, DelegatedRun}); native payload data cannot widen it, and
the LLM provider registry is never consulted. Observation/metadata bounds
(MAX_DETAIL_CHARS, bridge text bounds, verification-spec 64 KiB envelope)
are enforced by construction with fail-closed errors.

## 9. Documentation and operations

- `architecture/work_plan.md`: new M002 section (ownership, engines,
  verification spec, adapter, revalidation, call sites, parity tables)
  plus the new test/guard commands.
- Subsystem roadmap M002 → closed; registry M002 → closed (this record);
  implementation plan status → implemented.
- New guard `scripts/check_eggplan_assessment_boundary.py` runs as
  change-triggered evidence for the touched surfaces (same convention as
  the C002 guard; not wired into `verify.sh`).
- No operator action: no new migration, no config, no rollout flag. The
  two repaired clippy lints in `src/tool_advisor/retrieval_signal.rs`
  are behavior-identical (60 lib tests green) and unblock workspace
  `-D warnings`.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Two pre-existing clippy `-D warnings` failures in `src/tool_advisor/retrieval_signal.rs` (unrelated workstream) were repaired in the implementation commit to restore workspace clippy | Behavior-identical; covered by 60 existing tests | None; noted for traceability |
| info | Dirty-digest namespace translation (M001 bare hex → Eggplan `sha256:`) lives in the adapter; durable shape unchanged | None; pinned by unit test | None |
| info | Eggplan-side M002 plan stays `ready` until their own closure pass | CodeGG M002 is independently closed; cross-repo SHAs recorded in §12 | Eggplan maintainers close their M002 against these SHAs |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

M002 is closed. The hard dependency it satisfied unlocks exactly one
downstream milestone:

- CodeGG M003 repository Plan binding (`plans/subsystems/eggplan-
  assessment-integration-roadmap.md#m003`) moves from `deferred; blocked
  on positive M002` to dependency-ready: its hard predecessor (positive
  M002) is now closed. No M003 implementation plan is registered by this
  closure; registering its handoff remains future planning work.

Blocked-work audit (registry-wide, same commit): no other registered
`blocked` plan lists M002, the Eggplan facade, or the verification spec
as a hard/interface dependency. Specifically unaffected and still
blocked: Eggwork M003 (upstream Workspace/Artifact M004), tool-selection
M004/live trajectories (model evidence), order-invariance M005
(negative M004), retrieval-signal M003-M005 (await M002-V2/operating
point). The CI corrective workstream is already closed. Nothing else
changes state.

## 12. Registry updates

- Dependency-ready table: M002 row `closing` → `closed` with this
  closure pointer.
- Active subsystem roadmaps: Eggplan row current milestone `M001 closed;
  M002 closed`; roadmap status stays `active` (M003 dependency-ready,
  unregistered).
- Subsystem roadmap: M002 section `closing` → `closed`; M003 section
  notes the predecessor is closed and the binding handoff may now be
  registered (status stays `deferred` until a handoff plan exists —
  `deferred` here means unregistered, not blocked).
- Eggplan-side reconciliation (criterion 13): `eggstack/eggplan`
  `plans/implementation/codegg-integration/002-staged-eggplan-assessment-
  adoption.md` amended to record CodeGG implementation `85058541`,
  CodeGG closure (this record), Eggplan pin `0d4a6af7`, and bridge
  `088968bd` (commit `<eggplan-sha>`), leaving their M002 `ready` for
  their own closure pass.
