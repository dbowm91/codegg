# Eggplan Assessment Integration M003 C001 — Dirty-Subject Provenance and Bound Evidence Requalification

Status: closed (`plans/closure/eggplan-assessment-integration/005-m003-c001-status.md`;
implementations `36ec9322` and `6fd8d9f3`; Eggplan pin
`0dd33b761e85f1364320a9208aaebd5be281c6a5`)

Repository baseline:

- CodeGG reviewed planning baseline before corrective registration: `ffbd0bc9de09055fbd2df293a6cdd8b4b04a8e98`
- historical M003 implementation: `53dea47f414641c3f9756c3f8f181f6208be8115`
- historical M003 closure: `aa21cfe1763d7ea00d11e582ed4108233e4088a9`
- Eggplan M003 pin (historical, still the M003 closure pin):
  `3f7c603315131bb169bfdd2bb575531d228532b1`
- Eggplan C001 planning registration:
  `ee92edc1cf035010fe86ea2382694cc18a0dce45`
- Eggplan C001 fingerprint contract implementation:
  `352a0f782b0166aad8e850185019d78ec162d487`, portability repairs `faa6c87`
  and `0dd33b7`
- Eggplan C001 revision consumed by this implementation:
  `0dd33b761e85f1364320a9208aaebd5be281c6a5` (hosted `37063328954`)

Source roadmap:

- `plans/subsystems/eggplan-assessment-integration-roadmap.md`

Historical M003 plan:

- `plans/implementation/eggplan-assessment-integration/004-repository-plan-binding-and-writeback.md`

Coordinated Eggplan corrective:

- `eggstack/eggplan:plans/implementation/codegg-integration/003-c001-dirty-subject-fingerprint-and-bound-evidence-requalification.md`

Primary class: post-closure correctness corrective / execution provenance / exact-subject authority

## 1. Finding

M003 correctly avoided pretending CodeGG's native dirty digest and Eggplan's
repository dirty digest were byte-comparable.

However, the bound evidence translator currently copies the CodeGG-native
historical dirty digest into an Eggplan `SubjectRevision`.

That is not a valid exact-subject projection:

- CodeGG `egggit` uses its own domain-separated manifest digest;
- Eggplan `eggplan-repo` uses a different canonical dirty manifest;
- Eggplan assessment compares the full `SubjectRevision`, including
  `dirty_digest`.

Therefore a stable dirty CodeGG execution cannot reliably satisfy exact-subject
matching against a bound Eggplan repository Plan.

The existing M003 test
`dirty_same_subject_binds_and_mirror_acceptance_never_satisfies` covers only
binding. The execution/evidence/closure tests use clean provenance.

A second issue exists at binding time: Eggplan capture and CodeGG capture are
sequential, but only HEAD + clean/dirty state are compared. Dirty contents may
change between captures while both snapshots remain dirty.

Historical M003 closure remains historical evidence. C001 owns this later
correctness defect.

## 2. Objective

For dirty Git executions, persist the exact Eggplan-compatible dirty digest at
execution time alongside CodeGG's native provenance, then use that captured
digest for bound Eggplan subject translation.

Also harden binding identity proof with an Eggplan fingerprint sandwich so a
dirty content change during cross-owner capture fails closed.

Preserve:

- CodeGG's native `egggit` digest and existing M001/M002 semantics;
- `codegg-core` with no Eggplan crate dependency;
- SQLite v68 layout;
- historical v1 provenance readability;
- clean M003 behavior;
- repository-first lifecycle/closure ordering.

## 3. Hard dependency and parallelism

Eggplan C001 must first land a qualified public fingerprint API whose output is
exactly its existing repository subject revision/state/dirty-digest capture.

CodeGG may implement the nested provenance schema and fail-closed translation
logic in parallel, but it MUST NOT pin or claim the new dirty path until the
Eggplan implementation revision is immutable and hosted-qualified.

After Eggplan C001 lands:

1. update the exact `eggplan-core` / `eggplan-codegg-compat` /
   `eggplan-repo` pin together;
2. consume the fingerprint API only in the application layer;
3. run the dirty end-to-end matrix;
4. close/reconcile both repositories.

## 4. ExecutionSubjectRevision v2

The existing SQLite column `job_attempt.source_subject_json` is sufficient;
no database migration is required.

Evolve the nested JSON object to schema v2.

Recommended additive field:

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eggplan_dirty_digest: Option<String>

Semantics:

### v1

Historical current shape:

- native `dirty_digest` only;
- remains parseable/valid;
- clean v1 may still translate to a bound Eggplan subject;
- dirty v1 cannot satisfy bound exact-subject evidence because it lacks the
  Eggplan-native digest.

### v2

- retains the CodeGG-native `dirty_digest`;
- may carry `eggplan_dirty_digest` captured from the Eggplan fingerprint API;
- clean subjects require both dirty digests absent;
- dirty subjects require native `dirty_digest`;
- `eggplan_dirty_digest`, when present, must match Eggplan's
  `sha256:<64-lowercase-hex>` form.

Do not overwrite `dirty_digest` with Eggplan bytes.

`ExecutionSubjectProvenance::SCHEMA_VERSION` may remain unchanged if the
envelope semantics are unchanged; the nested revision version is the authority.

Validation MUST accept historical v1 and current v2 explicitly and reject
unknown versions.

## 5. Capture ownership

Do not introduce a second scheduler/Git owner.

Create one application-level capture helper used by the existing authoritative
attempt-start and attempt-seal paths.

Conceptually it returns:

    ExecutionSubjectRevision v2 {
        native CodeGG subject,
        optional Eggplan-compatible dirty digest
    }

The helper:

1. performs the governed native `egggit` capture;
2. when the canonical workspace contains the repository-local `.eggplan`
   administrative root, captures the Eggplan fingerprint with that root
   excluded;
3. requires equal revision and clean/dirty state between the two captures
   before attaching the Eggplan digest;
4. persists no paths/content.

If the Eggplan fingerprint is unavailable:

- native CodeGG provenance may remain available for unbound M002 use;
- the Eggplan-compatible field stays absent;
- later bound dirty translation fails closed.

Do not convert a current worktree fingerprint into historical provenance after
the attempt.

## 6. Start/seal stability

Attempt start and seal must capture v2 through the same helper.

A stable dirty attempt used by a bound repository Plan requires:

- captured native revision == sealed native revision;
- captured native dirty digest == sealed native dirty digest;
- captured Eggplan dirty digest == sealed Eggplan dirty digest.

Because `ExecutionSubjectRevision` equality participates in the existing
Stable/Drifted decision, the v2 field should naturally make an Eggplan-digest
change produce Drifted.

Add explicit tests; do not rely only on derived equality.

## 7. Binding-time sandwich proof

Replace the M003 two-owner check with:

    E1 = Eggplan fingerprint
    C  = CodeGG native capture excluding .eggplan
    E2 = Eggplan fingerprint

Require:

- E1 == E2 exactly;
- C.revision == E1.revision;
- CodeGG clean/dirty state == E1.state.

For a dirty binding, persist E1/E2's Eggplan dirty digest as the binding's
Eggplan-side proven digest.

The existing CodeGG-native dirty digest may still be persisted for diagnostics,
but it is not compared to Eggplan's digest.

Typed failure for E1 != E2:

    repository_subject_changed_during_identity_proof

or equivalent.

This closes the identified same-HEAD/same-dirty-classification TOCTOU gap.

## 8. Bound historical translation

Change only the M003 bound translator.

### Clean

v1 or v2 clean provenance translates as today:

- repository ID from the proven binding;
- exact historical revision;
- Clean;
- no dirty digest.

### Dirty v2

Require `eggplan_dirty_digest`.

Construct the bound Eggplan `SubjectRevision` with:

- Eggplan repository ID from the proven binding;
- historical revision;
- Dirty;
- `eggplan_dirty_digest`.

Never insert the CodeGG-native `dirty_digest` into a bound Eggplan subject.

### Dirty v1 / missing projection

Fail closed with a stable error, recommended:

    legacy_dirty_subject_missing_eggplan_digest

Do not:

- re-capture current source;
- derive from CodeGG native digest;
- mark it clean;
- fall back to unbound `EggplanGit`;
- fall back to legacy assessment.

## 9. M002 compatibility

M002 transient `EggplanGit` is a different namespace model: both transient
current subject and historical observations are CodeGG-derived.

Do not silently switch M002's existing native subject semantics as part of
C001.

The new Eggplan-compatible field exists specifically to cross the M003
repository-owned subject boundary.

Add regression tests proving:

- unbound clean M002 unchanged;
- unbound dirty M002 unchanged;
- legacy v1 provenance still behaves exactly as before outside repository
  binding.

## 10. Evidence writeback

For `EggplanRepositoryBound` terminal evidence:

- require Stable provenance;
- for dirty subjects require v2 + Eggplan digest;
- translate using that digest;
- preserve authoritative verification digest/provider/status rules from M002;
- append idempotently as before.

Observation IDs/timestamps/artifact refs are unchanged.

A missing Eggplan digest is subject-unavailable, not failed execution.

## 11. Dirty end-to-end qualification

Add a real dirty-worktree test matrix.

At minimum:

### Stable dirty pass

1. create repository Plan requiring a Test observation;
2. make a non-`.eggplan` source file dirty;
3. bind the Plan;
4. execute/capture a passing TestJob without modifying source;
5. verify v2 provenance has both digests;
6. sync terminal evidence;
7. assert observation subject dirty digest equals
   `RepositoryStore::subject_source().capture().dirty_digest`;
8. complete the repository item;
9. guarded-close the Plan while the same dirty state is stable;
10. reconcile CodeGG mirror Completed.

### Dirty content changed during attempt

- mutate dirty source between start and seal;
- provenance becomes Drifted;
- no passing observation is appended;
- item/Plan cannot complete.

### Dirty content changed during binding proof

Use a deterministic internal test seam around the three captures:

- E1 = dirty A;
- C = dirty B;
- E2 = dirty B or E2 != E1;
- binding fails with the typed changed-during-proof error;
- no mirror/binding rows are created.

Do not expose the seam as a public authority injection API.

### Legacy dirty provenance

- construct/read v1 dirty provenance;
- bound writeback refuses it;
- no current-worktree backfill occurs.

### Clean regression

Existing clean evidence/writeback/closure matrix remains green.

## 12. Provenance persistence/restart

Add JSON tests for:

- historical v1 parse/validate/roundtrip;
- v2 clean;
- v2 dirty with Eggplan digest;
- malformed Eggplan digest;
- unknown schema version;
- v2 start/seal equality;
- v2 drift.

SQLite `source_subject_json` roundtrip and daemon restart must preserve the
new field.

No v69 migration is expected because the physical column already stores
versioned JSON. If implementation proves a DB migration is necessary, stop and
register it explicitly rather than silently widening C001.

## 13. Static ownership guards

Update `scripts/check_execution_subject_ownership.py` and
`scripts/check_work_plan_repository_binding.py` to prove:

- all authoritative execution capture uses the approved v2 helper;
- raw Eggplan fingerprint capture is not scattered through tools/arbiter;
- bound dirty translation cannot read `ExecutionSubjectRevision.dirty_digest`
  as the Eggplan digest;
- no current-worktree historical backfill helper exists;
- `codegg-core` still has no Eggplan crate dependency.

## 14. Documentation cleanup

C001 also owns the planning drift found during audit.

Update CodeGG:

- subsystem roadmap status from fully closed to historical M001-M003 closed +
  C001 ready/active;
- registry subsystem/gate row similarly;
- architecture/work_plan.md dirty identity semantics;
- architecture/storage.md nested provenance schema v2;
- historical M003 closure only by additive follow-up note/reference; do not
  rewrite its evidence.

Eggplan separately fixes its stale M003 implementation-plan header and
external-interface baseline under its C001 registration.

## 15. Verification

At minimum:

    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo test -p codegg-core --lib -- execution_subject
    cargo test -p codegg-core --test work_plan_foundation
    cargo test -p codegg --test work_plan_repository_binding
    cargo test -p codegg --test work_plan_eggplan_differential
    cargo test --test work_plan_resolved_evidence
    cargo test --test work_plan_projection_arbiter
    cargo test --test long_horizon_trajectory_qualification
    cargo test --test scheduler_authority_matrix
    python3 scripts/check_execution_subject_ownership.py
    python3 scripts/check_work_plan_repository_binding.py
    python3 scripts/check_execution_ownership.py
    bash scripts/check-core-boundary.sh
    ./scripts/verify.sh quick
    git diff --check

Run the exact focused dirty E2E tests explicitly in closure evidence.

Hosted canonical CodeGG CI must be green on a head containing the C001
implementation and the exact qualified Eggplan C001 pin.

## 16. Acceptance criteria

C001 closes when:

1. CodeGG pins the qualified Eggplan fingerprint API revision;
2. ExecutionSubjectRevision v1 remains readable;
3. v2 can persist an Eggplan-native dirty digest without replacing CodeGG's
   native digest;
4. no DB migration or historical backfill is introduced unless separately
   planned;
5. binding identity proof uses E1/C/E2 and detects dirty change during proof;
6. bound dirty translation uses only the persisted Eggplan digest;
7. v1/missing dirty projection fails closed;
8. dirty stable execution -> observation -> item completion -> guarded closure
   passes end to end;
9. dirty attempt drift never becomes passing evidence;
10. M002 unbound behavior is unchanged;
11. ownership/static guards and full regression suite pass;
12. both repositories reconcile the corrective with exact implementation,
   pin, and hosted run IDs.

## 17. Stop conditions

Stop and report if:

- the solution changes Eggplan historical digest bytes;
- historical v1 dirty attempts would need current-worktree reconstruction;
- `codegg-core` would need an Eggplan crate dependency;
- CodeGG native `dirty_digest` must be redefined globally;
- dirty evidence can pass without the persisted Eggplan-compatible digest;
- the E1/C/E2 seam must become a public authority injection API;
- a physical storage migration becomes necessary but is not separately
  registered.

## 18. Closure evidence

Create:

    plans/closure/eggplan-assessment-integration/005-m003-c001-status.md

Record:

- CodeGG implementation SHA(s);
- exact Eggplan C001 pin;
- v1/v2 provenance compatibility matrix;
- start/seal dirty stability matrix;
- E1/C/E2 binding proof cases;
- dirty observation exact-subject equality;
- dirty item completion + guarded closure E2E;
- legacy dirty fail-closed case;
- dependency graph / core-boundary proof;
- exact hosted run IDs;
- Eggplan-side C001 closure SHA.
