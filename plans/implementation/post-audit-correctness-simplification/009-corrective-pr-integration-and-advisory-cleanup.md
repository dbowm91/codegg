# Post-Audit Correctness, Simplification, and Footprint C001 — Corrective PR Integration and Advisory Cleanup

Status: blocked

Source subsystem roadmap/addendum:

- `plans/subsystems/post-audit-correctness-simplification-roadmap.md`
- `plans/subsystems/post-audit-correctness-simplification-corrective-closure-addendum.md`

Corrective target:

- M008 closure: `plans/closure/post-audit-correctness-simplification/008-status.md`
- target corrective closure: `plans/closure/post-audit-correctness-simplification/009-corrective-status.md`

Long-term/governance references:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

Repository/PR state reviewed:

- `main`: `8bcc15e0663d610a132bc16c2f35fe637421a1b1`
- PR #73 head at review: `a3c22d129f7b0c2fe462e435acfe77daa39ab48f`
- PR #73: open, mergeable, draft
- PR title/body at review describe only M003 TUI work even though the branch contains M001-M008
- M008 records a successful hosted `verify` run for accepted production head `f404249`
- the current branch includes later closure/documentation commits after that accepted production head
- M008 notes a pre-existing transitive `lru` advisory for explicit follow-up disposition

Primary class: corrective closure / integration polish

## 1. Objective

Make the repository's final integration state truthful and complete without reopening the
accepted M001-M008 implementation.

C001 must:

1. make PR #73 metadata accurately represent the whole post-audit workstream;
2. confirm the actual merge candidate passes the existing routine CI contract;
3. disposition the pre-existing `lru` advisory from the actual locked graph with the smallest
   justified action;
4. merge the accepted work to `main` when the checks and repository state permit;
5. reconcile planning/closure records to the merged SHA;
6. stop without creating another bookkeeping-only corrective pass.

Success is a merged, accurately documented workstream, not another round of production
refactoring.

## 2. Explicit non-goals

C001 must not:

- reimplement or redesign M001-M008;
- split the binary or daemon/TUI packaging;
- redesign scheduler, daemon, protocol, storage, provider, ACP, projection, plugin, Git, or
  Tool Program boundaries;
- add new tests merely to increase evidence volume when existing focused and hosted evidence
  already covers the accepted behavior;
- add CI lanes, matrices, scheduled audits, release automation, dependency bots, coverage,
  benchmark, artifact, or size gates;
- perform a broad Ratatui, Comrak, RustPython, Tokio, reqwest, or workspace dependency
  upgrade merely because newer versions exist;
- raise the repository MSRV for dependency freshness;
- reopen the independent runtime-safety/Landlock evidence condition;
- change product behavior to make PR metadata or closure bookkeeping easier;
- create C002 unless a concrete new correctness/security defect is discovered and cannot be
  fixed within the narrow scope defined here.

## 3. Current implementation evidence

The implementation branch contains the complete M001-M008 production work and individual
closure records. Repository review found the production shape coherent:

- M001 pins validated HTTP destinations and bounds streamed bodies before accumulation;
- M002 validates live daemon identity before signalling and uses serializer-backed JSON;
- M003 corrects multiline tag offsets and unifies Unicode wrapping/counting;
- M004 narrows dependency defaults without removing features;
- M005 removes invalid/redundant routine CI machinery;
- M006 removes the global 32 MiB stack workaround after reproducing and fixing the actual
  stack-heavy future boundary;
- M007 deletes dead mirror representations while retaining real dispatch/persistence
  boundaries;
- M008 records final dependency/size evidence and a successful hosted verification run for
  the accepted production head.

The remaining discrepancy is integration state: `main` still contains only the planning
registration while PR #73 remains a draft with stale M003-only metadata. The implementation
must not be considered fully landed until that discrepancy is resolved.

## 4. Invariants that cannot regress

- The production diff accepted by M001-M008 remains the source of truth unless current CI or
  merge conflict exposes a concrete defect.
- PR cleanup must not silently alter M001-M008 semantics.
- The normal single `verify` job remains the only required hosted gate.
- `scripts/verify.sh quick` remains the local fast verification entry point; do not restore
  deleted duplicate verification machinery.
- `RUST_MIN_STACK=33554432` must not be reintroduced as a global workaround.
- The deleted Tokio flavor scanner/baseline and YAML parser boundary guard must not return.
- `qrcode`, Comrak, and RustPython feature narrowing remain unless a demonstrated
  compatibility failure requires a targeted correction.
- Single-daemon authority, single-binary topology, protocol/storage compatibility, supported
  features, scheduler authority, and manual release cadence remain unchanged.
- Closure status must reflect repository truth: an unmerged PR is not a strictly merged
  workstream closure.

## 5. Expected code/document changes

Expected mandatory changes are primarily repository metadata and planning records:

- PR #73 title and body;
- PR draft/ready-for-review state;
- `plans/registry.md`;
- `plans/subsystems/post-audit-correctness-simplification-corrective-closure-addendum.md`;
- `plans/closure/post-audit-correctness-simplification/009-corrective-status.md`;
- M008 closure only if a factual link/SHA statement must be corrected after merge.

Potential production/manifest changes are allowed only under the advisory decision rule or a
new concrete CI failure:

- `Cargo.toml` / `Cargo.lock` for a narrow compatible advisory fix;
- focused source/tests only when required to correct a newly demonstrated regression.

Do not edit unrelated files for cleanup aesthetics.

## 6. Storage, protocol, migration, and compatibility effects

Storage:

- no schema or state migration expected.

Protocol:

- no daemon/client wire change expected.

Compatibility:

- no CLI command, tool schema, config path, endpoint, feature, or packaging change expected.

Migration:

- none. PR/registry reconciliation is repository metadata, not a user migration.

If implementation discovers that any of these statements is false, stop and classify the
finding before continuing.

## 7. Work package A — Freeze and classify the merge candidate

1. Fetch current `main`, PR #73 metadata, head SHA, changed-file list, reviews/threads, and
   hosted workflow state.
2. Confirm PR #73 is still based on the intended planning baseline or identify any new
   `main` commits that materially overlap the changed ownership boundaries.
3. Compare current branch head to the last accepted production head named by M008.
4. Classify all commits after the accepted production head as:
   - documentation/planning only;
   - CI/manifest change requiring verification;
   - production change requiring focused review.
5. Confirm there is no unresolved review thread or requested-change review.
6. Do not modify production code during this package.

Expected result: one exact SHA designated as the merge candidate, plus a short explanation
of whether it differs materially from M008's accepted production head.

## 8. Work package B — Correct PR metadata and readiness state

Update PR #73 so a maintainer can understand the branch without reading 18 commits.

The title should describe the complete workstream, for example:

`runtime: close post-audit correctness and simplification workstream`

The body should summarize, at minimum:

- bounded/SSRF-safe untrusted HTTP;
- daemon-stop identity and JSON correctness;
- TUI layout/deduplication fixes;
- dependency feature slimming;
- CI/static-guard contraction;
- stack root-cause correction and removal of global `RUST_MIN_STACK`;
- execution-model dead-layer cleanup;
- final measurement/closure evidence;
- explicit statement that single-daemon/single-binary/manual-release architecture is
  unchanged;
- exact verification references appropriate to the current merge candidate.

Then:

1. leave the PR draft while any required current-head gate is incomplete;
2. mark it ready only after the merge candidate is stable and no corrective edit is pending;
3. do not request ceremonial reviewers unless repository policy actually requires one.

## 9. Work package C — Verify the latest relevant head without expanding CI

Use the existing CI flow only.

Required hosted condition:

- the normal PR `verify` job must complete successfully for the actual merge candidate or a
  documentation-only descendant whose tree is the one being merged.

Inspect failures rather than automatically rerunning them. If failure is:

- infrastructure/cache/transient and the production steps are not implicated, one rerun is
  acceptable;
- formatting/Clippy/test/static-guard failure, correct the concrete defect and rerun focused
  local verification first;
- a new product correctness/security failure, stop C001 and create a narrow corrective plan
  if the fix exceeds this plan's integration scope.

Do not add a workflow dispatch trigger, second lane, matrix, artifact, or alternate hosted
verification path to satisfy C001.

Local verification is intentionally minimal:

```bash
scripts/verify.sh quick
```

Run it only if C001 changes tracked repository content beyond PR metadata, or after a
manifest/source correction. Do not repeat the full 9,686-test local workspace run solely
for closure; hosted CI owns the broad final gate.

## 10. Work package D — Disposition the `lru` advisory

Treat this as a narrow dependency-risk classification, not an upgrade campaign.

1. Identify the locked package and reverse dependency path:

```bash
cargo tree -i lru --locked
cargo tree -d --locked
```

2. Confirm the current advisory identifier, affected versions, patched versions, severity,
   and vulnerable API/behavior from an authoritative advisory source.
3. Determine whether CodeGG directly uses `lru` or receives it only through a transitive
   dependency such as Ratatui.
4. Determine whether the vulnerable behavior is reachable in CodeGG's supported default
   configuration.
5. Check whether a patched `lru` can be selected with a compatible lockfile-only or narrow
   manifest update under the current dependency constraints and Rust 1.81+ contract.

Decision tree:

### D1 — Narrow compatible fix available

Apply the smallest manifest/lockfile change. Do not upgrade unrelated crates. Verify:

```bash
cargo tree -i lru --locked
cargo check --workspace --all-targets --locked
cargo test --lib tui::components::messages
cargo test --lib tui::components::dialogs::share
scripts/verify.sh quick
```

Record before/after dependency path and confirm no feature/MSRV change.

### D2 — Patched line requires broad upstream migration

Do not perform that migration in C001. Record:

- exact dependency owner/path;
- why the existing semver constraints cannot select the patched line;
- whether the vulnerable API is reachable;
- severity/exposure assessment;
- recommended future owner.

Register a separate dependency-maintenance plan only when the risk justifies active work.
A low-risk, unreachable, or migration-heavy transitive advisory may remain deferred with an
explicit record.

### D3 — Advisory no longer applies

If current advisory metadata or the actual lockfile shows M008's note is stale, correct the
closure record and state why no code change is required.

Do not add `cargo audit` to routine CI as part of this package.

## 11. Work package E — Merge and prove repository integration

Once PR metadata is accurate, the PR is ready, and required checks are green:

1. confirm PR #73 is mergeable against current `main`;
2. merge using the repository's normal merge method;
3. fetch the resulting `main` SHA;
4. confirm the merge contains the accepted production files and closure records;
5. confirm `main` no longer points only to the planning-registration state;
6. inspect post-merge status checks only as required by normal repository policy.

If the implementation agent lacks merge permission, stop with the PR in a clearly
ready-to-merge state and mark C001 `blocked` on maintainer merge. Do not mark C001 closed.

## 12. Work package F — Planning and closure reconciliation

Create:

- `plans/closure/post-audit-correctness-simplification/009-corrective-status.md`

The closure must record:

- PR #73 final title/body disposition;
- final PR head and merged `main` SHA;
- hosted `verify` run URL/result for the merge candidate;
- whether the current head differs from M008's accepted production head and why;
- advisory dependency path and chosen D1/D2/D3 disposition;
- any source/manifest correction made during C001;
- unresolved findings by severity;
- explicit confirmation that no new CI/release/size/audit machinery was added.

Then update `plans/registry.md` so:

- C001 is removed from dependency-ready/active work;
- the corrective addendum is `closed`;
- the post-audit workstream returns to strict `closed` against the merged SHA;
- no C002/M010-style evidence-only plan is created.

The original M001-M008 closure records remain historical evidence. Correct only factual
errors such as stale merge/head links; do not rewrite them to conceal that C001 was needed.

## 13. Focused verification matrix

PR metadata/planning-only changes:

```text
No production test required beyond normal hosted verify on the merge candidate.
```

Registry/Markdown changes:

```bash
git diff --check
```

Manifest/lockfile advisory correction, if any:

```bash
cargo check --workspace --all-targets --locked
scripts/verify.sh quick
```

TUI dependency movement, if any:

```bash
cargo test --lib tui::components::messages
cargo test --lib tui::components::dialogs::share
```

Do not run unrelated optional-feature, cross-target, LSP, plugin, benchmark, coverage, or
release checks absent a concrete changed boundary.

## 14. Static guards

Add no new static guard by default.

If a concrete C001 production correction touches an existing guarded boundary, run the
existing applicable guard. Do not create a regex scanner for PR metadata, advisory versions,
or closure status.

## 15. Acceptance criteria

C001 is complete only when all are true:

- PR #73 title/body accurately summarize the complete M001-M008 workstream;
- PR #73 is not draft when submitted for merge;
- no unresolved review/change-request condition remains;
- the actual merge candidate has a successful normal hosted `verify` run;
- any C001 source/manifest edit has focused local verification appropriate to its boundary;
- the `lru` advisory has an explicit, evidence-based D1/D2/D3 disposition;
- no broad dependency migration is hidden inside advisory cleanup;
- PR #73 is merged to `main`, or C001 is explicitly blocked on missing merge authority rather
  than falsely closed;
- the merged SHA is recorded and contains the accepted M001-M008 implementation;
- registry/addendum/closure state agrees with the actual repository state;
- no new workflow lane, matrix, release automation, audit gate, dependency bot, artifact,
  coverage/benchmark gate, or continuous size threshold is introduced;
- no binary split, protocol/storage migration, daemon-authority change, feature removal, or
  MSRV increase occurs;
- no follow-up corrective plan is created solely for PR/evidence bookkeeping.

## 16. Stop conditions

Stop C001 and report the exact blocker if:

- `main` has materially diverged and the PR requires nontrivial semantic conflict resolution;
- current hosted CI exposes a critical/high correctness or security issue in the accepted
  implementation;
- fixing the advisory requires a broad Ratatui/TUI migration or MSRV change;
- the merge candidate changes protocol, storage, daemon authority, binary topology, supported
  features, or release policy beyond M001-M008;
- the PR cannot be merged due repository permissions or branch policy.

A narrow formatting/Clippy/test defect introduced by the final integration commits may be
corrected within C001. A new architecture or product defect requires a separately scoped
corrective plan.

## 17. Required closure evidence

`plans/closure/post-audit-correctness-simplification/009-corrective-status.md` must include:

- implementation/cleanup commits;
- final PR #73 URL, title, draft state, merge result, head SHA, and merged SHA;
- merge-base/divergence statement against `main` immediately before merge;
- hosted workflow run/result for the merge candidate;
- advisory identifier, locked version, reverse dependency path, affected/patched range, and
  chosen disposition;
- any manifest/source changes plus focused test results;
- registry/addendum final state;
- unresolved findings by severity;
- recommendation: `closed` only when the implementation is actually present on `main`.
