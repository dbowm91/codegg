# Post-Audit Maintainability and Surface Milestone 001 — Compatibility-Surface Rationalization

Status: ready for handoff

Repository baseline: `15632a0483a8c4b9d573ff2ce43297b29be8f42a`

Source roadmap:

- `plans/subsystems/post-audit-maintainability-surface-roadmap.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md` where a compatibility name is callable from Tool Programs.

Primary class: polish / invariant

## 1. Objective

Create a complete, evidence-backed inventory of active compatibility aliases, deprecated re-exports, legacy default constructors, and implementation shims in the runtime/tool/provider/Git/search/compaction surfaces; remove those with no supported consumer and make retained compatibility paths explicit, thin, and time-bounded by a concrete removal condition.

The outcome is fewer parallel names and fewer transitional code paths without breaking serialized, configuration, CLI, plugin, or downstream-crate contracts that are still demonstrably in use.

## 2. Why this milestone is ready

The previous architecture-convergence work has already established canonical owners for Git, provider registration, search/eggsearch, tools/brokerage, execution, context/compaction, and coordinator responsibilities. This milestone does not need to choose new owners; it only disposes transitional surfaces around those owners.

Concrete compatibility evidence already exists at the baseline:

- `src/tool/codesearch.rs` is documented as a compatibility alias over eggsearch `repo_search` with coding profile semantics;
- permission, risk, plan-mode, prompt, and architecture docs still enumerate `codesearch` as a first-class visible name;
- `src/tool/todo.rs` retains a backward-compatible `TodoTool` default alongside explicit `TodoReadTool` / `TodoWriteTool` paths;
- `src/tool/multiedit.rs` remains present but is intentionally not in `ToolRegistry::with_defaults()`;
- root/provider/Git modules contain compatibility re-export seams left by crate extraction and ownership convergence;
- compaction has compatibility facade/history that must be inspected against the current canonical context runtime rather than assumed removable.

Because CodeGG is still pre-1.0, unsupported aliases should not become permanent by inertia. At the same time, prior closure records are immutable evidence: this milestone must preserve any compatibility contract those records intentionally accepted until its consumer/migration is explicitly dispositioned.

## 3. Current implementation evidence

The agent must inspect at minimum:

- `src/tool/mod.rs`, `catalog.rs`, `risk.rs`, permission-mode/tool-policy lists, plan-mode filtering, prompts, and agent tool-surface docs;
- `src/tool/codesearch.rs`, `repo_search.rs`, `todo.rs`, `task.rs`, `multiedit.rs`;
- `src/search_backend/` and `architecture/search_backend.md`;
- root provider/auth re-exports versus `crates/codegg-providers`;
- root Git mutation/service/re-export surfaces versus `codegg-git` and `egggit`;
- `src/agent/compaction.rs`, context/compaction modules, and their public imports;
- configuration schema aliases/defaults and serde rename/alias annotations;
- native protocol DTO compatibility names;
- docs/examples/skills/plugins that invoke names literally;
- integration tests and closure records documenting compatibility commitments.

A text search alone is not sufficient. For each candidate, classify whether it is:

```text
A. model-facing alias
B. CLI/config compatibility
C. serialized protocol/storage compatibility
D. Rust public API/re-export compatibility
E. test-only compatibility helper
F. dead/unregistered implementation residue
```

The class determines removal risk and evidence requirements.

## 4. Invariants that must not regress

- Every retained compatibility path delegates to one canonical owner.
- No retained alias may have divergent permission, risk, authority, backend, cancellation, output, or persistence semantics from its canonical replacement.
- Serialized protocol/storage/config compatibility cannot be removed merely because the model-facing alias is unnecessary.
- Tool Program authority and broker contracts remain canonical.
- Search execution remains owned by the accepted eggsearch/search backend boundary.
- Git execution/mutation ownership remains in the accepted Git layers; this milestone does not merge `egggit` and `codegg-git` simply because both are Git-related.
- Provider factories/resolution remain owned by `codegg-providers`; root re-exports may be removed only when internal/downstream use is disproven or migrated.
- Historical closure records are not edited to pretend a compatibility path never existed.
- User configuration must either continue parsing or receive an explicit migration/deprecation disposition.

## 5. Scope

### In scope

- Produce a repository-local compatibility inventory, preferably as a concise table in the closure record or architecture doc rather than a permanent new framework.
- Identify canonical replacement for every candidate.
- Remove dead/unregistered duplicate implementation files when no supported consumer exists.
- Remove obsolete model-facing aliases and all coupled permission/risk/prompt/doc/test references when safe.
- Convert retained compatibility implementations into minimal adapters if they currently duplicate behavior.
- Remove obsolete deprecated re-exports where all workspace imports already use canonical crate paths.
- Add focused tests for any retained alias whose delegation/semantic parity is important.
- Document removal conditions for compatibility retained due to configuration, protocol, external plugin, or downstream crate evidence.

### Explicitly out of scope

- Renaming canonical tools for aesthetics.
- Removing a compatibility field from durable storage/protocol without a migration plan.
- Rewriting search, Git, provider, task, or compaction implementations.
- General API-stability guarantees for every internal Rust module.
- Introducing a compatibility registry/runtime abstraction.
- Deprecation telemetry infrastructure.
- Model-visible tool minimization beyond the direct removal of aliases; broader disclosure belongs to M002.

## 6. Required production changes

### Core/domain

Use a `canonical / compatibility / removed` disposition for each candidate.

For `removed`, delete the adapter/module and all associated registration, permission/risk, prompt, documentation, and test-only assumptions. Ensure the canonical replacement remains registered and covered.

For `compatibility`, ensure execution immediately translates/delegates to canonical behavior and contains no independent backend selection, authority calculation, persistence, or resource-management logic.

For `canonical`, update internal code to import/use it directly so new references do not perpetuate the alias.

### Storage and migrations

Inventory serde aliases, stored enum strings, DB columns/values, and durable run/tool names before removal. Any compatibility encoded in persisted data needs either continued read support or a separate migration milestone. Do not rename historical rows merely for consistency.

### Protocol and DTOs

Search `crates/codegg-protocol`, server/ACP/native DTO conversions, and wire tests for literal names. A wire-visible compatibility field/name must remain unless backward compatibility can be preserved by aliases or a versioned migration already exists.

### Runtime and concurrency

Removal must not change runtime lifetime/cancellation behavior. Compatibility wrappers that own locks/tasks/services should be suspicious: migrate callers to the canonical service before deleting the wrapper rather than duplicating service construction.

### Frontend or operator surface

Update CLI help, plan-mode hints, TUI diagnostics, README/tool docs, and example configurations when user-visible names change. Prefer canonical names consistently.

### Security and authorization

Permission/risk/category lists must be reconciled atomically with alias removal. Removing a name from a list before removing its invocation path can create a policy gap. Retained aliases must map to the same effective category and authority as the canonical tool.

### Documentation and static guards

Update architecture docs that explicitly call something a compatibility alias. Avoid a new generic no-alias guard; focused tests or source searches are enough unless one durable alias-reintroduction invariant is demonstrably valuable.

## 7. Ordered work packages

### Work package A — Compatibility census and consumer proof

Intent: establish the real migration surface before deletion.

Required actions:

1. Search source, tests, config schema, protocol, docs, examples, plugins/skills, and closure records for compatibility/deprecated/legacy/alias/re-export markers.
2. Add known candidates (`codesearch`, legacy todo default, unregistered `multiedit`, provider/Git re-exports, compaction facade) even if marker text is absent.
3. Classify each candidate A–F and name its canonical replacement.
4. Record evidence of any external/public consumer. Absence of an internal reference is not enough for a serialized or documented public contract.

Acceptance evidence: inventory with disposition proposal and evidence column.

### Work package B — Remove low-risk dead and internal compatibility

Intent: eliminate clear maintenance residue first.

Required changes:

- remove F-class dead/unregistered residue with no supported construction path;
- migrate internal workspace imports away from unnecessary compatibility re-exports;
- delete corresponding stale docs/tests/comments.

Acceptance evidence:

- source search no longer finds removed symbols except historical planning/closure records;
- focused canonical-path tests pass.

### Work package C — Rationalize model/tool compatibility aliases

Intent: remove redundant model names without changing underlying capability.

Required changes:

- for each A-class alias, determine whether profiles/config/external assets require the literal name;
- if not, remove model definition/registration and coupled permission/risk/prompt/docs entries;
- if retained, make it a thin canonical delegate and state the removal trigger.

`codesearch` is the required first candidate because the repository itself describes it as a compatibility alias over canonical `repo_search` semantics.

Acceptance evidence:

- canonical tool is callable under all contexts where capability remains intended;
- no policy gap from stale alias lists;
- deferred M002 receives a stable canonical-name input.

### Work package D — High-risk compatibility disposition

Intent: avoid accidental protocol/config/downstream breaks.

Required changes:

- review B/C/D-class candidates separately;
- retain read/parse/re-export compatibility if evidence warrants it;
- where removal is safe, add migration note and compatibility test proving old persisted/config input still behaves as required or is intentionally rejected with actionable diagnostics.

Acceptance evidence: explicit retain/remove rationale for every high-risk item touched.

### Work package E — Documentation and closure inventory

Intent: leave one understandable canonical surface.

Required changes:

- reconcile architecture/tool/search/Git/provider/context docs;
- closure record includes final compatibility table with canonical replacement and retained removal condition.

Acceptance evidence: docs no longer advertise removed aliases as preferred paths.

## 8. Failure, cancellation, restart, and contention semantics

This milestone should be behavior-preserving. No new cancellation/restart mechanism is expected.

If a compatibility wrapper currently owns a task, lock, store, service, or restart/recovery behavior, do not simply delete it. Prove the canonical owner already provides equivalent lifetime semantics and migrate the caller first.

Persisted compatibility must survive restart when required. Tests should construct old-form config/data before startup rather than only exercising in-memory conversion.

## 9. Compatibility and migration

This section is the core of the milestone.

A candidate may be removed when all relevant conditions are true:

- no canonical requirement names it;
- no current config/protocol/storage format requires it;
- no README/user docs recommend it;
- no supported plugin/skill/agent format relies on it;
- no public Rust re-export is intentionally documented for downstream use, or the project accepts the pre-1.0 break explicitly;
- canonical replacement has equivalent functionality and authority semantics;
- migration is trivial or not required.

If any condition fails, retain the smallest compatibility adapter and record a concrete removal condition such as `remove after protocol vN reader support expires` rather than `remove later`.

## 10. Required tests

### Focused unit tests

- alias-to-canonical semantic parity for retained tool aliases;
- parser/serde compatibility for retained config names;
- permission/risk equality where a retained alias remains callable.

### Integration tests

- canonical replacement registered/callable through actual session registry/profile path after alias deletion;
- old supported config/protocol form still accepted if retained.

### Restart and recovery tests

Only for persisted compatibility items.

### Contention and cancellation tests

No new tests unless a removed wrapper formerly owned lifecycle behavior.

### Security and negative tests

- removed alias cannot bypass canonical permission/risk classification through an alternate registration path;
- unsupported old input fails explicitly rather than falling through to a different tool/provider.

### Migration and compatibility tests

Required for any config/protocol/storage compatibility touched.

## 11. Required verification commands

```bash
# focused tests selected for each compatibility family
cargo test -p codegg tool::
cargo test -p codegg-providers
cargo test -p codegg-git

# targeted source searches for removed runtime symbols; historical plans/closure may remain
rg 'codesearch|TodoTool|multiedit|deprecated|compatibility alias|legacy' src crates architecture docs README.md AGENTS.md

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

The agent should narrow package tests based on actual touched files rather than running unrelated expensive suites first.

## 12. Documentation updates

At minimum reconcile the touched portions of:

- `architecture/tool.md`;
- `architecture/agent-tool-surface.md`;
- `architecture/search_backend.md`;
- `architecture/permission.md`;
- provider/Git/context architecture docs if their re-exports/facades change;
- `AGENTS.md` and user-facing help/examples that list removed names.

Do not rewrite historical closure records.

## 13. Acceptance criteria

- A complete compatibility census exists for the targeted runtime surfaces.
- Every candidate is classified canonical, compatibility, or removed with evidence.
- Removed aliases/re-exports have no remaining live registration/import/policy/doc path.
- Retained compatibility contains no independent execution logic and has a concrete reason/removal condition.
- Canonical tools/services preserve permission, risk, authority, persistence, cancellation, and backend semantics.
- No serialized/config compatibility break occurs without explicit migration evidence.
- M002 can use the final canonical-name inventory without reopening M001.

## 14. Stop conditions

Stop and report when:

- an alias is part of a versioned public protocol and no backward-compatible migration exists;
- a durable stored value cannot be read after removal;
- an external plugin/skill/agent contract demonstrably depends on the literal name and no adapter can preserve it;
- resolving the candidate would change canonical ownership rather than remove compatibility;
- the work starts becoming generalized API-stability machinery;
- another active implementation changes the same registration/policy surface enough that the census is stale.

## 15. Closure evidence required

- implementation commits/PRs;
- final compatibility inventory with class, canonical replacement, disposition, evidence, and retained removal condition;
- list of removed source modules/re-exports/registrations;
- policy/risk/permission reconciliation evidence;
- focused unit/integration/migration tests and outcomes;
- source searches showing removed live references are gone while historical records remain untouched;
- formatting/lint/quick verification results actually run;
- known retained compatibility debt;
- recommendation for M002 readiness.

## 16. Handoff notes

Do not optimize for the maximum number of deletions. Optimize for removal of ambiguity and duplicated maintenance while preserving real compatibility.

`egggit` and `codegg-git` are not automatically duplicates: prior architecture work assigned them distinct execution/fact versus typed operation/risk responsibilities. Treat only compatibility re-exports/shims around those owners as candidates unless new evidence proves actual duplicate behavior.

Likewise, research/search wrappers can share a backend while still represent distinct contracts. M001 determines names and obsolete aliases; M002 decides prompt disclosure.
