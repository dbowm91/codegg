# Provider Connections Milestone 007 — Independent Closure Ratification and Governance Reconciliation

Status: implemented

Repository baseline reviewed:

- current reviewed `main`: `1abb2e2c3c4f8c7480fb74b780b80eb3485ff1f9`
- Provider M006 executable correction: `139c832c986106f31304d845860a66b17ba17099`
- Provider M006 evidence/closure commit: `f701925ccc3089d4bdc160367886a530ec1f1ffb`
- Provider branch merge/reconciliation head: `8eddda26c417043c1ce0a9112df98beff2edeba1`
- Provider merge to `main`: `7d8657e60aad85f677144b1bd0e7fb5d2929faa3`
- green hosted implementation run: `30603541350`, job `91071065732`

Source control documents:

- `plans/subsystems/provider-connections-roadmap.md`
- `plans/subsystems/provider-connections-storage-verification-reconciliation-addendum.md`
- `plans/subsystems/provider-tool-dvr-independent-closure-ratification-addendum.md`
- `plans/implementation/provider-connections/006-storage-layout-assertion-and-verification-reconciliation.md`
- `plans/closure/provider-connections/006-status.md`
- `plans/closure/development-verification-release/006-stop-condition.md`

Target independent closure record:

- `plans/closure/provider-connections/007-status.md`

Primary class: independent evidence ratification and closure-governance repair

Secondary class: downstream DVR dependency correctness

## 1. Objective

Perform an independently attributable review of Provider Connections M006 after its executable correction and closure record were merged, then either ratify the provider subsystem as strictly closed or restore a truthful non-closed state with one narrowly owned follow-up.

M007 exists because the M006 executable correction is technically narrow and well evidenced, but the strict closure record was authored on the same implementation branch and merged without independently attributable review. M007 must repair that process defect without reopening provider architecture, migration design, CI topology, or release scope.

M007 must produce all of the following outcomes:

1. independently inspect the exact M006 executable diff and merged lineage;
2. independently confirm that the `35` versus `33` failure was a stale global-layout assertion rather than a production migration defect;
3. verify that the assertion now derives from `crate::storage::STORAGE_LAYOUT_VERSION` and no production migration SQL or provider behavior changed;
4. confirm migration idempotency, provider CRUD, stale-revision rejection, and compatibility remain intact on an accepted descendant of the merged revision;
5. validate the hosted evidence and determine whether subsequent executable changes require fresh current-head evidence;
6. classify `plans/closure/provider-connections/006-status.md` as historical implementation-authored evidence rather than independently sufficient approval;
7. create `plans/closure/provider-connections/007-status.md` only from an agent/reviewer that did not author the M006 implementation or M006 closure commit;
8. update provider planning state and downstream DVR dependency state only after the independent disposition is complete;
9. avoid any production change unless review demonstrates a real provider/storage defect, in which case M007 must stop and register one narrow corrective implementation plan.

This is a review-and-ratification milestone. The expected executable diff is empty.

## 2. Trigger and current discrepancy

The M006 executable correction changed one test assertion:

```rust
assert_eq!(version, crate::storage::STORAGE_LAYOUT_VERSION as i64);
```

The correction is consistent with the current global migration contract:

- provider storage was introduced historically before the current terminal layout;
- later repository-wide migrations advance the shared schema through version 35;
- the test invokes the global migration dispatcher twice;
- a global terminal-version assertion must follow `STORAGE_LAYOUT_VERSION`, not a copied historical provider migration literal.

The associated hosted run succeeded on the implementation revision, and the branch was later reconciled with `main` and merged through PR #69. However:

- `plans/closure/provider-connections/006-status.md` was created by the same implementation branch;
- the record asserts that implementation and closure review were separate passes without independently attributable repository review evidence;
- PR #69 carried no requested reviewer or review discussion establishing that independence;
- the registry therefore overstates Provider M006 as strictly closed.

M007 owns only this closure-governance discrepancy and the evidence needed to decide it correctly.

## 3. Governing invariants

### Review independence

- The M007 reviewer must not be the agent that authored commit `139c832c986106f31304d845860a66b17ba17099`.
- The M007 reviewer must not be the agent that authored commit `f701925ccc3089d4bdc160367886a530ec1f1ffb`.
- The review must occur after the implementation has merged to `main`.
- The M007 closure record must identify the reviewed implementation, closure-evidence, merge, and accepted verification revisions.
- A second commit on the same implementation branch is not sufficient independence.
- Sharing the repository owner or GitHub credential is acceptable only when the closure record identifies a distinct review agent/pass that did not produce the implementation or provisional closure content.

### Storage and migration invariants

- `STORAGE_LAYOUT_VERSION` remains the canonical repository-wide terminal layout contract.
- Provider migration history remains historically accurate; M007 must not relabel the provider migration itself as version 35.
- Migrations v34 and v35 remain later repository-wide migrations.
- Global `migrate()` remains idempotent when invoked repeatedly.
- Provider tables, lifecycle metadata, secret references, revisions, scopes, and timestamps remain intact after all current migrations.
- No migration is added, removed, renumbered, reordered, or rewritten by this review milestone.

### Provider invariants

- Provider connection metadata remains credential-free.
- Secret ownership and opaque references remain unchanged.
- Endpoint, TLS, scope, lifecycle, health, selection, rotation, and revision semantics remain unchanged.
- Stale revisions remain rejected.
- No provider or Eggpool feature work is introduced.

### Verification invariants

- Existing green evidence must be inspected rather than merely copied.
- Any reused hosted run must be tied to the exact executable tree it proves.
- If the accepted review head contains executable changes after the green implementation run, the reviewer must obtain fresh relevant evidence for that descendant.
- No tests may be ignored, filtered, deleted, weakened, or converted to expected failure.
- CI topology, resource limits, thread count, and release behavior remain unchanged.

### Closure invariants

- M006's record remains historical evidence even if M007 ratifies the result.
- M007 is the independently attributable strict provider closure authority.
- Provider strict closure does not itself close Tool Programs M018 or DVR M006.
- DVR M006 remains blocked until both Provider M007 and Tool Programs M019 are independently closed.

## 4. Scope

### In scope

- commit and branch lineage for M006 and PR #69;
- the one executable assertion change in `crates/codegg-core/src/provider_connections.rs`;
- direct inspection of `crates/codegg-core/src/storage/mod.rs` and the current migration dispatcher;
- direct inspection of the named provider migration/CRUD/revision test;
- focused provider and codegg-core verification on the accepted review head;
- validation of hosted workflow run `30603541350`, job `91071065732`;
- detection of executable drift between the verified implementation commit, merge commit, and current review head;
- planning and closure-record reconciliation for Provider M006/M007 and DVR dependency state;
- creation of `plans/closure/provider-connections/007-status.md` by the independent reviewer if all criteria pass.

### Explicitly out of scope

- changing production migration behavior;
- adding or renumbering migrations;
- changing `STORAGE_LAYOUT_VERSION`;
- provider feature expansion;
- Eggpool routing or provider health work;
- credential, endpoint, TLS, scope, lifecycle, or selection changes;
- Tool Programs executable changes;
- DVR release documentation implementation;
- CI expansion, retries, resource increases, or test exclusions;
- release execution or crate publication;
- cosmetic rewriting of historical M001–M006 evidence beyond truthful status clarification.

## 5. Required review before disposition

### 5.1 Establish exact lineage

Record and inspect:

```text
M007 reviewed baseline: 1abb2e2c3c4f8c7480fb74b780b80eb3485ff1f9
M006 implementation:   139c832c986106f31304d845860a66b17ba17099
M006 closure evidence: f701925ccc3089d4bdc160367886a530ec1f1ffb
branch reconciliation: 8eddda26c417043c1ce0a9112df98beff2edeba1
merge to main:         7d8657e60aad85f677144b1bd0e7fb5d2929faa3
```

Confirm:

- the implementation commit contains only the provider test assertion plus implementation-state planning changes;
- the closure commit contains planning/evidence changes but no hidden production change;
- the merge retained the intended assertion;
- no conflict resolution altered provider or migration semantics;
- subsequent commits between the merge and reviewed head are classified as executable or planning-only.

### 5.2 Audit the executable change

Inspect the exact diff against the M006 baseline and confirm:

- the historical literal `33` was replaced with the canonical constant;
- the cast matches the queried schema-version type;
- the test still invokes the production global migration path twice;
- CRUD and stale-revision assertions still execute after the version assertion;
- no assertion was deleted or weakened;
- no production file outside the test module changed for M006.

### 5.3 Audit migration semantics

Read the current migration dispatcher and document:

1. the current value and meaning of `STORAGE_LAYOUT_VERSION`;
2. the historical provider migration version;
3. the purpose of migrations v34 and v35;
4. whether the global dispatcher always reaches the current terminal version from supported starting states;
5. whether a second invocation is a no-op or otherwise idempotent;
6. whether provider tables and lifecycle columns survive later migrations;
7. whether any legitimate current path should terminate at 33.

If any supported current path legitimately terminates at 33, stop and classify the issue as a production migration defect rather than ratifying M006.

### 5.4 Audit local evidence

At minimum run on the accepted review head:

```bash
cargo test -p codegg-core \
  provider_connections::tests::migration_is_idempotent_and_store_crud_is_revision_safe \
  -- --test-threads=1

cargo test -p codegg-core --locked -- --test-threads=1

cargo fmt --all -- --check
git diff --check
```

Also run:

```bash
scripts/verify.sh quick
```

Run `scripts/verify.sh full` when either condition applies:

- executable drift exists between the last green full/hosted revision and the accepted review head; or
- the reviewer cannot prove executable identity for the provider/storage surface and canonical workspace test graph.

A planning-only descendant may reuse earlier executable evidence only when the closure record proves the relevant tree identity and cites the exact comparison.

### 5.5 Audit hosted evidence

Inspect run `30603541350`, job `91071065732` and confirm:

- conclusion is success;
- checkout SHA is `139c832c986106f31304d845860a66b17ba17099`;
- formatting, workspace check, Clippy, and workspace tests all completed successfully;
- no relevant step was skipped or allowed to fail;
- the run used the canonical one-job read-only workflow.

If the accepted review head has executable drift, require one successful hosted `verify` run for the accepted executable revision or an executable-identical planning successor.

### 5.6 Audit closure governance

Document explicitly:

- why M006's closure record is considered provisional/historical evidence;
- the identity or role of the M007 independent reviewer;
- confirmation that the reviewer did not author M006 implementation or provisional closure;
- the review branch/commit used for ratification;
- whether any high- or medium-severity finding remains.

## 6. Allowed dispositions

### Disposition A — Strict ratification

Use only when all technical and governance criteria pass.

Required actions:

- create `plans/closure/provider-connections/007-status.md`;
- mark M006 as historically implemented with provisional self-authored closure evidence;
- mark M007 strictly closed by independent review;
- mark Provider Connections closed through M007;
- update registry and the cross-subsystem ratification addendum;
- leave Tool Programs and DVR closure states unchanged except that Provider M007 dependency becomes satisfied.

### Disposition B — Conditional ratification

Use when the implementation is correct but one bounded evidence item remains, such as current-head hosted evidence.

Required actions:

- create a conditional M007 status record identifying the exact missing evidence;
- keep Provider Connections `closing`;
- keep DVR M006 blocked;
- register no new production plan unless executable correction is required;
- identify the exact evidence-only follow-up.

### Disposition C — Rejection and corrective implementation

Use when review finds a real migration, provider, test-isolation, or compatibility defect.

Required actions:

- do not create a strict M007 closure record;
- mark M006/M007 conditionally closed or blocked as appropriate;
- record exact failure evidence;
- register one narrow provider/storage corrective implementation plan;
- leave DVR M006 blocked;
- do not absorb the defect into Tool Programs or DVR scope.

## 7. Ordered work packages

### Work package A — Independence and lineage freeze

Actions:

1. identify the independent reviewer;
2. record the exact reviewed head;
3. inspect M006 implementation, closure, reconciliation, and merge commits;
4. classify all later changes affecting executable identity.

Acceptance:

- the reviewer did not author M006 implementation or provisional closure;
- every relevant SHA is recorded;
- no merge conflict or later drift is assumed away.

### Work package B — Migration-contract inspection

Actions:

1. inspect the global layout constant;
2. inspect migration ordering through the current terminal version;
3. inspect the provider test and its global dispatcher use;
4. determine stale assertion versus real defect independently.

Acceptance:

- the conclusion is supported by current source, not the M006 narrative alone;
- provider migration history is described accurately;
- any production defect triggers the stop condition.

### Work package C — Focused regression evidence

Actions:

1. run the named provider test;
2. run all `codegg-core` tests;
3. run formatting and diff checks;
4. run quick verification;
5. run full verification if executable identity requires it.

Acceptance:

- all required commands exit zero;
- no test is skipped or weakened;
- exact revision and exit status are recorded.

### Work package D — Hosted evidence audit

Actions:

1. inspect the original green run;
2. verify exact checkout SHA and successful steps;
3. determine whether later executable drift requires a new run;
4. obtain and record a new run when required.

Acceptance:

- hosted evidence proves the accepted executable revision;
- no stale or unrelated run is represented as current proof.

### Work package E — Independent closure disposition

Actions:

1. classify M006 closure evidence as provisional history;
2. write the independent M007 disposition;
3. reconcile provider roadmap/addendum and registry;
4. preserve Tool Programs and DVR ownership boundaries.

Acceptance:

- `plans/closure/provider-connections/007-status.md` exists only if the independent review supports it;
- the registry no longer relies on M006's self-authored closure as the strict authority;
- DVR remains blocked until Tool Programs M019 also closes.

## 8. Required closure evidence matrix

The M007 status record must contain at least:

| Requirement | Source/command | Revision | Result | Notes |
|---|---|---|---|---|
| Reviewer independence | reviewer declaration and commit lineage | review SHA | pass/fail | no M006 authorship |
| Executable diff scope | commit comparison | `139c832c` | pass/fail | one test assertion only |
| Merge integrity | comparison to `7d8657e` | merge SHA | pass/fail | no semantic conflict |
| Migration terminal contract | source inspection | review SHA | pass/fail | current global version |
| Migration idempotency/provider CRUD | focused test | review SHA | pass/fail | two migrations + CRUD |
| codegg-core regression | package test | review SHA | pass/fail | exact totals |
| Quick verification | `scripts/verify.sh quick` | review SHA | pass/fail | exit code |
| Full verification | full run or identity proof | accepted executable SHA | pass/fail | rationale if reused |
| Hosted verify | workflow run/job | accepted executable SHA | pass/fail | all required steps |
| Planning ownership | records and registry | closure SHA | pass/fail | M007 strict authority |

No row may be marked pass when the referenced command or check is incomplete.

## 9. Registry and planning updates

Before M007 review completes:

- Provider Connections status: `closing`;
- current provider milestone: M007 ready/active;
- M006 status record: provisional historical evidence;
- DVR M006: blocked on Provider M007 and Tool Programs M019;
- no provider implementation plan is ready.

After strict M007 ratification:

- Provider Connections status: `closed`;
- current provider milestone: M007 closed;
- recently closed table links `plans/closure/provider-connections/007-status.md`;
- Provider M007 is removed from dependency-ready work;
- DVR remains blocked only on Tool Programs M019 if that review is still open.

## 10. Stop conditions

Stop and report rather than broadening scope when:

- migration semantics do not support the stale-assertion conclusion;
- current migration ordering or idempotency is defective;
- provider CRUD or stale-revision behavior fails;
- merge reconciliation changed provider/storage semantics;
- accepted-head executable drift lacks green local/hosted evidence;
- independent reviewer attribution cannot be established;
- resolving a finding requires production changes;
- CI or test resource changes are proposed;
- Tool Programs or DVR product work is proposed.

For a stop condition:

1. preserve valid evidence already gathered;
2. record exact SHA, command, exit code, and minimal failure output;
3. classify the owning subsystem;
4. create at most one narrow corrective implementation plan;
5. keep Provider M007 and DVR M006 non-closed.

## 11. Completion definition

M007 is strictly complete only when:

- an independently attributable reviewer has inspected the merged M006 work;
- the stale-assertion diagnosis is independently confirmed from source;
- migration ordering and idempotency are correct;
- provider CRUD and revision safety pass;
- the executable diff remains test-only and canonical;
- focused/package/quick evidence is green on the accepted review head;
- full and hosted evidence prove the accepted executable revision;
- M006's self-authored status is retained only as historical/provisional evidence;
- `plans/closure/provider-connections/007-status.md` records the independent decision;
- no unresolved high- or medium-severity provider finding remains;
- Provider closure does not prematurely close Tool Programs or DVR.

## 12. Handoff guidance

This plan must be assigned as a review task, not to the M006 implementation agent.

The reviewer should begin with lineage and source inspection, not by editing code. The expected repository changes are evidence and planning records only. A production diff is evidence that this plan has encountered a stop condition and must transfer work to a separately registered corrective implementation milestone.
