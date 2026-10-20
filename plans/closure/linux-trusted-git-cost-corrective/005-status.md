# Linux Trusted Git Cost Corrective C005 — Closure Status

Status: blocked — local Git recovery and production admission gates are not satisfied

Source implementation plan:

- `plans/implementation/linux-trusted-git-cost-corrective/005-local-git-recovery-and-admission.md`

Source subsystem roadmap:

- `plans/subsystems/linux-trusted-git-cost-corrective-roadmap.md#milestones-and-handoffs`

Repository baseline reviewed: `59ab0d2dc1ff5bea7d5fd79fea0cd1cb57cdadbd`

Implementation commits or pull requests:

- `1041f41d` — reconcile the corrective roadmap with current main and preserve unique ADR identities.
- `59ab0d2d` — latest pushed branch head; integration test and documentation reconciliation.

## 1. Executive finding

C005 cannot admit or restore ordinary local Git mutation workflows. C002 remains test-only with incomplete race/resource qualification, C004 has no production publication owner, ADR-0044 is proposed, M005 has no production admission, and the independent M003/C010 decision remains no-admission. This record closes the current review with a blocked disposition, not a capability-completion claim.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Freeze historical Git regressions and typed-denial behavior | Existing Git origin/network regression suites and C002/C004 plans | partial | New candidate dispatch is not enabled; complete recovery corpus has not been run against a production candidate. |
| Integrate authorized live reads and mutations through typed owners | C002/C004 implementation and closure records | partial | C002 read pilot and C004 staging are test-only; no ordinary dispatch route. |
| Prove hostile filters, hooks, helpers, network and host-authority denial | C001/C004 exact hosted M004/M005 probes | partial | Covered candidate probes pass; full planned version/configuration matrix is absent. |
| Complete parity and performance qualification | C002 performance run 39204689018 and pending/reviewed cost artifacts | partial | `git log -1` and `git status --short` matrix exists; full setup/resource/compatibility and consistency gate is incomplete. |
| Native host acceptance across supported configurations | M004/M005 hosted jobs and restricted-userns refusal | partial | Positive/negative hosted evidence exists, but no M005 production-owner admission or full supported-host matrix. |
| Cross-platform non-regression | macOS compile run 39255837320 succeeded on `59ab0d2d`; Windows probe run 39254355254 succeeded on `23c07f84` | partial | Compile/probe evidence is not a full production route/behavior matrix. |
| Separate M003/C010 admission and rollback decision | C010 closure and roadmap state | blocked | No new admission record or independent approval exists; old no-admission remains authoritative. |

## 3. Production implementation evidence

No new ordinary Git backend, default switch, production staging/promotion route, or public consistency receipt is present. `GitEnvPolicy` retains the immutable private-object-view route for trusted reads. The contained live read and private staging paths remain feature-gated/test-only and use the existing `ManagedProcessService` owner. No remote transport or credential mediation was added.

## 4. Verification executed

### Commands and hosted runs

```bash
python3 scripts/check_adr_integrity.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_sandbox_contract.py
python3 scripts/check_git_forbidden_patterns.py
scripts/verify.sh quick
```

- Full CI on exact head `59ab0d2d`: [run 39255875342](https://github.com/dbowm91/codegg/actions/runs/39255875342), failed after 12,464 passed, 1 failed, 7 skipped, with 2,374 not run. Its stale pre-v83 fixture lacked the `runtime_preferences` table present in every real v69 database; that fixture was corrected and all four `cached_models_composite_key` tests passed locally.
- Full CI on final implementation head `99a3743d`: [run 39263811371](https://github.com/dbowm91/codegg/actions/runs/39263811371), passed all guards, formatting, Clippy, and all 14,839 workspace tests (7 skipped). The exact-head macOS target compile [run 39263805234](https://github.com/dbowm91/codegg/actions/runs/39263805234) passed.
- macOS target compile on `59ab0d2d`: [run 39255837320](https://github.com/dbowm91/codegg/actions/runs/39255837320), passed.
- Latest protected-mount qualification on `59ab0d2d`: [run 39256064866](https://github.com/dbowm91/codegg/actions/runs/39256064866), passed.
- Linux restricted-user-namespace refusal checks in run 39254354958 passed. Its positive process job was canceled during PTY qualification and is not counted as a full pass.
- Earlier full M004 owner qualification on C004 service-owner source: [run 39200873597](https://github.com/dbowm91/codegg/actions/runs/39200873597), passed.

### Results

ADR integrity, execution-ownership, sandbox-contract, Git-policy, formatting, and diff checks passed. The `cached_models_composite_key` target (4/4), the focused v80 queue migration test (1/1), and the queue-mutation target (34/34) passed locally after both fixture/assertion corrections. Final exact-head hosted CI passed as recorded above. No hosted evidence in this section constitutes production admission.

## 5. Invariant review

- Existing immutable trusted-read behavior and obtained guarantees remain the ordinary production route.
- Proposed contained routes cannot silently fall back to FullHost or weaken `SandboxEnforcement` semantics.
- `DestructiveRepositoryChange` remains required for destructive operations.
- Remote Git, credentials, no-writer-lock claims, broad writable `.git` grants, and single-transaction claims for index/worktree/refs remain out of scope.
- C010 no-admission and C012's rejected no-writer lease remain unchanged.

## 6. Failure and recovery review

Test-only prototypes cover selected cancellation, child cleanup, stale index detection, interrupted index lock, and identity-refusal cases. They do not demonstrate production daemon restart/idempotency, every partial publication phase, real multi-agent identity contention, or external process writer exclusion. There is no production candidate to roll back; the legacy immutable path remains enabled.

## 7. Migration and compatibility review

The branch rebase moved the additive network-intent migration to v83 after main's v80–v82 migrations and updated `STORAGE_LAYOUT_VERSION` to 83. No C005 production Git storage or public protocol migration was added. Linux test-only facilities are opt-in; macOS compile and Windows native probe evidence is recorded above. Production behavior remains on the existing cross-platform route.

## 8. Security review

The tested children request actual M004/M005 enforcement, and hosts that cannot supply required user namespaces fail closed. Production authorization, audit, consistency receipts, helper/configuration compatibility, object-writer consistency, and broker publication safety remain incomplete. ADR-0044 remains proposed; this closure does not accept it.

## 9. Documentation and operations

The Linux trusted Git roadmap and C002/C004 closures distinguish candidate infrastructure from production capability. C010/C012 historical records remain intact. No operator enablement, rollback switch, or production recovery procedure is introduced.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | ADR-0044 is proposed, not accepted | The consistency model and promotion authority are not approved | Obtain the separate architecture decision before production switch work. |
| high | C002 writer-race, consistency, setup/resource and compatibility evidence is incomplete | A fast single-operation pilot cannot establish safe routine Git behavior | Complete the C002 closure gates and independent review. |
| high | C004 lacks production-owned durable publication/recovery | Ordinary mutations could not safely publish index/object/ref/worktree effects | Implement and qualify a daemon-owned publisher and failure recovery. |
| high | M005 production-owner host/configuration acceptance and independent M003/C010 admission are absent | No supported-host or release admission is established | Complete M005 owner matrix and record a separate explicit admission/rollback decision. |
| low | Two earlier full CI runs failed on stale pre-v83 test fixtures/assertions | They delayed integration but did not reveal a product migration defect; corrections now pass full CI | Preserve run history and retain the updated migration tripwires. |

## 11. Roadmap disposition

C005 is blocked. It cannot hand off positive local Git recovery to Linux C002 or M008, cannot alter M003/C010 admission, and does not unblock remote C003. Reopen implementation only after the named ADR and proof gates are resolved. Branch integration CI and cross-platform compile are green; those results do not waive the production/security gates.

## 12. Registry updates

- Record C005 as blocked with this closure record.
- Keep the Linux trusted Git corrective roadmap active; preserve C010 no-admission, C012 negative findings, and proposed ADR-0044 status.
- No downstream Linux C002, M003, M006, or M008 milestone is newly unblocked.

## 13. Addendum — status of the gates since this review

The disposition above is the review's disposition **at the reviewed baseline**
(`59ab0d2d`). This addendum records which of its named gates have since been
addressed, so it is not read as a statement about the current tree. **C005
remains blocked**; the blocked disposition is unchanged.

| Gate | Status |
|---|---|
| C004 lacks production-owned durable publication/recovery | **addressed.** C007 (`plans/closure/linux-trusted-git-cost-corrective/007-status.md`) implements and qualifies a daemon-owned publisher: a private staging index, a compare-and-swap publication under Git's own `index.lock`, a durable journal, a startup reconciler, and a recovery path that reports committed / aborted / ambiguous and never a silent wrong answer. Proven on a qualified host by hosted runs `39318206124`, `39339024314` and `39356728579`. |
| M005 production-owner host/configuration acceptance is absent | **partially addressed.** The owner qualification is now closed by C014 (hosted runs `39318206124` and `39339024314`, both reporting the M005 netns owner proof and the native restricted-userns proof `ok`), and the obtained network fact is now reported per launch by C023 (`plans/closure/linux-execution-isolation/023-status.md`), with hosted run `39356728579` proving the publication receipt states the boundary it obtained. **What is still absent:** the tool-program child-job route is unqualified, there is no machine-readable supported-host policy, and no independent M003/C010 admission decision has been recorded. |
| ADR-0044 is proposed, not accepted | **still unsatisfied.** It is an owner decision. C006's review (`plans/closure/linux-trusted-git-cost-corrective/006-status.md` §8) records which of its blocking items remain: the typed consistency class, the production live-ODB read route, the consumer-side admissibility fence, and `include.path` mediation are all still unimplemented. C023 removed the clause 2 M005 vacuity; it did not accept the ADR. |
| C002 writer-race, consistency, setup/resource and compatibility evidence is incomplete | **still unsatisfied.** C002's own closure is "conditionally closed — contained live-read and performance pilots complete for **test-only** use; production remains blocked": the pilot is gated behind a dev-only feature enabled through `[dev-dependencies]`, so it is absent from `cargo build` and from the shipped binary. Its measured cost improvement is evidence about the *mechanism*, not evidence that a production route exists, and the writer-race, consistency, setup/resource and compatibility evidence this review named is still missing. |
| Independent M003/C010 admission | **still unsatisfied.** C010 no-admission stands and is unchanged. |

So two of the five rows moved: one fully (C004's production publisher) and one
partially (M005's owner qualification — the owner path and the obtained fact
are closed; the tool-program child-job route, the supported-host policy and
the admission decision are not). The remaining rows are an **architecture
decision** (ADR-0044 acceptance) and an **admission decision** (M003/C010),
both of which are the owner's to make, plus C002's test-only pilot status.
C005's own ordered packages 1–6 (the parity work, the hostile filter/hook/
helper denial matrix, the cost comparison, the native acceptance matrix, the
cross-platform non-regression and rollback dry-run) are also unstarted; this
addendum closes a gate, it does not perform the plan.
