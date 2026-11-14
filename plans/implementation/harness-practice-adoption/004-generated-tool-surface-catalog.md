# Harness Practice Adoption Milestone 004 — Generated Tool-Surface Catalog

Status: closed

Repository baseline: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`
(branch `feat/auto-20261109-f8abad0d`)

Source roadmap:

- `plans/subsystems/harness-practice-adoption-roadmap.md` §7 M004, §9

Long-term requirements:

- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
  — one catalog, no frontend-held copy of a surface
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md`
- `architecture/agent-tool-surface.md` — the document this milestone replaces as
  the authoritative enumeration

Applicable ADRs:

- **None.** Documentation and a test. No production behaviour, no dependency, no
  protocol, no storage.

Primary class: infrastructure

## 1. Objective

The model-facing tool surface is **emitted by constructing the real registry**,
committed as a catalog, and checked on every sweep — so adding, removing, or
re-gating a tool cannot leave the documentation describing a surface the binary
does not have.

## 2. Why this milestone is ready

- No hard dependency.
- The registration sequence is already single and authoritative:
  `ToolRegistry::with_options` is documented as the only registration input, and
  `with_defaults` / `with_session_defaults` are thin wrappers over it.
- The generated-asset pattern is established and guarded:
  `scripts/generate_builtin_agents.py` and `scripts/generate_subagent_names.py`
  each own their output file and offer `--check`.
- The registry can be constructed with no workspace root: it registers an
  `UnrootedTool` for each workspace tool rather than reading a process
  directory, so a generator needs no cwd and no filesystem authority.
- `architecture/overview.md` already declares a "Verified counts" table whose
  rows name their own source, so the catalog has a place to be the source.

## 3. Current implementation evidence

**The surface is enumerated by hand.** `architecture/agent-tool-surface.md`
lists the tools grouped by capability, with counts and gating prose. Nothing
mechanically ties those names to `src/tool/mod.rs`.

**The registry is a single sequence.** `ToolRegistry::with_options` builds the
catalog and inserts every tool. Visibility is gated by todo policy, evidence
backend availability, LSP/security availability, eggsact availability, and
context-read configuration. A `DisabledTool` stub keeps a stable name for a
backend-gated domain and reports `expose_in_definitions() == false`, so a
disabled tool is *in the registry* but *not in the model's definitions*. A
`UnrootedTool` does the same for a missing workspace root.

**There is an existing catalog type.** `src/tool/catalog.rs` holds `ToolCatalog`
with a search mode and deferred-name registration for `tool_search` on-demand
discovery. It is a runtime discovery structure, not a documentation source; this
milestone must not turn it into one.

**The generated-asset precedent is narrow and proven.** Two generators exist,
both Python, both writing a `generated.rs`, both with `--check`. They work
because their input is a directory of TOML files. This milestone's input is a
Rust registry, so the generator must be Rust and the comparison must be a test;
the *pattern* is reused, not the mechanism.

**`quick` does not run tests.** `scripts/verify.sh quick` runs formatting, the
generated-asset checks, the guard scripts, and a workspace `cargo check`. A
test-only freshness gate therefore runs in the nextest sweep (CI and
`verify.sh full`), not in `quick`. This milestone therefore has two layers: a
cheap structural guard that can run in `quick`, and the authoritative
registry-comparison test that runs in the sweep.

## 4. Invariants that must not regress

- **One source.** The catalog is produced by `ToolRegistry::with_options`. A
  generator that hand-lists tools, or reads a TOML mirror, has the same defect
  as the document it replaces.
- **A gated tool is listed as gated, never omitted.** Omission is the drift this
  milestone exists to catch: a reader cannot tell an omitted tool from one that
  was never registered.
- **The catalog is deterministic.** Two runs on one baseline produce
  byte-identical output: sorted rows, no timestamps, no absolute paths, no
  environment-dependent values.
- **The catalog carries no schema.** Names, category, effect class,
  defer-loading, and visibility. Schemas are large, change often, and already
  have one authority in the registry itself.
- **The hand-written document keeps its prose and loses its enumeration.**
  `architecture/agent-tool-surface.md` keeps the *why* — grouping, gating
  rationale, the deferred-loading story — and links to the catalog for the
  *what*. It must not keep a second list.
- **No production change.** No tool is added, removed, renamed, or re-gated by
  this milestone. The catalog describes the surface; it does not define it.

## 5. Scope

### In scope

- A registry-rendering helper in the test tree that constructs the registry for
  a declared matrix of option sets and renders a deterministic markdown table.
- A committed catalog at `architecture/tool-catalog.md`.
- An update mode (`CODEGG_UPDATE_TOOL_CATALOG=1`) and a comparison mode (the
  default).
- A structural guard runnable in `quick` that checks the catalog's shape and its
  agreement with the hand-written document's remaining references.
- The hand-written document reduced to prose plus a link.

### Explicitly out of scope

- A second binary target. The distribution is one executable per target; a
  generator binary would be a second entry point for a documentation artifact.
- A catalog of schemas, examples, or descriptions. §4.
- Generating the document at build time. The catalog is committed and diffed.
- Changing any tool's gating. If the catalog reveals a gate that looks wrong,
  that is a finding for a separate milestone, not a fix here.
- The config and storage catalogs. They are M005 and M006 and follow this
  milestone's shape.

## 6. Required production changes

### Core/domain

None. The rendering helper lives in the test tree.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None. The generator constructs a registry in-process; no daemon, no store, no
scheduler.

### Frontend or operator surface

None.

### Security and authorization

None. The generator must not require a workspace root, a credential, or a
network; a test asserts it constructs successfully with `workspace_root: None`
and no configuration file present.

### Documentation and static guards

- `architecture/tool-catalog.md` — the generated artifact, with a "generated,
  do not edit" header naming its generator and its update command.
- `architecture/agent-tool-surface.md` — prose retained, enumeration replaced by
  a link.
- `architecture/overview.md` — the tool-count row names the catalog as its
  source instead of a hand count.
- `scripts/check_tool_catalog.py` — the structural guard, with `--self-test`.

## 7. Ordered work packages

### Work package A — The rendering matrix

Intent: decide *which* surfaces are catalogued before rendering any.

Required changes:

- A declared matrix of option sets, each with a stable name:
  `default`, `plan-mode`, `restricted-subagent`, `backend-disabled`. Each is a
  `ToolRegistryOptions` value the test constructs; the names are the table's
  section headers.
- The renderer reads each registry's definitions and each tool's metadata:
  name, category, effect class, defer-loading, and whether
  `expose_in_definitions()` is true.
- A tool present in the registry but hidden by a gate renders as
  `hidden (<reason>)` where the reason comes from the tool's own contract or
  gate, not from a hardcoded string in the renderer.
- Rows sort by name within a section; sections sort by their declared order.

Acceptance evidence:

- Rendering the same matrix twice produces byte-identical output.
- A gated tool appears in its section with a reason; it is not absent.
- The renderer needs no workspace root and no configuration file.

### Work package B — The comparison test and update mode

Intent: the authoritative gate.

Required changes:

- A test in the root test tree that renders the catalog and compares it to the
  committed file.
- `CODEGG_UPDATE_TOOL_CATALOG=1` rewrites the file and passes; without it, a
  mismatch fails and prints the first divergent line plus the update command.
- The test asserts the committed file's header names the generator and the
  update command, so a hand-edited file is detectable.

Acceptance evidence:

- With the file matching, the test passes.
- After adding a tool to the registry, the test fails and names the added row.
- After running the update mode, the test passes and the diff contains only the
  added row.

### Work package C — The structural guard

Intent: a cheap check that runs where tests do not.

Required changes:

- `scripts/check_tool_catalog.py` verifies: the catalog exists and carries the
  generated header; every row is well-formed; no duplicate tool name within a
  section; sections appear in the declared order; and every tool name still
  mentioned in `architecture/agent-tool-surface.md`'s prose appears in the
  catalog.
- It does **not** re-derive the registry; that is the test's job.
- `--self-test` injects a duplicate row, a malformed row, a missing section, a
  name referenced in prose but absent from the catalog, and a hand-edited header
  without the generated marker — and proves each is rejected.

Acceptance evidence:

- All five injections are rejected.
- The guard is green on the committed catalog.

### Work package D — Documentation reduction

Intent: the hand-written document stops being a second source.

Required changes:

- `architecture/agent-tool-surface.md` keeps: the grouping rationale, the gating
  story, the deferred-loading and `tool_search` explanation, and the note that a
  disabled domain keeps a stable name and hides itself from definitions.
- It loses: the per-tool enumeration and the counts.
- It links to `architecture/tool-catalog.md` and states that the catalog is
  generated.
- `architecture/overview.md`'s tool-count row names the catalog as its source.

Acceptance evidence:

- The document contains no tool table and no count.
- Every tool name it still mentions resolves in the catalog.

## 8. Failure, cancellation, restart, and contention semantics

- **Registry construction fails** for one matrix entry. The test fails naming the
  entry; it must not fall back to a smaller matrix, because a silently reduced
  matrix is a catalog that omits a surface.
- **A tool's metadata is unavailable.** Fail; do not emit a blank cell.
- **Update mode in CI.** The test asserts update mode is not set in CI (an
  environment check), so a CI run can never rewrite the artifact it is
  verifying.
- **Concurrent runs.** The test writes only under update mode and only to one
  path; two concurrent update runs are not supported and are not needed.
- **Restart.** Not applicable: no durable state.

## 9. Compatibility and migration

- No production change, so no compatibility surface.
- The catalog is additive documentation; a reader of the old document finds the
  same prose plus a link.
- If a later milestone adds a tool, the catalog changes in that milestone's
  commit — the test forces it.

## 10. Required tests

### Focused unit tests

- The renderer is deterministic across two runs.
- A hidden tool renders with a reason.
- Rows sort by name; sections keep their declared order.

### Integration tests

- The comparison test passes on the committed catalog.
- The comparison test fails after an injected registry change and names the row.

### Restart and recovery tests

- Not applicable.

### Contention and cancellation tests

- Not applicable. The test asserts update mode is refused under CI.

### Security and negative tests

- The generator constructs with no workspace root and no configuration.
- The catalog contains no absolute path and no home directory.
- The structural guard rejects all five injections.

### Migration and compatibility tests

- A test asserts `architecture/agent-tool-surface.md` contains no tool table, so
  the second source cannot return.

## 11. Required verification commands

```bash
# regenerate, then verify the comparison
CODEGG_UPDATE_TOOL_CATALOG=1 cargo test --test tool_catalog
cargo test --test tool_catalog

# the structural guard and its self-test
python3 scripts/check_tool_catalog.py
python3 scripts/check_tool_catalog.py --self-test

# the existing generated-asset checks still pass
python3 scripts/generate_builtin_agents.py --check
python3 scripts/generate_subagent_names.py --check

# formatting and linting
cargo fmt --check
cargo clippy -p codegg --tests -- -D warnings

# the routine set
scripts/verify.sh quick
```

Note: `quick` runs the structural guard but not the comparison test. The closure
record states which ran.

## 12. Documentation updates

- `architecture/tool-catalog.md` — new, generated.
- `architecture/agent-tool-surface.md` — enumeration removed, link added.
- `architecture/overview.md` — the tool-count row names its source.
- `architecture/testing.md` — the sweep-only gate is named where the test tiers
  are described.
- `plans/subsystems/harness-practice-adoption-roadmap.md` §12 — status only.

## 13. Acceptance criteria

1. The committed catalog is byte-identical to a fresh render on the closure
   baseline.
2. Adding a tool to the registry makes the comparison test fail and name the
   added row; running the update mode makes it pass.
3. A backend-gated tool appears in the catalog as hidden with a reason, not as
   an absent row.
4. The structural guard rejects all five of its self-test injections.
5. `architecture/agent-tool-surface.md` contains no tool table and no count, and
   every tool name it mentions resolves in the catalog.
6. No production file changed, proven by the diff.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- constructing the registry requires a workspace root, a credential, a network,
  or a configuration file — the generator must be hermetic;
- a tool's visibility cannot be determined without executing it;
- the matrix needs a second binary target to run — the distribution is one
  executable per target, and a documentation artifact does not justify a second;
- the catalog would need to include schemas to be useful, which §4 excludes and
  which would make the artifact large and volatile;
- the only way to keep the comparison test out of a sweep is a
  `required-features` gate — that is the 149-unrun-functions trap.

## 15. Closure evidence required

- The committed catalog, with its row count and section names.
- The two-run determinism result.
- The injected-tool failure and the update-mode pass, with the diff.
- The structural guard's `--self-test` output with all five injections rejected.
- The diff proving no production file changed.
- The named non-claim: the catalog describes visibility and metadata, not
  behaviour or schema.

## 16. Handoff notes

- **The registry is the source; do not mirror it.** If the renderer needs a
  fact the registry does not expose, add an accessor to the tool contract — do
  not maintain a parallel table.
- **Hidden is not absent.** A gated tool with a reason is the whole point.
- **Update mode is local.** CI must refuse it; assert that.
- **Keep the catalog small.** One row per tool, five columns. A reader should be
  able to diff it.
- **The prose stays.** The hand-written document's value is the *why*; deleting
  it and keeping only a table would be a net loss.
