# Linux Sandbox Admission Follow-up Corrective — Milestone C001 — Closure Status

Status: **corrective pass required**
Source implementation plan: `plans/implementation/linux-sandbox-admission-follow-up-corrective/001-branch-ci-evidence-stabilization.md`
Source subsystem roadmap: `plans/subsystems/linux-sandbox-admission-follow-up-corrective-addendum.md` (C001)
Repository baseline reviewed: `2c1953347896848c2f52c2dfbd9f04029d1c2cc2` (`main`, the merge of PR #149)
Implementation commits: `ef3083ab38a5f216a67813090f67b2e62a73d124` — the C001 closure review, the two process-unique Git fixtures, and the workflow path triggers

## 1. Executive finding

C001's objective was to make the long-lived Linux candidate **reviewable and
reproducibly testable against current `main`**, without merging it to
production or implicitly closing any capability.

The bulk of that objective is delivered. The branch was reconciled with
`main` five times, each reconciliation proved at both the file level and the
compile level, the candidate was merged to `main` as PR #149 (merge commit
`2c1953347`), and the branch-wide evidence for that head was captured on
hosted runners.

What is **not** delivered is the tail of the plan: the complete
test-fixture isolation audit (WP3), the load-sensitive LSP characterization
(WP4), the security-filter execution controls (WP5), and the review pack
(WP6). This milestone's closure review therefore does not claim closure. The
status is **corrective pass required**, and the remaining work is registered
as a new numbered plan under this subsystem (§12).

Nothing in this milestone changed a Linux security profile, a default, an
M003/C010 admission, an ADR, or a Git semantic.

## 2. Requirement-to-evidence matrix

| Plan requirement | Evidence | Status |
|---|---|---|
| WP1: freeze provenance; record heads and merge base; inventory by subsystem; determine whether `plans/.mutate-probe` is required or stray | `plans/closure/branch-and-ci-stabilization/001-status.md` §§3.12–§3.21 records five reconciliations with both the file-level and the compile-level proof for each. `plans/.mutate-probe` was dropped as a stray probe in `f24058c74` ("drop the stray plans probe"); it is absent from `main` | met |
| WP1: record exact CI runner successes/failures and revision | `plans/closure/branch-and-ci-stabilization/001-status.md` §4; the hosted run identifiers and revisions are recorded there | met |
| WP2: merge/rebase current `main` into an isolated worktree with no force push or loss of parallel work; inspect every conflict | Five reconciliations, all merges (not rebases), recorded in the branch-and-ci closure. The candidate was merged to `main` as PR #149 — merge commit `2c1953347` | met |
| WP2: preserve accepted ceilings, C007 journal/migration, C014/C023 obtained facts, and current mainline features | Reconciliation #5 proved that a textually clean merge was **not** sufficient: `main` changed the scratch domain type and the scratch DTO without touching the mapping site in `src/core/daemon_scratch.rs`. The compile-level proof (`cargo check --workspace --all-targets --locked`, exit 0) is what closed it | met |
| WP3: the four Git-routing and ten security-review tests must own a temp Git worktree or bind a scoped repository cwd per test; no reliance on mutable shared process cwd or host repo state | **Partially advanced in this pass.** The two fixtures that used a *fixed* shared path under `.codegg/test_tmp/` — `tests/git_execution_origin_matrix.rs` (`row_5c`) and `tests/identity_m002_live_audit_hooks.rs` (`m002-routed-git`) — now use a process-unique directory, so two concurrent test processes can no longer `remove_dir_all` each other's repository. The complete audit of the named test set is **not** done | **discharged by C004** — `plans/closure/linux-sandbox-admission-follow-up-corrective/004-status.md` §3.1 |
| WP3: reproduce with true concurrent tests and temporary repositories | Not done as a dedicated reproduction; the fix removes the shared-mutable precondition rather than demonstrating the race | **discharged by C004** — C004 §3.2, `tests/git_fixture_isolation.rs` |
| WP3: preserve legitimate typed `BackendUnavailable[RepositoryAlias]` refusals; no blanket widening of Git subcommand authorization | Preserved. No Git authorization was changed in this pass; the fixtures still assert the typed refusal | met |
| WP4: instrument elapsed phase and contention for the three known load-sensitive LSP tests; retain normal functional assertions; prefer per-test fixtures or a bounded scheduling fix; if the cause remains unknown, mark **conditional** rather than lowering the timeout, disabling or silently excluding tests | The three live-language-server tests are **not** in the default sweep: they are the `#[cfg(feature = "lsp-real-server-tests")]` unit tests in `src/tool/lsp.rs`, and the timeout was not lowered — the 120-second bound and the functional assertions are retained | **discharged by C004** — the cause was diagnosed rather than marked conditional; see the factual correction below and C004 §3.3 |
| WP5: ensure security test filters actually execute named tests; assert count, no self-skip marker and positive/negative controls | **Not delivered.** No filter-execution control was added in this pass | **discharged by C004** — C004 §3.4 |
| WP5: identify workflow path-trigger omissions in `.github/workflows/linux-m004-native.yml` so Linux owner code changes trigger native qualification | **Delivered in this pass for the Tool Program / policy paths.** `src/tool/tool_program.rs` and `src/tool/tool_program_context.rs` were added to both the `pull_request` and `push` path lists; `src/scheduler/**` already covered the executor. The omission is now enforced mechanically by `scripts/check_network_intent_owner_wiring.py`, which checks the coverage in **both** blocks — a path that qualifies only a pull request would leave a direct push to `main` unqualified | met (for the paths in scope) |
| WP5: record scope for `.github/workflows/linux-c006-native.yml` and macOS compile | Not delivered in this pass | **discharged by C004** — C004 §5.3 records the C006 native tier's scope and states the macOS compile as **not run** on the corrective head, not as a pass |
| WP6: provide a small per-boundary changed-file/commit ledger, exact `main` merge baseline, migration/ADR integrity result, CI qualification matrix and recommended smaller review chunks | The merge baseline and the CI qualification matrix exist in `plans/closure/branch-and-ci-stabilization/001-status.md`. The per-boundary ledger and the recommended review chunks are **not** produced | **discharged by C004** — C004 §4.4, `plans/closure/linux-sandbox-admission-follow-up-corrective/004-review-pack.md` |
| Verification: `git diff --check`, `cargo fmt --all -- --check`, `cargo check --workspace --all-targets --locked`, `scripts/verify.sh quick`, and the named static guards | §4 | met for the local scope |
| Verification: `cargo test --locked --test git_execution_origin_matrix`, `--test command_routing_execution_ownership`, `cargo test -p codegg --lib security::workflow` | §4.4 | met |
| Verification: `cargo nextest run --workspace --locked --profile ci --no-fail-fast` at an unchanged revision, plus hosted native/macOS jobs | The branch-wide sweep was **not** re-run locally in this pass; the local scope was narrowed at the user's direction because the environment is contended. The branch-wide hosted evidence for the merged baseline is recorded in the branch-and-ci closure (hosted CI `39649501713`: 15,841 run, 15,841 passed, 8 skipped, on `4c08f2cc5`) | **outstanding** (§10.5) |
| Exit: "Pass: clean (non-destructive) integration candidate, every required focused check genuinely executed with positive controls, branch-wide test outcome and remaining known failures classified, links to source and tested heads, no false admission/default/ADR claims" | The integration candidate, the links and the classification are in place. "Every required focused check genuinely executed with positive controls" is **not** satisfied — the filter-execution controls are missing (§10.3) | **not met** |
| Stop: "Distinguish readiness from actual merge: no production merge is authorized here" | Respected. The merge was performed by the owner as PR #149, on the owner's authority; this milestone authorized nothing. No admission, default or ADR claim was made | met |

## 3. Production implementation evidence

### 3.1 The reconciliation and the merge

The candidate was reconciled with `main` five times, all as merges rather
than rebases, so that the 309-commit history was preserved and the hosted
evidence for earlier heads stayed valid. Each reconciliation recorded the
heads, the merge base, the file-level diff, and — for the fifth — a
compile-level proof, because that reconciliation was textually clean and was
still wrong: `main` had changed a domain type and its DTO without touching
the mapping site that joined them.

The merged result is `main` at `2c1953347`. `git diff --name-only
4c08f2cc5..2c1953347` is empty: the merge changed no source file relative to
the head whose hosted evidence was captured, so that evidence qualifies the
merged tree's code exactly.

### 3.2 The test-fixture isolation change made in this pass

`tests/git_execution_origin_matrix.rs` and
`tests/identity_m002_live_audit_hooks.rs` each created their Git fixture
repository at a **fixed** path under `.codegg/test_tmp/`, and each began with
`remove_dir_all` on that path. Two concurrent test processes would therefore
delete each other's fixture repository mid-test.

Both now use a process-unique directory name. The placement requirement is
unchanged — the fixture must live inside the package working directory for
the typed active-routing validator, which compares the repository against the
workspace — and the change is confined to the fixture's identity, not to its
semantics or to any Git authorization.

### 3.3 The workflow trigger change made in this pass

See the WP5 row in §2. The guard that enforces it is
`scripts/check_network_intent_owner_wiring.py`, whose coverage check spans
both trigger blocks.

## 4. Verification executed

All commands ran in the linked worktree `~/.codex/worktrees/032b/codegg`
under `umask 0022` with an isolated `XDG_CACHE_HOME` (the GitHub runner's
default umask). The local workspace's default umask is `0002`, under which
`core::scratch_sweep::tests` fails for an environmental reason unrelated to
this work: those fixtures use `create_dir_all`, so their root lands
group-writable and the scratch allocator refuses it with `WrongOwner`. The
same binary passes when only the umask changes.

**Local scope was deliberately narrowed** at the user's direction: the
environment is contended and previously cancelled rust processes. No
full-workspace sweep was run, no Windows job was awaited, and no macOS
compile was run.

### 4.1 Compile gate

`cargo check --workspace --all-targets --locked` — **exit 0**.

### 4.2 Static gates

| Command | Result |
|---|---|
| `scripts/verify.sh quick` | **"Quick verification passed."**, exit 0 |
| `python3 scripts/check_network_intent_owner_wiring.py` | pass |
| `python3 scripts/check_network_intent_owner_wiring.py --self-test` | pass — 7 injected defects, each rejected |
| `python3 scripts/check_adr_integrity.py` | pass (via `scripts/verify.sh quick`) |
| `python3 scripts/check_execution_ownership.py` | pass (via `scripts/verify.sh quick`) |

### 4.3 Fixture-isolation change

`cargo test --locked --test git_execution_origin_matrix` and
`cargo test --locked --test identity_m002_live_audit_hooks` — see §4.4.

### 4.4 Changed-surface regression slice

`cargo nextest run --locked --profile ci --no-fail-fast -p codegg` with
`--lib` and the eleven changed-surface targets — the two fixtures this
milestone changed, the two targets this plan names
(`git_execution_origin_matrix`, `command_routing_execution_ownership`, and
`security::workflow` inside the library target), and the eight Tool Program
targets that exercise the tool boundary and the executor this milestone's
sibling changed.

**7522 tests run, 7522 passed, 4 skipped** — `Summary [ 164.410s]`, exit 0.

The slice was **not** the workspace sweep: no other target was built or run.

## 5. Invariant review

- **No blanket widening of Git subcommand authorization.** Untouched.
- **No timeout lowered, no test disabled, no filter widened.** The LSP
  timeout remains 120 seconds and its functional assertions remain.
- **No admission, default or ADR claim.** C010's no-admission decision and
  ADR-0044's proposed status are unchanged.
- **No unrelated bulk change to obtain green CI.** The fixture change is
  confined to two fixture paths.
- **A closure that claims less than it did.** This record does not claim
  closure, because the exit condition's "every required focused check
  genuinely executed with positive controls" is not satisfied.

## 6. Failure and recovery review

- The fixture change removes a shared-mutable precondition. If two
  concurrent processes do run the same fixture, they now use separate
  repositories.
- The LSP tests remain excluded from the default sweep by an explicit,
  documented feature rather than by a silent skip; the feature's scope is
  recorded in the repository's own policy.

## 7. Migration and compatibility review

No migration, protocol or storage change was made in this pass. The two
fixture paths are test-local.

## 8. Security review

- The fixture change does not alter any authorization path.
- The workflow trigger change makes the native qualification job reachable
  for Tool Program policy changes, which it previously was not. That is a
  coverage improvement, and it is enforced mechanically.

## 9. Documentation and operations

- `plans/registry.md` — the subsystem row records C001's status and the
  remaining items.
- `plans/subsystems/linux-sandbox-admission-follow-up-corrective-addendum.md`
  — the milestone status table was updated.
- `docs/` and `architecture/` — no change was required; this milestone
  changed no product behaviour, only test fixtures and a workflow trigger.

## 10. Unresolved findings

**Update — 2026-10-22: all five findings owned by this milestone are
discharged.** C004 closed them; the closure record is
`plans/closure/linux-sandbox-admission-follow-up-corrective/004-status.md`.
§10.1–§10.5 are kept below as the findings **as they were stated here**, so
that the record of what was open stays intact, with a discharge line on each.
§10.6 was already assigned to upstream and is unchanged.

### 10.1 The complete test-fixture isolation audit (severity: medium, owner: C001 corrective)

> **Discharged by C004** (§3.1, §3.2). All 14 named tests own a per-test
> temporary repository; the concurrency reproduction is a committed test,
> proved non-vacuous against an injected shared fixed path.

Two shared-path Git fixtures were fixed in this pass. The plan's named set —
the four Git-routing and ten security-review tests — has **not** been
audited in full, and the plan's concurrent reproduction with temporary
repositories was not performed.

**Exact future evidence required.** A per-test audit of the named set, each
test either owning a temp Git worktree or binding a scoped repository cwd,
with a concurrent reproduction demonstrating that two processes running the
same fixture do not interfere.

### 10.2 The LSP load-sensitivity characterization (severity: medium, owner: C001 corrective)

> **Discharged by C004** (§3.3). The cause was **diagnosed**, so the
> conditional path was not needed: the cold call is bounded by the 30 s
> readiness wait, which waits for a published
> `textDocument/publishDiagnostics`, and a fixture whose opened file produces
> no diagnostic never publishes one. A per-test workspace with a
> diagnostic-bearing file cuts the cold call from >120 s to ~3.7–4.4 s. No
> timeout was lowered and no test was excluded.
>
> **Factual correction.** This finding — and the §2 matrix row above —
> cited `crates/egglsp/tests/real_server_smoke.rs` as the location of "the
> three live-language-server tests". That file holds five *different* smoke
> tests with its own `TEST_TIMEOUT = 120s`. The three tests are the
> `#[cfg(feature = "lsp-real-server-tests")]` unit tests in
> `src/tool/lsp.rs`. The correction does not change the finding or its
> severity; it corrects where the tests are.

The three live-language-server tests are behind `lsp-real-server-tests` and
keep their functional assertions and their 120-second bound. What is missing
is the elapsed-phase and contention instrumentation, and the per-test
service/workspace fixtures the plan prefers.

**Exact future evidence required.** Per-test fixtures for the three tests, a
recorded characterization of their elapsed phase under contention, and a
decision on whether the cause is understood — marked conditional if it is
not, rather than lowering the timeout or widening an exclusion.

### 10.3 Security-filter execution controls and the C006/macOS scope record (severity: medium, owner: C001 corrective)

> **Discharged by C004** (§3.4, §5.3). All 20 security filters are
> registered and wrapped; the runtime control proves a non-zero executed
> count, the absence of a self-skip marker, and every named test's witness,
> with a positive/negative control pair. The C006 native tier's scope and
> the macOS compile are recorded — the macOS compile is recorded as **not
> run** on the corrective head, not as a pass.

No control asserts that a security test filter actually executed the named
tests, and no count, self-skip-marker or positive/negative control check was
added. The scope of `.github/workflows/linux-c006-native.yml` and of the
macOS compile was not recorded.

**Exact future evidence required.** For each security filter: the executed
count, an assertion that it is non-zero, an assertion that no self-skip
marker was emitted, and a positive/negative control pair. Plus a written
scope record for the C006 native workflow and the macOS compile.

### 10.4 The per-boundary review pack (severity: low, owner: C001 corrective)

> **Discharged by C004** (§4.4). The pack is
> `plans/closure/linux-sandbox-admission-follow-up-corrective/004-review-pack.md`:
> a 44-boundary changed-file/commit ledger over the line's 322 commits and
> 302 files, the exact merge baseline, the migration/ADR integrity result,
> the CI qualification matrix, a per-boundary owner map, and a 9-chunk
> risk-ordered review proposal.

The per-boundary changed-file/commit ledger and the recommended smaller
review chunks were not produced.

**Exact future evidence required.** A ledger mapping each changed boundary
to its commits and its owner, the exact `main` merge baseline, the
migration/ADR integrity result, the CI qualification matrix, and a proposed
review chunking.

### 10.5 Branch-wide hosted evidence on the corrective head (severity: medium, owner: C001 corrective)

> **Discharged by C004** (§5). The branch-wide hosted sweep was run on the
> corrective head `a6e596642` and the run links and executed counts are
> recorded there. The Windows and macOS platform behaviour of that head is
> recorded as **not awaited / not run** — not as a pass. The Windows job was
> subsequently awaited and passed; the macOS target compile was not run and
> is still recorded as *not run*.

The branch-wide sweep was not re-run locally in this pass, and the hosted
sweep was not run on the corrective head. The branch-wide hosted evidence for
the merged baseline is in `plans/closure/branch-and-ci-stabilization/001-status.md`
(hosted CI `39649501713`: 15,841 run, 15,841 passed, 8 skipped, on
`4c08f2cc5`, content-identical to `main` for every source file).

**Exact future evidence required.** A hosted `CI` run on the corrective head,
with the run link recorded.

### 10.6 The pre-existing Desktop E2E failure (severity: low, owner: upstream)

`apps/desktop/e2e/specs/m004-session.e2e.ts:169` fails on the `[data-testid="permission-list"]`
assertion after 120 s. It was reproduced on `main` before this milestone and
is not attributable to this branch. It is recorded, not fixed here.

## 11. Roadmap disposition

- **C001** — **closed** (updated 2026-10-22). The integration and evidence
  objective was delivered and merged; the remaining findings — the
  fixture-isolation audit, the LSP characterization, the filter-execution
  controls, the review pack and the hosted evidence — were discharged by
  C004, whose closure record is
  `plans/closure/linux-sandbox-admission-follow-up-corrective/004-status.md`.
  §10.6 was already assigned to upstream and remains recorded, not fixed
  here.
- **C002** — closed conditionally; see
  `plans/closure/linux-sandbox-admission-follow-up-corrective/002-status.md`.
- **C003** — untouched and still blocked behind ADR-0044 acceptance, the C004
  production vertical in the M005 writer-containment line and the M005
  production owner qualification. C004 in *this* line is the evidence and
  review-pack milestone and does not satisfy that dependency.

## 12. Registry updates

- The subsystem row for "Linux sandbox admission post-review corrective" in
  `plans/registry.md` records C001 as **closed** (updated 2026-10-22) with
  this record's path.
- A corrective plan is registered under this subsystem for the remaining
  C001 work: `plans/implementation/linux-sandbox-admission-follow-up-corrective/004-c001-remaining-evidence-and-review-pack.md`.
  It is **closed**; its record is
  `plans/closure/linux-sandbox-admission-follow-up-corrective/004-status.md`
  and its review pack is
  `plans/closure/linux-sandbox-admission-follow-up-corrective/004-review-pack.md`.
- **Unblock audit:** C003 was audited. Its hard dependencies (ADR-0044
  acceptance, the C004 production vertical, the M005 production owner
  qualification) are unchanged by this milestone, so C003 is **not**
  unblocked.
- No other registered plan's blocker is resolved by this milestone.

## 13. Verification policy note

The local scope was narrowed deliberately and is recorded here rather than
hidden. The reasons were the contended host and the user's explicit
instruction to prune testing that was not needed for this change. The
commands that were run cover the changed surface, the fixtures this
milestone changed, and the gates this plan names. Everything that was not
run is named in §10 with the exact evidence that would close it.
