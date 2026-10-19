# M005 Writer-Containment Corrective C001 — Closure Status

Status: closed
Source plan: `plans/implementation/agent-workstream-m005-writer-containment-corrective/001-restricted-profile-contract-and-writer-matrix.md`
Contract deliverable: `plans/implementation/agent-workstream-m005-writer-containment-corrective/c001-restricted-profile-contract.md`
Source subsystem roadmap: `plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md`
Repository baseline reviewed: `e78497d29444e5130c50948cc1b55e914afae04c`
Decision outcome: **ADR-0042 remains `proposed` — still proposed; not accepted**
Recommendation: closed; C002 remains blocked on an independent maintainer accept/reject decision on ADR-0042.

## 1. Finding and disposition

The M005 source audit found **48 CodeGG writer roots** across every surface the
milestone named: model-facing native file and Git tools, typed Git mutations
and their network and recovery paths, process-backed and persistent-lifetime
writers, non-broker daemon/HTTP/project-root writers, and project-local state.
Every root now has a declared final effect owner, checkout locator, authority
path, lifetime, adversarial bypass example, and a restricted-profile
disposition.

The decisive finding is that the current coordination is **not a write
boundary**. Batch serialization is eligibility- and path-overlap-gated, and a
batch containing `git`, `bash` or `formatter` takes no lease at all. The
repository lock is taken only by the LSP preview-apply path and the worktree
service, and those use **disjoint lock tables**, so a lock on one path is
invisible to the other. The HTTP file route is a second, non-broker writer on
the same paths, with no lock, no digest and no atomic replace. The `git`
tool's `workdir` is model-supplied and is not constrained to the workspace
root.

C001's disposition is therefore a **decision package, not an implementation**.
It publishes the complete matrix, the policy each root receives, the
user-visible guarantee and its exact refusal contract, the lock order and
ownership the successor must implement, and a guard that makes an undecided
writer visible. It creates no permit, fences nothing, and enables nothing.

## 2. Changes landed

- Added the C001 contract deliverable: the complete 48-root writer matrix, the
  two-axis policy matrix, the user-visible guarantee and the exact
  unsupported-capability error contract, the lock-order and ownership model,
  the C002 protocol shape, and the twelve named holes the audit found.
- Added `docs/writer-disposition.toml` — a declarative manifest of every
  writer root and its disposition, with a vocabulary that deliberately
  contains no enabling value.
- Added `scripts/check_writer_disposition.py` — a narrow static inventory
  guard that fails when a model-facing writer tool, or a daemon/HTTP/
  project-state module that performs a filesystem effect, has no declared
  disposition. It enumerates *code sites*, never the effects of dynamic
  command contents, and says so in its own docstring.
- Wired the guard and its self-test into `scripts/verify.sh quick`.
- Added `tests/writer_disposition_guards.rs` — the executable negative
  guards: a writer-category tool may not declare a read-only effect class;
  only reviewed tools may enter the read-only programme palette; every writer
  tool must carry a non-read disposition; and the manifest may not gain an
  authority vocabulary.
- Added four source-audit evidence files under `c001-evidence/`, each with
  `file:line` citations for every claim and explicit `UNPROVEN` markers where
  a claim could not be established.

## 3. Requirement-to-evidence matrix

| C001 acceptance criterion | Evidence | Status |
|---|---|---|
| One complete table per CodeGG writer root: final owner, checkout locator, authority path, lifetime, disposition, test/guard, OS status, adversarial bypass example | `c001-restricted-profile-contract.md` §4 — 48 rows, each backed by `c001-evidence/0{1..4}-*.md` with `file:line` citations | Pass |
| Exercise failure for a prospective shared mutator without authority/profile | `tests/writer_disposition_guards.rs::writer_category_tools_may_not_declare_a_read_only_effect_class` and `::the_disposition_manifest_grants_no_authority` | Pass |
| Legacy isolated mutator continues | `tests/writer_disposition_guards.rs::the_legacy_isolated_mutator_still_dispatches` — the default catalog still registers and resolves `write`, `edit`, `replace`, `apply_patch`, `bash`, `git`. C001 changed no runtime behaviour. | Pass |
| Verify no hidden CodeGG API (editor save, HTTP POST, restore, init, run-local `.codegg`) omitted | `c001-evidence/03` covers editor save, HTTP write/delete, document create/save/delete, project init, edit-checkpoint restore; `c001-evidence/04` enumerates the 13 paths physically inside a checkout under `.codegg/`, including the legacy `sessions.db` and the run store. The guard then keeps the set from drifting. | Pass |
| Verify policy denies arbitrary shell/Python/plugin/PTY/LSP/MCP until native proof; ordinary group read does not grant mutation | 15 `denied-process` roots in the matrix cover bash, terminal, python_script, test, verify, ci_watch, job, the local MCP child, the LSP server child, the process-backed plugin and the hook adapter. Read-only roots are `read-proven` and are not writers. | Pass |
| Write an explicit decision outcome | **Still proposed.** ADR-0042 is unchanged and remains `proposed — NOT accepted`. This closure records that outcome explicitly. | Pass |
| `cargo fmt`, scoped tests/guards, `scripts/verify.sh quick`, source inventory checks | See §5. | Pass |

## 4. Before/after trace

**Before.** The writer surface was spread across the broker, the scheduler, the
daemon, the HTTP routes, the LSP host, the project-init authority and the
project-local state root, with no single inventory. A reader asking "what can
write to this checkout, and who authorises it?" had to reconstruct it from a
dozen modules, and a new writer route could appear without anyone deciding
its disposition. The `git` tool accepted a model-supplied `workdir` that
`resolve_repo_root` validated only as "exists, is a directory, has a `.git`".

**After.** The 48 roots are enumerated with their dispositions in one matrix
and one machine-readable manifest. A new writer tool or a new daemon module
that writes now fails `scripts/check_writer_disposition.py` until someone
declares its disposition, and the guard is wired into `verify.sh quick`. The
twelve holes the audit found are named, cited, and carried into C002–C005
rather than left implicit. Nothing is permitted, fenced, or enabled.

## 5. Verification

Passed:

- `cargo fmt --all -- --check`.
- `cargo check --workspace --all-targets`.
- `cargo test --test tool_execution` — 61 tests.
- `cargo test --test writer_disposition_guards` — 5 tests.
- `python3 scripts/check_writer_disposition.py --self-test`.
- `python3 scripts/check_writer_disposition.py` — 39 declared roots, 17 writer
  tools, 20 writer modules.
- `scripts/verify.sh quick`.

Guard non-vacuity was proven on the real tree, not only against synthetic
fixtures: a new `ToolCategory::Mutating` tool was injected into `src/tool/`
with no manifest entry, the guard was confirmed to fail with
`writer tool 'zz_injected_writer_probe' ... has no disposition`, and the
guard was confirmed to pass again once the probe was removed.

## 6. Gaps carried into C002–C006 (not fixed here, and not fixable here)

C001 cannot fix these: the fence does not exist yet. They are recorded in
`c001-restricted-profile-contract.md` §8 with citations.

1. The `git` tool's model-supplied `workdir` is not constrained to the
   workspace root (`src/tool/git.rs:342-345`, `src/git_mutations.rs:343-364`).
2. No native file or Git sink takes the repository lock, and the LSP and
   worktree paths use **disjoint** `WorkspaceLockTable` instances.
3. A batch containing `git`, `bash` or `formatter` takes no lease at all, and
   `replace` and `commit` are path-invisible to the permission layer.
4. The HTTP file route is a second, non-broker writer: no lock, no digest, no
   atomic replace, no `FileChanged` event.
5. Every digest check in the document path is check-then-act, not an atomic
   compare-and-swap.
6. A linked worktree does not collapse to one identity; no writer consults a
   git common-dir mapping.
7. The run-store index has a provable lost update across three independent
   `FsRunStore` instances sharing one root.
8. Relative-path fallbacks defeat the "outside the repository root" fence.
9. The scheduler's exclusivity key sets are empty for the process kinds that
   matter, so `workspace-mutation` and `worktree-mutation` never conflict.
10. `JobExecutionContext.workspace_root` is not honoured by the argv executor.
11. The daemon-cwd guard does not cover `src/hooks/mod.rs` or
    `src/plugin/runtime/process.rs`, and the local MCP child sets no
    `current_dir` at all.
12. No sink has proven descendant-tree quiescence on Unix; only the Windows
    Job Object gives a real tree kill, and that bounds lifetime, not write
    authority.

## 7. The historical M005 gap, and what C001 changed about it

The original milestone, `plans/implementation/agent-workstream-blackboard/005-group-worktree-placement.md`,
left a Q0–Q5 qualification sequence unrun. C001 does not run any of them —
it cannot; the permit does not exist. What C001 does is fix the *inputs* to
that sequence, and turn the Q0 posture from a claim into an inventory.

| Historical M005 question | State at C001 | What C001 added |
|---|---|---|
| **Q0** — safe-default posture (pass; negative evidence only) | Still negative evidence only | The posture is no longer a claim: 48 writer roots are enumerated with their disposition, and `scripts/check_writer_disposition.py` keeps that enumeration from silently drifting. A new writer route now fails the build until someone decides its disposition. |
| **Q1** — identity/state machine (durable CAS on `(WorktreeId, generation)`) | Unrun; belongs to C002 | Fixed the tuple the permit must resolve — `(WorktreeId, lease generation, group reservation, authenticated actor/run, execution attempt)` — and identified that **a linked worktree does not collapse to one identity** (no writer consults a git common-dir mapping), and that the scheduler's exclusivity key sets are empty for the process kinds that matter. |
| **Q2** — direct native writers (two overlapping runs, one barrier) | Unrun; belongs to C003 | Identified what would make a Q2 race vacuous: **no native file or Git sink takes the repository lock**, a batch containing `git`/`bash`/`formatter` takes **no lease at all**, and `replace`/`commit` are path-invisible to the permission layer. |
| **Q3** — every enabled writer owner (and every disabled surface refused before spawn) | Unrun; belongs to C004/C005 | Enumerated the enabled writer owners and the surfaces that must be refused before spawn or mutation, and recorded the descendant verdict: **no Unix sink has proven descendant-tree quiescence**; only the Windows Job Object gives a real tree kill, and it bounds lifetime, not write authority. |
| **Q4** — native platform matrix (Linux, macOS, Windows separately) | Unrun; belongs to C006 | Recorded the platform status C006 must qualify against, and the rule that a surface without evidenced process-tree or filesystem containment is marked unavailable rather than inferred. |
| **Q5** — integration and recovery (nested worktree base, handoff, crash recovery) | Unrun; belongs to C006 | Recorded the integration hazards: the run-store **lost update** across three `FsRunStore` instances, the relative-path fallbacks that defeat the "outside the repository root" fence, the **disjoint** `WorkspaceLockTable` instances, and the legacy project database keyed to the **process cwd**. |

The original milestone's own gate — "do not start Q1 storage until the
product boundary and per-platform capability matrix are accepted" — is
unchanged and still binding. C001 supplies the product-boundary package
(ADR-0042, `proposed`) and the writer inventory Q1–Q5 will be measured
against. It does not start Q1.

## 8. C002 unblock audit

**C002 is NOT unblocked.** C002 requires an explicit maintainer
accept/reject decision on ADR-0042, and ADR-0042 is `proposed`. The
corrective's own gate is unchanged: "C002 may not start until the decision
is accepted and C001 is closed."

C001 is closed, and that half of the gate is satisfied. The other half is
not. The C001 closure therefore records the decision outcome as **still
proposed**, which is an honest and complete C001 outcome — C001's scope is
the decision package, and the package is complete.

| Gate condition | State |
|---|---|
| C001 closed | **Satisfied** — this record. |
| ADR-0042 explicitly accepted by an independent maintainer | **Not satisfied** — ADR-0042 remains `proposed`. |
| C002 may start | **No.** |

No part of this closure, the contract document, the manifest, the guard or the
new tests changes ADR-0042's status, and none of them mark it accepted in
code or in the registry.

## 9. Provenance

- Source audit performed against `e78497d2`, in four independent surface
  clusters, with every claim carrying a `file:line` citation and every
  unestablished claim marked `UNPROVEN` with its reason. The evidence files
  are `plans/implementation/agent-workstream-m005-writer-containment-corrective/c001-evidence/`.
- The audit was executed as four read-only work units against disjoint
  surfaces; each wrote exactly one evidence file and modified no source.
- The writer-disposition manifest was reconciled against the audit's
  dispositions: the native file/Git roots that already carry a strong native
  fence in-tree (the LSP mutation primitives, the document path, the
  edit-checkpoint restore, the worktree service) are `fenced-native`;
  the managed state inside the project tree is `fenced-or-denied`; the
  process-backed roots are `denied-process`; and the dormant legacy snapshot
  restore is `out-of-contract`.
- The `tool_program` read-only label was independently verified as
  **enforced and fail-closed** (`src/tool/tool_program_context.rs:200-242`):
  a delegated tool must be program-callable *and* explicitly read-only, and
  the default contract is `NonIdempotent`. The Tool Program's *background*
  job surface remains `denied-process`.
- The `git` workdir hole, the disjoint lock tables, and the descendant
  quiescence verdict were independently re-verified against source before
  being recorded here.
