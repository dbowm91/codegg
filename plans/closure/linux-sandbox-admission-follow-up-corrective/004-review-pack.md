# C004 — Review pack for the Linux sandbox admission post-review corrective line

Status: **closed** (evidence and review tooling; no admission, no default, no ADR)
Base: `main` `95bb9c5ae` (the C002 merge, PR #158)
C004 branch: `plans/linux-sandbox-c004-evidence-and-review-pack`
Companion closure: `plans/closure/linux-sandbox-admission-follow-up-corrective/004-status.md`

This pack exists because C001's review left four findings open and one of them
was that a reviewer had no per-boundary map of the line. It is the map. It
does not authorize anything: the C001/C002 lines were merged by the owner, and
nothing here reopens them.

---

## 1. Heads and the exact merge baseline

| Item | Value |
|---|---|
| C001 line branch | `planning/linux-sandbox-next-admission-and-adr-cleanup` |
| Merge base with `main` | `239f06e7fc876eaa36156dbf4658bb56314c9673` |
| Head whose hosted evidence was captured | `4c08f2cc56a0e8ba227706dce70dee2e736475fa` |
| C001 line → `main` | PR #149, merge commit `2c1953347896848c2f52c2dfbd9f04029d1c2cc2` |
| C002 line → `main` | PR #158, merge commit `95bb9c5ae` |
| C004 line base | `95bb9c5ae` |
| Commits on the C001 line | **322** (`239f06e7f..4c08f2cc5`) |
| Changed files on the C001 line | **302** |

**The baseline is exact, not approximate.** `git diff --name-only
4c08f2cc5..2c1953347` is **empty**: the merge changed no file relative to the
head whose hosted evidence was captured, so the hosted evidence for `4c08f2cc5`
qualifies the merged tree's code exactly, file for file. This is the property
that lets a reviewer accept a 322-commit line without re-running it at the
merge commit.

The C001 line was reconciled with `main` **five** times, all as merges rather
than rebases, so the commit history stayed intact and each earlier head's
hosted evidence stayed valid. The fifth reconciliation was textually clean and
still wrong — `main` had changed a domain type and its DTO without touching
the mapping site that joined them — so it was closed by a compile-level proof
rather than by a textual diff. The reconciliations are recorded in
`plans/closure/branch-and-ci-stabilization/001-status.md` §§3.12–§3.21.

---

## 2. Per-boundary changed-file / commit ledger

`files` is the number of changed files in the boundary; `commits` is the number
of distinct commits on the line that touched at least one file in it. A commit
that spans three boundaries is counted in each, so the commit column sums to
more than 322 — that is the point of a boundary ledger, not an arithmetic
error.

| Boundary | Files | Commits | Example file in the boundary |
|---|---|---|---|
| `plans/**` | 84 | 207 | `plans/adrs/ADR-0044-contained-git-live-objects-and-metadata-authority.md` |
| `tests/**` | 65 | 101 | `tests/approval_router.rs` |
| `research/**` | 18 | 26 | `research/command-authorization-m004-route-coverage.md` |
| `crates/codegg-core/**` | 15 | 15 | `crates/codegg-core/src/approval.rs` |
| `src/tool/**` | 13 | 17 | `src/tool/authorization.rs` |
| `architecture/**` | 12 | 44 | `architecture/authorization.md` |
| `src/core/**` | 10 | 16 | `src/core/daemon.rs` |
| `src/security/**` | 8 | 18 | `src/security/sandbox.rs` |
| `src/scheduler/**` | 7 | 6 | `src/scheduler/eggwork.rs` |
| `crates/egglsp/**` | 6 | 3 | `crates/egglsp/src/client.rs` |
| `src/agent/**` | 6 | 5 | `src/agent/agent_loop_factory.rs` |
| `src/tui/**` | 6 | 8 | `src/tui/app/mod.rs` |
| `.github/**` | 5 | 30 | `.github/workflows/ci.yml` |
| `scripts/**` | 5 | 9 | `scripts/check_adr_integrity.py` |
| `src/git_publication/**` | 4 | 4 | `src/git_publication/mod.rs` |
| `.opencode/**` | 3 | 18 | `.opencode/skills/lsp-ide/SKILL.md` |
| `docs/**` | 3 | 10 | `docs/execution-ownership.toml` |
| `src/python_script/**` | 3 | 11 | `src/python_script/executor.rs` |
| `crates/egggit/**` | 2 | 22 | `crates/egggit/Cargo.toml` |
| `src/lsp/**` | 2 | 3 | `src/lsp/m004.rs` |
| `src/test_runner/**` | 2 | 4 | `src/test_runner/mod.rs` |
| `AGENTS.md` | 1 | 1 | `AGENTS.md` |
| `Cargo.lock` | 1 | 2 | `Cargo.lock` |
| `Cargo.toml` | 1 | 3 | `Cargo.toml` |
| `crates/codegg-protocol/**` | 1 | 4 | `crates/codegg-protocol/src/core.rs` |
| `crates/eggwork-test-node/**` | 1 | 1 | `crates/eggwork-test-node/src/main.rs` |
| `src/bin/**` | 1 | 9 | `src/bin/codegg-sandbox-helper.rs` |
| `src/error.rs` | 1 | 1 | `src/error.rs` |
| `src/exec.rs` | 1 | 1 | `src/exec.rs` |
| `src/git_mutations.rs` | 1 | 4 | `src/git_mutations.rs` |
| `src/git_network_ops.rs` | 1 | 1 | `src/git_network_ops.rs` |
| `src/git_service.rs` | 1 | 9 | `src/git_service.rs` |
| `src/interactive_process.rs` | 1 | 9 | `src/interactive_process.rs` |
| `src/interactive_process_attach.rs` | 1 | 3 | `src/interactive_process_attach.rs` |
| `src/job_dispatcher.rs` | 1 | 1 | `src/job_dispatcher.rs` |
| `src/lib.rs` | 1 | 1 | `src/lib.rs` |
| `src/main.rs` | 1 | 4 | `src/main.rs` |
| `src/managed_process.rs` | 1 | 38 | `src/managed_process.rs` |
| `src/managed_process/**` | 1 | 4 | `src/managed_process/windows.rs` |
| `src/policy_surface.rs` | 1 | 1 | `src/policy_surface.rs` |
| `src/run_rerun.rs` | 1 | 2 | `src/run_rerun.rs` |
| `src/scratch.rs` | 1 | 5 | `src/scratch.rs` |
| `src/tool_advisor/**` | 1 | 1 | `src/tool_advisor/causal_frontier.rs` |
| `src/work_plan_eggplan.rs` | 1 | 4 | `src/work_plan_eggplan.rs` |

**How to re-derive this table.** The boundary of a path is its top-level
directory, except that `src/` and `crates/` are cut one level deeper, so
`src/tool/bash.rs` is `src/tool/**` and `crates/egglsp/src/client.rs` is
`crates/egglsp/**`. The commit column is the number of commits in
`239f06e7f..4c08f2cc5` whose touched-file set intersects the boundary. The
ledger was produced by a scripted derivation over `git diff --name-only` and
`git log`, not by reading the diff by hand, so it can be recomputed exactly.

---

## 3. Ownership map

Each boundary has one owner. The owner is the registry subsystem that owns the
boundary, and the contract is the architecture document that states the
boundary's rules — a reviewer should read the contract first.

| Boundary | Owner (registry subsystem) | Contract |
|---|---|---|
| `plans/**` | Linux sandbox admission post-review corrective | `plans/registry.md`, `plans/README.md` |
| `tests/**` | Linux sandbox admission post-review corrective | `architecture/testing.md` |
| `research/**` | Linux execution isolation | `architecture/research.md` |
| `crates/codegg-core/**` | Linux execution isolation | `architecture/core.md` |
| `src/tool/**` | Command authorization and execution boundaries | `architecture/tool.md`, `architecture/tool_broker.md` |
| `architecture/**` | Architecture convergence and incomplete verticals | `architecture/overview.md` |
| `src/core/**` | Linux execution isolation | `architecture/core.md` |
| `src/security/**` | Shell-policy ADR-0028 post-closure corrective | `architecture/security.md` |
| `src/scheduler/**` | Async background jobs and CI watching | `architecture/scheduler.md` |
| `crates/egglsp/**` | Linux execution isolation | `architecture/lsp.md` |
| `src/agent/**` | Linux sandbox admission post-review corrective | `architecture/agent.md` |
| `src/tui/**` | TUI live work visibility (runtime corrective) | `architecture/tui.md` |
| `.github/**` | CI and test throughput optimization | `architecture/testing.md` |
| `scripts/**` | CI and test throughput optimization | `architecture/testing.md` |
| `src/git_publication/**` | Linux trusted Git cost corrective | `architecture/git.md` |
| `.opencode/**` | Cross-agent skills compatibility successor | `architecture/skills.md` |
| `docs/**` | Linux sandbox admission post-review corrective | `architecture/overview.md` |
| `src/python_script/**` | Linux execution isolation | `architecture/python_scripting.md` |
| `crates/egggit/**` | Linux trusted Git cost corrective | `architecture/git.md` |
| `src/lsp/**` | Linux execution isolation | `architecture/lsp.md` |
| `src/test_runner/**` | CI and test throughput optimization | `architecture/test_runner.md` |
| `AGENTS.md` | Linux sandbox admission post-review corrective | `AGENTS.md` itself |
| `Cargo.toml`, `Cargo.lock` | Dependency security and workspace consolidation | `docs/dependency-maintenance.md` |
| `crates/codegg-protocol/**` | Linux execution isolation | `architecture/projection.md` |
| `crates/eggwork-test-node/**` | Eggwork worker targeting and host telemetry | `architecture/overview.md` |
| `src/bin/**` | Windows native sandbox | `architecture/security.md` |
| `src/git_*.rs`, `src/git_service.rs` | Linux trusted Git cost corrective | `architecture/git.md` |
| `src/interactive_process*.rs`, `src/managed_process*` | Linux execution isolation | `architecture/process-tool-execution-ownership.md` |
| `src/tool_advisor/**` | Tool-selection advisor | `architecture/tool-advisor.md` |
| `src/scratch.rs`, `src/run_rerun.rs`, `src/job_dispatcher.rs` | Managed agent scratch spaces | `architecture/overview.md` |
| `src/lib.rs`, `src/main.rs`, `src/error.rs`, `src/exec.rs`, `src/policy_surface.rs`, `src/work_plan_eggplan.rs` | Linux sandbox admission post-review corrective | `architecture/overview.md` |

---

## 4. Migration and ADR integrity

| Check | Command | Result |
|---|---|---|
| ADR identity | `python3 scripts/check_adr_integrity.py` | **pass** — "active ADR IDs are unique and local paths resolve" |
| ADR identity, non-vacuity | `python3 scripts/check_adr_integrity.py --self-test` | **pass** |
| Storage layout identity | `python3 scripts/check_project_catalog_invariants.py` | **7/7 pass** |
| Storage layout version | `crates/codegg-core/src/storage/mod.rs:48` | `STORAGE_LAYOUT_VERSION = 87` |
| Highest wired migration | `crates/codegg-core/src/session/schema.rs:384,3655` | `migrate_v87` — the version marker tracks it |

**The ADR inventory.** 45 active ADRs live in `plans/adrs/` (ADR-0001 …
ADR-0047, less ADR-0027 and ADR-0028, which are archived under
`plans/archive/linux-command-adr-id-aliases/` after the command-ADR id
migration). The C001 line touched exactly three ADRs — **ADR-0044**
(contained Git live objects and metadata authority), **ADR-0045** (command
syntax and execution authority) and **ADR-0046** (composable Linux execution
isolation) — plus the ADR README and the command-id migration note. The
C004 head touches **no** ADR: it is evidence and tooling, and it settles no
decision.

**The migration identity.** The line's migrations were renumbered during the
reconciliations (v84/v85 → v86/v87) because `main` had already published
v84 and v85. The C001 record's companion M005 line records the same
renumbering for its own writer-permit and session-launch migrations. The
storage-layout marker and the highest wired migration agree at 87, which is
what `check_project_catalog_invariants.py` asserts.

---

## 5. CI qualification matrix

### 5.1 The merged baseline (the evidence that qualifies the line)

| Run | Head | Workflow | Event | Result |
|---|---|---|---|---|
| `39696056230` | `95bb9c5ae` | `CI` — the default sweep | `push` | **success** — 16,029 tests run, 16,029 passed, 8 skipped |
| `39696056208` | `95bb9c5ae` | Linux M004 process and IPC qualification | `push` | **success** — the delegated-child proof reported `... ok`, 18/18 in the native binary, `restricted-userns-fails-closed` **success** |
| `39649501713` | `4c08f2cc5` | `CI` — the default sweep | `pull_request` | **success** — 15,841 tests run, 15,841 passed, 8 skipped |

The earlier per-head hosted matrix (every workflow, every reconciliation head)
is in `plans/closure/branch-and-ci-stabilization/001-status.md` §4.3. The
three rows above are the ones that qualify the line as merged.

### 5.2 The C004 corrective head

Recorded in `plans/closure/linux-sandbox-admission-follow-up-corrective/004-status.md`
§5, with the run links and the executed counts. The sweep ran on three
successive heads of the branch, because the sweep itself found two of the
five defects the milestone introduced; the intermediate heads and their
failures are recorded there rather than hidden.

### 5.3 Which boundaries need which hosted job

The ledger says which hosted job a boundary needs; a reviewer does not need the
whole matrix for every chunk.

| Boundary | Hosted job it needs | Why |
|---|---|---|
| `src/security/**`, `src/bin/**`, `crates/egggit/**` | Linux C006 protected mount qualification | the protected-mount and trusted-Git regressions live here |
| `src/managed_process*`, `src/interactive_process*`, `src/lsp/**` | Linux M004 process and IPC qualification | the managed-process, PTY and LSP duplex probes live here |
| `crates/egggit/src/process.rs` | Linux Git object snapshot CoW qualification | the CoW snapshot isolation proof lives here |
| `src/security/sandbox/**`, `src/managed_process/**` | Windows process launcher | the Windows sandbox surface is only provable on Windows |
| everything else | `CI` — the default sweep | the unit and integration suites are host-independent |

---

## 6. Proposed review chunking

The line is 322 commits and 302 files. The chunks below are ordered by risk,
and each is independently reviewable. The C004 head adds one chunk of its own.

| # | Chunk | Boundaries | Size | Risk | The question a reviewer asks |
|---|---|---|---|---|---|
| 1 | Sandbox and process containment | `src/security/**`, `src/bin/**`, `src/managed_process*`, `src/interactive_process*` | 8 boundaries, ~16 files | **highest** | Does every launcher fail closed, and is the containment boundary the one the contract names? |
| 2 | Command authorization and routing | `src/tool/**`, `src/python_script/**`, `src/test_runner/**`, `src/exec.rs`, `src/policy_surface.rs` | 5 boundaries, ~20 files | **high** | Is the routing decision derived from the classified intent, and can any path widen a Git authorization? |
| 3 | Durable domain and its projection | `crates/codegg-core/**`, `crates/codegg-protocol/**`, `src/core/**`, `src/job_dispatcher.rs` | 4 boundaries, ~27 files | **high** | Do the migrations and the DTOs still agree, and does the layout version track the highest wired migration? |
| 4 | Git read and publication | `src/git_publication/**`, `src/git_*.rs`, `src/git_service.rs`, `crates/egggit/**` | 5 boundaries, ~10 files | **medium** | Is `egggit` still read-only, and does every mutation still go through the typed path? |
| 5 | Agent, scheduler and TUI | `src/agent/**`, `src/scheduler/**`, `src/tui/**`, `src/tool_advisor/**`, `src/run_rerun.rs`, `src/scratch.rs`, `src/work_plan_eggplan.rs` | 8 boundaries, ~22 files | **medium** | Did the durable delegation keep its owner, and did the TUI keep its authority? |
| 6 | LSP and egglsp | `crates/egglsp/**`, `src/lsp/**` | 2 boundaries, 8 files | **medium** | Is the LSP read surface still workspace-authoritative, with no ambient fallback? |
| 7 | CI, scripts and skills | `.github/**`, `scripts/**`, `.opencode/**`, `docs/**`, `AGENTS.md`, `Cargo.toml`, `Cargo.lock` | 8 boundaries, ~17 files | **low** | Do the guards still fail on an injected defect, and do the workflow paths still cover what the guards watch? |
| 8 | Plans and research | `plans/**`, `research/**` | 2 boundaries, 102 files | **low** (highest churn) | Do the closure records claim only what their evidence supports? |
| 9 | **C004 head only** | `src/security/workflow/report.rs`, `src/tool/bash.rs`, `src/tool/bash/tests/git_fixture.rs`, `src/tool/lsp.rs`, `src/interactive_process.rs`, `tests/command_routing_execution_ownership.rs`, `tests/git_fixture_isolation.rs`, `tests/interactive_process_attach_resume.rs`, `scripts/check_test_filter_execution.py`, `docs/security-test-filters.toml`, `scripts/verify.sh`, 5 workflow files | 16 files | **low** (no production behaviour) | Do the new controls fail on an injected defect, and did any assertion or timeout get weakened? |

**The recommended order is 1 → 9 → 7 → the rest.** Chunk 1 carries the real
risk. Chunk 9 is the C004 head itself and is the cheapest to review, because
it changes no production behaviour and every claim in it is backed by a
non-vacuity proof. Chunk 7 gates everything else, because the guards in it are
what make the other chunks' claims checkable.

---

## 7. What this pack does not claim

- It does not claim a host is admitted, a Linux security profile is a
  default, or any ADR is accepted. C002 remains **conditionally closed** on
  the maintainer-owned admission record and the residual cargo attribution.
- It does not claim the Windows or macOS platform behaviour of the C004 head.
  Those jobs were not awaited, and their results are recorded as *not run*,
  not as passes.
- It does not claim the C001 line's 8 skipped tests are failures. They are the
  repository's own `#[ignore]`d tests, and the count is stable across every
  head on this line.
- It does not reopen the closed C023, M004, or the M005 writer-containment
  line.
