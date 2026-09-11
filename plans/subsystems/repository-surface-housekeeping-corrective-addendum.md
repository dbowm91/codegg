# Repository Surface Housekeeping Corrective Addendum

Status: closed

Closure record: `plans/closure/repository-surface-housekeeping/001-status.md`
(implementation commit `931fb709`).

Repository audit baseline: `db6fe01920624ccaaed4200dbd98a38628f85abb`

Related closed work:

- `plans/subsystems/tui-project-sessions-frontend-convergence-corrective-addendum.md` — TUI M005-M010 closed.
- `plans/subsystems/post-audit-maintainability-surface-corrective-addendum.md` — maintainability M006-M007 closed.
- `plans/subsystems/project-catalog-roadmap.md` — project catalog milestones closed.
- `plans/subsystems/development-verification-release-ci-reproducibility-corrective-addendum.md` — verification/release corrective work closed.

Long-term and governance references:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md#2-4-milestone-implementation-plans`
- `plans/003-planning-process.md#7-corrective-passes`
- `plans/003-planning-process.md#9-registry-requirements`

No ADR is required. This pass changes no product, protocol, daemon, scheduler,
storage, security, or execution ownership. It reconciles active repository
surfaces with architecture that is already selected and implemented. If the
work discovers a real behavioral or architectural defect, that defect must be
split into a separate corrective plan rather than silently fixed as
"documentation housekeeping."

## 1. Purpose and corrective trigger

The September 11, 2026 post-implementation review found that the frontend and
maintainability corrective work landed correctly, but the repository still has
a class of low-severity drift: source comments, active architecture documents,
maintainer guidance, operational skills, the README, and one static guard have
copied implementation facts that are no longer current.

Concrete evidence at the audit baseline includes:

- `scripts/check_project_catalog_invariants.py` requires
  `STORAGE_LAYOUT_VERSION == 54`, while
  `crates/codegg-core/src/storage/mod.rs` declares `56` and
  `session/schema.rs` contains `migrate_v56`;
- the same guard has accumulated repeated stale-version findings in historical
  closure records after unrelated schema bumps, showing that the exact-number
  assertion is the wrong invariant rather than a one-time typo;
- `AGENTS.md` and `.opencode/skills/core/SKILL.md` describe the current storage
  layout as `36`;
- active architecture documents still contain current-state claims for layout
  versions such as `49`, `51`, `54`, and `55`, even though some of those
  numbers are valid only as the migration version where a subsystem was
  introduced;
- `CHANGELOG.md`'s **Unreleased** documentation section calls layout `36`
  current;
- `.opencode/skills/tui/SKILL.md` says `src/tui/commands/` contains 19 command
  handler submodules, while the current module declaration contains 25;
- `src/tui/app/mod.rs` still describes the project-tab collection as holding
  exactly one compatibility tab, despite the now-closed multi-project work;
- `src/tui/components/component/focus.rs` says unhandled key events bubble to
  lower modal components, while the implementation routes only to the top
  mounted component;
- `plans/registry.md` retains a relative `this commit` implementation reference
  for TUI M007 and has accumulated closed-work sequencing prose that should be
  reviewed for compact current-control-surface value;
- the README predates the final TUI convergence in several user-facing areas
  and its install/version examples must be checked against actual package and
  release availability rather than treated as timeless prose.

These are not reasons to reopen the closed functional milestones. They justify
one bounded repository-surface reconciliation pass.

## 2. Work classification

Primary class: **polish / maintainability**.

### Invariants

- Active documentation describes the behavior and repository structure that
  exists on `main`; historical closure/archive evidence remains historical and
  is not rewritten to look current.
- A static guard checks a semantic invariant, not a volatile exact value that
  changes whenever unrelated schema work increments a global version.
- User-facing install/release claims distinguish implemented support from
  actually published/available artifacts.
- Operational skills reference real paths, commands, ownership boundaries, and
  current workflows.
- Source comments may summarize ownership and invariants but must not preserve
  superseded lifecycle assumptions.
- `plans/registry.md` remains a compact current control surface with resolvable
  commit/closure references.

### Capabilities

- A new contributor can follow README/AGENTS/skills commands without being sent
  to stale modules, counts, versions, or workflows.
- A user can understand the current multi-project TUI and install paths without
  relying on pre-convergence or unpublished assumptions.
- Maintainers can run the project-catalog guard after future schema increments
  without editing an unrelated magic number.

### Infrastructure

- A future-proof project-catalog layout-version check derived from canonical
  schema/storage evidence or an equivalent semantic relationship.
- A documented current-truth census for active docs and skills used during
  closure.

### Polish

- Stale source comments and planning traceability are corrected.
- Fragile exact counts/current-version prose is removed where the number is not
  itself a durable contract.
- Cross-links among README, architecture docs, docs/, AGENTS, and skills are
  checked while those files are already under review.

## 3. Non-goals

- No production behavior changes, schema migration, storage-layout bump,
  protocol change, daemon/scheduler change, permission change, or dependency
  update.
- No package version bump, release publication, tag creation, GitHub release,
  crates.io publication, installer execution, or release automation.
- No rewrite of historical `plans/closure/`, `plans/archive/`, accepted ADRs,
  or released changelog history merely because they record an older version.
- No broad prose/style rewrite of every document.
- No generated documentation system, docs site, linter framework, link-check
  service, new CI lane, coverage gate, or documentation freshness bot.
- No conversion of all numeric facts into generated files. Stable protocol or
  historical migration numbers may remain when they are actually part of the
  statement being documented.
- No reopening of TUI M005-M010, post-audit M006/M007, or project-catalog
  milestones unless the census finds a substantive behavioral defect.

## 4. Current-state evidence and ownership rules

### Volatile current facts

The following classes SHOULD be referenced symbolically or derived from
canonical source when practical instead of copied as evergreen constants:

- current `STORAGE_LAYOUT_VERSION`;
- counts of command modules, slash commands, configurable actions, workspace
  crates, or similar inventories that naturally grow;
- exact source-file line counts;
- active/ready/closed milestone state;
- current package/release availability.

When a numeric value is useful for a closure record, benchmark, migration
history, or release note, it MAY remain there because the document is
point-in-time evidence.

### Current architecture versus introduction history

Architecture docs often need both facts. They MUST distinguish them explicitly:

- "migration v54 introduced audit tables" is historical and stable;
- "the current storage layout is 54" is a volatile current-state claim and is
  wrong once later migrations land.

The pass must not blindly replace every old migration number with `56`; doing
so would destroy useful subsystem history. Prefer wording such as "introduced
by migration v54; current layout version is defined by
`storage::STORAGE_LAYOUT_VERSION`."

### Documentation classes in scope

The implementation census covers all active, non-historical repository
documentation surfaces:

- `README.md`;
- `AGENTS.md`;
- `CHANGELOG.md` **Unreleased** section only, plus any live index text;
- `RELEASING.md`;
- `architecture/*.md`;
- `docs/*.md` and relevant nested documentation;
- every `.opencode/skills/*/SKILL.md`;
- source/module comments in files materially changed by TUI M005-M010 and
  maintainability M006/M007;
- `plans/registry.md` and the active addenda/implementation records needed to
  represent this corrective pass.

Historical closure/archive records are read-only inputs to the census.

## 5. Target state

After this pass, active repository guidance should follow a simple hierarchy:

```text
canonical source / runtime contract
        |
        +--> architecture docs: ownership and durable behavior
        |
        +--> AGENTS + skills: operational implementation guidance
        |
        +--> README: user-facing supported behavior
        |
        +--> CHANGELOG Unreleased: point-in-time notable changes
        `--> registry: current planning control state
```

Documents may summarize source truth, but they should avoid duplicating
fast-moving exact values unless the value is itself meaningful to the reader.

The project-catalog static guard should check the relationship that matters.
The preferred implementation is to parse the canonical
`STORAGE_LAYOUT_VERSION` and compare it with the highest migration actually
wired into the schema migration path, or another equally durable semantic
relationship discovered during implementation. It must not merely change the
hard-coded expected value from 54 to 56.

## 6. Dependency graph

```text
Closed TUI M005-M010 -------------------\
Closed post-audit M006-M007 -------------+--> M001 Repository-surface reconciliation
Closed project-catalog milestones -------/
```

All dependencies are **hard and already closed**. M001 is dependency-ready.
There is no interface or operational dependency required to begin the source
and documentation work. Release availability checks are evidence inputs to
README wording, not a release dependency.

## 7. Milestone

### M001 — Active repository surface, guard, and traceability reconciliation

Class: polish / maintainability.

Implementation:

- `plans/implementation/repository-surface-housekeeping/001-active-surface-guard-traceability-reconciliation.md`

Objective:

Reconcile active source comments, static guard semantics, current planning
traceability, README, architecture/docs, AGENTS, CHANGELOG Unreleased,
RELEASING, and all operational skills against current source truth without
changing runtime behavior.

Exit conditions:

- `check_project_catalog_invariants.py` no longer pins an unrelated exact
  current version and passes against the current schema/storage relationship;
- a future storage-layout increment that is correctly paired with a new
  migration does not require editing the catalog guard, while an actual
  mismatch still fails;
- current-state storage/version language in active architecture/AGENTS/skills
  is correct or references the canonical source rather than stale copied
  values;
- historical migration-introduction statements remain historically accurate;
- TUI source comments and the TUI skill describe the multi-project,
  top-modal-only focus, scoped-command, async prompt/session, sidebar/agent-tree,
  and decomposed-App contracts that actually exist;
- fragile module/command/action counts are removed unless they serve a real
  acceptance or release purpose;
- every `.opencode/skills/*/SKILL.md` has been checked for existing paths,
  commands, ownership rules, and current architecture links; affected skills
  are updated;
- README user-facing TUI/project behavior and installation/release wording are
  reconciled with current source and actual supported availability;
- AGENTS static-guard commands and implementation guidance are executable and
  point at the correct interpreter/shell/path;
- `CHANGELOG.md` Unreleased no longer calls a superseded layout value current
  and records the housekeeping correction without rewriting historical
  releases;
- `plans/registry.md` contains no unresolved relative commit placeholder for
  the recent TUI closures and remains compact/structurally valid after M001 is
  registered;
- no historical closure/archive evidence was rewritten;
- focused guard/document checks plus `scripts/verify.sh quick` pass.

## 8. Cross-cutting requirements

### Storage and migration

No storage migration is permitted. The pass may document existing migrations
and repair a static check around them, but `STORAGE_LAYOUT_VERSION` and schema
SQL are read-only unless a substantive defect is discovered and separately
planned.

### Protocol and compatibility

No protocol or compatibility surface changes. Documentation must preserve the
difference between retained compatibility paths and canonical paths instead of
silently declaring compatibility aliases removed.

### Security

Do not place secrets, real tokens, local credential values, or machine-specific
private paths in README/docs/skills/examples. MCP crypto documentation should
continue to distinguish canonical new-write encryption from legacy decrypt-only
compatibility. Terminal documentation should continue to identify Bash policy
as the shell-safety owner.

### Verification

Verification stays deliberately light. This milestone may improve the existing
catalog guard and use narrow source/doc scans, but MUST NOT add another CI job,
docs linter framework, release check, network-dependent test, or broad
repository crawler to routine CI.

### Release documentation

README and RELEASING must distinguish:

- source/package version in manifests;
- release procedure capability;
- actual availability of a published binary/crate/release.

A release process being implemented does not prove a release artifact exists.
Do not publish or create one as part of this milestone.

## 9. Risks and decision points

- **Historical-number destruction:** replacing every old migration number with
  the latest value would make architecture history less accurate. Separate
  "introduced at" from "current" language instead.
- **Guard tautology:** a guard that merely reads `STORAGE_LAYOUT_VERSION` and
  asserts it equals itself provides no value. It must compare independent
  canonical evidence or enforce a semantic minimum/relationship.
- **Scope creep:** a documentation census may expose real runtime defects. Stop
  and register those separately rather than fixing them invisibly here.
- **Release ambiguity:** if repository files describe prebuilt installation but
  no corresponding artifact is available, wording must reflect that state;
  the milestone does not create the missing release.
- **Fragile inventories:** exact counts should be kept only when a test or
  public contract makes the count meaningful. Otherwise describe the owning
  registry/module instead.

## 10. Deferred work

- automated docs generation or docs-site publication;
- network link checking in CI;
- release automation, crates.io publication, signing, notarization, or package
  manager work;
- broad source-comment style cleanup unrelated to a discovered contradiction;
- historical closure/archive normalization;
- any behavioral defect discovered by the census, pending a dedicated
  corrective plan.
