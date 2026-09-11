# Repository Surface Housekeeping M001 — Active Surface, Guard, and Traceability Reconciliation

Status: closed

Closure record: `plans/closure/repository-surface-housekeeping/001-status.md`
(implementation commit `931fb709`).

Source subsystem roadmap:

- `plans/subsystems/repository-surface-housekeeping-corrective-addendum.md`

Primary class: polish / maintainability.

Audit baseline: `db6fe01920624ccaaed4200dbd98a38628f85abb`

Planning baseline: `1fe6cbfe77546be87320db7d437e4ed3c9211c94`

Relevant closed foundations:

- TUI/frontend convergence M005-M010;
- post-audit maintainability corrective M006-M007;
- project-catalog M001-M004;
- development verification/release closure;
- `plans/003-planning-process.md#7-corrective-passes`.

No ADR is required. This implementation must remain behavior-neutral except for
repairing the semantics of the existing static guard. If a repository census
finds a runtime bug, stop on that item and create a separate corrective plan.

## 1. Objective

Perform one bounded repository housekeeping pass that makes the active
human-facing and maintainer-facing surfaces agree with current `main`:

1. remove stale or misleading source/module comments introduced by prior
   architecture states;
2. repair `check_project_catalog_invariants.py` so it validates a durable
   schema/storage relationship instead of pinning an unrelated magic version;
3. reconcile README, AGENTS, CHANGELOG Unreleased, RELEASING, architecture,
   docs/, and every operational `SKILL.md` with current source truth;
4. repair current planning/registry traceability without rewriting historical
   closure evidence;
5. leave production behavior, schema, protocol, versioning, and release state
   unchanged.

The output should be a smaller amount of more durable documentation, not a
larger layer of generated or duplicated truth.

## 2. Explicit non-goals

- Do not change `STORAGE_LAYOUT_VERSION`, add a migration, change SQL, or
  modify durable storage behavior.
- Do not change TUI behavior, command behavior, focus routing, project routing,
  shell policy, MCP crypto, daemon behavior, scheduler behavior, or
  authorization.
- Do not bump package versions or dependency versions.
- Do not publish to crates.io, create tags/releases, upload binaries, or run
  release automation.
- Do not edit historical `plans/closure/` or `plans/archive/` documents to
  replace their point-in-time version numbers.
- Do not rewrite released changelog history. Only the active **Unreleased**
  section may be corrected/extended.
- Do not add a docs framework, markdown linter, link-check service, CI lane,
  dependency, or broad generated-doc pipeline.
- Do not turn all stable historical migration numbers into vague prose; preserve
  the version at which a subsystem was introduced when that is the fact being
  documented.

## 3. Current implementation evidence to verify before editing

The implementation agent must re-read the current tree before making changes.
At the planning baseline, the following discrepancies are known:

### Static guard drift

`crates/codegg-core/src/storage/mod.rs` declares:

```text
STORAGE_LAYOUT_VERSION = 56
```

and `crates/codegg-core/src/session/schema.rs` includes `migrate_v56`, while
`scripts/check_project_catalog_invariants.py` fails unless the storage version
is exactly `54`.

Historical closure records show the same guard becoming stale at several prior
layout values. Treat that recurrence as evidence that changing `54` to `56` is
not an acceptable solution.

### TUI source-comment drift

Known examples:

- `src/tui/app/mod.rs` still says `project_tabs` "currently always holds
  exactly one compatibility tab";
- `src/tui/components/component/focus.rs` says unhandled events bubble to
  underlying modal components, while `FocusManager::handle_key` addresses only
  the top component;
- the App/module documentation must be checked against the M008 split into
  `commands`, `input`, `modal`, `project_session`, `prompt_turn`, `plugin_ui`,
  and `render`.

### Active documentation drift

Known current-state claims include:

- `AGENTS.md`: `STORAGE_LAYOUT_VERSION = 36`;
- `.opencode/skills/core/SKILL.md`: current layout `36`;
- `architecture/core.md`: current layout `49`;
- `architecture/codegg_core.md`: current layout `49`;
- `architecture/identity.md`: wording that makes layout `51` sound current;
- `architecture/collaboration.md`: source/current-layout references around
  `55` even though later collaboration work introduced v56;
- `CHANGELOG.md` **Unreleased**: earlier skills/docs refresh calls layout `36`
  current;
- `.opencode/skills/tui/SKILL.md`: 19 command-handler submodules versus the
  current 25 declarations in `src/tui/commands/mod.rs`.

These are examples, not the complete census.

### Planning traceability drift

At the planning baseline:

- `plans/registry.md` records TUI M007's implementation commit as `this commit`
  rather than the actual implementation commit `3b41fea37235a26ad9e903ee7d7fc2ac61bd8c7e`;
- the registry contains detailed all-closed sequencing prose that should be
  reviewed against the governance requirement that the registry remain a
  compact current control surface;
- registering this M001 will make the dependency-ready section non-empty, but
  the final closure pass should remove/normalize it again rather than leaving
  malformed empty table prose.

### README/release-state review

`README.md` correctly identifies the Cargo package as `0.1.0`, but its install
section contains prebuilt/pinned-release language that must be validated
against actual repository release availability and `RELEASING.md`. The README
also predates the final closed TUI convergence in several user-facing details.

Do not infer a release exists because the installer code exists. Conversely,
do not delete supported installer documentation merely because a particular
release is absent. Describe implemented support and actual availability
precisely.

## 4. Invariants that cannot regress

- Historical evidence remains historical. Do not alter closure/archive records
  to make old verification values look current.
- Architecture docs distinguish "introduced by migration vN" from "current
  storage layout".
- Operational docs do not copy volatile current values unless the exact value
  is meaningful to the operation.
- All documented shell commands use the correct interpreter. For example,
  shell scripts are invoked with `bash`/direct execution as appropriate, not
  `python3` merely because they appear in a static-guard list.
- Skills never grant authority. Their wording must continue to reflect daemon,
  scheduler, permission, tool-broker, and project-context ownership.
- README remains user-facing and does not become an internal architecture dump.
- Source comments describe actual behavior and ownership without creating a
  second normative specification.
- No source code changes are permitted beyond comments/module docs and the
  existing Python static guard unless a separately planned defect is found.

## 5. Expected production-code changes

No Rust behavior changes are expected.

Permitted Rust edits are comments/module documentation only, principally in
files materially touched by the recently closed work, including at minimum:

- `src/tui/app/mod.rs`;
- `src/tui/components/component/focus.rs`;
- `src/tui/app/state/project_tabs.rs` if its comments or terminology disagree
  with current tab semantics;
- adjacent TUI command/runtime/state modules only where the census finds an
  objectively stale ownership/lifecycle comment;
- terminal/MCP source comments only if they contradict the already-closed
  canonical policy/crypto contracts.

The one behavioral verification-script change is:

- `scripts/check_project_catalog_invariants.py`.

It must continue to fail on a real project-catalog/storage invariant mismatch,
but must not fail merely because an unrelated later schema migration increments
`STORAGE_LAYOUT_VERSION`.

## 6. Static-guard design requirement

Do **not** implement:

```text
expected_version = 56
```

Preferred shape:

1. parse `STORAGE_LAYOUT_VERSION` from the canonical storage module;
2. independently derive the highest migration that the canonical schema
   migration path actually wires/applies, or identify a stronger semantic
   relationship during implementation;
3. assert the relationship between those independent sources;
4. retain the catalog-specific table/column/locator checks already present;
5. produce an actionable error that reports both observed values without
   embedding a volatile expected current number in the source.

If the storage-layout marker is intentionally allowed to diverge from the
highest schema migration number, stop and document that evidence before
choosing another invariant. In that case, the acceptable fallback is a
catalog-specific minimum/feature assertion tied to migration v28 plus an
independent general storage-version consistency check. A tautological
"constant exists" check is not sufficient.

Regression proof must include both:

- current repository state passes;
- a synthetic or temporary mismatch between the independently derived values
  fails, while a matched future-number pair would pass without changing the
  guard source.

Use Python standard library only. Do not introduce pytest or another test
framework solely for this script.

## 7. Ordered work packages

### Package A — Build a current-truth census before editing

Inventory the active documentation surfaces and classify findings as:

- stale current-state fact;
- valid historical/introduction fact;
- broken path/command;
- stale ownership/lifecycle description;
- volatile exact count/value that should be removed;
- user-facing omission caused by recently closed work;
- no change required.

Required census roots:

```text
README.md
AGENTS.md
CHANGELOG.md                    # Unreleased only for edits
RELEASING.md
architecture/**/*.md
docs/**/*.md
.opencode/skills/*/SKILL.md
src/tui/**                      # comments/module docs relevant to M005-M010
src/tool/bash* / terminal.rs    # comments only, if stale
src/mcp/auth.rs                 # comments only, if stale
plans/registry.md
```

Also validate every path and command named by an affected skill before keeping
it. Avoid mass reformatting unchanged documents.

### Package B — Repair the project-catalog static guard

Refactor the exact-version assertion according to section 6.

Keep existing catalog-specific checks for:

- local-only workspace binding;
- remote locator path safety;
- catalog/discovery migration tables;
- v28 logical-project columns;
- module export.

Rename check labels/docstrings so they describe the semantic invariant rather
than "STORAGE_LAYOUT_VERSION is 54".

Do not add the guard to another CI lane. Its existing manual/operator role may
remain unless current verification ownership already calls it; this plan does
not use documentation cleanup as justification to expand routine CI.

### Package C — Correct source comments and module docs

At minimum:

- update the `project_tabs` field comment to describe the actual multi-tab
  collection and active-tab authority;
- correct `FocusManager` wording so only the mounted top modal receives normal
  key input and unhandled modal keys do not imply bubbling into lower modals or
  the prompt;
- ensure App's module/event/render comments match the M008 module split and
  M007 focus ownership;
- remove stale "future" language for capabilities that are now implemented,
  where found in active source comments.

Do not move code or rename types in this package.

### Package D — Reconcile architecture, docs, AGENTS, and CHANGELOG Unreleased

Audit all active architecture/docs files, with special attention to:

- `architecture/core.md`;
- `architecture/codegg_core.md`;
- `architecture/storage.md`;
- `architecture/workspace_services.md`;
- `architecture/project_catalog.md`;
- `architecture/identity.md`;
- `architecture/audit.md`;
- `architecture/collaboration.md`;
- `architecture/tui.md`;
- `architecture/command.md`;
- `architecture/tool.md`;
- `architecture/mcp.md`;
- `architecture/crypto.md`;
- `architecture/auth.md`;
- `architecture/config.md`;
- `docs/MCP.md`, `docs/TROUBLESHOOTING.md`, `docs/execution-ownership.md`, and
  any other `docs/` file the census identifies;
- `AGENTS.md`;
- `RELEASING.md`;
- `CHANGELOG.md` **Unreleased**.

Rules:

- preserve introduction versions such as "migration v54 introduced audit
  tables";
- replace stale "current layout = N" claims with either current verified truth
  or, preferably, a reference to `storage::STORAGE_LAYOUT_VERSION` when the
  numeric value is not operationally required;
- remove fragile counts where a growing registry/module is the actual owner;
- validate AGENTS command examples by interpreter and path;
- preserve the repository's deliberately minimal verification/release policy;
- add a concise Unreleased changelog item for the housekeeping pass and correct
  stale current-state text already inside Unreleased.

### Package E — Audit and reconcile every operational skill

Enumerate every directory under `.opencode/skills/` and open its `SKILL.md`.
Do not limit the pass to skills already known to be stale.

For each skill, validate:

- referenced source/architecture paths exist;
- commands are executable as written;
- ownership statements match current daemon/scheduler/tool/project authority;
- exact counts/current versions are either useful and current or removed;
- renamed/extracted modules are reflected;
- cross-links point to the canonical architecture doc;
- compatibility paths are not presented as canonical when the repo now has a
  canonical replacement.

Known required corrections include:

- `core` skill current storage-layout wording;
- `tui` skill command-handler count and any wording made stale by M005-M010;
- check `human-shell` against the canonical Bash/terminal policy ownership;
- check planning/architecture-review skills against the current registry and
  closure conventions.

If a skill has its own version field and local convention requires bumping it
when semantics materially change, follow that convention consistently; do not
bump untouched skills solely because they were audited.

### Package F — Reconcile README as the user-facing truth surface

Keep README concise. Update only claims that are stale, misleading, or missing
important current user-visible behavior.

Required review points:

- multi-project project picker/tabs and project-correct execution without
  process-cwd mutation;
- keyboard-focusable sidebar and nested agent-run/tree inspection;
- command palette/discovery and configurable keybindings as the discoverability
  mechanism rather than a giant README command list;
- daemon-owned scheduling/session model;
- current package version versus published-release availability;
- prebuilt installer language and pinned-version example against actual
  supported release assets/procedure;
- source-install and crates.io language against `RELEASING.md` and manifest
  state;
- MCP/credentials wording against the final canonical crypto/key lifecycle;
- links to the relevant architecture/docs pages.

Do not add internal milestone numbers to README.

### Package G — Planning and registry traceability cleanup

Because this milestone itself is active planning, update the registry in the
normal sequence during implementation/closure.

Before closure:

- replace TUI M007's `this commit` implementation placeholder with
  `3b41fea37235a26ad9e903ee7d7fc2ac61bd8c7e`;
- verify all recent implementation-commit references resolve;
- trim or collapse all-closed sequencing prose that no longer helps determine
  current execution order, while preserving links to closure evidence;
- ensure dependency-ready/active/blocked tables are structurally valid and do
  not contain prose rows under table headers;
- keep unrelated Architecture M009 and Runtime Safety C002 blockers unchanged;
- do not rewrite prior closure records.

At closure, add:

- `plans/closure/repository-surface-housekeeping/001-status.md`;
- implementation commit(s), verification results, census disposition, and any
  intentionally retained historical/current-value claims.

## 8. Storage, protocol, migration, and compatibility effects

Expected effects: **none**.

- No storage schema or layout change.
- No protocol change.
- No serialized-format change.
- No CLI/TUI compatibility change.
- No environment-variable change.
- No shell-policy change.
- No MCP encryption-format change.
- No migration of user files.

The project-catalog Python guard is verification code, not runtime storage
behavior.

## 9. Focused verification

Required focused commands, adapting only for repository-standard wrappers if
necessary:

```bash
python3 scripts/check_project_catalog_invariants.py --verbose
python3 -m py_compile scripts/check_project_catalog_invariants.py
bash scripts/check-core-boundary.sh
python3 scripts/check_tui_project_authority.py
scripts/verify.sh quick
git diff --check
```

Also run targeted text/path checks created from the census. They should be
review aids, not a new permanent docs-lint framework. Examples:

```bash
rg 'STORAGE_LAYOUT_VERSION' README.md AGENTS.md architecture docs .opencode/skills CHANGELOG.md
rg 'currently always holds exactly one compatibility tab|bubble to underlying' src/tui
```

If source comments are the only Rust edits, broad executable tests are not
required beyond existing quick verification. If the guard refactor causes any
Rust file to change beyond comments, stop and reassess scope.

## 10. Broad verification posture

Before closure:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Do not add `scripts/verify.sh full`, a network release check, or a documentation
crawler as a permanent closure requirement unless implementation evidence shows
an actual need.

For README release-availability wording, record how availability was checked
at implementation time. Network evidence may inform prose, but no
network-dependent check becomes routine CI.

## 11. Documentation acceptance matrix

Closure must include a matrix with at least these rows:

| Surface | Required closure evidence |
|---|---|
| README | user-visible TUI/install/provider claims checked against source/current availability |
| AGENTS | commands, paths, static guards, storage/project/TUI ownership checked |
| CHANGELOG Unreleased | stale current-state claims corrected; housekeeping entry added |
| RELEASING | manifest/package/release procedure wording checked; no release performed |
| architecture/ | all active docs censused; stale current claims corrected without destroying migration history |
| docs/ | all active docs censused; affected files corrected |
| `.opencode/skills/*/SKILL.md` | every skill inspected; changed/unchanged disposition recorded |
| source comments | known TUI contradictions removed; no runtime behavior changed |
| planning registry | absolute/resolvable recent implementation refs and compact valid current control surface |
| project-catalog guard | future-proof semantic relationship and mismatch regression evidence |

A statement that "docs were reviewed" without a file/surface disposition is
not sufficient closure evidence.

## 12. Stop conditions

Stop and split a new corrective plan if any of the following is discovered:

- runtime behavior contradicts the intended architecture rather than only the
  docs/comments;
- `STORAGE_LAYOUT_VERSION` is intentionally not supposed to track any
  independently derivable migration/schema relationship and repairing the
  guard requires changing production storage semantics;
- README accuracy requires actually publishing or changing release artifacts;
- a skill depends on a missing capability rather than merely stale guidance;
- fixing a documented command requires CLI behavior changes;
- a protocol/storage migration would be needed.

Do not broaden M001 to make those changes.

## 13. Acceptance criteria

M001 is acceptable for closure only when all of the following are true:

1. the catalog guard passes current state without an exact hard-coded current
   version and has evidence that a mismatch still fails;
2. active docs no longer present superseded storage-layout values as current;
3. valid historical migration versions remain intact and clearly contextualized;
4. known TUI comment contradictions are gone;
5. every operational skill has a recorded audit disposition and affected
   skills are updated;
6. README accurately summarizes current multi-project/TUI behavior and does
   not overstate release/install availability;
7. AGENTS commands and paths are executable/correct as written;
8. CHANGELOG Unreleased and RELEASING are reconciled where affected;
9. registry traceability uses resolvable commit IDs and the current-control
   sections are structurally valid;
10. no historical closure/archive record was rewritten;
11. no production behavior, schema, version, dependency, or release state was
    changed;
12. focused checks, Clippy, quick verification, formatting, and diff check pass.

## 14. Closure evidence required

Create `plans/closure/repository-surface-housekeeping/001-status.md` containing:

- implementation commit(s);
- requirement-to-evidence matrix;
- static-guard before/after invariant explanation;
- current-doc census with changed/unchanged disposition by documentation class;
- full skill census with changed/unchanged disposition;
- README/release-availability evidence used;
- exact verification commands and outcomes;
- confirmation that Rust/runtime behavior was unchanged;
- confirmation that historical closure/archive records were not rewritten;
- unresolved findings classified critical/high/medium/low;
- final recommendation: closed, conditionally closed, corrective pass required,
  or blocked.

If any active documentation surface was intentionally left with a volatile
exact value, the closure record must state why that exact value is a durable or
operationally necessary contract.
