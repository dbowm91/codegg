# Repository Audit Corrective Milestone C007 — Comment Corpus Reduction and Drift Guard

Status: closed — closure record at `plans/closure/repository-audit-corrective/007-status.md`

Repository baseline: `3b29c5b88` (`origin/main`)
Audit evidence was gathered at `04536eeeb`; every file this workstream cites is
byte-identical at that commit and at the baseline above, so the findings carry
forward unchanged.

Source roadmap: `plans/subsystems/repository-audit-corrective-roadmap.md#milestone-c007--comment-corpus-reduction-and-drift-guard`

Long-term requirements: `plans/000-long-term-specification.md`, `plans/002-long-term-roadmap.md`, `plans/003-planning-process.md`, `architecture/overview.md`

Applicable ADRs: none required. This milestone changes no contract; it makes the existing one legible.

Primary class: **polish**

## 1. Objective

The in-code comment corpus is smaller, its stale references are repaired, and it cannot re-inflate. The rationale that the code comments repeat is left where it belongs — in the owning `architecture/` document — and the code keeps a one-line pointer.

## 2. Why this milestone is ready

Its hard dependency is that **C001–C006 have closed**, for two reasons. The corpus budget must be initialised from the **final** shape of the tree, not a shape that is about to change. And a comment cleanup applied to a tree that is about to move produces comments that are stale again immediately, which is the failure mode this milestone exists to prevent.

C007 is the last milestone in the workstream and is deliberately the last: it is polish, and a polish phase created before the capability's correctness boundary is closed is a prohibited planning anti-pattern.

## 3. Current implementation evidence

### The corpus, measured

| Measure | Value |
|---|---|
| Inline `//` lines | ~43,480 |
| Rustdoc `///` lines | ~63,983 |
| Module-doc `//!` lines | ~11,045 |
| Total comment lines | ~112,000–124,000, depending on whether block comments are counted separately |
| Comment words | ~944,000 |
| Comment-to-code ratio | ~0.145 |
| Comment blocks | 34,374 |
| Blocks of 12 or more consecutive comment lines | 1,071 |
| Blocks whose leading token is a plan or contract id (`M###`, `C###`) | 1,256 (3.7% of blocks) |
| Blocks containing a plan or contract id anywhere | 2,779 (8%) |

### The honest ceiling

The defensible reduction is approximately **8,800–10,000 lines — about 7–8% of the corpus, and roughly 1.1% of the codebase**. A larger number is not defensible, and a plan that claimed one would be wrong.

That percentage understates the value, because the reduction lands in the longest blocks of the most-read files, and because a reader — human or agent — reaches the current contract with less text.

### Exact duplication is not the problem

Only **21 multi-line comment blocks totalling 365 lines** appear more than once in the tree. Copy-paste is not the source of the bloat. The bloat is **single-instance prose**: a long explanation that is written once, in one place, at length.

### The rationale already exists canonically

This is the fact that makes the reduction safe. The reasoning behind the code is already written down, in the document that owns it:

- `architecture/tui.md` documents the chat activity indicator, the todo strip, the removal of the working strip, and the strip-reservation rules in **more** detail than the corresponding code comments do.
- `architecture/tui.md`'s text-wrapping section and its "styled, word-aware wrap" subsection restate what `src/tui/composer_claims.rs` says at length.
- `architecture/lsp.md` restates the LSP restart module's rationale.
- `architecture/writer_permit.md` restates the writer fence's module documentation.

**The code comments are effectively a third copy.** "Essay → one-line pointer" is therefore mechanical, safe, and reviewable — the reasoning is not lost, it moves to where it already lives.

### Twenty-three stale path references

These are references in comments to paths that resolve to nothing. The audit counted them twice, independently, and got the same twenty-three.

**Plan documents that no longer exist anywhere in the repository** — fourteen occurrences:

| Referenced path | Cited from |
|---|---|
| `plans/native_tool_crates_hardening.md` | `src/security/mod.rs`, `src/lsp/mod.rs`, `src/tool/disabled.rs` |
| `plans/lsp_phase_9_12_hardening_plan.md` | `crates/egglsp/src/root.rs`, `crates/egglsp/src/tui_summary.rs` |
| `plans/shell_output_projection_phase_01_command_event_model.md` | `src/shell/projection.rs`, `src/tui/app/mod.rs` |
| `plans/shell_output_projection_phase_02_projection_trait.md` | `src/shell/projection.rs`, `src/shell/projector.rs` |
| `plans/tui_phase_7_background_task_lifecycle.md` | `src/tui/task_lifecycle.rs` |
| `plans/security_review_async_dispatch.md` | `src/tui/app/commands.rs` |
| `plans/eggsearch-eggsact-phase-06-backend-config-policy.md` | `src/tool/integrated_config.rs` |
| `plans/.../002-model-visible-tool-surface-minimization.md` (truncated placeholder path) | `src/tool/disclosure.rs` |

**Test files whose path *and* extension changed** when the root package's integration tests became families — eight occurrences. The members are now `tests/<family>/<member>.inc`, not `tests/<member>.rs`:

| Referenced path | Actual path | Cited from |
|---|---|---|
| `tests/modifier_chords.rs` | `tests/tui_render/modifier_chords.inc` | `src/tui/input.rs` (×2) |
| `tests/tui_side_panels.rs` | `tests/tui_render/tui_side_panels.inc` | `src/tui/input.rs` |
| `tests/tui_theme_source_of_truth.rs` | `tests/tui_render/tui_theme_source_of_truth.inc` | `src/tui/components/dialogs/ui_node.rs` |
| `tests/storage_migrations.rs` | `tests/core_daemon/storage_migrations.inc` | `crates/codegg-core/tests/user_prompt_queue.rs` |
| `tests/linux_native_scratch_containment.rs` | `tests/security_permission/linux_native_scratch_containment.inc` | `crates/codegg-core/src/run_placement.rs`, `tests/macos_native_shared_checkout_containment.rs` |
| `tests/lib.rs` | `tests/common/mod.rs` | `src/core/daemon_lsp.rs` |

**One asset with the wrong extension** — `assets/tool-advisor/causal-frontier-v1.json` is actually `causal-frontier-v1.jsonl`, cited from `src/tool_advisor/causal_frontier.rs`.

**False positives that must NOT be "fixed":** `src/small.rs`, `src/large.rs`, `src/http/ratelimit.rs`, `src/foo.rs`, `src/bar.rs` and `tests/test_foo.py` are **illustrative example paths** inside prose, not references. A guard that rewrites or fails on them would be wrong.

### Fix archaeology

Around **889 lines** of comments record what *used to* to be true — "used to", "previously", "no longer", "was fixed", "the old", and similar. The worst single site is a 27-line changelog inside a test body that walks a sequence of revisions ("164 → 165 → … → 169"). That history is the repository's, and belongs in git and in the closure records — not in a comment that is read forever and describes a state that no longer exists.

### Plan-id-anchored blocks

**1,344 blocks — about 6,655 lines — lead with a plan or contract identifier**: `//! C007: production publication of a private Git index.`, `/// M003: the bound on how many stranded delivering rows one startup pass will reconcile.`

The identifier is **worth keeping** — it is the traceability anchor, and it is what makes a comment auditable against the plan that justified the code. What is removable is the restatement of what the identifier means. `C007: production publication of a private Git index` can become `Durable publication of a private Git index into the repository index.` — the same information, the same traceability, a third of the words.

### The planning registry is the largest single consumer of an agent's context

`plans/registry.md` is the file AGENTS.md directs every agent to read first. It is **99,859 words**. Of that, **87,688 words — 88% — sit inside the table cells**, spread over 739 data rows.

The distribution is extremely skewed:

- the **median** row carries 44 words, which is healthy;
- **30 rows** carry 500 to 2,000 words each;
- one row carries 2,086 words.

Those fat rows embed the entire closure narrative — CI run identifiers, commit identifiers, merge-reconciliation history, renumbering incidents, defect narratives — **in the cell of the status table**. The narrative belongs in the `plans/closure/` record the row already cites. The cell should carry the status and the pointer.

The wider `plans/` tree holds **3,175,724 words across 1,670 markdown files** (`closure/` 707 files and 1.24M words; `implementation/` 696 files and 1.31M words). That corpus is the repository's legitimate planning history and is **not** in scope to prune. The bounded fix is the registry's own fat rows.

### Two guards are sensitive to comment content

Most of the repository's roughly seventy guards **strip comments before matching** — `strip_comments`, `_code()`, `normalised_body()` — so they are insensitive to comment content, and deleting prose can only remove a false positive from them. Two are **not**:

1. **`scripts/check_project_agent_pwd_inference.py`** allows a finding when the matched line contains `//…PWD`. Its allowlist also contains `re.compile(r"CLI bootstrap reads cwd exactly once")` — a phrase that occurs **nowhere** in `src/`. The mechanism is **latent**: no live line depends on a trailing `PWD` comment, and both real PWD reads are suppressed by non-comment mechanisms. But the dead allowlist entry is a trap, and a comment cleanup that added a trailing `PWD` comment to a `current_dir()` line would silently suppress a finding.
2. **`scripts/rust_test_scan.py`'s `_SKIP_MARKER`** is `\b(?:skipped|SKIP|Skipped)\s*:`, matched against `body_text`, which **includes comments**. There is one live instance: `src/security/workflow/report.rs` contains a comment inside a test body quoting the product's own note format — `"hunkSourceContext skipped: patch size N bytes exceeds cap M bytes"`. That comment currently makes the guard treat the test as carrying a skip marker. **Deleting it makes the guard stricter** and newly applies the guard to that test. A second candidate, in `crates/egglsp/tests/real_server_smoke.rs`, is at module scope, outside any test body, and is harmless.

These are the two places where "delete a comment" is not comment-neutral, and the cleanup procedure must treat them explicitly.

## 4. Invariants that must not regress

- **A comment that states a rule the code cannot enforce is kept.** This is the primary keep-rule.
- **A comment that names a failure that looks impossible until you know the history is kept**, in its current-state form. The history may go; the rule stays.
- **A comment that is the only statement of a security invariant is kept.**
- **A comment that is a guard's machine-readable marker is kept.** The two guards in §3 are the reason this is a rule and not a preference.
- **A comment that is a guard's human-readable contract is kept** — a guard that documents the boundary it enforces needs that text to stay next to the code it enforces.
- **The rationale is not lost.** Every removed essay's substance must exist in the owning `architecture/` document, or be restated in one line at the code. The implementer must record, per removal, where the reasoning went.
- **Historical records are untouched.** `plans/closure/`, `plans/validation/`, and `docs/validation/` pin line numbers and historical states by design. Their citations are not stale references and must not be "fixed".
- **Illustrative example paths are not references.** A guard must not flag `src/small.rs` or `src/foo.rs`.
- **The guard is non-vacuous.** A comment-budget guard that reports "clean" on a tree it could not read is worse than no guard.

## 5. Scope

**In scope**

- The twenty-three stale path references, corrected or removed.
- The fix-archaeology comments, reduced to their current-state rule.
- The plan-id-anchored blocks, re-anchored on the invariant with the identifier kept.
- The oversized rows in `plans/registry.md`.
- A comment-budget guard with a measured manifest.
- The dead allowlist entry in `scripts/check_project_agent_pwd_inference.py`, and the skip-marker hazard in `scripts/rust_test_scan.py` — **hardened, not weakened**.

**Out of scope**

- The `plans/` corpus beyond `plans/registry.md`'s own rows. The 1,670 historical files are the repository's legitimate record.
- `plans/closure/` and `plans/validation/` records, and `docs/validation/`.
- `architecture/` documents. The rationale **moves into** them; this milestone does not restructure them, and does not reduce their own verbosity. That is a separate pass with its own risk profile.
- Rustdoc API documentation. The public API's documentation is load-bearing for consumers and is not a reduction target.
- The comment content of any guard script, except the two specific hazards in §3.
- The `src/` tree's *content*. No code change accompanies a comment change, except the two guard hardenings.

## 6. Required production changes

### Core

- The twenty-three stale references are corrected to the path that exists, or removed where the reference no longer carries meaning. The implementer must choose per reference: a plan document that was deleted may have been superseded by an ADR or by an `architecture/` section, and the replacement must be the thing that actually holds the reasoning.
- The fix-archaeology comments are reduced to the current-state rule. The "what used to be true" is removed from the code; it remains in git and in the closure records.
- The plan-id-anchored blocks keep their identifier and lose the restatement.
- `plans/registry.md`'s fat rows are reduced to a status and a pointer to the closure record that holds the narrative. **The narrative must not be deleted** — it moves.

### Storage

None.

### Protocol

None.

### Runtime

None.

### Frontend

None.

### Security

- The two comment-sensitive guards are hardened so that the cleanup is safe **and** the hazards are closed:
  - `scripts/check_project_agent_pwd_inference.py`'s dead allowlist entry is removed, and the `//…PWD` allowance is narrowed so it cannot suppress a finding for an unrelated line.
  - `scripts/rust_test_scan.py`'s skip-marker detection is made to distinguish a marker in a **comment** from a marker in **code**, so that quoting a product note inside a test body does not grant a skip. The implementer must confirm the change does not weaken the skip-marker's effect on genuine markers.

### Docs

- The comment corpus's own policy is documented, in the guard's docstring and in the architecture section that owns the code.

## 7. Ordered work packages

### WP1 — Repair the stale references

Intent: make every path reference in a comment resolve, or stop claiming to be a reference.

Required changes: work through the twenty-three. For each, determine what the reference was pointing at and what now holds that content — an ADR, an `architecture/` section, a closure record — and point at that, or remove the reference if the content is now simply part of the code's behaviour.

The implementer must **not** rewrite the illustrative example paths. The guard must distinguish a path that is a reference from a path that is an example, and the distinction is the hard part of WP5.

Acceptance evidence: every reference resolves, and the illustrative paths are untouched.

### WP2 — Reduce the fix archaeology

Intent: comments describe the current contract.

Required changes: reduce the archaeology comments to the rule they were carrying. The worst instance, a 27-line revision changelog inside a test body, becomes one line or none.

Acceptance evidence: the archaeology class is measured before and after, and the count falls.

### WP3 — Re-anchor the plan-id blocks

Intent: keep the traceability, lose the restatement.

Required changes: for each of the 1,344 blocks, keep the identifier and replace the restatement with the invariant itself.

This is the largest mechanical edit in the milestone and the one most likely to be done mechanically without judgement. The implementer must sample the result and confirm the rewrites are meaningful, not formulaic.

Acceptance evidence: the blocks are re-anchored, the identifiers are all still present, and the spot-check is recorded.

### WP4 — Reduce the registry's fat rows

Intent: the file every agent reads first should be an index.

Required changes: for each of the roughly thirty rows carrying 500 or more words, move the narrative into the closure record the row already cites — **or** into a new closure record if the narrative has no home — and reduce the cell to a status and a pointer. No narrative is deleted.

The implementer must be careful here: the registry's fat rows are also the registry's **evidence** for a great many closures, and a reader who reads only the registry today sees a much richer picture than a reader who follows pointers. The reduced registry must still be a usable control surface, and the pointers must be exact.

Acceptance evidence: the registry's word count falls by the recorded amount, the median and maximum row lengths are stated, and a sample of reduced rows is checked to confirm the pointer resolves to a record that holds the narrative.

### WP5 — Add the comment-budget guard

Intent: the corpus cannot re-inflate.

Required changes: a new `scripts/check_comment_budget.py` and a `docs/comment-budget.toml` manifest, modelled on `scripts/check_tui_editor_doc_drift.py`, the repository's closest precedent. Five rules:

| Rule | Fails on |
|---|---|
| **R1** stale path reference | a comment references a repo path that does not exist |
| **R2** unresolvable doc pointer | a comment points at an `architecture/` section or plan that does not exist |
| **R3** oversized block | a comment block exceeds a configured line count |
| **R4** per-file budget | a file's comment-to-code ratio exceeds its configured ceiling |
| **R5** orphaned contract | a block that restates a contract without naming its owning document |

Design requirements:

- R4 must be **bankable**: a file whose ratio falls below its ceiling has its ceiling lowered, so a reduction lowers the bar for that file rather than being undone by the next edit.
- R1 and R2 must distinguish a **reference** from an **example**. The illustrative paths in §3 must not be flagged. A practical rule is that a path is a reference when it appears in a comment that also names a module, a test, or a document, and an example when it appears in a sentence describing a hypothetical. The implementer must find a rule that is not a guess, and must encode the known examples as negative cases in the self-test.
- The guard must use `scripts/rust_test_scan.py` for anything requiring Rust parsing, so a lifetime is never misread as a character literal.
- The guard must **not** report "clean" when it could not read a file. A partial scan reported as a pass is the failure mode that makes a guard worse than nothing.
- The guard must provide `--self-test` that injects each defect class and proves the guard rejects it, and that proves the known examples are accepted. The self-test must be non-vacuous in both directions.
- The guard must be wired into `scripts/verify.sh quick` and CI.

**Ship R1 and R2 first**, as a standalone change, before the corpus reduction. They need no manifest, they catch all twenty-three known-stale references immediately, and they prevent new ones while the reduction proceeds.

Acceptance evidence: the guard runs in `verify.sh quick`; its `--self-test` passes; it fails on each injected defect class; and it accepts the known illustrative examples.

### WP6 — Harden the two comment-sensitive guards

Intent: close the two hazards, so that the cleanup is safe and future comment edits cannot be used to suppress a finding.

Required changes: as §6 describes.

Acceptance evidence: the PWD guard's dead entry is gone, its allowance is narrowed, and its `--self-test` still rejects the defect class it exists for. The skip-marker change is shown to preserve the effect on genuine markers while no longer being triggered by a comment that quotes a product note — with the live instance in `src/security/workflow/report.rs` as the test case.

### WP7 — Measure and record

Intent: the reduction is a number, not a claim.

Required changes: record the corpus size before and after, per class, and the reduction against the ceiling in §3. If the achieved reduction falls short of the ceiling, the shortfall must be explained per class — the point of the ceiling is that it is honest, and a plan that silently under-delivers against its own estimate is a defect in the plan.

Acceptance evidence: the before-and-after measurement, per class, is recorded in the closure.

## 8. Failure, cancellation, restart, contention semantics

Not applicable in the usual sense: this milestone changes no runtime behaviour. The one behavioural consideration is that the two hardened guards become **stricter** in one respect and must not become weaker in any. The implementer must state, for each, whether any previously-failing case now passes, and why that is correct.

## 9. Compatibility and migration

- **No code behaviour changes**, with the single exception of the two guard hardenings in WP6, whose effect must be stated explicitly.
- **No protocol, storage, or configuration change.**
- **The `docs/comment-budget.toml` manifest is new**, and it is a repository artefact that must be registered like any other.
- The `plans/registry.md` reduction changes that file's shape. Every inbound link to it must still resolve, and the registry's own sections must remain coherent — a reduced registry that is no longer a usable control surface would be a regression against `plans/003-planning-process.md`'s maintenance rules.

## 10. Required tests

**Focused**

- The guard's `--self-test`, including a case for each of the five rules and a negative case for each of the known illustrative paths.
- A test that the guard reports a failure when it cannot read a file, rather than reporting success.

**Integration**

- A check that every remaining path reference in a comment resolves — which is R1 and R2 themselves, so the guard is the test. There is no separate test to write; the guard is the coverage.

**Security**

- The two hardened guards' `--self-test` outputs, and the demonstration that the defect classes they exist for are still rejected.

**Migration**

- None.

**Placement.** The guard's self-test lives beside the guard, in the repository's established style. There is no test family for this milestone, because the guard *is* the coverage.

**Vacuity.** Each of the five rules must be shown failing on an injected defect, and the guard must be shown failing — not passing — when it cannot read the tree.

## 11. Required verification commands

```bash
# Scoped: quick's guards plus only the tests this change can affect.
scripts/verify.sh scoped HEAD~1

# The new guard, including its non-vacuity proof.
python3 scripts/check_comment_budget.py --self-test

# The two hardened guards.
python3 scripts/check_project_agent_pwd_inference.py --self-test
python3 scripts/rust_test_scan.py --validate

# Rust-aware test inventory (ground truth; never regex-scan Rust).
python3 scripts/rust_test_scan.py --validate

# Full workspace, before the closure record is written.
bash scripts/verify.sh full
```

`scripts/verify.sh full` runs `cargo clippy --workspace --all-targets --locked -- -D warnings`. On macOS this currently fails for reasons unrelated to this milestone (C005 owns that asymmetry); the implementer must record the failure set before and after and state that the change added and removed no lint finding. Linux must stay green.

## 12. Documentation updates

- `docs/comment-budget.toml`: the new manifest.
- `architecture/overview.md` or the equivalent: the comment policy — what a comment is for, and what the guard enforces.
- `plans/registry.md`: the reduced rows, and this milestone's own row.
- `plans/README.md` or `plans/003-planning-process.md`: if the registry's reduced shape changes what the registry is for, that document says so. The reduction is a change to the control surface's density, and the governance document should reflect it.

## 13. Acceptance criteria

Externally observable:

1. Every path reference in a comment resolves, or no longer claims to be a reference.
2. The known illustrative example paths are unchanged.
3. The fix-archaeology class is substantially reduced, and the reduction is measured.
4. The plan-id-anchored blocks keep their identifiers and state the invariant.
5. `plans/registry.md` is materially shorter, its median and maximum row lengths are stated, and every reduced row's pointer resolves to a record holding the narrative.
6. The new guard runs in `verify.sh quick`, its `--self-test` passes, and it fails on each injected defect class.
7. The PWD guard's dead allowlist entry is gone and the skip-marker hazard no longer fires on a quoted product note.
8. The total reduction is measured and stated, against the ceiling.

## 14. Stop conditions

The implementer must report rather than improvise if any of the following holds.

- A removal cannot be traced to an owning `architecture/` section or to a restated one-line rule. Stop: that removal deletes knowledge.
- A rule the code cannot enforce exists **only** in the comment being removed, and it is not a security invariant. Stop and restate it in one line at the code.
- The registry's reduction would remove **evidence** rather than relocate it. Stop: the narrative moves; it is not deleted.
- The guard cannot distinguish a reference from an example without a heuristic that would produce false positives on the known examples. Stop: ship R1 and R2 with a narrower, defensible rule rather than a guess.
- Hardening the skip-marker would weaken its effect on a genuine marker. Stop: that would trade a comment hazard for a real coverage loss.
- The measured reduction falls materially short of the ceiling and the shortfall cannot be explained per class. Stop and report: the ceiling is the plan's own estimate, and an unexplained shortfall means the estimate or the work is wrong.

## 15. Closure evidence required

- The before-and-after corpus measurement, per class, and the reduction against the ceiling.
- The disposition of each of the twenty-three stale references, and the confirmation that the illustrative paths are untouched.
- A spot-check of the plan-id rewrites, with the sample recorded.
- The registry's before-and-after word count, the median and maximum row lengths, and a sample of reduced rows with their resolved pointers.
- The guard's `--self-test` output, each injected defect's rejection, the acceptance of the known examples, and the demonstration that the guard fails when it cannot read the tree.
- The two hardened guards' before-and-after behaviour, stated per guard.
- The `verify.sh scoped` and `verify.sh full` results, labelled local versus CI.
- The registry update, and the unblock audit of the registry's Blocked work section.

## 16. Handoff notes

- **The ceiling is 7–8%.** Do not promise more, and do not reach for more. A reduction beyond that is not a reduction, it is a loss of reasoning.
- **The rationale is already written down** in `architecture/`. That is the whole reason this is safe. Verify the target section actually contains the reasoning before deleting the code's copy of it — several places say the opposite, and the reduction inverts the direction of the reference.
- **Do not "fix" the illustrative paths.** `src/small.rs`, `src/large.rs`, `src/http/ratelimit.rs`, `src/foo.rs`, `src/bar.rs` and `tests/test_foo.py` are examples in prose. A guard that flags them will be disabled by the first person it inconveniences.
- **Ship R1 and R2 first**, as their own change, before touching the corpus. They need no manifest, they catch all twenty-three known-stale references, and they stop new ones arriving during the reduction.
- **Two guards read comment text.** `check_project_agent_pwd_inference.py` can suppress a finding with a trailing `PWD` comment, and `rust_test_scan.py`'s skip marker fires on the word "skipped:" inside a comment — which one live test body currently does. Hardening them is part of this milestone, and it is a prerequisite for the cleanup being safe, not a follow-up.
- **The registry's fat rows are evidence.** Their narrative moves to the closure records they already cite. It does not get deleted, and the reduced registry must still be a usable control surface.
- This milestone closes the workstream.
