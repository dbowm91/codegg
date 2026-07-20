# Domain Identity Milestone 004 — Closure and Legacy-Removal Criteria

Status: closed; see `plans/closure/domain-identity/004-status.md`

Repository baseline: `ab9d345` (`agent/project-catalog-lazy-activation-health`)

Source roadmap:

- `plans/subsystems/domain-identity-roadmap.md#milestone-4--closure-and-legacy-removal-criteria`

Long-term requirements:

- `plans/000-long-term-specification.md` — stable, path-independent project identity
- `plans/001-terminology-and-domain-model.md` — project, repository, workspace, and locator terminology
- `plans/002-long-term-roadmap.md#phase-0--canonical-domain-and-compatibility-foundation`
- `plans/003-planning-process.md` — closure, compatibility, and dependency rules

Applicable ADRs:

- None. The canonical documents already establish the identity and migration model.

Primary class: polish

## 1. Objective

Close the Domain Identity and Compatibility roadmap by consolidating evidence from
Milestones 1–3, classifying the remaining legacy identity projections, and
recording explicit removal prerequisites and owners. This is a closure and
governance milestone; it does not remove compatibility columns or implement the
Project Catalog protocol/server migration.

## 2. Why this milestone is ready

- Domain Identity 001 is closed in `f203ed9` with typed identity primitives and
  validation/guard seams.
- Domain Identity 002 is closed in `84d92f0` with additive project/repository
  storage, deterministic migration, and canonical binding tables.
- Domain Identity 003 corrective work is closed in `ec42dce` with daemon,
  protocol, session, and server adoption of canonical context.
- Representative migration, context, protocol, session, and workspace evidence
  is available and the current checkout passes the required focused checks.
- No unresolved architecture decision changes ownership or the canonical
  project/repository/workspace model.

## 3. Current implementation evidence

- `crates/codegg-core/src/identity.rs` owns typed identity validation and
  compatibility annotations.
- `crates/codegg-core/src/project_storage.rs` owns canonical project,
  repository, workspace, and session bindings plus additive migration/rebind
  behavior.
- `crates/codegg-core/src/context.rs` resolves explicit identity context and
  performs bounded directory compatibility lookup without deriving identity from
  path text.
- `crates/codegg-protocol/src/dto.rs` and `src/core.rs` carry additive
  `ProjectContextDto`/`SessionBindingDto` data with legacy defaults.
- Daemon, session, REST, WebSocket, and TUI compatibility paths consume the
  canonical context contract as recorded by the M003 corrective closure.
- Historical fields remain intentionally readable. `ServerState.project_dir`
  and the remaining single-project route locator surface are owned by Project
  Catalog 004, not removed here.

## 4. Invariants that must not regress

- Paths, directories, Git roots, and process cwd are locators, never durable
  project identity.
- New executable session work has a validated canonical project/workspace
  binding before execution.
- Legacy rows remain readable and inspectable; compatibility projections never
  override canonical bindings.
- Migration is additive, restart-safe, idempotent, and conservative when
  repository lineage is ambiguous.
- Compatibility-only requests fail with bounded actionable diagnostics when no
  unique canonical context exists.
- Identity parsing does not grant authorization.

## 5. Scope

### In scope

- Requirement-to-evidence closure matrix for the Domain Identity roadmap.
- Representative migration and restart evidence review.
- Inventory of remaining legacy fields, rows, owners, and removal prerequisites.
- Review of downstream dependencies and precise registry/roadmap status updates.
- Closure record and architecture/planning documentation.

### Explicitly out of scope

- Removing `project`, `session.project_id`, `session.workspace_id`, or
  `session.directory` columns.
- Removing `ServerState.project_dir` before Project Catalog 004 completes its
  project-scoped protocol/server migration.
- Full project catalog REST/WS protocol work, TUI tabs, session projections,
  authorization, remote execution, or new identity types/stores.
- Reinterpreting ambiguous legacy rows or silently rewriting migration data.

## 6. Required production changes

No production-code changes are required for this milestone. The production
boundary was established by the closed M001–M003 implementation commits. The
required changes are evidence and status updates:

### Core/domain and storage

- Confirm canonical project/repository/workspace/session bindings and additive
  migration behavior remain the authority.
- Classify unresolved, ambiguous, archived, and legacy rows without deleting or
  guessing over them.

### Protocol and runtime

- Confirm additive identity-bearing DTOs, capability negotiation, and bounded
  compatibility diagnostics remain in place.
- Keep daemon and scheduler authority unchanged.

### Documentation and static guards

- Record legacy owners and removal gates in the closure record.
- Update the subsystem roadmap and active registry only after evidence review.
- Retain the identity-path, daemon-CWD, core-boundary, and execution-ownership
  guards as release gates.

## 7. Ordered work packages

### Work package A — Consolidate implementation evidence

Intent: prove the Phase 0 identity boundary from the M001–M003 closure records
and current focused verification.

Acceptance evidence: every roadmap exit condition maps to a landed implementation,
test, migration fixture, guard, or documented compatibility result.

### Work package B — Classify legacy compatibility surface

Intent: name the owner and removal prerequisites for each historical identity
field or locator without removing it prematurely.

Acceptance evidence: the closure record lists `project`, session compatibility
fields, `ServerState.project_dir`, and directory-only request compatibility with
explicit owners and gates.

### Work package C — Audit downstream readiness

Intent: determine whether closing this milestone changes any future plan status.

Acceptance evidence: direct dependencies are checked; only plans whose hard
dependency is actually closed are updated, while Project Catalog 004, TUI 001,
and Session Projections 001 retain their named blockers.

### Work package D — Record formal closure

Intent: mark this plan, the subsystem roadmap, and registry consistently.

Acceptance evidence: a closed status record links the implementation evidence,
verification results, limitations, compatibility gates, and downstream decision.

## 8. Failure, cancellation, restart, and contention semantics

This milestone adds no runtime operation. The reviewed semantics remain:

- additive migrations are restart-safe and idempotent;
- concurrent registration converges through canonical uniqueness and binding
  transactions;
- ambiguous or missing directory compatibility produces a bounded context
  error rather than a new project;
- failed binding prevents executable session creation or is compensated before
  returning failure;
- legacy storage reads remain available while daemon execution requires resolved
  context.

## 9. Compatibility and migration

The following compatibility surface remains intentionally active:

| Surface | Owner | Removal prerequisites |
|---|---|---|
| Historical `project` table and path-valued provenance | Project storage/catalog migration | All durable consumers use canonical catalog/binding records; unresolved rows have an explicit operator disposition; rollback/read compatibility is no longer needed. |
| `session.project_id`, `session.workspace_id`, `session.directory` | Session storage and protocol compatibility | All supported clients consume `SessionBindingDto`; existing rows are backfilled or explicitly unbound; daemon/server reads and writes no longer require the projections; an additive rollback path is retained through the removal release. |
| `ServerState.project_dir` and single-project route locators | Project Catalog 004 | Project-scoped REST/WS operations use catalog `ProjectId` plus explicit workspace/locator context; default-locator compatibility is documented and tested; no route treats the field as identity. |
| Directory-valued `CoreRequest::SessionList.project_id` compatibility input | Daemon compatibility adapter | Capability negotiation shows stable-ID client adoption; old clients receive a bounded context-required/unsupported response; no production write path accepts it as authoritative identity. |

No field is removed by this plan. Removal requires a later owner plan with
backfill/rollback evidence and an explicit compatibility deprecation decision.

## 10. Required tests

### Focused unit and integration tests

- Context resolution and canonical session binding tests.
- Identity, project-storage, project-catalog, protocol, and session tests.
- Storage migration and workspace isolation fixtures.

### Restart, contention, and compatibility tests

- Idempotent migration/restart fixtures and canonical binding persistence.
- Unique/ambiguous/missing directory compatibility outcomes.
- Old and identity-aware protocol fixture decoding.

### Static and negative tests

- Identity path-derived construction negative fixture.
- Core-boundary, daemon-CWD, execution-ownership, and project-catalog guards.

The broad capped workspace result is inherited from the M003 corrective closure;
this milestone has no production-code delta requiring a second full-suite run.

## 11. Required verification commands

```bash
rtk cargo test -p codegg-core --test context -- --test-threads=1
rtk cargo test -p codegg-core session -- --test-threads=1
rtk cargo test -p codegg-protocol -- --test-threads=1
rtk cargo test --test storage_migrations -- --test-threads=1
rtk cargo test --test workspace_isolation -- --test-threads=1
rtk cargo test --test workspace_services_isolation -- --test-threads=1
rtk cargo fmt --all -- --check
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_execution_ownership.py
rtk python3 scripts/check_identity_path_usage.py
rtk python3 scripts/check_project_catalog_invariants.py
```

## 12. Documentation updates

- Add this closure-oriented implementation handoff.
- Add `plans/closure/domain-identity/004-status.md`.
- Mark Milestone 4 and the Domain Identity roadmap closed.
- Update `plans/registry.md` and preserve unrelated blockers.

## 13. Acceptance criteria

- All Domain Identity Phase 0 exit conditions are evidenced by closed M001–M003
  records and current focused verification.
- Representative existing-database migration behavior is restart-safe,
  idempotent, conservative, and tested.
- Unresolved/ambiguous legacy rows and every remaining compatibility field have
  a named owner and explicit removal prerequisites.
- No silent path-derived project identity fallback remains in production writes.
- The identity, daemon-CWD, core-boundary, project-catalog, and execution-
  ownership guards pass.
- The registry and roadmap accurately state that no unrelated future plan became
  unblocked by this closure.

## 14. Stop conditions

Stop and report instead of closing if migration evidence is absent, a high or
critical identity/security finding remains, a canonical invariant is contradicted,
or closure would require taking ownership from Project Catalog, TUI, session
projection, or authorization workstreams.

## 15. Closure evidence required

The closure record must include the requirement matrix, exact commands and
outcomes, inherited M001–M003 evidence, legacy inventory and removal gates,
security/compatibility limitations, unresolved findings, and downstream status
disposition.

## 16. Handoff notes

This is a documentation/evidence milestone. Do not delete compatibility data,
invent a new identity model, or author Project Catalog 004 as a side effect.
Keep the repository's capped test-resource guidance and preserve unrelated
working-tree changes.
