# Repository Surface Housekeeping M001 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/repository-surface-housekeeping/001-active-surface-guard-traceability-reconciliation.md`
Source subsystem roadmap: `plans/subsystems/repository-surface-housekeeping-corrective-addendum.md#7-milestone`
Repository baseline reviewed: `931fb709`

Implementation commits:

- `931fb709` — docs(housekeeping): reconcile active surface, guard, and traceability (M001 implementation)

## 1. Executive finding

M001 is complete. One bounded, behavior-neutral housekeeping pass reconciled
the active repository surfaces with current `main`: the project-catalog
static guard now asserts a durable storage/migration relationship instead of
a volatile magic version; stale source comments, storage-layout claims,
fragile counts, wrong-interpreter guard invocations, and planning
traceability were corrected; README release wording now distinguishes
implemented installer support from actual (zero) published releases. No
production behavior, schema, protocol, version, dependency, or release state
changed, and no historical closure/archive record was rewritten.

## 2. Requirement-to-evidence matrix

| Requirement (plan acceptance #) | Evidence | Result |
|---|---|---|
| (1) Catalog guard passes without hard-coded version; mismatch still fails | `scripts/check_project_catalog_invariants.py`: 7/7 pass; `--self-test` 4/4 (matched current pair, matched future pair, both mismatch directions fail); live synthetic mismatch (layout 57 vs wired 56) fails, restored tree passes | pass |
| (2) No superseded storage-layout value presented as current | `rg STORAGE_LAYOUT_VERSION` over README/AGENTS/architecture/docs/skills/CHANGELOG: every hit is a symbolic `storage::STORAGE_LAYOUT_VERSION` reference or an explicit introduction-version statement | pass |
| (3) Historical migration versions intact and contextualized | identity v51/v52/v53, authorization v53, audit v54, collaboration v55/v56, session v22/v33/v37, command v139-test kept; reworded `is N` → `introduced by/at N` | pass |
| (4) Known TUI comment contradictions gone | `rg 'currently always holds exactly one compatibility tab\|bubble to underlying' src/tui` empty; `project_tabs` multi-tab comment, `FocusManager` top-modal-only doc, App M008 module doc, picker-tense fixes landed; `cargo ckroot` clean | pass |
| (5) Every operational skill audited; affected skills updated | 13/13 skills inspected (dispositions below); `core`, `tui`, `jobs`, `architecture-review` corrected | pass |
| (6) README multi-project/TUI behavior accurate; release availability not overstated | Multi-tab/picker/sidebar-tree paragraph added; installer availability note added (0 releases published at implementation time); pinned example corrected to `0.1.0` with published-tag qualifier | pass |
| (7) AGENTS commands/paths executable and correct | `check-core-boundary.sh` now invoked with `bash` in AGENTS + 4 architecture docs; all other guard lines already used `python3` (`.py`) or `bash` (`.sh`) correctly | pass |
| (8) CHANGELOG Unreleased + RELEASING reconciled | Unreleased stale `= 36` claims corrected; M001 entry added; RELEASING verified consistent with manifests (0.1.0, 10 crates, manual procedure) — no change required | pass |
| (9) Registry uses resolvable refs; control sections structurally valid | TUI M007 `this commit` → `3b41fea37235a26ad9e903ee7d7fc2ac61bd8c7e` (resolves); M001 registered active → closed; dependency-ready section normalized (no empty-table prose); sequencing prose collapsed | pass |
| (10) No historical closure/archive record rewritten | `git show --stat 931fb709` touches only active surfaces; `plans/closure/`, `plans/archive/` untouched | pass |
| (11) No production behavior/schema/version/dependency/release change | Rust edits are comments/module docs only (`src/tui/...` 3 files); `cargo ckroot`, `verify.sh quick`, all-features Clippy clean | pass |
| (12) Focused checks, Clippy, quick verification, fmt, diff check pass | See section 4 | pass |

## 3. Production implementation evidence

No Rust behavior changes. Comment/module-doc edits only:

- `src/tui/app/mod.rs` — `project_tabs` field comment now describes the
  multi-tab collection and active-tab authority; added module doc matching
  the M008 split (`commands`, `input`, `modal`, `project_session`,
  `prompt_turn`, `plugin_ui`, `render`); two `future picker` comments moved
  to present tense (picker is implemented).
- `src/tui/components/component/focus.rs` — module doc now states only the
  top mounted modal receives key input; unhandled keys are dropped, never
  bubbled.
- `src/tui/app/state/project_tabs.rs` — `set_active_identities` no longer
  calls the picker "future". (`from_compat`'s exactly-one-tab wording kept:
  it documents the constructor's contract, which is still true.)

Terminal/MCP census: `src/tool/bash.rs` ownership comments accurate;
`src/tool/terminal.rs` and `src/mcp/auth.rs` carry no module-level
ownership claims — no change required.

### Static-guard before/after invariant

Before: `check_storage_layout_version()` asserted
`STORAGE_LAYOUT_VERSION == 54` — a volatile exact value that broke on every
unrelated schema bump (layout is now 56; the guard was already red).

After: `check_storage_layout_tracks_wired_migrations()` parses
`STORAGE_LAYOUT_VERSION` from `crates/codegg-core/src/storage/mod.rs` and
independently derives the highest migration wired into
`crates/codegg-core/src/session/schema.rs` (upgrade chain +
`migrate_and_record` dispatch arms + `migrate_vN` definitions must agree as
sets, be contiguous `1..=max`, and equal the layout marker). The equality
relationship is corroborated by the executable contract in
`tests/storage_migrations.rs:75-79` (fully migrated DB version must equal
`STORAGE_LAYOUT_VERSION`), so the stop condition (intentional divergence)
does not apply. A `--self-test` flag (stdlib only, temp files) proves a
matched future-number pair passes without guard edits while either
mismatch direction fails. Check label renamed from
`STORAGE_LAYOUT_VERSION is 54` to the semantic invariant. All pre-existing
catalog checks (locator binding, PathBuf anti-pattern, v28 tables/columns,
lib re-export) retained.

## 4. Verification executed (commands + results; local)

| Command | Result |
|---|---|
| `python3 scripts/check_project_catalog_invariants.py --verbose` | 7/7 PASS |
| `python3 scripts/check_project_catalog_invariants.py --self-test` | 4/4 PASS |
| synthetic mismatch (layout 57 vs wired 56, then restored) | guard FAILs as required; tree restored, `git diff` clean for that file |
| `python3 -m py_compile scripts/check_project_catalog_invariants.py` | OK |
| `bash scripts/check-core-boundary.sh` | pass |
| `python3 scripts/check_tui_project_authority.py` | pass |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| `scripts/verify.sh quick` | pass |
| `git diff --check` | clean |
| `rg STORAGE_LAYOUT_VERSION README.md AGENTS.md architecture docs .opencode/skills CHANGELOG.md` | only symbolic/introduction references remain |
| `rg 'currently always holds exactly one compatibility tab\|bubble to underlying' src/tui` | no hits |

Broad `verify.sh full` not run: comment-only Rust edits plus one Python
guard change do not warrant the heavy suite beyond existing quick
verification (plan section 9/10 posture).

## 5. Invariant review

- Historical evidence untouched: `plans/closure/`, `plans/archive/`, and
  released CHANGELOG history unmodified (verified via commit file list).
- Architecture docs distinguish "introduced by migration vN" from current
  layout (section 2, row 3).
- Operational docs no longer copy the volatile layout number; the one
  pinned count kept (`command.md` 139) is enforced by
  `built_in_command_count_matches_release_docs`.
- All documented shell commands use the correct interpreter (`bash` for
  `.sh`, `python3` for `.py`, direct execution where already used).
- Skills grant no authority; daemon/scheduler/permission/tool-broker/
  project-context ownership wording verified unchanged and accurate.
- README stayed user-facing (two concise additions, no architecture dump).
- Source comments describe behavior/ownership without creating a second
  normative spec.

## 6. Failure and recovery review

Not applicable: no runtime, storage, protocol, or concurrency surface
changed. Guard failure mode is fail-closed with an actionable message
reporting both observed values.

## 7. Migration and compatibility review

None. No storage schema, layout, protocol, serialized format, CLI/TUI,
env-var, shell-policy, or MCP crypto change. The guard script is
verification code, not runtime storage behavior.

## 8. Security review

No secrets, tokens, credential values, or machine-specific paths added to
docs/skills/examples. MCP crypto wording untouched (already distinguishes
canonical new-write encryption from legacy decrypt-only compatibility).
README installer commands use HTTPS pipe-to-shell with a pre-inspection
alternative, as before.

## 9. Documentation and operations

### Active-doc census disposition by class

- `README.md` — changed (release-availability note, multi-project TUI paragraph, pinned-example correction).
- `AGENTS.md` — changed (layout symbolic ref, TUI decomposition + commands pointer, LSP de-pin, docs-count de-pin, `bash` interpreter fix).
- `CHANGELOG.md` (Unreleased only) — changed (M001 entry, stale `= 36` corrections). Released history untouched.
- `RELEASING.md` — inspected; consistent with manifests, no change required.
- `architecture/` — changed: `storage`, `core`, `codegg_core`, `workspace_services`, `overview` (layout symbolic; LSP/tests/docs/command counts de-pinned or test-pinned), `session` (migration chain, was v1–v36), `command` (108 → test-asserted 139), `lsp` (server-table heading de-pinned), `tui` (commands tree completed to 25 modules, state tree completed, counts de-pinned), `identity`/`authorization`/`audit`/`collaboration` (introduction-version rewording), `jobs`/`codegg_core`/`workspace_services`/`overview` (`bash` interpreter fix). Unchanged with reason: `tool_programs` storage section was updated (layout symbolic); all other architecture docs censused with no stale current-state fact found; `lsp.md:4149` Phase-6 "39 server definitions" kept as point-in-time verification evidence.
- `docs/` — changed: `LSP.md` (server-count de-pin). `MCP.md`, `TROUBLESHOOTING.md`, `execution-ownership.md`, `security-semantics.md`, `themes.md`, `dependency-maintenance.md`, `PLUGINS.md` censused with no stale current-state fact found. `docs/validation/` records are dated closure evidence — intentionally untouched.
- Source comments — changed as listed in section 3.
- Registry — changed as listed in section 12.

### Full skill census (13/13 under `.opencode/skills/`)

| Skill | Disposition |
|---|---|
| `architecture-review` | changed — Key-counts table reconciled (LSP de-pinned, AppEvent 45→53, commands 108→139 with test pointer, agents 9→10, tables de-pinned, fragile line ref dropped) |
| `context` | unchanged — all 15 referenced `src/context/*.rs` files exist; no stale version/count claims |
| `core` | changed — storage layout `= 36` → symbolic ref; daemon/family/transport paths verified |
| `human-shell` | unchanged — `src/shell/` files, `classify_prompt_submission`, `evaluate_command`, store/digest symbols verified; ownership wording matches canonical Bash policy |
| `jobs` | changed — `durable_jobs_phase4` count de-pinned (actual 45 vs claimed 42); module paths verified |
| `planning` | unchanged — process/registry guidance consistent with this closure's execution |
| `scheduler` | unchanged — all 16 `src/scheduler/*.rs` paths plus manifest/tests verified; per-file test counts are closure-time history, kept |
| `server` | unchanged — all `src/server/**` paths and `run_server(host, port, daemon)` signature verified; compat paths not presented as canonical |
| `skills` | unchanged — all `src/skills/*.rs` paths verified; discovery/precedence accurate |
| `tool-program-harness` | unchanged — all 4 test files + harness script exist; no stale claims |
| `tui` | changed — 19-submodule count → registry pointer; top-modal-only focus note added; all layout paths verified |
| `upgrade` | unchanged — module paths, types, and function descriptions accurate; no stale claims |
| `util` | unchanged — all `src/util/*.rs` files verified; no stale claims |

No skill has a semantics-bump convention triggered: edits were corrections
to match existing behavior, and frontmatter versions were left untouched.

### README/release-availability evidence used

At implementation time (2026-09-11, via public GitHub API): repository
`dbowm91/codegg` exists (200); `/releases/latest` → 404;
`/releases?per_page=5` → `[]` (zero published releases). README now states
installer support is implemented but nothing is published yet, and source
install is the working path. No release was performed; no network check was
added to CI.

## 10. Unresolved findings (severity: critical/high/medium/low)

- Low: overview inventory still carries unverified point-in-time counts
  (tools ~50, providers 15, git ops 54, guard scripts 19, etc.). Left as-is;
  a future pass should either verify or de-pin them, but inventing numbers
  here would repeat the defect class this milestone removes.
- Low: `architecture/tui.md` directory trees remain enumerative and will
  drift again when modules are added. Counts were removed so drift is now
  omission rather than falsehood.
- None critical/high/medium. No stop-condition trigger fired (no runtime
  bug found; layout/migration equality corroborated by
  `tests/storage_migrations.rs`).

## 11. Roadmap disposition

Subsystem roadmap
`plans/subsystems/repository-surface-housekeeping-corrective-addendum.md`
M001 is closed by this record. The addendum describes a single-milestone
pass; no follow-up milestone is registered and none is required. The
TUI M005-M010, post-audit M006/M007, project-catalog, and
verification/release foundations remain closed and were not reopened.

## 12. Registry updates

- `plans/registry.md`: housekeeping row M001 `active` → `closed`;
  dependency-ready M001 row removed and the section normalized to a
  no-pending-plans sentence (no malformed empty table); execution-order
  sequencing prose collapsed now that no implementation plan is
  dependency-ready; M001 added to Recently closed with closure record and
  implementation commit `931fb709`; TUI M007 `this commit` → `3b41fea3`
  (verified: `feat(tui): converge modal focus state ownership`).
- Implementation plan status: `ready for handoff` → `active` (at start) →
  `closed` (with this record).
- Subsystem roadmap status: `active` → `closed`.

### Unblock audit (required by planning process)

Blocked work at closure: Architecture convergence M009 (compatible-host
root runtime / all-feature Clippy evidence) and Runtime Safety C002
(supported-Linux Landlock fixture evidence). M001 is behavior-neutral
docs/guard/comments work and satisfies neither operational condition, so
both remain `blocked` with blockers unchanged. No registered
`blocked`/`proposed` plan lists M001 as a hard or interface dependency;
deferred unregistered product work remains intentionally unregistered. No
plan was moved to `ready`, and no corrective follow-up is registered (no
defect found). Final recommendation: **closed**.
