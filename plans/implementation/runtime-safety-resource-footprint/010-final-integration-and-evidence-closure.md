# Runtime Safety, Resource Control, and Footprint Corrective C002 — Final Integration and Evidence Closure

Status: implemented — see `plans/closure/runtime-safety-resource-footprint/010-status.md`

Source subsystem roadmap:

- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`

Predecessor dispositions:

- M001 — conditionally closed;
- M002 — conditionally closed;
- M003 — closed;
- M004 — closed;
- M005 — conditionally closed;
- M006 — conditionally closed;
- M007 — closed;
- M008 — conditionally closed;
- corrective C001 — conditionally closed.

Repository baseline reviewed: `27bf5ae6f172defaf1f0be79566d0f94f7859498`

Pull request context: PR #72, branch `planning/runtime-safety-resource-footprint`

Target closure record:

- `plans/closure/runtime-safety-resource-footprint/010-status.md`

Primary class: branch integration, external evidence, and truthful final closure

## 1. Objective

Close the runtime-safety, resource-control, dependency, and footprint workstream without adding product scope or verification machinery.

This corrective closure pass owns exactly five outcomes:

1. reconcile the completed implementation branch with current `main` so PR #72 is mergeable without losing accepted production or planning state;
2. replace stale PR metadata with a truthful workstream-level title, summary, validation record, and remaining-evidence statement;
3. obtain one normal hosted `verify` result on the final reconciled PR head;
4. obtain one supported-Linux Landlock enforcement result, reusing the hosted run when it provides sufficient fixture evidence;
5. correct the M003 documentation/closure language so the current UTF-8 command representation is not described as arbitrary platform-byte-lossless argv.

This is the final registered plan for this workstream. It is not a new runtime feature milestone.

## 2. Current state

The implementation branch is complete enough for final integration:

- M003 implemented typed executable/argv routing and removed whitespace reconstruction and silent native-to-shell fallback;
- M007 completed dependency/feature reduction and retained the measured single-binary topology;
- M008 reconciled architecture, testing, CI, release, execution-ownership, and planning documentation;
- local focused checks and `scripts/verify.sh quick` are recorded as passing;
- no critical, high, or hidden production-correctness medium finding remains in the accepted closure records.

The remaining defects are integration and evidence defects:

- PR #72 remains draft, non-mergeable, and one `main` commit behind while carrying 41 workstream commits;
- its title/body still describe only M005 despite containing the complete M001–M008/C001 workstream;
- no hosted workflow run or combined status is attached to final head `18ce24b9`;
- the corrected Landlock implementation has not produced one supported-Linux enforcement record;
- M003 uses `String`/`Vec<String>` rather than `OsString`, so arbitrary non-UTF-8 Unix argv is not preserved even though quoted, empty, whitespace, and shell-special UTF-8 arguments are preserved.

## 3. Explicit non-goals

This pass must not:

- add another runtime feature, command route, sandbox backend, parser, daemon, binary, package, or protocol;
- redesign M001–M008 or reopen accepted implementation without a reproducible defect;
- split PR #72 into a new milestone series solely for aesthetics;
- rewrite the 41-commit evidence chain through an unnecessary force rebase;
- add a workflow lane, matrix, manual-dispatch workflow, artifact requirement, benchmark gate, continuous size gate, audit bot, or release automation;
- require duplicate full-workspace local verification;
- require a fixed release cadence or perform a release;
- implement arbitrary non-UTF-8 command transport unless an existing documented public contract clearly requires it;
- create another evidence-transfer or ratification plan after this plan closes.

## 4. Invariants

- Accepted production behavior from M001–M007 and C001 must survive branch reconciliation unchanged unless a concrete merge conflict exposes a defect.
- The daemon remains the single execution and scheduling authority.
- Required Landlock requests remain fail-closed; unsupported hosts do not count as enforcement evidence.
- Native typed arguments remain distinct from explicit shell programs.
- Existing manual release ownership and one bounded routine CI job remain unchanged.
- Closure records remain factual: no hosted or Linux result is claimed before it exists.
- PR mergeability must be obtained through normal Git history and conflict resolution, not by dropping accepted commits or force-overwriting `main`.

## 5. Work package A — Reconcile the branch with `main`

Preferred method: merge current `main` into `planning/runtime-safety-resource-footprint`.

A merge is preferred over rebasing because:

- the branch already contains 41 ordered implementation and closure commits;
- closure records cite accepted commit identities;
- the branch is only one `main` commit behind;
- rewriting the full history creates more evidence churn than resolving the duplicated planning merge.

Procedure:

1. record the pre-merge branch head, current `main` head, and merge base;
2. merge `main` into the workstream branch without force-pushing;
3. resolve production-code conflicts by preserving the later accepted implementation unless `main` contains a newer independent correction;
4. resolve planning/registry conflicts by preserving the latest branch closure state while retaining canonical planning text already accepted on `main`;
5. do not duplicate roadmap, implementation-plan, closure, or registry rows;
6. run `git diff --check` and inspect the merge commit against both parents;
7. confirm the branch is no longer behind `main` and GitHub reports the PR mergeable or identifies only normal required checks.

Conflict stop condition:

- stop and report a specific corrective defect if reconciliation produces a production conflict whose correct resolution cannot be determined from accepted closure records and current source behavior;
- do not create a broad replacement branch or silently choose one side of a security/process conflict.

Expected conflict scope is planning/registry history. A broader conflict set must be explained in the closure record.

## 6. Work package B — Correct PR metadata and review state

Update PR #72 only after branch reconciliation.

Required title shape:

`runtime: close safety, process, dependency, and footprint workstream`

The body must summarize, at minimum:

- maintained child-only Landlock enforcement and private helper status transport;
- canonical bounded process execution and typed native argv/shell routing;
- bounded grep concurrency/context extraction;
- dependency, memory namespace, and YAML parser maintenance;
- measured dependency/feature reduction and no-split binary decision;
- documentation, registry, minimal CI, and manual-release reconciliation;
- exact local verification already accepted;
- final hosted and Linux evidence produced by this pass;
- any remaining operational limitation, including UTF-8-only typed command representation if retained.

Remove stale statements that describe the PR as M005-only or cache-post-step-only work.

Keep the PR draft until:

- merge conflicts are resolved;
- PR metadata matches the diff;
- the normal hosted run is scheduled.

Mark ready for review only when the final head and expected checks are stable.

## 7. Work package C — Hosted verification on final head

Use the existing normal PR-triggered workflow. Do not add or expand workflow topology.

Required result:

- one run attached to the final reconciled PR head;
- formatting, boundary/static guards, workspace check, Clippy, and workspace tests complete under the existing bounded settings;
- record run URL, workflow name, commit SHA, conclusion, and failed step if any.

Disposition rules:

- a compile, Clippy, test, parser guard, sandbox guard, or execution-ownership failure is a real defect and blocks closure;
- a cache/log post-step failure after all required commands pass remains operational evidence, but the branch must still satisfy repository merge policy;
- do not create a new lane or rerun matrix to work around runner storage;
- if a normal rerun on the unchanged final head succeeds, use that result and record the earlier operational failure without creating a corrective plan.

Run one local `scripts/verify.sh quick` only if conflict resolution or final documentation changes alter governed files after the previously accepted quick result. Do not duplicate the broad local suite.

## 8. Work package D — Supported-Linux Landlock evidence

Run the existing fixture on one Landlock-capable Linux kernel:

```bash
cargo test --test sandbox_landlock -- --test-threads=1
```

Record:

- exact commit SHA;
- Linux distribution and kernel version;
- effective Landlock ABI reported by the implementation/fixture;
- allowed-read result;
- read-only write-denial result;
- allowed workspace-write result;
- outside-root denial result;
- helper setup/exec status-channel result;
- confirmation that the parent daemon/process remains unrestricted;
- test conclusion.

The final hosted PR run may satisfy this requirement when:

- it runs on Linux;
- the kernel supports the required Landlock ABI;
- the fixture actually executes enforcement assertions rather than an unsupported-host skip;
- kernel/ABI and fixture outcomes are recoverable from the run logs or a minimal rerun of the single existing test.

Otherwise, run only the named fixture on one suitable Linux host. Do not add a permanent CI lane.

Failure rules:

- unsupported-kernel skip is not evidence and leaves the condition open;
- an enforcement assertion failure is a real M001/C001 defect and blocks merge/strict closure;
- a runner provisioning failure is operational and may be retried without a new plan.

## 9. Work package E — Truthful M003 argv disposition

The current typed command model uses UTF-8 `String` values. It correctly preserves:

- empty arguments;
- embedded spaces, tabs, and newlines represented in UTF-8;
- literal quotes and backslashes;
- shell metacharacters as native argument content;
- separation of executable, argv, and display rendering.

It does not preserve arbitrary non-UTF-8 Unix path/argument bytes.

Default closure action:

1. retain the current implementation;
2. update the M003 implementation/closure and command architecture documentation to state that the typed command boundary is lossless for the supported UTF-8 command representation;
3. record arbitrary non-UTF-8 native argv as an unsupported/known limitation, not a hidden defect;
4. remove or qualify any claim that `OsString`-equivalent platform-byte preservation was achieved.

Implement `OsString` end to end only if source review finds an existing documented public contract that explicitly promises arbitrary non-UTF-8 command/path execution. Such a change must remain narrowly contained and must not introduce lossy durable JSON/protocol conversion. If satisfying that promise requires a protocol or storage redesign, record it as deferred product compatibility work and retain truthful UTF-8 documentation for this closure.

No new standalone milestone is required for the known limitation.

## 10. Work package F — Reconcile closure records and registry

After the final evidence is available:

1. update M001, M002, C001, and M008 closure records with the Linux fixture evidence;
2. update M008 with the final hosted run evidence;
3. update M003 with the UTF-8 argv disposition;
4. mark conditional records strictly `closed` only when their named condition is actually satisfied;
5. add `plans/closure/runtime-safety-resource-footprint/010-status.md` summarizing integration, PR, hosted, Linux, and argv-disposition evidence;
6. mark this plan `closed`;
7. mark the runtime-safety roadmap strictly `closed` if all conditions pass;
8. remove C002 from dependency-ready/active registry sections and retain one recently closed workstream row;
9. confirm no downstream registered plan is blocked or newly ready;
10. preserve the compact registry and historical closure files.

If Linux evidence remains unavailable but branch reconciliation and hosted verification complete, mark C002/M008 `conditionally closed` with exactly that one remaining condition. Do not create C003.

## 11. Focused verification

Required checks are intentionally small:

```bash
git diff --check
python3 scripts/check_execution_ownership.py --self-test
python3 scripts/check_execution_ownership.py
python3 scripts/check_sandbox_contract.py --self-test
python3 scripts/check_sandbox_contract.py
scripts/verify.sh quick
cargo test --test sandbox_landlock -- --test-threads=1  # Linux only
```

Execution policy:

- run the static checks/quick script after final conflict resolution;
- use the existing hosted `verify` job for the final combined tree;
- run the one Linux fixture once on a capable host unless the hosted run already supplies complete evidence;
- do not run another local full workspace/all-feature matrix solely for closure.

## 12. Acceptance criteria

C002 is strictly closed only when:

- the workstream branch contains current `main` and is not behind it;
- conflict resolution is documented and preserves accepted production behavior;
- PR #72 metadata accurately represents the complete workstream;
- PR #72 is no longer draft and GitHub reports it mergeable subject only to normal policy/checks;
- one hosted `verify` run is attached to the final reconciled head and its required commands pass;
- one Landlock-capable Linux run proves the existing enforcement fixture rather than skipping it;
- the M003 record truthfully describes UTF-8 argv support and does not claim arbitrary platform-byte losslessness unless implemented;
- M001, M002, C001, and M008 conditional evidence is reconciled factually;
- the C002 closure record is complete and the registry/roadmap show the correct final disposition;
- no new CI lane, release automation, binary split, feature removal, or verification matrix was added;
- no critical, high, or product-correctness medium finding remains;
- the branch is ready for normal merge to `main`.

The actual PR merge may occur immediately after these criteria or as the final action of the executor, according to repository ownership policy. Record the merge commit when performed.

## 13. Stop conditions

Stop and report the precise blocker when:

- branch reconciliation exposes an unresolved production/security conflict;
- hosted verification fails a required command on the final head;
- the Linux fixture executes and fails an enforcement assertion;
- PR protection or repository policy requires an unavailable approval that cannot be satisfied by code/planning changes;
- an explicit existing public contract requires non-UTF-8 argv and the necessary fix would require a broad protocol/storage redesign.

Do not stop for:

- an already-understood cache post-step failure after required commands pass;
- absence of release artifacts;
- lack of a binary-size reduction beyond the accepted M007 no-split result;
- unavailable optional feature matrices not required by the existing bounded CI contract;
- cosmetic desire to split the completed workstream into multiple PRs.

## 14. Required closure evidence

`plans/closure/runtime-safety-resource-footprint/010-status.md` must contain:

- pre/post branch, `main`, merge-base, and merge commit SHAs;
- conflict inventory and resolution rationale;
- final PR title, state, mergeability, head SHA, and eventual merge SHA if merged;
- final hosted run URL, commit SHA, required-step outcomes, and conclusion;
- Linux kernel, Landlock ABI, exact fixture command, and enforcement outcomes;
- M003 UTF-8/non-UTF-8 support disposition;
- focused local command results;
- updated M001/M002/M003/C001/M008 disposition links;
- confirmation that CI/release/topology scope remained minimal;
- unresolved findings by severity;
- final recommendation: merge/closed, or conditionally closed on one precisely named external item.
