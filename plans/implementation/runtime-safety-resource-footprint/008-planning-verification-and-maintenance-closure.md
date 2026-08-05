# Runtime Safety, Resource Control, and Footprint Milestone 008 — Planning, Verification, and Maintenance Closure

Status: blocked on M001–M007

Source subsystem roadmap:

- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`
- Milestone 008

Hard dependencies:

- M001 through M007 must each have an accepted implementation disposition and compact closure record.

Repository baseline reviewed: `4d540ce315c9ef2a1c07544cd42df0efc43708e1`

Primary class: documentation, verification, and governance reconciliation

Target closure record:

- `plans/closure/runtime-safety-resource-footprint/008-status.md`

## 1. Objective

Close the workstream with a small, accurate maintenance surface after the production milestones land.

M008 must:

- reconcile architecture, security, execution, tool, testing, dependency, and release documentation with the accepted implementation;
- keep `plans/registry.md` compact and active-work-oriented;
- ensure each milestone has one closure record rather than a chain of ratification/evidence-transfer plans;
- reconcile fragile or contradictory test-count claims;
- ensure routine CI remains one bounded verification job and manual release cadence remains unchanged;
- remove redundant verification work only when the resulting contract remains clear and equivalent;
- archive or de-register completed interim plans according to `plans/003-planning-process.md` without deleting history;
- leave no unowned high/medium defect or misleading security/footprint claim.

This is a closure and maintenance pass. It must not become another production refactor or expansive evidence project.

## 2. Explicit non-goals

This milestone must not:

- reopen M001–M007 merely for stylistic preferences;
- add a new CI workflow, job matrix, scheduled job, artifact, benchmark, coverage gate, dependency bot, audit lane, or release job;
- perform a release, query package ownership, or run package-by-package publication checks when no release is being prepared;
- require a duplicate local full-workspace run when the accepted hosted `verify` result covers the same executable tree;
- create separate independent-ratification milestones for ordinary successful closure;
- retain every historical closed milestone in the active registry;
- rewrite canonical long-term documents merely to reflect transient file layout;
- archive accepted ADRs or canonical roadmap/specification files;
- change runtime behavior except for a narrowly demonstrated documentation/guard mismatch that cannot be reconciled otherwise;
- add hard binary-size limits to CI;
- create a documentation generator or planning database.

## 3. Current implementation evidence

Inspect at minimum:

- `plans/003-planning-process.md`;
- `plans/registry.md`;
- the runtime-safety roadmap and M001–M007 plans/closure records;
- `architecture/security.md` or current security/sandbox documentation;
- `architecture/scheduler.md`;
- `architecture/jobs.md`;
- `architecture/tool_programs.md` where execution ownership is described;
- `architecture/testing.md`;
- `AGENTS.md`;
- `CONTRIBUTING.md`;
- `RELEASING.md`;
- `.github/workflows/ci.yml`;
- `scripts/verify.sh`;
- static execution/sandbox/dependency guards introduced by M001–M006;
- package metadata and binary/service documentation changed by M007.

The reviewed baseline already has a contracted one-job CI and a manual release contract. The principal maintenance problem is historical planning/evidence accumulation and documentation drift, including inconsistent reported test totals in testing documentation.

M008 must treat the accepted production tree and closure records as source evidence. It must not replay every predecessor command or require each milestone author to restate the same broad run.

## 4. Invariants that cannot regress

- routine hosted CI remains bounded and non-releasing;
- manual release cadence, version choice, credentials, and publication remain maintainer-owned;
- `scripts/verify.sh quick` remains the ordinary local handoff command;
- broad verification is not duplicated merely for closure bookkeeping;
- static guards fail closed when their matcher/runtime fails;
- security documentation distinguishes required enforcement, best-effort fallback, and disabled sandboxing truthfully;
- execution documentation states actual output, timeout, cancellation, and descendant semantics;
- dependency documentation states actual feature ownership rather than aspirational removals;
- binary documentation states the measured topology decision and does not promise unsupported role-specific packages;
- active registry contains current roadmaps/plans/blockers and a small recent-closure summary, not a complete historical ledger;
- completed history remains discoverable under `plans/closure/`, `plans/archive/`, Git history, and source roadmap links;
- no unresolved high/medium finding is hidden by marking a milestone closed.

## 5. Closure-record reconciliation

Confirm that these records exist and contain the evidence required by their plans:

- `plans/closure/runtime-safety-resource-footprint/001-status.md`;
- `plans/closure/runtime-safety-resource-footprint/002-status.md`;
- `plans/closure/runtime-safety-resource-footprint/003-status.md`;
- `plans/closure/runtime-safety-resource-footprint/004-status.md`;
- `plans/closure/runtime-safety-resource-footprint/005-status.md`;
- `plans/closure/runtime-safety-resource-footprint/006-status.md`;
- `plans/closure/runtime-safety-resource-footprint/007-status.md`.

Each should remain compact and include:

- accepted revision;
- focused mechanism evidence;
- one relevant quick/hosted broad result;
- compatibility/migration disposition;
- unresolved findings by severity;
- explicit closure recommendation.

Do not create replacement records solely to standardize prose. Correct a record only when it materially misstates implementation, evidence, or residual risk.

M001 must retain independent security review. M002 must retain its second review. Do not invent independent-review requirements for M003–M008 absent a demonstrated high/medium finding.

## 6. Active registry target

Reconcile `plans/registry.md` to the normative compact control-surface model.

At M008 closure it should contain only:

- canonical planning references;
- currently active subsystem roadmaps;
- dependency-ready implementation plans;
- blocked plans and named blockers;
- newly dependency-ready work;
- a concise recent-closure section;
- deferred unregistered product work that remains intentional.

When this roadmap is complete:

- mark the runtime-safety roadmap closed;
- remove M001–M008 from dependency-ready/blocked tables;
- add one concise recent-closure row for the workstream with the M008 record and accepted revision;
- retain links to M001–M007 through the roadmap and closure directory rather than eight permanent active rows;
- do not restore the large historical milestone ledger.

## 7. Archive policy

Apply `plans/003-planning-process.md` proportionately.

- Canonical documents and accepted ADRs remain in place.
- The completed subsystem roadmap may remain under `plans/subsystems/` as the durable workstream index.
- Completed implementation plans may remain in their subsystem directory while recently relevant, or move as a coherent set under `plans/archive/implementation/runtime-safety-resource-footprint/` when the repository's established archive convention supports it.
- Closure records remain under `plans/closure/` and are not duplicated into the archive.
- Do not move hundreds of unrelated historical files in this milestone.
- Do not create redirect stub files for every archived plan unless existing links cannot be updated cheaply.

Prefer de-registration and clear roadmap links over a broad file-movement churn.

## 8. Documentation reconciliation

### 8.1 Sandbox and security

Documentation must state:

- supported backend/platform;
- required versus best-effort behavior;
- actual fallback;
- host capability/ABI reporting;
- read-only versus workspace-write profile;
- known non-Linux limitations;
- that the daemon is not confined by a child-only Landlock policy.

Remove claims based on the old handwritten implementation.

### 8.2 Process execution

Document:

- canonical process owner;
- typed argv versus explicit shell route;
- stdout/stderr bounds and overflow behavior;
- timeout versus cancellation;
- Unix process-group cleanup;
- non-Unix limitations;
- approved long-lived-process exemptions;
- sandbox outcome propagation.

Ensure `docs/execution-ownership.toml`, its guard, and architecture prose agree.

### 8.3 Search

Document only stable user/maintainer-relevant behavior:

- bounded worker count;
- cancellation and result limits;
- deterministic ordering when promised;
- no persistent index.

Do not expose incidental batch sizes as public API.

### 8.4 Dependency and parser maintenance

Document:

- explicit TLS/SQLx/clipboard feature ownership where maintainers need it;
- YAML maintained-parser or compatibility-only disposition;
- canonical generated configuration formats;
- manual, bounded dependency maintenance;
- no scheduled/update automation requirement.

### 8.5 Binary topology

Document the accepted measured split/no-split result.

For a split:

- binary roles;
- installation and invocation;
- daemon discovery/singleton behavior;
- manual release/package contents;
- compatibility command.

For no split:

- retain current usage docs;
- record the measured decision only in durable architecture/closure notes where useful;
- do not advertise a future split as planned work.

## 9. Test-count and verification documentation

Testing documentation currently contains fragile or inconsistent exact totals. Choose one of these bounded outcomes:

1. remove exact global test totals and describe test classes/commands; or
2. keep one clearly labeled generated snapshot with the exact command/date/revision and no second contradictory total.

Prefer removing exact totals unless they provide operational value. Per-target counts in closure records are acceptable because they are tied to a specific command/revision.

Reconcile:

- quick versus hosted verification;
- focused mechanism tests;
- optional/manual full checks;
- feature-specific checks;
- resource limits;
- serial test terminology;
- release-time-only package/audit checks.

No document may describe a multi-threaded command as serial.

## 10. CI and local verification review

The accepted target remains one routine hosted job.

Inspect `.github/workflows/ci.yml` and `scripts/verify.sh` for exact duplication. The current hosted job may run `cargo check`, Clippy, and tests that recompile overlapping targets. Remove a redundant explicit `cargo check` only when all conditions hold:

- Clippy/tests cover the intended workspace/all-target/default-feature compile boundary;
- failure diagnostics remain sufficiently actionable;
- the change demonstrably reduces duplicate work;
- the quick local contract remains clear;
- no closed roadmap requirement depends specifically on a separate check command.

Retaining `cargo check` is acceptable when actionability or target coverage justifies it. M008 must record the decision and must not force a change.

Do not alter:

- one-job topology;
- bounded build/test concurrency;
- static guard execution;
- no-artifact/no-release behavior;
- manual release ownership.

## 11. Static guard reconciliation

Inventory guards introduced or extended by this workstream:

- custom Landlock syscall/pre-exec boundary;
- canonical process ownership/unbounded output;
- argv reparsing;
- direct parser imports;
- dependency declarations where guarded;
- existing daemon cwd/core-boundary/Tokio guards.

For each guard:

- confirm its matcher failure is nonzero;
- confirm one narrow negative fixture or test;
- remove duplicated guards checking the same exact boundary;
- keep allow-lists small and reasoned;
- document owner and scope;
- avoid generalizing regex checks into a framework.

Do not require replaying negative fixtures on every closure after the guard is established and included in quick/hosted verification.

## 12. Expected production-code changes

Normally none.

Allowed narrowly scoped changes:

- fix a static guard that is fail-open or does not match its documented boundary;
- remove a proven redundant hosted command under Section 10;
- correct package/binary metadata or documentation path references;
- delete obsolete helper scripts used only by superseded verification/evidence machinery.

A substantive runtime defect discovered during M008 must produce one narrow corrective plan against the owning milestone. Do not repair broad production behavior inside the closure pass.

## 13. Storage, protocol, migration, and compatibility effects

Storage:

- no new migration;
- verify M005/M006 migration/compatibility documentation matches accepted behavior;
- do not rewrite user state.

Protocol:

- no new protocol change;
- verify execution/sandbox optional result fields and M007 binary behavior are documented accurately.

Compatibility:

- preserve current CLI/config/state/service behavior established by M001–M007;
- archive/de-registration changes must not break durable source links without updating them;
- release instructions remain manual.

## 14. Ordered work packages

### Work package A — Closure audit

1. inspect M001–M007 records against plan acceptance criteria;
2. identify only material missing/misleading evidence;
3. classify residual findings by severity;
4. stop and register a narrow corrective plan for any high/medium production defect.

### Work package B — Architecture/documentation reconciliation

1. update sandbox/security docs;
2. update process/argv/execution ownership docs;
3. update grep resource notes;
4. update dependency/parser maintenance docs;
5. update measured binary topology and manual release docs.

### Work package C — Testing and CI reconciliation

1. remove contradictory/fragile global test counts;
2. align quick/hosted/focused/full terminology;
3. inspect redundant `cargo check` without forcing removal;
4. confirm bounded one-job/no-release CI;
5. remove only obsolete evidence helpers.

### Work package D — Guard review

1. run existing guards;
2. prove fail-closed behavior through existing focused tests/fixtures;
3. deduplicate exact overlaps;
4. retain explicit ownership allow-lists.

### Work package E — Planning registry and archive

1. update roadmap milestone statuses;
2. update active registry to closed workstream disposition;
3. remove closed milestones from active/blocked tables;
4. retain one recent closure row;
5. de-register or narrowly archive completed implementation plans without broad historical churn.

### Work package F — Final proportional verification

1. run focused checks only for M008 changes;
2. run `scripts/verify.sh quick` once on the accepted revision;
3. use one existing hosted `verify` result on that executable tree;
4. do not run a duplicate local full workspace suite unless a concrete hosted failure requires reproduction.

## 15. Focused verification

Expected command shape:

```bash
cargo fmt --all -- --check
<all existing cheap static guards>
scripts/verify.sh quick
```

Run additional focused tests only for code/guard behavior changed in M008. Documentation-only changes do not require replaying M001–M007 mechanism suites.

Review the existing hosted `verify` job for the accepted SHA or an executable-identical planning-only descendant. One green run is sufficient broad evidence.

Do not run:

- package-by-package `cargo package` or `cargo publish --dry-run` absent a release;
- crates.io checks;
- `cargo bloat` again unless M008 changed executable code/manifests;
- every predecessor focused test;
- a second broad hosted workflow.

## 16. Acceptance criteria

M008 is complete only when:

- M001–M007 each have an accepted disposition and compact closure record;
- no unresolved critical/high/medium finding remains hidden;
- architecture/security/execution/search/dependency/parser/binary docs match accepted behavior;
- exact global test-count contradictions are removed or reduced to one revision-scoped source;
- quick/hosted/focused/full terminology is consistent;
- static guards are owned, scoped, fail closed, and non-duplicative;
- routine CI remains one bounded non-release job;
- any `cargo check` retention/removal decision is evidence-based and documented;
- manual release cadence and publication remain unchanged;
- `plans/registry.md` is compact and marks this workstream closed;
- completed plans are de-registered or narrowly archived without deleting history;
- one `scripts/verify.sh quick` and one hosted `verify` result cover the accepted tree;
- no new evidence framework, CI lane, release automation, size gate, or broad production refactor is introduced.

## 17. Stop conditions

Stop and register one narrow corrective plan when:

- a closure record reveals an unresolved high/medium production or compatibility defect;
- documentation cannot be made truthful without changing runtime behavior;
- a static guard is fail-open and repair changes a material authority boundary;
- M007 split topology introduced incompatible state/protocol ownership;
- a claimed migration cannot be reproduced safely.

Do not create follow-up plans for:

- preferred wording;
- an optional benchmark;
- a desire for additional platform matrices;
- release-time package evidence;
- a small historical formatting inconsistency;
- lack of duplicate broad verification.

## 18. Required closure evidence

`plans/closure/runtime-safety-resource-footprint/008-status.md` must include:

- accepted commit/PR;
- M001–M007 final disposition links;
- material documentation changes;
- test-count/verification reconciliation result;
- guard ownership/fail-closed summary;
- CI command/topology decision;
- registry/archive disposition;
- focused commands and outcomes;
- one quick result and one hosted run reference;
- unresolved findings by severity;
- confirmation that manual release ownership and minimal verification policy remain intact;
- recommendation that the workstream is closed or one named corrective plan is required.