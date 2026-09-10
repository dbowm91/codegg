# Post-Implementation Maintainability Closure M001 — Authorization/Collaboration Physical Decomposition

Status: closed

Repository production baseline: `f04866b1d817eee06fb3e51e73cc8f7b6679ad86`

Planning roadmap commit: `00cd722440eaa70685e0494803af1bd50ae16f53`

Source roadmap:

- `plans/subsystems/post-implementation-maintainability-closure-roadmap.md#7-milestone`

Related completed roadmaps and closure evidence:

- `plans/subsystems/identity-authorization-audit-roadmap.md` and `plans/closure/identity-authorization-audit/001-status.md` through `005-status.md`
- `plans/subsystems/project-collaboration-roadmap.md` and `plans/closure/project-collaboration/001-status.md` through `003-status.md`
- `plans/subsystems/residual-runtime-consolidation-roadmap.md` and `plans/closure/residual-runtime-consolidation/001-status.md` through `003-status.md`
- `plans/subsystems/post-audit-maintainability-surface-roadmap.md`, especially the closed physical-decomposition precedent in `plans/implementation/post-audit-maintainability-surface/003-agent-runtime-physical-decomposition.md`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Applicable ADRs:

- No new ADR is required. Existing accepted ownership decisions remain authoritative. If implementation discovers that authorization, collaboration, daemon, scheduler, job, audit, or persistence ownership must change, stop and request a separately scoped architecture decision/corrective plan.

Primary class: polish

Hard dependencies: none outstanding. Identity/auth/audit and project collaboration are closed at the production baseline.

Closure record to create:

- `plans/closure/post-implementation-maintainability-closure/001-status.md`

## 1. Objective

Reduce source-level maintenance concentration in:

- `crates/codegg-core/src/authorization.rs`; and
- `crates/codegg-core/src/collaboration.rs`

by moving coherent existing responsibilities into narrowly named child modules while preserving the current canonical subsystem owners, public/durable contracts, runtime behavior, error semantics, authorization semantics, transaction boundaries, scheduler/job submission boundaries, audit/redaction behavior, and test behavior.

Also reconcile `plans/registry.md` so it remains a compact truthful planning control surface and contains no stale dependency-ready prose after this milestone closes.

This is deliberately a physical/source-organization refactor. It must make the implementation easier to navigate and reason about without introducing a second authorization engine, collaboration service, repository abstraction, workflow engine, state machine, or generic framework.

## 2. Why this milestone is ready

The preceding implementation tranche established and closed the semantic architecture:

- identity/auth/audit now has canonical typed identity, transport-derived principal/context, daemon authorization, durable audit storage, and instrumentation ownership;
- presence/observation consumes those established identities and capabilities;
- collaboration has durable authorized channels/messages and separately authorized structured actions;
- structured actions submit executable work through the existing job/scheduler boundary rather than executing from chat text;
- residual daemon decomposition removed stranded old team/shell-session paths and preserved one `CoreDaemon` composition/lifecycle owner.

The remaining concern is local physical concentration inside two accepted canonical implementations. That makes this work suitable for polish only after functional/correctness closure, consistent with `plans/003-planning-process.md`.

The implementer must not infer that a large file is defective. The plan is ready because there are visible coherent responsibility clusters that can potentially be moved without changing semantics. If current HEAD no longer contains those clusters, re-audit and stop rather than forcing this decomposition.

## 3. Current implementation evidence to verify before editing

Before modifying production code, inspect current HEAD and write a concise responsibility map for each target file. Do not use line count alone.

### 3.1 `authorization.rs` responsibility census

At minimum identify current code belonging to:

- canonical principal/identity/authority context types or conversions owned by authorization;
- capability and role taxonomy/mapping;
- operation/request descriptors and authorization matrix definitions;
- policy evaluation/default-deny decision logic;
- project/session/workspace scope resolution helpers;
- persistence/schema/query helpers, if present in this module;
- authentication/token/key/credential verification helpers, if present in this module;
- audit-safe authorization metadata shaping;
- error types/stable error-code mapping;
- test fixtures and policy/store/authentication test families.

For every proposed extraction, record what state/types it depends on and whether any caller exists outside `codegg-core`.

### 3.2 `collaboration.rs` responsibility census

At minimum identify current code belonging to:

- channel and message domain types;
- message/channel validation and bounds;
- durable channel/message persistence and ordering;
- membership/project binding checks owned here versus authorization-owned checks;
- structured `ChatActionKind` taxonomy and required-capability mapping;
- action payload validation, secret/control-character rejection, and bounds;
- action idempotency/conflict/status/reference logic;
- action/channel/message persistence and query helpers;
- audit-safe action/message metadata shaping;
- collaboration configuration and stable error-code mapping;
- focused unit tests for messages/storage/actions/idempotency/restart behavior.

### 3.3 Boundary census

Search direct callers and imports before deciding module visibility. In particular inspect:

- `src/core/daemon.rs` and daemon family modules;
- `crates/codegg-protocol/src/core.rs`;
- audit instrumentation/projection replay modules;
- TUI collaboration paths;
- integration tests under `tests/identity_*`, `tests/presence_*`, and `tests/collaboration_*`;
- documentation naming current source ownership.

The closure record must include the final before/after responsibility map. Descriptive line counts/file sizes may be included, but there is no numerical size target.

## 4. Invariants that must not regress

### Authorization and identity

- Transport-authenticated identity remains the source of request authority. DTO/request fields cannot mint authority.
- Default-deny behavior remains unchanged.
- The same operation descriptors map to the same capabilities/scopes/roles.
- Local-owner or equivalent privileged semantics remain exactly as currently defined; do not widen or narrow them as cleanup.
- Project/session/workspace mismatch behavior and stable denial/error codes remain unchanged.
- Revocation behavior remains unchanged.
- Authorization decisions remain attributable to the same canonical principal/project/session/workspace context.

### Collaboration

- Free-text message bodies never become execution commands by parsing, prefix, mention, inference, or module relocation.
- `ChatActionSubmit` or its current typed equivalent remains a separate explicit protocol operation.
- Structured actions require both ordinary chat authorization and their existing semantic capability.
- Executable chat actions submit through `JobSubmissionService`/the existing scheduler-owned boundary.
- Job references remain references; they do not create execution.
- Channel/message/project association, ordering, retention, visibility/privacy, and bounds remain unchanged.
- Existing idempotency uniqueness/conflict semantics remain unchanged across retries and restart.
- Secret-bearing or malformed action payloads remain rejected with zero privileged side effect.

### Storage, protocol, audit, and runtime

- `STORAGE_LAYOUT_VERSION` must not change.
- No migration is expected or permitted in this milestone.
- SQL statements may move but transaction boundaries, unique constraints, ordering, and conflict handling must not change semantically.
- No `CoreRequest`, `CoreResponse`, `CoreEvent`, DTO field, serde tag/default, serialized enum/string, capability version, or stable protocol error code changes.
- Audit events retain the same event names, causation/correlation, structural locators, authorization-decision linkage, and content/secret redaction.
- Projection/event publication and replay classification remain unchanged.
- Cancellation/task lifetime and lock acquisition ordering remain unchanged unless a correctness defect is independently proven; such a defect is out of scope and requires stop/report.
- No new background task is introduced.

### API and module ownership

- `authorization` remains the canonical authorization subsystem facade.
- `collaboration` remains the canonical collaboration subsystem facade.
- Existing documented or externally consumed Rust paths should remain stable through facade re-exports where necessary.
- Tightening visibility is allowed only after repository evidence shows the item is crate-internal and supported compatibility is unaffected.
- Child modules must not expose alternative public entry points that encourage bypassing the canonical facade.

## 5. Explicit non-goals

Do not perform any of the following under this plan:

- change roles/capabilities or authorization policy behavior;
- change authentication/token formats or credential storage;
- redesign the authorization matrix;
- introduce a policy DSL or policy-engine trait abstraction;
- introduce repository/store traits solely to wrap the current SQLite/persistence implementation;
- normalize or redesign collaboration storage;
- change channel/message sequence allocation or retry algorithms;
- add DMs, channel administration, membership management, bridges, reactions, threads, or multi-window chat;
- add or alter structured action kinds;
- parse natural language/free text into actions;
- change job/scheduler semantics;
- refactor `CoreDaemon` beyond import/path updates required by code movement;
- decompose unrelated large files such as `interactive_process.rs` or `tool_program.rs`;
- add dependency-injection frameworks, actor systems, service buses, generic workflow engines, or generalized persistence abstractions;
- add line-count/file-size CI gates;
- add CI lanes, scanners, coverage/benchmark gates, dependency bots, release automation, or broader verification machinery.

If a useful correctness improvement is discovered while moving code, record it as a finding rather than opportunistically folding it into this plan unless it is a trivial compile-preserving mechanical consequence.

## 6. Expected production-code shape

The exact decomposition must follow the responsibility census. A likely shape is:

```text
crates/codegg-core/src/
  authorization.rs
  authorization/
    policy.rs
    identity.rs          # only if the responsibility is truly authorization-owned
    store.rs             # only if persistence forms a coherent unit
    tests.rs             # optional; prefer colocated tests when clearer

  collaboration.rs
  collaboration/
    messages.rs
    actions.rs
    store.rs
    tests.rs             # optional
```

Alternative names are acceptable when they better match current terminology. The following rules are mandatory:

1. Prefer 2–4 coherent child modules per subsystem over many tiny files.
2. Keep shared domain types in the facade when moving them would create circular imports or obscure the public contract.
3. A child `store` module is allowed only as code organization around the existing concrete persistence owner. Do not create a new repository trait/facade/service layer.
4. A child `policy` module must contain the existing policy logic; it must not become a new runtime-configurable policy engine.
5. An `actions` module must remain collaboration logic around explicit typed actions; it must not own job execution.
6. Avoid `use super::*` in new production modules where explicit imports improve dependency direction, but do not create wrapper types merely to reduce import lists.
7. Do not clone mutable state into helper structs just to satisfy borrow/module boundaries.
8. Do not widen visibility merely because code moved. Prefer private or `pub(super)`/`pub(crate)` only as required by existing callers.

## 7. Ordered work packages

### Work package A — Baseline and responsibility map

Intent: ensure decomposition follows semantics rather than aesthetics.

Required actions:

1. Confirm production HEAD and whether the target files materially match the baseline.
2. Record approximate line/file size only for descriptive context.
3. Inventory top-level types, impl blocks, free functions, constants, SQL/schema statements, validation helpers, and test modules in both files.
4. Map each cluster to its canonical responsibility and external callers.
5. Identify no more than 2–4 extraction units per target subsystem.
6. Identify elements that intentionally remain in the facade because they define the domain/public contract or coordinate the child responsibilities.

Acceptance evidence:

- closure record contains the responsibility map;
- no proposed module duplicates an existing canonical owner elsewhere in the repository.

### Work package B — Authorization physical decomposition

Intent: make authorization ownership navigable without changing its semantics.

Required changes:

- extract the most coherent implementation clusters found in WP-A;
- preserve the facade/re-export paths required by existing callers;
- move focused tests with their implementation when that improves locality;
- keep operation descriptors, capability mapping, default-deny behavior, identity/authority derivation, and store semantics byte/semantic-equivalent;
- keep stable error strings/codes and audit metadata unchanged.

Expected preference order:

1. pure/static policy/capability/descriptor helpers with narrow inputs;
2. concrete persistence/query helpers if they are clearly separable without transaction changes;
3. authenticated authority-context helpers only if doing so does not split identity ownership or create conversion churn.

Do not extract a new service object merely to group functions.

Acceptance evidence:

- focused authorization/core tests pass;
- authorization matrix/static guard passes;
- direct caller paths still use the same canonical API;
- no change to protocol/storage schema or decision semantics.

### Work package C — Collaboration physical decomposition

Intent: separate message/action/storage implementation families while preserving one collaboration subsystem.

Required changes:

- extract channel/message validation/domain helpers where coherent;
- extract structured-action validation/idempotency/status/audit-safe shaping where coherent;
- extract concrete persistence/query helpers only if transaction and conflict boundaries remain identical;
- preserve facade types and caller paths where public/documented;
- move tests with extracted responsibilities when useful.

The action module may decide what semantic capability an action requires and validate its collaboration-owned payload. It must not become a scheduler/job executor. Daemon/job submission remains on the current path.

Acceptance evidence:

- collaboration M001/M002/M003 focused suites remain green;
- free-text zero-execution tests remain green;
- duplicate/restart/revocation/project-mismatch/action authorization tests remain green;
- no second job submission or execution path exists.

### Work package D — Boundary and dependency-direction review

Intent: ensure smaller files actually improve maintainability.

Required actions:

- inspect new modules for circular imports, pass-through wrappers, duplicated validation, or duplicated SQL;
- inspect visibility changes and re-exports;
- search for direct imports of child internals from daemon/TUI/protocol code that should instead use the facade;
- search for duplicated capability checks or collaboration action execution logic introduced during movement;
- remove dead imports/comments/test helpers exposed by movement.

Acceptance evidence:

- each child module can be described in one sentence;
- facade modules remain the obvious entry point;
- dependency direction is no worse than baseline;
- no new abstraction exists solely to hide file movement.

### Work package E — Documentation and registry reconciliation

Intent: leave architecture and planning metadata truthful.

Required actions:

- update `architecture/authorization.md` if it names obsolete source locations or ownership internals;
- update `architecture/collaboration.md` if it names obsolete source locations or ownership internals;
- update `.opencode/skills/` or `AGENTS.md` only where they contain source-layout guidance made stale by the move;
- update `plans/registry.md` from M001 `ready/active/closing` to `closed` only after closure evidence exists;
- ensure the dependency-ready section contains no stale counts/prose such as the pre-registration "These four plans may be implemented in parallel" artifact;
- link the new closure record without rewriting historical closure records.

Acceptance evidence:

- docs describe semantic owners rather than forcing readers to rely on obsolete monolithic filenames;
- registry is compact and internally consistent.

## 8. Failure, cancellation, restart, and contention semantics

This milestone intentionally changes none of these semantics. Closure must nevertheless review them because physical movement can accidentally alter them.

### Authorization failure/revocation

The same denial path and stable codes must be produced for the same principal/request/context. Moving helpers must not convert fail-closed `None`/error cases into permissive defaults or reorder authority derivation around a side effect.

### Collaboration retries and restart

Idempotency lookups, unique-conflict winner rereads, channel/project/message linkage, and action/job references must retain their current order. Do not split a transaction or move a lookup outside the synchronization/transaction boundary merely to place it in a separate module.

### Cancellation/task lifetime

No new spawned tasks. Existing async functions retain the same caller-owned cancellation/lifetime. If module movement requires detaching a future or cloning a mutable owner into a new task, stop.

### Lock/transaction ordering

Do not change lock acquisition or SQL transaction order as a borrow checker workaround. If a mechanical extraction cannot compile without changing ordering, keep that cluster in the facade and record why.

## 9. Security review requirements

Closure must explicitly establish that:

- authority still originates from authenticated transport/canonical server-side context;
- no DTO or chat body is treated as authority;
- default-deny remains intact;
- observer/viewer/read-only constraints remain intact;
- structured action dual-capability checks remain intact;
- malformed/secret-bearing action payloads retain zero privileged side effects;
- audit metadata still excludes titles/prompts/secrets where current contracts require structural locators only;
- child modules do not expose a lower-level public function that bypasses authorization or scheduler admission.

Do not add a new security scanner or static-analysis framework for this milestone.

## 10. Storage, migration, protocol, and compatibility

### Storage

No schema migration. `STORAGE_LAYOUT_VERSION` must remain unchanged. Existing table/index/constraint definitions may move source location but must remain semantically identical.

### Protocol

No protocol changes. Do not touch DTOs except import/path updates that produce identical serialization and API behavior.

### Configuration

No config keys/defaults/role mappings/capability settings may change.

### Public Rust compatibility

Search repository consumers before changing visibility or paths. Prefer facade re-exports to preserve supported imports. A purely crate-private path may be tightened when proven safe, but closure must record the evidence.

### Rollback

The implementation should be reviewable/revertible as one behavior-neutral source-organization tranche. Avoid mixing unrelated cleanup so a revert restores the prior module layout without requiring data migration or operator action.

## 11. Required focused verification

Use the repository's actual current package/test names discovered at implementation time. The following is the expected minimum shape; do not invent nonexistent packages/selectors merely to copy this plan literally.

Authorization/identity/audit coverage should include the currently existing equivalents of:

```text
cargo test -p codegg-core --lib authorization
cargo test --test identity_m003_daemon_authorization
cargo test --test identity_m004_audit_foundation
cargo test --test identity_m005_audit_instrumentation
python3 scripts/check_authorization_matrix.py
```

Collaboration/presence coverage should include the currently existing equivalents of:

```text
cargo test -p codegg-core --lib collaboration
cargo test --test collaboration_m001_chat
cargo test --test collaboration_m002_chat_tui
cargo test --test collaboration_m003_chat_actions
cargo test --test presence_m002_collaborators
cargo test --test presence_m003_observation
```

Run execution/ownership guards only to ensure movement did not introduce a bypass:

```text
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
bash scripts/check-core-boundary.sh
```

If a named selector no longer exists, use the closest current focused suite and explain the substitution in closure. Do not add replacement tests solely to preserve obsolete selector names.

## 12. Required broad verification

Keep broad verification minimal, matching the registry policy:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

A full workspace/root test sweep is optional unless focused results, touched shared code, or current repository conventions make it necessary. Do not create hosted-CI requirements beyond the existing closure convention. Do not block closure on unrelated historical operational-evidence items.

## 13. Documentation updates

At minimum review:

- `architecture/authorization.md`;
- `architecture/collaboration.md`;
- `architecture/core.md` only if it explicitly names the moved helpers;
- `.opencode/skills/` source-ownership guidance where applicable;
- `AGENTS.md` only if it names obsolete file locations;
- `plans/registry.md`.

Documentation should describe semantic ownership first and source paths second. Do not create duplicate architecture documents for child modules.

## 14. Acceptance criteria

M001 may close only when all of the following are true:

- the implementer produced a before/after responsibility map for both target modules;
- code was moved only along stable conceptual boundaries discovered from current HEAD;
- each new child module has one clear responsibility and is not merely a pass-through layer;
- `authorization` remains the canonical facade/owner and all authorization semantics are unchanged;
- `collaboration` remains the canonical facade/owner and all message/action semantics are unchanged;
- free text still cannot execute work;
- chat actions still use existing authorization plus semantic capability checks and the canonical job/scheduler submission boundary;
- storage layout/schema, transaction/conflict semantics, protocol serialization, config, stable error codes, audit/redaction, and public supported behavior are unchanged;
- no new coordinator, policy engine, repository abstraction, state machine, workflow engine, service bus, DI framework, background task, or execution path was introduced;
- focused identity/auth/audit/collaboration/presence tests and relevant ownership guards pass;
- format, all-feature Clippy, and `scripts/verify.sh quick` pass;
- architecture/source-layout docs are accurate;
- `plans/registry.md` is compact, has no stale dependency-ready count/prose, and points to the closure record;
- closure records any remaining concentration that was intentionally left in the facade and explains why moving it would worsen ownership/locality.

There is no line-count acceptance threshold.

## 15. Stop conditions

Stop implementation and report rather than broadening this plan if:

- current HEAD materially changed either target module such that the responsibility map no longer matches this baseline;
- a proposed extraction requires changing a canonical subsystem owner;
- authorization behavior, role/capability semantics, stable denial codes, or audit attribution would change;
- a schema, storage-layout, serialized protocol, or config migration is required;
- moving collaboration persistence would alter transaction, sequence, idempotency, uniqueness, restart, or contention behavior;
- moving an action helper would require direct job/scheduler execution in `codegg-core`;
- borrow/lifetime issues appear solvable only by detached tasks, cloned mutable authority/state, process-global state, or reordered locks/transactions;
- a proposed trait/repository/service abstraction has one implementation and exists only to make the source tree look smaller;
- the refactor begins expanding into unrelated large modules or feature cleanup;
- focused tests expose a pre-existing correctness defect that requires semantic changes. Record that defect and create a separately owned corrective plan instead.

## 16. Closure evidence required

Create `plans/closure/post-implementation-maintainability-closure/001-status.md` containing at minimum:

- implementation commit(s) or PR(s);
- exact production baseline and final reviewed HEAD;
- requirement-to-evidence matrix;
- before/after responsibility map for `authorization` and `collaboration`;
- descriptive before/after source sizes/line counts if useful, explicitly not used as a gate;
- list of new/moved modules and one-sentence ownership for each;
- proof that public/durable protocol, storage-layout version, config, stable error codes, and audit semantics did not change;
- focused test commands/results and any justified selector substitutions;
- ownership/static guard results;
- format/Clippy/quick verification results;
- security review including default-deny, authority derivation, free-text inertness, structured-action authorization, secret rejection, and scheduler-boundary confirmation;
- restart/idempotency/transaction/lock-order review;
- visibility/re-export compatibility disposition;
- documentation and registry updates;
- unresolved findings classified by severity;
- recommendation: `closed`, `conditionally closed`, `corrective pass required`, or `blocked`.

A commit message claiming closure is insufficient.

## 17. Handoff guidance

Optimize for conceptual locality, not small files. It is acceptable for `authorization.rs` or `collaboration.rs` to remain large if the remaining code is genuinely the facade/domain contract or tightly coupled high-level sequencing. Prefer three coherent moves over fifteen tiny modules.

The key success criterion is that a future maintainer can answer "where does this responsibility live?" quickly without creating alternative ways to perform authorization, persist collaboration state, or submit work. If decomposition makes ownership less obvious, revert that extraction rather than completing it for cosmetic symmetry.
